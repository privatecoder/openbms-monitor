//! Firmware update from the app: a guarded wrapper around `openbms-flash`. The app's image policy is
//! the vendor allowlist; other images only after an explicit opt-in and their own confirmation phrase.
//! The flash runs on its own thread and reports progress as events.

use crate::{flash_check, maint, reveal, stop_monitor_and_tell, AppState};
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
    /// a listed patched build with power and clock over the CAN socket (lock Group0 bit 3, taken over unchanged)
    power_rtc: bool,
    /// a listed patched build with the stored pack cutoff (Group0 bit 4 extended off, bit 5 the lock, taken over unchanged)
    pack_cutoff: bool,
    /// a listed patched build whose Group0 bit 3 at 1 allows power and clock (image flag bit 3: every EMU1101-EXT)
    positive: bool,
    /// a listed patched build: its OEM base ("16.06.13"), which its name no longer carries; the 0x51 answer after the
    /// flash is checked against it
    base: Option<String>,
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
    /// the target before and once back (`flash_check`), only after a finished flash
    comparison: Option<crate::flash_check::Comparison>,
    /// a recovery (`firmware_recovery_start`): the DIP address the restarted application answered at; None for a flash,
    /// and for a recovery whose application did not show (transfer confirmed, application unchecked)
    recovered_at: Option<u8>,
    /// true for the result of a recovery
    recovery: bool,
}

/// What `firmware_start` checked before it began: the right-bank check ("match" | "stored" | "unchecked" |
/// "none" without an installation).
#[derive(Serialize, Clone)]
pub struct FlashStarted {
    bank: &'static str,
    /// address 0: "checked" (a fresh control channel read decided C'), "unchecked" (no fresh read; the marker last seen
    /// was not on, the flash goes on with that hint), "latent" (the current image has no 0x64 and the marker was last
    /// seen on: the patched image starts with it); None for another address
    #[serde(skip_serializing_if = "Option::is_none")]
    marker: Option<&'static str>,
}

/// What a flash of the master needs from its marker (C'), for the installation whatever pack bus endpoint is used: a fresh
/// read of the installation's control channel (`fresh`: None without a control channel or when the read failed) decides
/// only with a valid marker state; a patched master without one (lost or foreign `02`), an unknown patch or no read falls
/// back to the marker last reliably seen (on refuses), else the flash goes on "unchecked". A master without 0x64 runs a
/// firmware that ignores the marker: nothing to release. Err: the refusal key; Ok: "checked" or "unchecked".
pub fn master_marker_check(address: u8, image: known::ImageKind, fresh: Option<&Master>, last_on: bool) -> Result<Option<&'static str>, &'static str> {
    use openbms_proto::MarkerWord;
    if address != 0 {
        return Ok(None);
    }
    let marker = fresh.and_then(|m| if m.caps().is_some() { m.marker() } else { None });
    match (fresh, marker) {
        (Some(m), Some(MarkerWord::On)) if policy::release_first(m, image) => Err("release-master-first"),
        (Some(_), Some(MarkerWord::On | MarkerWord::Off)) => Ok(Some("checked")),
        // the current image ignores the marker: a marker kept on is latent. A patched image would start with it (no slave
        // polling): said explicitly; another image without 0x64 goes on unchecked
        (Some(Master::No0x64), _) if last_on && image == known::ImageKind::Maintenance => Ok(Some("latent")),
        (Some(Master::No0x64), _) => Ok(Some("unchecked")),
        _ if image != known::ImageKind::Maintenance && last_on => Err("release-master-first-unread"),
        _ => Ok(Some("unchecked")),
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

/// The phrase of a flash series (several packs, one confirmation): it names the count, the confirmed list is kept by
/// `series_begin` and every pack's start must name exactly that list. Custom images again have their own phrase.
pub fn series_phrase(count: usize, custom: bool) -> String {
    if custom { format!("FLASH CUSTOM {count} PACKS") } else { format!("FLASH {count} PACKS") }
}

pub fn series_confirmation_ok(count: usize, custom: bool, typed: &str) -> bool {
    typed.trim() == series_phrase(count, custom)
}

/// The order of a series: the slaves by address, the master (address 0) last, so the bus stays quiet for every slave.
/// None: fewer than two packs, an address twice or one that is no DIP address.
pub fn series_order(addresses: &[u8]) -> Option<Vec<u8>> {
    let mut order = addresses.to_vec();
    order.sort_by_key(|a| (*a == 0, *a));
    order.dedup();
    (order.len() == addresses.len() && order.len() >= 2 && order.iter().all(|a| openbms_proto::address::is_dip(*a))).then_some(order)
}

/// A confirmed flash series: one installation's pack bus, one image, the packs in their order, and its own run id. Each
/// pack is admitted only in turn and only after the one before finished cleanly (`SeriesSlot::after`); a refusal, a
/// failure, a pending record or a missing or deviating after-check ends it. Ended by the UI as well (done, cancelled,
/// link changed), always by its run id.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    run: u64,
    installation: String,
    key: String,
    sha256: String,
    custom: bool,
    order: Vec<u8>,
    /// the index in `order` of the next pack
    next: usize,
    /// the pack admitted now (from its admission, through its checks and flash, to its end)
    running: Option<u8>,
}

/// What a series pack's start names: its run and the list as confirmed.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct SeriesStart {
    pub run: u64,
    pub listed: Vec<u8>,
}

/// What `firmware_series_begin` returns: the run id every later call names, and the order.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct SeriesBegun {
    pub run: u64,
    pub order: Vec<u8>,
}

/// The one series slot. Every change names the run it belongs to: a late end, a drop guard or a completion of an
/// earlier run never touches a newer one. Admission and marking the pack as running are one step. A single flash or a
/// recovery enters here too (`admit_single`): it and a series exclude each other under this one lock.
#[derive(Debug, Default)]
pub struct SeriesSlot {
    current: Option<Series>,
    last_run: u64,
    /// a single flash or a recovery is admitted (from its series check to its end)
    single: bool,
}

impl SeriesSlot {
    pub const fn new() -> Self {
        Self { current: None, last_run: 0, single: false }
    }

