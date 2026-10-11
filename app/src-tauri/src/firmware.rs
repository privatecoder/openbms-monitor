//! Firmware update from the app: a guarded wrapper around `openbms-flash`. The app's image policy is
//! the vendor allowlist; the flash runs on its own thread and reports progress as events.

use crate::{reveal, stop_monitor, AppState};
use openbms_flash::{ehex, known, session, session::Progress, Lang};
use openbms_proto::DeviceInfo;
use openbms_transport::{Connection, Endpoint};
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
}

fn lang(code: &str) -> Lang {
    if code.starts_with("de") { Lang::De } else { Lang::En }
}

pub fn confirmation_ok(address: u8, typed: &str) -> bool {
    typed.trim() == format!("FLASH {address}")
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

pub fn log_name(unix_s: u64, address: u8, what: &str) -> String {
    let what: String = what.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    format!("{}_pack{address}_{what}.log", utc_stamp(unix_s))
}

/// Parse and authorize an image: only allowlisted vendor releases.
pub fn image_info(bytes: &[u8]) -> openbms_flash::Result<(ehex::Image, ImageInfo)> {
    let image = ehex::parse(bytes)?;
    let release = known::identify(&image)?;
    let blocks = session::dry_run(&image)?.len() - 3;
    let info = ImageInfo {
        release: release.name.into(),
        device: image.header.device.clone(),
        bytes: image.code.len(),
        blocks,
        sha256: image.code_sha256.clone(),
    };
    Ok((image, info))
}

fn read_image(path: &str, l: Lang) -> Result<(ehex::Image, ImageInfo), String> {
    if !path.to_ascii_lowercase().ends_with(".ehex") {
        return Err(l.pick("Erwartet wird eine .ehex-Datei", "expected a .ehex file").into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    image_info(&bytes).map_err(|e| e.text(l))
}

fn logs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("firmware-logs"))
}

fn new_log(app: &AppHandle, address: u8, what: &str) -> Result<(File, PathBuf), String> {
    let dir = logs_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let path = dir.join(log_name(now, address, what));
    let file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((file, path))
}

fn open_port(port: &str, l: Lang) -> Result<Connection, String> {
    Connection::open(&Endpoint::Serial { path: port.into(), baud: PACK_BUS_BAUD })
        .map_err(|e| openbms_flash::Error::from(e).text(l))
}

#[tauri::command]
pub async fn firmware_check_image(path: String, lang: String) -> Result<ImageInfo, String> {
    read_image(&path, self::lang(&lang)).map(|(_, info)| info)
}

/// Stops the monitor, then checks the target on the pack bus without resetting it.
#[tauri::command]
pub async fn firmware_inspect(app: AppHandle, state: State<'_, AppState>, port: String, address: u8, lang: String) -> Result<DeviceInfo, String> {
    let l = self::lang(&lang);
    if state.flashing.load(Ordering::SeqCst) {
        return Err(l.pick("Ein Firmware-Update läuft", "a firmware update is running").into());
    }
    stop_monitor(&state)?;
    let mut link = open_port(&port, l)?;
    let (mut log, _) = new_log(&app, address, "inspect")?;
    session::inspect(&mut link, address, &mut log, session::Options::default()).map_err(|e| e.text(l))
}

/// Validates everything again, then flashes on its own thread and returns at once.
#[tauri::command]
pub async fn firmware_start(app: AppHandle, state: State<'_, AppState>, port: String, address: u8, path: String, confirmation: String, lang: String) -> Result<(), String> {
    let l = self::lang(&lang);
    if address > 15 {
        return Err(openbms_flash::Error::Address(address).text(l));
    }
    if !confirmation_ok(address, &confirmation) {
        return Err(openbms_flash::Error::Cancelled.text(l));
    }
    // the file may have changed since it was checked
    let (image, info) = read_image(&path, l)?;
    if !try_begin(&state.flashing) {
        return Err(l.pick("Ein Firmware-Update läuft bereits", "a firmware update is already running").into());
    }
    let flashing = state.flashing.clone();
    let prepared = (|| {
        stop_monitor(&state)?;
        let link = open_port(&port, l)?;
        let (mut log, log_path) = new_log(&app, address, &info.release)?;
        writeln!(log, "CONFIRMED target={address} code_sha256={} release={} source=app", info.sha256, info.release).map_err(|e| e.to_string())?;
        Ok::<_, String>((link, log, log_path))
    })();
    let (mut link, mut log, log_path) = match prepared {
        Ok(v) => v,
        Err(e) => {
            flashing.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    std::thread::spawn(move || {
        let mut last = None;
        let result = session::flash_with_progress(&mut link, address, &image, &mut log, session::Options::default(), &mut |p| {
            last = Some(p);
            let _ = app.emit("firmware-progress", progress_event(p));
        });
        drop(link);
        let done = match result {
            Ok(r) => FirmwareDone { ok: true, error: None, stage: None, written: true, before: Some(r.before), after: Some(r.after), log: log_path.display().to_string() },
            Err(e) => FirmwareDone { ok: false, error: Some(e.text(l)), stage: last.map(stage_name), written: written(last), before: None, after: None, log: log_path.display().to_string() },
        };
        flashing.store(false, Ordering::SeqCst);
        let _ = app.emit("firmware-done", done);
    });
    Ok(())
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
        assert!(confirmation_ok(3, "FLASH 3"));
        assert!(confirmation_ok(3, "  FLASH 3 "));
        assert!(!confirmation_ok(3, "FLASH 4"));
        assert!(!confirmation_ok(3, "flash 3"));
        assert!(!confirmation_ok(3, "FLASH 3 now"));
        assert!(!confirmation_ok(3, ""));
    }

    #[test]
    fn log_name_is_a_plain_file_name() {
        // 2026-01-02 03:04:05 UTC
        let name = log_name(1_767_323_045, 7, "16.06.05 SP80");
        assert_eq!(name, "20260102-030405Z_pack7_16.06.05_SP80.log");
        assert!(!name.contains(['/', '\\', ':', ' ']));
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
        let err = image_info(lines.join("\n").as_bytes()).err().unwrap();
        assert!(err.to_string().contains("unknown or unsupported"));
    }
}
