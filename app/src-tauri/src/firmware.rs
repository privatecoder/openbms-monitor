//! Firmware update from the app: a guarded wrapper around `openbms-flash`. The app's image policy is
//! the vendor allowlist; other images only after an explicit opt-in and their own confirmation phrase.
//! The flash runs on its own thread and reports progress as events.

use crate::maint_store::endpoint_key;
use crate::{maint, reserve_port_wait, reveal, stop_monitor, AppState};
use openbms_flash::maint::policy::{self, Master};
use openbms_flash::maint::{bus, SystemClock};
use openbms_flash::{ehex, known, session, session::Progress, Lang};
use openbms_proto::DeviceInfo;
use openbms_transport::{Connection, Endpoint};
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

const PACK_BUS_BAUD: u32 = 19_200;

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
}

/// What `firmware_start` checked before it began: the right-bank check ("match" | "stored" | "unchecked" |
/// "none" without an installation).
#[derive(Serialize, Clone)]
pub struct FlashStarted {
    bank: &'static str,
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
        release: release.or(known_build.map(|r| r.name)).unwrap_or("custom").into(),
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

fn open_port(port: &str, l: Lang) -> Result<Connection, String> {
    Connection::open(&Endpoint::Serial { path: port.into(), baud: PACK_BUS_BAUD })
        .map_err(|e| openbms_flash::Error::from(e).text(l))
}

#[tauri::command]
pub async fn firmware_check_image(path: String, allow_custom: bool, lang: String) -> Result<ImageInfo, String> {
    read_image(&path, allow_custom, self::lang(&lang)).map(|(_, info)| info)
}

/// Stops the monitor, then checks the target on the pack bus without resetting it. A pack that answers is no
/// longer pending in its bootloader.
#[tauri::command]
pub async fn firmware_inspect(app: AppHandle, state: State<'_, AppState>, port: String, address: u8, lang: String, installation: Option<String>) -> Result<DeviceInfo, String> {
    let l = self::lang(&lang);
    let _port_slot = reserve_port_wait(&state.port_busy)
        .ok_or_else(|| l.pick("Der Port wird bereits verwendet", "the port is busy with another operation").to_string())?;
    if state.flashing.load(Ordering::SeqCst) {
        return Err(l.pick("Ein Firmware-Update läuft", "a firmware update is running").into());
    }
    stop_monitor(&state)?;
    let mut link = open_port(&port, l)?;
    let (mut log, _, _) = new_log(&app, &state, installation.as_deref(), address, "inspect", None)?;
    let info = session::inspect(&mut link, address, &mut log, session::Options::default()).map_err(|e| e.text(l))?;
    let key = endpoint_key(false, &port);
    let _ = maint::with_store(&state, |s| s.clear_answered(&key, &[address], crate::history::now_ms()));
    Ok(info)
}

/// Validates everything again, then the kept checks before the restart (design: simple flash flow): no other pack
/// of this pack bus endpoint pending in its bootloader, and with `checks` (the installation's own RS485-1/2 port) a
/// patched master with its marker on is released before a non-maintenance image (C') and the master's serial number
/// matches (right bank). Quiet bus and the target's 0x51/0x47 are the flash's own preflight. A flash belongs to an
/// installation; its log is named and indexed with it on the installation's own port (`checks`), else „ohne Anlage“.
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
    let port_slot = reserve_port_wait(&state.port_busy)
        .ok_or_else(|| l.pick("Der Port wird bereits verwendet", "the port is busy with another operation").to_string())?;
    let _action = maint::begin_action(&state, "flash");
    let to_text = |e: maint::MaintError| serde_json::to_string(&e).unwrap_or_default();
    let key = endpoint_key(false, &port);
    maint::with_store(&state, |s| s.pending_on(&key)).and_then(|p| maint::pending_refusal(&p, address)).map_err(to_text)?;
    let owner = maint::installation(&state, &installation).map_err(to_text)?;
    let inst = checks.then_some(owner);
    if let Some(i) = inst.as_ref().filter(|i| address == 0 && i.control.is_some()) {
        let master = maint::read_master(&state, i).unwrap_or(Master::Unknown);
        let kind = maint_kind(info.kind);
        if policy::release_first(&master, kind) {
            return Err(to_text(maint::refused("release-master-first")));
        }
        // no fresh read: the marker last seen on still counts
        if master == Master::Unknown && kind != known::ImageKind::Maintenance && maint::with_store(&state, |s| s.marker_last_on(&i.id)).map_err(to_text)? {
            return Err(to_text(maint::refused("release-master-first-unread")));
        }
    }
    if !try_begin(&state.flashing) {
        return Err(l.pick("Ein Firmware-Update läuft bereits", "a firmware update is already running").into());
    }
    let slot = FlashSlot(state.flashing.clone());
    stop_monitor(&state)?;
    let mut link = open_port(&port, l)?;
    let bank = match &inst {
        // a bus with traffic gets no 0xA4: the preflight refuses it anyway
        Some(_) if !bus::listen(&mut link, &SystemClock::new(), maint::LISTEN).map_err(|e| e.text(l))?.quiet() => "unchecked",
        Some(i) => match maint::check_bank(&state.maint, i, &mut link).map_err(to_text)? {
            maint::Bank::Mismatch(serial) => return Err(to_text(maint::MaintError::OtherBank { serial })),
            maint::Bank::Match => "match",
            maint::Bank::Stored(_) => "stored",
            maint::Bank::Unchecked => "unchecked",
        },
        None => "none",
    };
    let (mut log, log_path, log_id) = new_log(&app, &state, checks.then_some(installation.as_str()), address, "flash", Some(&info.release))?;
    writeln!(log, "CONFIRMED target={address} code_sha256={} release={} custom={} source=app bank={bank}", info.sha256, info.release, info.custom).map_err(|e| e.to_string())?;
    // written before the restart, so a crash or power loss leaves it behind
    let pending = maint::with_store(&state, |s| s.add_pending(&key, address, &info.release, crate::history::now_ms())).map_err(to_text)?;
    maint::event(&state, Some(&installation), &format!("flash start address={address} release={} custom={} bank={bank} log={}", info.release, info.custom, log_path.display()));
    let store = state.maint.clone();
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
        let pending = maint::settle_pending(&store, pending, address, ok, last, &mut link);
        drop(link);
        let log = log_path.display().to_string();
        let done = match result {
            Ok(Ok(r)) => FirmwareDone { ok: true, error: None, stage: None, written: true, before: Some(r.before), after: Some(r.after), log, pending },
            Ok(Err(e)) => FirmwareDone { ok: false, error: Some(e.text(l)), stage: last.map(stage_name), written: written(last), before: None, after: None, log, pending },
            Err(_) => FirmwareDone { ok: false, error: Some(l.pick("Interner Fehler", "internal error").into()), stage: last.map(stage_name), written: written(last), before: None, after: None, log, pending },
        };
        let state = app.state::<AppState>();
        maint::event(&state, Some(&installation), &format!("flash done address={address} ok={} stage={:?} error={:?}", done.ok, done.stage, done.error));
        if let Some(id) = log_id {
            let result = if done.ok { "ok" } else if done.pending { "pending" } else { "failed" };
            let _ = maint::with_store_arc(&store, |s| s.set_log_result(id, result));
        }
        drop(slot);
        drop(port_slot);
        let _ = app.emit("firmware-done", done);
    });
    Ok(FlashStarted { bank })
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