    /// A new series replaces an idle one; while a pack of the current one is admitted or flashing, or a single flash or
    /// recovery is admitted, it is refused.
    pub fn begin(&mut self, installation: &str, key: &str, sha256: &str, custom: bool, order: Vec<u8>) -> Result<u64, &'static str> {
        if self.current.as_ref().is_some_and(|s| s.running.is_some()) {
            return Err("series-busy");
        }
        if self.single {
            return Err("flash-admitted");
        }
        self.last_run += 1;
        self.current = Some(Series { run: self.last_run, installation: installation.into(), key: key.into(), sha256: sha256.into(), custom, order, next: 0, running: None });
        Ok(self.last_run)
    }

    /// A series is open: a single flash or a recovery waits for its end.
    #[cfg(test)]
    pub fn open(&self) -> bool {
        self.current.is_some()
    }

    /// Admits a single flash or a recovery, in one step with the check: no series open, no other single one admitted.
    /// Released by `end_single` (the `SingleAdmitted` guard, at its end or refusal).
    pub fn admit_single(&mut self) -> Result<(), &'static str> {
        if self.current.is_some() {
            return Err("series-open");
        }
        if self.single {
            return Err("flash-admitted");
        }
        self.single = true;
        Ok(())
    }

    pub fn end_single(&mut self) {
        self.single = false;
    }

    /// Ends a series none of whose packs is admitted or flashing (one left open by a view that went away); no run id
    /// needed. Pending records and markers are not touched.
    pub fn end_idle(&mut self) -> Result<(), &'static str> {
        match &self.current {
            Some(s) if s.running.is_some() => Err("series-busy"),
            _ => {
                self.current = None;
                Ok(())
            }
        }
    }

    /// Admits `address` of run `run` and marks it running, in one step: the same installation, port and image, exactly
    /// the confirmed list (`listed`, in any order) and `address` the next pack in turn while none is admitted.
    #[allow(clippy::too_many_arguments)] // each argument is one bound value of the grant
    pub fn admit(&mut self, run: u64, installation: Option<&str>, key: &str, sha256: &str, custom: bool, listed: &[u8], address: u8) -> bool {
        let Some(s) = self.current.as_mut().filter(|s| s.run == run) else { return false };
        let ok = installation == Some(s.installation.as_str())
            && key == s.key
            && sha256 == s.sha256
            && custom == s.custom
            && series_order(listed).as_deref() == Some(s.order.as_slice())
            && s.running.is_none()
            && s.order.get(s.next) == Some(&address);
        if ok {
            s.running = Some(address);
        }
        ok
    }

    /// The flash of `address` of run `run` returned (or its start was refused, `clean` false): the series goes on only
    /// after a clean end of exactly the pack admitted in turn; anything else ends it, as does its last pack. Another
    /// run is not touched. True: the series goes on.
    pub fn after(&mut self, run: u64, address: u8, clean: bool) -> bool {
        let Some(s) = self.current.as_mut().filter(|s| s.run == run) else { return false };
        let in_turn = s.running == Some(address) && s.order.get(s.next) == Some(&address);
        if clean && in_turn && s.next + 1 < s.order.len() {
            s.running = None;
            s.next += 1;
            return true;
        }
        self.current = None;
        false
    }

    /// Ends run `run` (a later one stays).
    pub fn end(&mut self, run: u64) {
        if self.current.as_ref().is_some_and(|s| s.run == run) {
            self.current = None;
        }
    }
}

/// The series confirmed now (at most one; the flash slot keeps flashes one at a time anyway).
static SERIES: std::sync::Mutex<SeriesSlot> = std::sync::Mutex::new(SeriesSlot::new());

fn series_lock() -> std::sync::MutexGuard<'static, SeriesSlot> {
    SERIES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ends its run unless disarmed: every refusal of a series pack's start (any early return) ends that series.
struct SeriesEnds(Option<u64>);

impl Drop for SeriesEnds {
    fn drop(&mut self) {
        if let Some(run) = self.0 {
            series_lock().end(run);
        }
    }
}

/// A single flash or a recovery, admitted against the series slot (`SeriesSlot::admit_single`): released when dropped.
struct SingleAdmitted;

impl Drop for SingleAdmitted {
    fn drop(&mut self) {
        series_lock().end_single();
    }
}

/// A single flash or a recovery: refused while a series is open or another is admitted, else admitted until its end.
fn admit_single() -> Result<SingleAdmitted, maint::MaintError> {
    series_lock().admit_single().map(|()| SingleAdmitted).map_err(maint::refused)
}

/// Ends a series left open with none of its packs admitted (`SeriesSlot::end_idle`): the way out when a view went away
/// without ending its series.
#[tauri::command]
pub fn firmware_series_end_idle() -> Result<(), String> {
    series_lock().end_idle().map_err(|k| serde_json::to_string(&maint::refused(k)).unwrap_or_default())
}

/// A pack of a series answered cleanly with the new image: the flash finished, no pending record, the after-check
/// compared everything and found nothing changed, and the version it reports fits the image (an unknown custom image
/// has no known version: its answer is taken; a listed EMU1101-EXT build counts as custom but has its base).
pub fn series_clean(ok: bool, pending: bool, verdict: Option<&str>, after: Option<&str>, info: &ImageInfo) -> bool {
    ok && !pending && verdict == Some("same") && after.is_some_and(|v| info.kind == "custom" || version_fits(v, info))
}

/// 0x51 reports major.minor ("16.06"): it fits "16.06.13", not "16.06.1x" of another minor. An EMU1101-EXT name carries
/// no OEM base: the listed build's base stands for it.
fn version_fits(reported: &str, info: &ImageInfo) -> bool {
    let reference = if info.kind == "maintenance" { info.base.as_deref() } else { Some(info.release.as_str()) };
    reference.is_some_and(|r| r == reported || r.starts_with(&format!("{reported}.")) || r.starts_with(&format!("{reported} ")))
}

