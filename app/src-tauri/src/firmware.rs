//! Firmware update from the app: a guarded wrapper around `openbms-flash`. The app's image policy is
//! the vendor allowlist; other images only after an explicit opt-in and their own confirmation phrase.
//! The flash runs on its own thread and reports progress as events.

use crate::{maint, reveal, stop_monitor, AppState};
use openbms_flash::maint::policy::{self, Master};
use openbms_flash::maint::{bus, SystemClock};
use openbms_flash::{ehex, known, session, session::Progress, Lang};
use openbms_proto::DeviceInfo;
use openbms_transport::Connection;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize, Clone)]
pub struct ImageInfo {
    release: String,
    device: String,
    bytes: usize,
    blocks: usize,
    sha256: String,
    /// "original" | "maintenance" | "custom" (see `known::ImageKind`)
    kind: &'static str,
    /// not in the vendor allowlist (accepted only with custom firmware allowed)
    custom: bool,
}

#[derive(Serialize, Clone)]
pub struct ProgressEvent {
    stage: &'static str,
    block: Option<usize>,
    total: Option<usize>,
}

#[derive(Serialize, Clone)]
struct FirmwareDone {
    ok: bool,
    error: Option<String>,
    stage: Option<&'static str>,
    /// true once W F E was sent: an error may have left the board without an application
    written: bool,
    before: Option<DeviceInfo>,
    after: Option<DeviceInfo>,
    log: String,
    /// the pack may hang in its bootloader: its record stays (`bootloader-pending`)
    pending: bool,
    /// a log write error after the confirmed erase: the flash went on, the log is incomplete
    log_error: Option<String>,
}

/// What `firmware_start` checked before it began: the right-bank check ("match" | "stored" | "unchecked" |
/// "none" without an installation).
#[derive(Serialize, Clone)]
pub struct FlashStarted {
    bank: &'static str,
    /// address 0: "checked" (a fresh control channel read decided C'), "unchecked" (no fresh read; the marker last seen
    /// was not on, the flash goes on with that hint); None for another address
    #[serde(skip_serializing_if = "Option::is_none")]
    marker: Option<&'static str>,
}

/// What a flash of the master needs from its marker (C'), for the installation whatever pack bus endpoint is used: a fresh
/// read of the installation's control channel (`fresh`: None without a control channel or when the read failed or showed
/// nothing) decides; without one the marker last seen counts. Err: the refusal key; Ok: "checked" or "unchecked".
pub fn master_marker_check(address: u8, image: known::ImageKind, fresh: Option<&Master>, last_on: bool) -> Result<Option<&'static str>, &'static str> {
    if address != 0 {
        return Ok(None);
    }
    match fresh.filter(|m| **m != Master::Unknown) {
        Some(m) if policy::release_first(m, image) => Err("release-master-first"),
        Some(_) => Ok(Some("checked")),
        None if image != known::ImageKind::Maintenance && last_on => Err("release-master-first-unread"),
        None => Ok(Some("unchecked")),
    }
}

fn lang(code: &str) -> Lang {
    if code.starts_with("de") { Lang::De } else { Lang::En }
}

/// The phrase differs for custom images, so consent to one kind never covers the other.
pub fn confirmation_ok(address: u8, custom: bool, typed: &str) -> bool {
    let phrase = if custom { format!("FLASH CUSTOM {address}") } else { format!("FLASH {address}") };
    typed.trim() == phrase
}

/// Clears the flash flag on drop, so even a panic in the flash thread cannot leave the app locked.
pub struct FlashSlot(Arc<AtomicBool>);

impl Drop for FlashSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Claim the single flash slot; false if a flash is already running.
pub fn try_begin(flag: &AtomicBool) -> bool {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
}

fn stage_name(p: Progress) -> &'static str {
    match p {
        Progress::Preflight => "preflight",
        Progress::Bootloader => "bootloader",
        Progress::Erase => "erase",
        Progress::Block { .. } => "block",
        Progress::Finalize => "finalize",
        Progress::AppRestart => "app_restart",
        Progress::PostCheck => "post_check",
    }
}