/// Starts a flash series: the image checked again, the typed phrase for exactly `addresses` (their count, the custom
/// phrase for a custom image), no flash running and no pending bootloader record anywhere. Keeps the confirmed list for
/// the installation's port and returns the order (slaves by address, the master last). Each pack is then started with
/// `firmware_start` and `series` = this list, with every check of a single flash.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // each argument is one value from the UI
pub async fn firmware_series_begin(state: State<'_, AppState>, port: String, installation: String, addresses: Vec<u8>, path: String, allow_custom: bool, expected_sha256: String, confirmation: String, lang: String) -> Result<SeriesBegun, String> {
    let l = self::lang(&lang);
    let to_text = |e: maint::MaintError| serde_json::to_string(&e).unwrap_or_default();
    let order = series_order(&addresses).ok_or_else(|| to_text(maint::refused("series-not-confirmed")))?;
    let (_, info) = read_image(&path, allow_custom, l)?;
    confirmed_image(&info, &expected_sha256, l)?;
    if !series_confirmation_ok(addresses.len(), info.custom, &confirmation) {
        return Err(openbms_flash::Error::Cancelled.text(l));
    }
    if state.flashing.load(Ordering::SeqCst) {
        return Err(l.pick("Ein Firmware-Update läuft bereits", "a firmware update is already running").into());
    }
    let key = maint::pack_key(&port).map_err(to_text)?;
    maint::installation(&state, &installation).map_err(to_text)?;
    series_pending(&state).map_err(to_text)?;
    let run = series_lock().begin(&installation, &key, &info.sha256, info.custom, order.clone()).map_err(|k| to_text(maint::refused(k)))?;
    maint::event(&state, Some(&installation), &format!("flash series {run} confirmed order={order:?} release={} custom={}", info.release, info.custom));
    Ok(SeriesBegun { run, order })
}

/// Ends the flash series `run` (done, cancelled, the link or installation changed): no further pack of it is admitted.
/// A later series is not touched.
#[tauri::command]
pub fn firmware_series_end(run: u64) {
    series_lock().end(run);
}

/// A series refuses with any pending bootloader record, on any endpoint (also of a picked pack: finishing that one is a
/// single flash).
fn series_pending(state: &AppState) -> Result<(), maint::MaintError> {
    series_pending_refusal(&maint::with_store(state, |s| s.all_pending())?)
}

pub fn series_pending_refusal(pending: &[crate::maint_store::Pending]) -> Result<(), maint::MaintError> {
    match pending.first() {
        Some(p) => Err(maint::MaintError::BootloaderPending { pack: p.pack, endpoint: p.endpoint.clone() }),
        None => Ok(()),
    }
}

/// Clears the flash flag on drop, so even a panic in the flash thread cannot leave the app locked.
pub struct FlashSlot(Arc<AtomicBool>);

impl Drop for FlashSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
        // a bus rule change given during the flash takes effect now (`wiring::settle`)
        crate::wiring::settle();
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
        power_rtc: known::patch_by_code(&image.code_sha256).is_some_and(|b| b.power_rtc()),
        pack_cutoff: known::patch_by_code(&image.code_sha256).is_some_and(|b| b.pack_cutoff()),
        positive: known::patch_by_code(&image.code_sha256).is_some_and(|b| b.info().positive()),
        base: known::patch_by_code(&image.code_sha256).map(|b| b.info().base_text()),
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

/// The pack bus port by its endpoint key (`maint::pack_key`): USB at the pack bus's baud rate or a network gateway.
fn open_port(key: &str, l: Lang) -> Result<Connection, String> {
    let ep = maint::pack_endpoint(key).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    crate::port::open(&ep, openbms_transport::Bus::PackBus).map_err(|e| openbms_flash::Error::from(e).text(l))
}

#[tauri::command]
pub async fn firmware_check_image(path: String, allow_custom: bool, lang: String) -> Result<ImageInfo, String> {
    read_image(&path, allow_custom, self::lang(&lang)).map(|(_, info)| info)
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
/// pending in its bootloader, on any endpoint (`maint::pending_refusal`). With an `installation` (the pack bus panel, on
/// its own RS485-1/2 port) a patched master with its marker on is released before a non-maintenance image (C'), the
/// master's serial number matches (right bank) and the target's String 2 is the board stored for its address (when both
/// are known). Without one („Platine flashen“) the board is known by its String 2 alone: the proof and hard-stop sighting
/// of every place where it is the current board end, and the board's marker is checked over RS485-1/2 where its build
/// shows it there (image flag bit 4) and cleared before an image without the maintenance functions; the board must be
/// the one the search showed (`expected`, its String 2). Quiet bus and the target's
/// 0x51/0x47 are the flash's own preflight. Flashes on its own thread and returns at once. Refusals reach the UI as a JSON
/// string of `MaintError`. `series`: a pack of a confirmed flash series (`firmware_series_begin`), the list as confirmed;
/// the series phrase confirms it, every check stays, and any refusal ends the series.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // each argument is one value from the UI
pub async fn firmware_start(app: AppHandle, state: State<'_, AppState>, port: String, address: u8, path: String, allow_custom: bool, expected_sha256: String, confirmation: String, lang: String, installation: Option<String>, expected: Option<String>, series: Option<SeriesStart>) -> Result<FlashStarted, String> {
    let l = self::lang(&lang);
    if !openbms_proto::address::is_dip(address) {
        return Err(openbms_flash::Error::Address(address).text(l));
    }
    // Re-read the file and bind the confirmation to the exact code hash shown in the UI.
    // a series pack's refusal, wherever it returns, ends the series
    let mut ends = SeriesEnds(series.as_ref().map(|s| s.run));
    let (image, info) = read_image(&path, allow_custom, l)?;
    confirmed_image(&info, &expected_sha256, l)?;
    let confirmed = match &series {
        Some(sr) => series_confirmation_ok(sr.listed.len(), info.custom, &confirmation),
        None => confirmation_ok(address, info.custom, &confirmation),
    };
    if !confirmed {
        return Err(openbms_flash::Error::Cancelled.text(l));
    }
    let to_text = |e: maint::MaintError| serde_json::to_string(&e).unwrap_or_default();
    let port_slot = crate::control::reserve_bus_wait(&state)
        .ok_or_else(|| l.pick("Der Port wird bereits verwendet", "the port is busy with another operation").to_string())?;
    let _action = maint::begin_action(&state, "flash");
    // the endpoint key as the UI formed it: the pending record below is written and checked under this one key
    let key = maint::pack_key(&port).map_err(to_text)?;
    // a series pack: admitted and marked running in one step, no other begin or start can come between
    if let Some(sr) = &series {
        if !series_lock().admit(sr.run, installation.as_deref(), &key, &info.sha256, info.custom, &sr.listed, address) {
            return Err(to_text(maint::refused("series-not-confirmed")));
        }
        // a series never passes a pending record, not even of its own pack
        series_pending(&state).map_err(to_text)?;
    }
    // a single flash: admitted against the series in one step, until its end
    let single = if series.is_none() { Some(admit_single().map_err(to_text)?) } else { None };
    maint::pending_check(&state.maint, &key, address).map_err(to_text)?;
    let inst = installation.as_deref().map(|id| maint::installation(&state, id)).transpose().map_err(to_text)?;
    let kind = maint_kind(info.kind);
    // C' belongs to the installation's master
    let marker = match &inst {
        Some(owner) => {
            let fresh = if address == 0 && owner.control.is_some() { maint::read_master(&state, owner).ok() } else { None };
            let last_on = address == 0 && maint::with_store(&state, |s| s.marker_last_on(&owner.id)).map_err(to_text)?;
            master_marker_check(address, kind, fresh.as_ref(), last_on).map_err(|k| to_text(maint::refused(k)))?
        }
        None => None,
    };
    if !try_begin(&state.flashing) {
        return Err(l.pick("Ein Firmware-Update läuft bereits", "a firmware update is already running").into());
    }
    let slot = FlashSlot(state.flashing.clone());
    // the pack's live 64/03 finding of this connection no longer holds once its flash begins (the stored proof ends with
    // the pending record below); the next CAN connect confirms or replaces the provisional proof
    crate::bank::clear_live(&state, crate::link_now(&state).gen, address);
    // the UI is told, so it shows "not connected" instead of counting a lost connection
    stop_monitor_and_tell(&state, &app)?;
    let mut link = open_port(&key, l)?;
    // `found_before`: the identity the board shows over RS485-1/2 before (image flag bit 4), for the comparison and
    // the bench marker; read on a quiet bus only
    let (bank, target, marker, found_before) = match &inst {
        Some(i) => {
            // traffic refuses at once: no 0xA4 checks skipped for a bus that is quiet only by the preflight
            quiet_first(&bus::listen(&mut link, &SystemClock::new(), maint::LISTEN).map_err(|e| e.text(l))?).map_err(|e| e.text(l))?;
            let bank = match maint::check_bank(&state.maint, i, &mut link).map_err(to_text)? {
                maint::Bank::Mismatch(serial) => return Err(to_text(maint::MaintError::OtherBank { serial })),
                maint::Bank::Match => "match",
                maint::Bank::Stored(_) => "stored",
                maint::Bank::Unchecked => "unchecked",
            };
            // another board than the one stored for its address is not flashed. Its String 2 goes with the pending
            // record: only that board's answer clears it.
            let target = maint::check_target_board(&state.maint, i, address, &mut link).map_err(to_text)?;
            (bank, target, marker, crate::bench::rs485_marker(&mut link, address))
        }
        None => {
            // the board shown at the search, before anything changes on it
            let target = maint::string2(&mut link, address);
            crate::bench::chosen_board(expected.as_deref(), target.as_deref()).map_err(to_text)?;
            let found = crate::bench::rs485_marker(&mut link, address);
            let m = crate::bench::bench_marker(&mut link, &found, address, kind).map_err(to_text)?;
            if m == "released" {
                maint::event(&state, None, &format!("bench flash: marker of pack {address} cleared over RS485-1/2"));
            }
            let marker = Some(m);
            ("none", target, marker, found)
        }
    };
    // the pending pack again: only as the same board
    let pending_now = maint::with_store(&state, |s| s.all_pending()).map_err(to_text)?;
    maint::pending_same_board(&pending_now, &key, address, target.as_deref()).map_err(to_text)?;
    // where the app knows this board: its place in the installation, else every place where it is the current board
    let places: Vec<(String, u8)> = match (&inst, target.as_deref()) {
        (Some(i), _) => vec![(i.id.clone(), address)],
        (None, Some(serial)) => maint::with_store(&state, |s| crate::bench::places_of(s, serial)).map_err(to_text)?,
        (None, None) => vec![],
    };
    let (mut log, log_path, log_id) = new_log(&app, &state, installation.as_deref(), address, "flash", Some(&info.release))?;
    writeln!(log, "CONFIRMED target={address} code_sha256={} release={} custom={} source=app bank={bank} string2={}", info.sha256, info.release, info.custom, target.as_deref().unwrap_or("-")).map_err(|e| e.to_string())?;
    // written before the restart, so a crash or power loss leaves it behind; in the same transaction the version proof
    // and the hard-stop observation of its places end (also for a flash that breaks off): all or nothing, so a failed
    // step leaves no pending record. Only a finished flash of an image with the patch functions sets the proof again,
    // for the String 2 read back.
    let pending = maint::with_store(&state, |s| s.add_pending_with(&key, address, &info.release, target.as_deref(), crate::history::now_ms(), |s| {
        for (inst, a) in &places {
            s.clear_patch_proof(inst, *a)?;
            // what the old image showed about the hard stop no longer holds
            s.clear_hard_stop(inst, *a)?;
        }
        Ok(())
    })).map_err(to_text)?;
    maint::event(&state, installation.as_deref(), &format!("flash start address={address} release={} custom={} bank={bank} marker={} places={} log={}", info.release, info.custom, marker.unwrap_or("-"), places.len(), log_path.display()));
    let patch_image = known::patch_image(&info.sha256);
    // the bit-3 direction of the image flashed (None: no patch functions)
    let positive_after = known::patch_by_code(&info.sha256)
        .filter(|b| b.caps().is_some())
        .map(|b| openbms_proto::access::positive_build(openbms_proto::access::Patch::Version(b.patch), Some(b.flags)));
    let store = state.maint.clone();
    let stored = target.clone();
    // a series pack: its end decides whether the next one may start
    let in_series = series.as_ref().map(|sr| (sr.run, info.clone()));
    // started: from here the flash's own end decides
    ends.0 = None;
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
        // after a finished flash: the board's String 2 and identity again, for the proof and the comparison
        let mut comparison = None;
        if let Ok(Ok(r)) = &result {
            let serial_after = maint::string2(&mut link, address);
            let found_after = crate::bench::rs485_marker(&mut link, address);
            if let Some(release) = patch_image {
                if let Some(serial) = proof_serial(stored.as_deref(), serial_after.clone()) {
                    for (inst, a) in &places {
                        let _ = maint::with_store_arc(&store, |s| s.set_patch_proof(inst, *a, &serial, release, "flash", crate::history::now_ms()));
                    }
                }
            }
            let before = flash_check::Side::new(stored.clone(), Some(&found_before), None, &r.state_before);
            let after = flash_check::Side::new(serial_after, Some(&found_after), positive_after, &r.state_after);
            let c = flash_check::compare(before, after, &r.state_before, &r.state_after);
            let _ = writeln!(log, "{}", flash_check::summary(&c));
            comparison = Some(c);
        }
        drop(link);
        let log = log_path.display().to_string();
        let done = match result {
            Ok(Ok(r)) => FirmwareDone { ok: true, error: None, stage: None, written: true, before: Some(r.before), after: Some(r.after), log, pending, log_error: r.log_error, comparison, recovered_at: None, recovery: false },
            Ok(Err(e)) => FirmwareDone { ok: false, error: Some(e.text(l)), stage: last.map(stage_name), written: written(last), before: None, after: None, log, pending, log_error: None, comparison: None, recovered_at: None, recovery: false },
            Err(_) => FirmwareDone { ok: false, error: Some(l.pick("Interner Fehler", "internal error").into()), stage: last.map(stage_name), written: written(last), before: None, after: None, log, pending, log_error: None, comparison: None, recovered_at: None, recovery: false },
        };
        let state = app.state::<AppState>();
        maint::event(&state, installation.as_deref(), &format!("flash done address={address} ok={} stage={:?} error={:?} log_error={:?} compare={}", done.ok, done.stage, done.error, done.log_error, done.comparison.as_ref().map_or("-", |c| c.verdict)));
        if let Some(id) = log_id {
            let result = if done.ok { "ok" } else if done.pending { "pending" } else { "failed" };
            let _ = maint::with_store_arc(&store, |s| s.set_log_result(id, result));
        }
        if let Some((run, info)) = &in_series {
            let clean = series_clean(done.ok, done.pending, done.comparison.as_ref().map(|c| c.verdict), done.after.as_ref().map(|a| a.firmware_version.as_str()), info);
            let goes_on = series_lock().after(*run, address, clean);
            maint::event(&state, installation.as_deref(), &format!("flash series {run} pack={address} clean={clean} goes_on={goes_on}"));
        }
        drop(single);
        drop(slot);
        drop(port_slot);
        let _ = app.emit("firmware-done", done);
    });
    Ok(FlashStarted { bank, marker })
}