pub fn progress_event(p: Progress) -> ProgressEvent {
    let (block, total) = match p {
        Progress::Block { index, total } => (Some(index), Some(total)),
        _ => (None, None),
    };
    ProgressEvent { stage: stage_name(p), block, total }
}

/// Whether the flash had already reached W F E (from then on the old application is gone).
pub fn written(last: Option<Progress>) -> bool {
    !matches!(last, None | Some(Progress::Preflight) | Some(Progress::Bootloader))
}

/// `YYYYMMDD-HHMMSSZ` in UTC (civil-from-days, H. Hinnant), without a date crate.
pub fn utc_stamp(unix_s: u64) -> String {
    let days = (unix_s / 86_400) as i64;
    let secs = unix_s % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

/// `<UTC>_<installation>_pack<addr>_<what>.log`; without an installation (a check on a port of none) the name of
/// earlier versions, `<UTC>_pack<addr>_<what>.log`.
pub fn log_name(unix_s: u64, installation: Option<&str>, address: u8, what: &str) -> String {
    let clean = |s: &str| -> String { s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect() };
    match installation {
        Some(i) => format!("{}_{}_pack{address}_{}.log", utc_stamp(unix_s), clean(i), clean(what)),
        None => format!("{}_pack{address}_{}.log", utc_stamp(unix_s), clean(what)),
    }
}

/// Parse and authorize an image: allowlisted vendor releases, or any image that passes the strict
/// format and header checks when custom firmware is allowed.
pub fn image_info(bytes: &[u8], allow_custom: bool) -> openbms_flash::Result<(ehex::Image, ImageInfo)> {
    let image = ehex::parse(bytes)?;
    let release = match known::identify(&image) {
        Ok(r) => Some(r.name),
        Err(_) if allow_custom => None,
        Err(e) => return Err(e),
    };
    let (kind, known_build) = known::kind(&image);
    let kind = match kind {
        known::ImageKind::Original => "original",
        known::ImageKind::Maintenance => "maintenance",
        known::ImageKind::Custom => "custom",
    };
    let blocks = session::dry_run(&image)?.len() - 3;
    let info = ImageInfo {
        release: release.or(known_build).unwrap_or("custom").into(),
        kind,
        custom: release.is_none(),
        device: image.header.device.clone(),
        bytes: image.code.len(),
        blocks,
        sha256: image.code_sha256.clone(),
    };
    Ok((image, info))
}

fn confirmed_image(info: &ImageInfo, expected_sha256: &str, l: Lang) -> Result<(), String> {
    if info.sha256 != expected_sha256 {
        return Err(l.pick(
            "Firmware-Datei wurde seit der Prüfung geändert; bitte erneut auswählen",
            "firmware image changed since validation; choose it again",
        ).into());
    }
    Ok(())
}

fn read_image(path: &str, allow_custom: bool, l: Lang) -> Result<(ehex::Image, ImageInfo), String> {
    if !path.to_ascii_lowercase().ends_with(".ehex") {
        return Err(l.pick("Erwartet wird eine .ehex-Datei", "expected a .ehex file").into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    image_info(&bytes, allow_custom).map_err(|e| e.text(l))
}

pub fn logs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("firmware-logs"))
}

/// A new log file, indexed for its installation (`logs`); the index row's id for its result.
fn new_log(app: &AppHandle, state: &AppState, installation: Option<&str>, address: u8, kind: &str, release: Option<&str>) -> Result<(File, PathBuf, Option<i64>), String> {
    let dir = logs_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let name = log_name(now, installation, address, release.unwrap_or(kind));
    let path = dir.join(&name);
    let file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let id = installation.and_then(|i| {
        maint::with_store(state, |s| s.add_log(Some(i), Some(address), kind, release, &name, crate::history::now_ms()))
            .map_err(|e| eprintln!("log index: {e:?}"))
            .ok()
    });
    Ok((file, path, id))
}

/// The pack bus port by its endpoint key (`maint::pack_key`): USB at 19200 baud or a network gateway.
fn open_port(key: &str, l: Lang) -> Result<Connection, String> {
    let ep = maint::pack_endpoint(key).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    let mut c = Connection::open(&ep).map_err(|e| openbms_flash::Error::from(e).text(l))?;
    c.set_label(crate::capture::label(&ep, openbms_transport::Bus::PackBus));
    Ok(c)
}

#[tauri::command]
pub async fn firmware_check_image(path: String, allow_custom: bool, lang: String) -> Result<ImageInfo, String> {
    read_image(&path, allow_custom, self::lang(&lang)).map(|(_, info)| info)
}

/// Stops the monitor, then checks the target on the pack bus without resetting it. A pack that answers with the
/// String 2 stored in its pending record is no longer pending in its bootloader.
#[tauri::command]
pub async fn firmware_inspect(app: AppHandle, state: State<'_, AppState>, port: String, address: u8, lang: String, installation: Option<String>) -> Result<DeviceInfo, String> {
    let l = self::lang(&lang);
    let _port_slot = crate::control::reserve_bus_wait(&state)
        .ok_or_else(|| l.pick("Der Port wird bereits verwendet", "the port is busy with another operation").to_string())?;
    if state.flashing.load(Ordering::SeqCst) {
        return Err(l.pick("Ein Firmware-Update läuft", "a firmware update is running").into());
    }
    let key = maint::pack_key(&port).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    stop_monitor(&state)?;
    let mut link = open_port(&key, l)?;
    let (mut log, _, _) = new_log(&app, &state, installation.as_deref(), address, "inspect", None)?;
    let info = session::inspect(&mut link, address, &mut log, session::Options::default()).map_err(|e| e.text(l))?;
    // only the board that was flashed (its String 2 in the record) clears a pending record
    if let Ok(pending) = maint::with_store(&state, |s| s.pending_on(&key)) {
        let answered = maint::answered_with_serial(&pending, &[address], &mut link);
        let _ = maint::with_store(&state, |s| s.clear_answered(&key, &answered, crate::history::now_ms()));
    }
    Ok(info)
}

/// Before flashing an image with the patch functions: the target's Group0 now, from a fresh 0x47 on the pack bus (the
/// flash takes over the release bits as they are). Stops the monitor like the other pack bus steps; reads only while the
/// bus is quiet (a polling master would collide). None: not readable now (the UI shows the last snapshot with its age).
#[tauri::command]
pub async fn firmware_group0(app: AppHandle, state: State<'_, AppState>, port: String, address: u8) -> Result<Option<u8>, String> {
    if state.flashing.load(Ordering::SeqCst) {
        return Err(FLASHING_TEXT.into());
    }
    let _port_slot = crate::control::reserve_bus_wait(&state).ok_or("the port is busy with another operation")?;
    let key = maint::pack_key(&port).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    let mut link = maint::open_pack_for(&state, &app, &key).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    let quiet = bus::listen(&mut link, &SystemClock::new(), maint::LISTEN).map(|t| t.quiet()).unwrap_or(false);
    Ok(if quiet { link.parameters(address).ok().map(|p| p.function_switches[0]) } else { None })
}

const FLASHING_TEXT: &str = "a firmware update is running";

/// Validates everything again, then the kept checks before the restart (design: simple flash flow): no other pack
/// pending in its bootloader, on any endpoint (`maint::pending_refusal`), and with `checks` (the installation's own
/// RS485-1/2 port) a patched master with its marker on is released before a non-maintenance image (C'), the master's
/// serial number matches (right bank) and the target's String 2 is the board stored for its address (when both are
/// known). Quiet bus and the target's 0x51/0x47 are the flash's own preflight. A flash belongs to an installation; its
/// log is named and indexed with it on the installation's own port (`checks`), else „ohne Anlage“.
/// Flashes on its own thread and returns at once. Refusals reach the UI as a JSON string of `MaintError`.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // each argument is one value from the UI
pub async fn firmware_start(app: AppHandle, state: State<'_, AppState>, port: String, address: u8, path: String, allow_custom: bool, expected_sha256: String, confirmation: String, lang: String, installation: String, checks: bool) -> Result<FlashStarted, String> {
    let l = self::lang(&lang);
    if address > 15 {
        return Err(openbms_flash::Error::Address(address).text(l));
    }
    // Re-read the file and bind the confirmation to the exact code hash shown in the UI.
    let (image, info) = read_image(&path, allow_custom, l)?;
    confirmed_image(&info, &expected_sha256, l)?;
    if !confirmation_ok(address, info.custom, &confirmation) {
        return Err(openbms_flash::Error::Cancelled.text(l));
    }
    let port_slot = crate::control::reserve_bus_wait(&state)
        .ok_or_else(|| l.pick("Der Port wird bereits verwendet", "the port is busy with another operation").to_string())?;
    let _action = maint::begin_action(&state, "flash");
    let to_text = |e: maint::MaintError| serde_json::to_string(&e).unwrap_or_default();
    // the endpoint key as the UI formed it: the pending record below is written and checked under this one key
    let key = maint::pack_key(&port).map_err(to_text)?;
    maint::pending_check(&state.maint, &key, address).map_err(to_text)?;
    let owner = maint::installation(&state, &installation).map_err(to_text)?;
    let inst = checks.then(|| owner.clone());
    // C' belongs to the installation's master, not to the pack bus endpoint chosen: also a manual endpoint (another
    // gateway channel or adapter on the same bus) gets the check
    let fresh = if address == 0 && owner.control.is_some() { maint::read_master(&state, &owner).ok() } else { None };
    let last_on = address == 0 && maint::with_store(&state, |s| s.marker_last_on(&owner.id)).map_err(to_text)?;
    let marker = master_marker_check(address, maint_kind(info.kind), fresh.as_ref(), last_on).map_err(|k| to_text(maint::refused(k)))?;
    if !try_begin(&state.flashing) {
        return Err(l.pick("Ein Firmware-Update läuft bereits", "a firmware update is already running").into());
    }
    let slot = FlashSlot(state.flashing.clone());
    stop_monitor(&state)?;
    let mut link = open_port(&key, l)?;
    let bank = match &inst {
        Some(i) => {
            // traffic refuses at once: no 0xA4 checks skipped for a bus that is quiet only by the preflight
            quiet_first(&bus::listen(&mut link, &SystemClock::new(), maint::LISTEN).map_err(|e| e.text(l))?).map_err(|e| e.text(l))?;
            match maint::check_bank(&state.maint, i, &mut link).map_err(to_text)? {
                maint::Bank::Mismatch(serial) => return Err(to_text(maint::MaintError::OtherBank { serial })),
                maint::Bank::Match => "match",
                maint::Bank::Stored(_) => "stored",
                maint::Bank::Unchecked => "unchecked",
            }
        }
        None => "none",
    };
    // the target itself, also on another port than the installation's (manual tab): another board than the one stored
    // for its address is not flashed. Its String 2 goes with the pending record: only that board's answer clears it.
    let target = maint::check_target_board(&state.maint, &owner, address, &mut link).map_err(to_text)?;
    // the pending pack again: only as the same board
    let pending_now = maint::with_store(&state, |s| s.all_pending()).map_err(to_text)?;
    maint::pending_same_board(&pending_now, &key, address, target.as_deref()).map_err(to_text)?;
    let (mut log, log_path, log_id) = new_log(&app, &state, checks.then_some(installation.as_str()), address, "flash", Some(&info.release))?;
    writeln!(log, "CONFIRMED target={address} code_sha256={} release={} custom={} source=app bank={bank}", info.sha256, info.release, info.custom).map_err(|e| e.to_string())?;
    // written before the restart, so a crash or power loss leaves it behind; in the same transaction the place's version
    // proof ends (also for a flash that breaks off): either both or neither, so a failed step leaves no pending record.
    // Only a finished flash of an image with the patch functions sets the proof again, for the String 2 read back.
    let pending = maint::with_store(&state, |s| s.add_pending_with(&key, address, &info.release, target.as_deref(), crate::history::now_ms(), |s| s.clear_patch_proof(&owner.id, address))).map_err(to_text)?;
    maint::event(&state, Some(&installation), &format!("flash start address={address} release={} custom={} bank={bank} marker={} log={}", info.release, info.custom, marker.unwrap_or("-"), log_path.display()));
    let patch_image = known::patch_image(&info.sha256);
    let owner_id = owner.id.clone();
    let store = state.maint.clone();
    let stored = target.clone();
    std::thread::spawn(move || {
        let mut last = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session::flash_with_progress(&mut link, address, &image, &mut log, session::Options::default(), &mut |p| {
                last = Some(p);
                let _ = app.emit("firmware-progress", progress_event(p));
            })
        }));
        // a panic inside the flash counts as a failure at the last reported stage
        let ok = matches!(result, Ok(Ok(_)));
        // before the port is closed: a failed flash asks the pack for 0x51/0x47 on this link
        let pending = maint::settle_pending(&store, pending, address, stored.as_deref(), ok, last, &mut link);
        if let (true, Some(release)) = (ok, patch_image) {
            if let Some(serial) = proof_serial(stored.as_deref(), maint::string2(&mut link, address)) {
                let _ = maint::with_store_arc(&store, |s| s.set_patch_proof(&owner_id, address, &serial, release, "flash", crate::history::now_ms()));
            }
        }
        drop(link);
        let log = log_path.display().to_string();
        let done = match result {
            Ok(Ok(r)) => FirmwareDone { ok: true, error: None, stage: None, written: true, before: Some(r.before), after: Some(r.after), log, pending, log_error: r.log_error },
            Ok(Err(e)) => FirmwareDone { ok: false, error: Some(e.text(l)), stage: last.map(stage_name), written: written(last), before: None, after: None, log, pending, log_error: None },
            Err(_) => FirmwareDone { ok: false, error: Some(l.pick("Interner Fehler", "internal error").into()), stage: last.map(stage_name), written: written(last), before: None, after: None, log, pending, log_error: None },
        };
        let state = app.state::<AppState>();
        maint::event(&state, Some(&installation), &format!("flash done address={address} ok={} stage={:?} error={:?} log_error={:?}", done.ok, done.stage, done.error, done.log_error));
        if let Some(id) = log_id {
            let result = if done.ok { "ok" } else if done.pending { "pending" } else { "failed" };
            let _ = maint::with_store_arc(&store, |s| s.set_log_result(id, result));
        }
        drop(slot);
        drop(port_slot);
        let _ = app.emit("firmware-done", done);
    });
    Ok(FlashStarted { bank, marker })
}