/// The phrase a recovery is confirmed with (no pack address: the bootloader has none).
pub fn recovery_confirmation_ok(custom: bool, typed: &str) -> bool {
    typed.trim() == if custom { "RECOVER CUSTOM" } else { "RECOVER" }
}

/// Recovery, step 1: whether a bootloader already answers on the RS485-1/2 port `port` (`maint::pack_key`): one R F L,
/// no reset, no write. Ok: its packet size (256).
#[tauri::command]
pub async fn firmware_recovery_probe(app: AppHandle, state: State<'_, AppState>, port: String, lang: String) -> Result<u16, String> {
    let l = self::lang(&lang);
    if state.flashing.load(Ordering::SeqCst) {
        return Err(l.pick("Ein Firmware-Update läuft bereits", "a firmware update is already running").into());
    }
    let to_text = |e: maint::MaintError| serde_json::to_string(&e).unwrap_or_default();
    let _port_slot = crate::control::reserve_bus_wait(&state)
        .ok_or_else(|| l.pick("Der Port wird bereits verwendet", "the port is busy with another operation").to_string())?;
    let key = maint::pack_key(&port).map_err(to_text)?;
    stop_monitor_and_tell(&state, &app)?;
    let mut link = open_port(&key, l)?;
    tauri::async_runtime::spawn_blocking(move || session::probe_bootloader(&mut link, &mut std::io::sink(), session::Options::default()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.text(l))
}

/// Recovery, step 2: flash a board whose bootloader already answers (it has no application to restart), over the
/// RS485-1/2 port `port`. The operator has confirmed that exactly one board is on the line: the bootloader has no pack
/// address. The same image checks and progress/done events as a flash; afterwards the application is looked for at
/// every DIP address, and a pending record of that board at that address is cleared.
#[allow(clippy::too_many_arguments)] // each argument is one value from the UI
#[tauri::command]
pub async fn firmware_recovery_start(app: AppHandle, state: State<'_, AppState>, port: String, path: String, allow_custom: bool, expected_sha256: String, confirmation: String, lang: String) -> Result<(), String> {
    let l = self::lang(&lang);
    let (image, info) = read_image(&path, allow_custom, l)?;
    confirmed_image(&info, &expected_sha256, l)?;
    if !recovery_confirmation_ok(info.custom, &confirmation) {
        return Err(openbms_flash::Error::Cancelled.text(l));
    }
    let to_text = |e: maint::MaintError| serde_json::to_string(&e).unwrap_or_default();
    let port_slot = crate::control::reserve_bus_wait(&state)
        .ok_or_else(|| l.pick("Der Port wird bereits verwendet", "the port is busy with another operation").to_string())?;
    let _action = maint::begin_action(&state, "flash");
    let key = maint::pack_key(&port).map_err(to_text)?;
    // a recovery: admitted against the series in one step, until its end
    let single = admit_single().map_err(to_text)?;
    if !try_begin(&state.flashing) {
        return Err(l.pick("Ein Firmware-Update läuft bereits", "a firmware update is already running").into());
    }
    let slot = FlashSlot(state.flashing.clone());
    stop_monitor_and_tell(&state, &app)?;
    let mut link = open_port(&key, l)?;
    let (mut log, log_path, log_id) = new_log(&app, &state, None, 0, "recovery", Some(&info.release))?;
    writeln!(log, "CONFIRMED recovery code_sha256={} release={} custom={} source=app one_board_confirmed=true", info.sha256, info.release, info.custom).map_err(|e| e.to_string())?;
    // written before anything is sent, so a crash or a break after W F E leaves it: every normal flash is refused until
    // it is settled (a recovery itself is not)
    let record = maint::with_store(&state, |s| s.add_pending(&key, maint::RECOVERY_PACK, &info.release, None, crate::history::now_ms())).map_err(to_text)?;
    maint::event(&state, None, &format!("recovery start release={} custom={} log={}", info.release, info.custom, log_path.display()));
    let store = state.maint.clone();
    std::thread::spawn(move || {
        let mut last = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session::recover_with_progress(&mut link, &image, &mut log, session::Options::default(), &mut |p| {
                last = Some(p);
                let _ = app.emit("firmware-progress", progress_event(p));
            })
        }));
        // the application found: a pending record of exactly this board (by its String 2) at this address is settled
        if let Ok(Ok(r)) = &result {
            if let Some((address, _)) = &r.found {
                let serial = maint::string2(&mut link, *address);
                if let Ok(all) = maint::with_store_arc(&store, |s| s.all_pending()) {
                    for p in all.iter().filter(|p| maint::recovery_clears(p, &key, *address, serial.as_deref())) {
                        let _ = maint::with_store_arc(&store, |s| s.clear_pending(p.id, "recovered", crate::history::now_ms()));
                    }
                }
            }
        }
        // the recovery's own record
        let ok = matches!(result, Ok(Ok(_)));
        let kept = match maint::recovery_record_after(ok, last) {
            Some(how) => maint::with_store_arc(&store, |s| s.clear_pending(record, how, crate::history::now_ms())).is_err(),
            None => true,
        };
        drop(link);
        let log = log_path.display().to_string();
        let done = match result {
            Ok(Ok(r)) => FirmwareDone { ok: true, error: None, stage: None, written: true, before: None, after: r.found.as_ref().map(|(_, i)| i.clone()), log, pending: kept, log_error: r.log_error, comparison: None, recovered_at: r.found.map(|(a, _)| a), recovery: true },
            Ok(Err(e)) => FirmwareDone { ok: false, error: Some(e.text(l)), stage: last.map(stage_name), written: written(last), before: None, after: None, log, pending: kept, log_error: None, comparison: None, recovered_at: None, recovery: true },
            Err(_) => FirmwareDone { ok: false, error: Some(l.pick("Interner Fehler", "internal error").into()), stage: last.map(stage_name), written: written(last), before: None, after: None, log, pending: kept, log_error: None, comparison: None, recovered_at: None, recovery: true },
        };
        let state = app.state::<AppState>();
        maint::event(&state, None, &format!("recovery done ok={} found={:?} stage={:?} error={:?}", done.ok, done.recovered_at, done.stage, done.error));
        if let Some(id) = log_id {
            let _ = maint::with_store_arc(&store, |s| s.set_log_result(id, if done.ok { "ok" } else { "failed" }));
        }
        drop(single);
        drop(slot);
        drop(port_slot);
        let _ = app.emit("firmware-done", done);
    });
    Ok(())
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
        use openbms_proto::{ExtInfo, MaintState};
        let info = ExtInfo { format: 5, base: [16, 6, 13], patch: [1, 5], build_id: [0x89, 0xD8, 0x5C, 0x61, 0xB4, 0x0E, 0x03, 0xDA], hard_stop: None, power_rtc: false, pack_cutoff: false , reserved_flags: 0, read_caps: 0, patch_rev: 4, uart0: false };
        let with = |word: u32| Master::Patched { patch: Patch::recognise(info).unwrap(), state: Some(MaintState { marker_word: word, effective_slaves: 0 }) };
        let (on, off) = (with(0), with(u32::MAX));
        let (orig, maint) = (known::ImageKind::Original, known::ImageKind::Maintenance);
        // a patched master without a valid state (lost or foreign 02): the marker last seen counts
        let none = Master::Patched { patch: Patch::recognise(info).unwrap(), state: None };
        let foreign = with(0x1234_5678);
        for m in [&none, &foreign] {
            assert_eq!(master_marker_check(0, orig, Some(m), true), Err("release-master-first-unread"));
            assert_eq!(master_marker_check(0, orig, Some(m), false), Ok(Some("unchecked")));
        }
        assert_eq!(master_marker_check(0, orig, Some(&Master::UnknownPatch(None)), true), Err("release-master-first-unread"));
        // fresh read: the marker decides
        assert_eq!(master_marker_check(0, orig, Some(&on), false), Err("release-master-first"));
        assert_eq!(master_marker_check(0, maint, Some(&on), true), Ok(Some("checked")));
        assert_eq!(master_marker_check(0, orig, Some(&off), true), Ok(Some("checked")));
        // no control channel or no answer: the marker last seen on refuses, else the flash goes on marked unchecked
        for fresh in [None, Some(&Master::Unknown)] {
            assert_eq!(master_marker_check(0, orig, fresh, true), Err("release-master-first-unread"));
            assert_eq!(master_marker_check(0, known::ImageKind::Custom, fresh, true), Err("release-master-first-unread"));
            assert_eq!(master_marker_check(0, orig, fresh, false), Ok(Some("unchecked")));
            assert_eq!(master_marker_check(0, maint, fresh, true), Ok(Some("unchecked")));
        }
        // RTN 04 with a marker kept on: latent; a patched image says so, original to original goes on unchecked
        assert_eq!(master_marker_check(0, maint, Some(&Master::No0x64), true), Ok(Some("latent")));
        assert_eq!(master_marker_check(0, orig, Some(&Master::No0x64), true), Ok(Some("unchecked")));
        assert_eq!(master_marker_check(0, orig, Some(&Master::No0x64), false), Ok(Some("unchecked")));
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
        assert_eq!(known::patch_image("20e26bf8a0a0cd9d9b21c9018fc2c5b4e2325cf510f0c01b48c72da1c684265d"), Some("EMU1101-EXT Full 1.6.5"));
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
    fn a_series_phrase_names_the_count_and_never_stands_for_a_single_flash() {
        assert!(series_confirmation_ok(3, false, " FLASH 3 PACKS "));
        assert!(!series_confirmation_ok(4, false, "FLASH 3 PACKS"));
        assert!(!series_confirmation_ok(3, false, "flash 3 packs"));
        assert!(!series_confirmation_ok(3, false, "FLASH 3"));
        // a custom image: its own phrase, either way round
        assert!(series_confirmation_ok(3, true, "FLASH CUSTOM 3 PACKS"));
        assert!(!series_confirmation_ok(3, true, "FLASH 3 PACKS"));
        assert!(!series_confirmation_ok(3, false, "FLASH CUSTOM 3 PACKS"));
        // the single flash stays as it was
        assert!(!confirmation_ok(3, false, "FLASH 3 PACKS"));
        assert!(confirmation_ok(3, false, "FLASH 3"));
    }

    #[test]
    fn a_series_flashes_the_slaves_by_address_and_the_master_last() {
        assert_eq!(series_order(&[0, 3, 1]), Some(vec![1, 3, 0]));
        assert_eq!(series_order(&[2, 1]), Some(vec![1, 2]));
        assert_eq!(series_order(&[0, 15, 14, 7]), Some(vec![7, 14, 15, 0]));
        // one pack is a single flash; an address twice or no DIP address is no list
        assert_eq!(series_order(&[1]), None);
        assert_eq!(series_order(&[1, 1, 2]), None);
        assert_eq!(series_order(&[1, 16]), None);
    }

    const SHA: &str = "aaaa";
    fn slot_with(order: &[u8]) -> (SeriesSlot, u64) {
        let mut slot = SeriesSlot::new();
        let run = slot.begin("inst-1", "serial:/dev/p", SHA, false, series_order(order).unwrap()).unwrap();
        (slot, run)
    }
    fn admit(slot: &mut SeriesSlot, run: u64, listed: &[u8], a: u8) -> bool {
        slot.admit(run, Some("inst-1"), "serial:/dev/p", SHA, false, listed, a)
    }

    #[test]
    fn a_series_admits_exactly_the_confirmed_list_in_turn() {
        // out of turn, another list of the same count, another count: refused without changing anything
        let (mut slot, run) = slot_with(&[0, 2, 1]);
        for (listed, a) in [(&[0u8, 2, 1][..], 2), (&[0, 2, 1], 0), (&[0, 3, 1], 1), (&[2, 1], 1), (&[0, 2, 1, 3], 1)] {
            assert!(!admit(&mut slot, run, listed, a), "{listed:?} {a}");
        }
        // another installation, port, image, kind or run
        assert!(!slot.admit(run, Some("inst-2"), "serial:/dev/p", SHA, false, &[0, 1, 2], 1));
        assert!(!slot.admit(run, None, "serial:/dev/p", SHA, false, &[0, 1, 2], 1));
        assert!(!slot.admit(run, Some("inst-1"), "serial:/dev/q", SHA, false, &[0, 1, 2], 1));
        assert!(!slot.admit(run, Some("inst-1"), "serial:/dev/p", "bbbb", false, &[0, 1, 2], 1));
        assert!(!slot.admit(run, Some("inst-1"), "serial:/dev/p", SHA, true, &[0, 1, 2], 1));
        assert!(!admit(&mut slot, run + 1, &[0, 1, 2], 1));
        // the list as confirmed, in any order; admitted means running: nothing else meanwhile
        assert!(admit(&mut slot, run, &[1, 2, 0], 1));
        assert!(!admit(&mut slot, run, &[0, 1, 2], 1));
        assert!(!admit(&mut slot, run, &[0, 1, 2], 2));
        assert!(slot.after(run, 1, true));
        assert!(admit(&mut slot, run, &[0, 1, 2], 2));
        assert!(slot.after(run, 2, true));
        assert!(admit(&mut slot, run, &[0, 1, 2], 0));
        // done after the last pack
        assert!(!slot.after(run, 0, true));
        assert!(!slot.open());
    }

    #[test]
    fn a_begin_cannot_replace_a_series_whose_pack_is_admitted() {
        let (mut slot, a) = slot_with(&[1, 2, 0]);
        // A's pack 1 admitted, its pre-checks running: a second begin is refused, A is kept
        assert!(admit(&mut slot, a, &[1, 2, 0], 1));
        assert_eq!(slot.begin("inst-1", "serial:/dev/p", SHA, false, vec![3, 4, 0]), Err("series-busy"));
        assert!(slot.after(a, 1, true));
        assert!(admit(&mut slot, a, &[1, 2, 0], 2));
        // an idle series is replaced by a new one; the old run's starts and completion touch nothing of the new one
        let (mut slot, a) = slot_with(&[1, 2, 0]);
        let b = slot.begin("inst-1", "serial:/dev/p", SHA, false, vec![3, 4, 0]).unwrap();
        assert_ne!(a, b);
        assert!(!admit(&mut slot, a, &[1, 2, 0], 1));
        assert!(!slot.after(a, 1, true));
        assert!(admit(&mut slot, b, &[3, 4, 0], 3));
        assert!(!admit(&mut slot, b, &[3, 4, 0], 4));
        assert!(slot.after(b, 3, true));
        assert!(admit(&mut slot, b, &[3, 4, 0], 4));
    }

    #[test]
    fn a_late_end_or_drop_of_an_old_run_leaves_the_new_one() {
        let (mut slot, a) = slot_with(&[1, 2]);
        let b = slot.begin("inst-1", "serial:/dev/p", SHA, false, vec![3, 4]).unwrap();
        // `firmware_series_end` and the drop guard both end by run id
        slot.end(a);
        assert!(slot.open());
        assert!(admit(&mut slot, b, &[3, 4], 3));
        slot.end(b);
        assert!(!slot.open());
    }

    #[test]
    fn the_end_of_a_pack_not_admitted_in_turn_ends_the_series() {
        let (mut slot, run) = slot_with(&[1, 2, 0]);
        // nothing admitted: a completion for pack 1 ends it (it was never checked)
        assert!(!slot.after(run, 1, true));
        assert!(!slot.open());
        let (mut slot, run) = slot_with(&[1, 2, 0]);
        assert!(admit(&mut slot, run, &[1, 2, 0], 1));
        assert!(!slot.after(run, 2, true));
        assert!(!slot.open());
        // a failure ends it
        let (mut slot, run) = slot_with(&[1, 2, 0]);
        assert!(admit(&mut slot, run, &[1, 2, 0], 1));
        assert!(!slot.after(run, 1, false));
        assert!(!slot.open());
    }

    #[test]
    fn a_single_flash_waits_while_a_series_is_open_and_the_series_goes_on() {
        let (mut slot, run) = slot_with(&[1, 2]);
        assert!(admit(&mut slot, run, &[1, 2], 1));
        assert!(slot.after(run, 1, true));
        // between two packs the series is open: a single flash or recovery is refused (`series_open_refusal`), and
        // that refused foreign action changes nothing: the series goes on with pack 2
        assert!(slot.open());
        assert!(admit(&mut slot, run, &[1, 2], 2));
        assert!(!slot.after(run, 2, true));
        assert!(!slot.open());
    }

    #[test]
    fn a_begin_cannot_slip_in_while_a_single_flash_or_recovery_is_admitted() {
        let mut slot = SeriesSlot::new();
        // the single flash (or recovery) passed its series check and is admitted, its flashing mark not yet set: the
        // check and the admission are one step, so a begin now is refused
        slot.admit_single().unwrap();
        assert_eq!(slot.begin("inst-1", "serial:/dev/p", SHA, false, vec![1, 2]), Err("flash-admitted"));
        // a second single one waits too
        assert_eq!(slot.admit_single(), Err("flash-admitted"));
        // its end (or refusal) frees the slot
        slot.end_single();
        let run = slot.begin("inst-1", "serial:/dev/p", SHA, false, vec![1, 2]).unwrap();
        // and the other way round: with the series open no single flash or recovery is admitted
        assert_eq!(slot.admit_single(), Err("series-open"));
        assert!(admit(&mut slot, run, &[1, 2], 1));
        assert_eq!(slot.admit_single(), Err("series-open"));
    }

    #[test]
    fn an_idle_series_can_be_ended_without_its_run_but_not_while_a_pack_is_admitted() {
        let (mut slot, run) = slot_with(&[1, 2]);
        assert!(admit(&mut slot, run, &[1, 2], 1));
        assert_eq!(slot.end_idle(), Err("series-busy"));
        assert!(slot.after(run, 1, true));
        // left idle (the view went away): ended, a single flash is admitted again
        assert_eq!(slot.end_idle(), Ok(()));
        assert!(!slot.open());
        assert_eq!(slot.admit_single(), Ok(()));
        // nothing open: ending is a no-op
        slot.end_single();
        assert_eq!(slot.end_idle(), Ok(()));
    }

    #[test]
    fn a_pending_record_anywhere_refuses_a_series() {
        use crate::maint_store::Pending;
        assert!(series_pending_refusal(&[]).is_ok());
        let p = Pending { id: 1, endpoint: "serial:/dev/q".into(), pack: 2, release: "16.06.13".into(), since: 0, bms_serial: Some("SYN-2".into()) };
        assert!(matches!(series_pending_refusal(&[p]), Err(maint::MaintError::BootloaderPending { pack: 2, .. })));
    }

    #[test]
    fn a_series_pack_is_clean_only_with_a_full_after_check_and_the_new_version() {
        let info = |kind: &'static str, release: &str, base: Option<&str>, custom: bool| ImageInfo {
            release: release.into(), device: "EMU1101".into(), bytes: 0, blocks: 0, sha256: String::new(), kind, custom,
            power_rtc: false, pack_cutoff: false, positive: false, base: base.map(Into::into),
        };
        let orig = info("original", "16.06.13", None, false);
        assert!(series_clean(true, false, Some("same"), Some("16.06"), &orig));
        assert!(!series_clean(true, false, Some("same"), Some("16.05"), &orig));
        assert!(!series_clean(true, false, Some("incomplete"), Some("16.06"), &orig));
        assert!(!series_clean(true, false, Some("differs"), Some("16.06"), &orig));
        assert!(!series_clean(true, false, None, Some("16.06"), &orig));
        assert!(!series_clean(true, true, Some("same"), Some("16.06"), &orig));
        assert!(!series_clean(false, false, Some("same"), Some("16.06"), &orig));
        assert!(!series_clean(true, false, Some("same"), None, &orig));
        // "16.06" does not fit another minor's release text
        assert!(!series_clean(true, false, Some("same"), Some("16.0"), &orig));
        // EMU1101-EXT: its OEM base stands for it
        // (accepted only with custom firmware allowed, yet its version is checked)
        let ext = info("maintenance", "EMU1101-EXT Full 1.6.5", Some("16.06.13"), true);
        assert!(series_clean(true, false, Some("same"), Some("16.06"), &ext));
        assert!(!series_clean(true, false, Some("same"), Some("16.05"), &ext));
        // a custom image has no version to compare: its answer counts
        assert!(series_clean(true, false, Some("same"), Some("99.01"), &info("custom", "custom", None, true)));
    }

    #[test]
    fn a_recovery_is_confirmed_with_its_own_phrase_only() {
        assert!(recovery_confirmation_ok(false, " RECOVER "));
        assert!(!recovery_confirmation_ok(false, "FLASH 0"));
        assert!(!recovery_confirmation_ok(true, "RECOVER"));
        assert!(recovery_confirmation_ok(true, "RECOVER CUSTOM"));
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