/// The String 2 a version proof after a finished patched flash is bound to: the one read back, and only if it is the
/// board read before the flash (when that was read).
pub fn proof_serial(before: Option<&str>, after: Option<String>) -> Option<String> {
    after.filter(|a| before.is_none_or(|b| b == a))
}

/// The first listen of a flash with checks: any traffic refuses, before any 0xA4.
pub fn quiet_first(t: &bus::Traffic) -> openbms_flash::Result<()> {
    if t.quiet() { Ok(()) } else { Err(openbms_flash::Error::BusBusy(t.bytes)) }
}

fn maint_kind(kind: &str) -> known::ImageKind {
    match kind {
        "original" => known::ImageKind::Original,
        "maintenance" => known::ImageKind::Maintenance,
        _ => known::ImageKind::Custom,
    }
}

/// Show a firmware log (or the log folder) in the file manager.
#[tauri::command]
pub fn reveal_firmware_logs(app: AppHandle, path: Option<String>) -> Result<(), String> {
    reveal(&logs_dir(&app)?, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn confirmation_requires_exact_phrase() {
        assert!(confirmation_ok(3, false, "FLASH 3"));
        assert!(confirmation_ok(3, false, "  FLASH 3 "));
        assert!(!confirmation_ok(3, false, "FLASH 4"));
        assert!(!confirmation_ok(3, false, "flash 3"));
        assert!(!confirmation_ok(3, false, "FLASH 3 now"));
        assert!(!confirmation_ok(3, false, ""));
    }

    #[test]
    fn custom_image_needs_its_own_phrase() {
        assert!(confirmation_ok(3, true, "FLASH CUSTOM 3"));
        assert!(!confirmation_ok(3, true, "FLASH 3"));
        assert!(!confirmation_ok(3, false, "FLASH CUSTOM 3"));
        assert!(!confirmation_ok(3, true, "FLASH CUSTOM 4"));
    }

    #[test]
    fn log_name_is_a_plain_file_name() {
        // 2026-01-02 03:04:05 UTC
        let name = log_name(1_767_323_045, None, 7, "16.06.05 SP80");
        assert_eq!(name, "20260102-030405Z_pack7_16.06.05_SP80.log");
        assert!(!name.contains(['/', '\\', ':', ' ']));
        // with its installation, whose id cannot leave the folder either
        assert_eq!(log_name(1_767_323_045, Some("inst-1a/../x"), 0, "inspect"), "20260102-030405Z_inst-1a_.._x_pack0_inspect.log");
    }

    #[test]
    fn utc_stamp_handles_leap_day() {
        // 2024-02-29 23:59:59 UTC
        assert_eq!(utc_stamp(1_709_251_199), "20240229-235959Z");
    }

    #[test]
    fn the_master_marker_is_checked_for_the_installation_whatever_endpoint() {
        use openbms_flash::maint::policy::Patch;
        use openbms_proto::{MaintInfo, MaintState};
        let info = MaintInfo { format: 2, base: [16, 6, 13], patch: [1, 5], build_id: [0x90, 0x9A, 0x60, 0x4E, 0x60, 0xE9, 0x22, 0x5B] };
        let with = |word: u32| Master::Patched { patch: Patch::recognise(info).unwrap(), state: Some(MaintState { marker_word: word, effective_slaves: 0 }) };
        let (on, off) = (with(0), with(u32::MAX));
        let (orig, maint) = (known::ImageKind::Original, known::ImageKind::Maintenance);
        // fresh read: the marker decides
        assert_eq!(master_marker_check(0, orig, Some(&on), false), Err("release-master-first"));
        assert_eq!(master_marker_check(0, maint, Some(&on), true), Ok(Some("checked")));
        assert_eq!(master_marker_check(0, orig, Some(&off), true), Ok(Some("checked")));
        assert_eq!(master_marker_check(0, orig, Some(&Master::No0x64), false), Ok(Some("checked")));
        // no control channel or no answer: the marker last seen on refuses, else the flash goes on marked unchecked
        for fresh in [None, Some(&Master::Unknown)] {
            assert_eq!(master_marker_check(0, orig, fresh, true), Err("release-master-first-unread"));
            assert_eq!(master_marker_check(0, known::ImageKind::Custom, fresh, true), Err("release-master-first-unread"));
            assert_eq!(master_marker_check(0, orig, fresh, false), Ok(Some("unchecked")));
            assert_eq!(master_marker_check(0, maint, fresh, true), Ok(Some("unchecked")));
        }
        // another address: no marker question
        assert_eq!(master_marker_check(3, orig, Some(&on), true), Ok(None));
    }

    #[test]
    fn a_proof_after_a_patched_flash_needs_the_same_board_read_back() {
        assert_eq!(proof_serial(Some("SYN-3"), Some("SYN-3".into())).as_deref(), Some("SYN-3"));
        assert_eq!(proof_serial(None, Some("SYN-3".into())).as_deref(), Some("SYN-3"));
        assert_eq!(proof_serial(Some("SYN-3"), Some("SYN-B".into())), None);
        assert_eq!(proof_serial(Some("SYN-3"), None), None);
        // only images with the patch functions set it
        assert_eq!(known::patch_image("5cd0f08fdbed8adce2a340b8ca842a02f4f2de61eee3f6bdb5b268fa5ec78c12"), Some("16.06.13 MAINT 1.5"));
        assert_eq!(known::patch_image(known::RELEASES[5].code_sha256), None);
    }

    #[test]
    fn traffic_in_the_first_listen_refuses_the_flash() {
        let mut t = bus::Traffic::default();
        assert!(quiet_first(&t).is_ok());
        t.bytes = 7;
        assert!(matches!(quiet_first(&t), Err(openbms_flash::Error::BusBusy(7))));
    }

    #[test]
    fn try_begin_allows_only_one_flash() {
        let flag = AtomicBool::new(false);
        assert!(try_begin(&flag));
        assert!(!try_begin(&flag));
        flag.store(false, Ordering::SeqCst);
        assert!(try_begin(&flag));
    }

    #[test]
    fn flash_slot_clears_flag_on_drop_and_panic() {
        let flag = Arc::new(AtomicBool::new(true));
        drop(FlashSlot(flag.clone()));
        assert!(!flag.load(Ordering::SeqCst));
        flag.store(true, Ordering::SeqCst);
        let slot = FlashSlot(flag.clone());
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _slot = slot;
            panic!("boom");
        }));
        assert!(r.is_err());
        assert!(!flag.load(Ordering::SeqCst));
    }

    #[test]
    fn progress_events_name_the_stage() {
        let e = progress_event(Progress::Block { index: 4, total: 209 });
        assert_eq!((e.stage, e.block, e.total), ("block", Some(4), Some(209)));
        assert_eq!(progress_event(Progress::AppRestart).stage, "app_restart");
        assert!(written(Some(Progress::Erase)));
        assert!(!written(Some(Progress::Bootloader)));
        assert!(!written(None));
    }

    #[test]
    fn image_info_rejects_unknown_code_hash() {
        // structurally valid image (same builder as crates/openbms-flash/tests/protocol.rs), hash not allowlisted
        fn record(kind: u8, data: &[u8], header: bool) -> String {
            let mut b = vec![data.len() as u8, 0, 0, kind];
            b.extend_from_slice(data);
            let sum = if header { 0xaa } else { 0u8.wrapping_sub(b.iter().fold(0u8, |s, x| s.wrapping_add(*x))) };
            b.push(sum);
            format!(":{}", b.iter().map(|x| format!("{x:02X}")).collect::<String>())
        }
        let mut lines = Vec::new();
        for text in ["SWM181RCT6_64_M0", "EMU1101_10D_HW16", "RS485Y19200N9600"] {
            lines.push(record(0, text.as_bytes(), true));
        }
        for _ in 3..16 {
            lines.push(record(0, &[0x55; 16], true));
        }
        let mut code = [0u8; 16];
        code[0..4].copy_from_slice(&0x2000_1000u32.to_le_bytes());
        code[4..8].copy_from_slice(&0x0000_00c1u32.to_le_bytes());
        lines.push(record(0, &code, false));
        lines.push(record(5, &[0, 0, 0, 0xc1], false));
        lines.push(record(1, &[], false));
        let bytes = lines.join("\n").into_bytes();
        let err = image_info(&bytes, false).err().unwrap();
        assert!(err.to_string().contains("unknown or unsupported"));
        // with custom firmware allowed, the same image passes the format checks and is marked
        let (_, info) = image_info(&bytes, true).unwrap();
        assert!(info.custom);
        assert_eq!(info.release, "custom");
        assert_eq!(info.kind, "custom");
        assert_eq!(info.device, "EMU1101_10D_HW16");
        assert!(confirmed_image(&info, &info.sha256, Lang::En).is_ok());
        assert!(confirmed_image(&info, &"0".repeat(64), Lang::En).is_err());
    }
}
