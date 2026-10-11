# Firmware Update View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A guided "Firmware" view in the desktop app that flashes an allowlisted vendor image to one pack over the serial pack bus, with progress, log file and the same safety checks as the CLI.

**Architecture:** `openbms-flash` gains a progress callback (`flash_with_progress`) and returns the post-flash device info. A new Tauri module `firmware.rs` wraps the library (image check, pack inspection, flash on its own thread, events) and guards the rest of the app with a `flashing` flag. The React view `Firmware.tsx` drives the steps; gating and progress math live in a pure module `firmware.ts`. The view is reachable only when the sidebar's maintenance switch is on.

**Tech Stack:** Rust (workspace crates, Tauri 2), React 19 + TypeScript + Tailwind 4, i18next, Vitest, Python 3 help generator.

**Spec:** `docs/superpowers/specs/2026-10-11-firmware-update-view-design.md`

## Global Constraints

- First version: one pack, serial (USB-RS485) on RS485-1/2 at 19200 baud, quiet bus. No TCP, no multi-pack, no retry/recovery.
- Images: only `known::RELEASES` (allowlist). No custom-firmware option in the app.
- Timing: `session::Options::default()`; no timing settings in the app.
- Confirmation phrase exactly `FLASH <address>`, checked in the UI and again in the backend.
- Log files: app data folder `firmware-logs/`, created new, never overwritten.
- No cancel button during a run. During a run: navigation and sidebar connection controls disabled, window close refused with a notice; `connect` and `start_polling` refuse.
- Maintenance switch: `localStorage` key `maint.enabled`, default off.
- Every user-visible string in German and English (`i18n.ts`, help via `tools/gen_help.py`). Content generic: no installation's pack numbers, dates or readings.
- Match surrounding code: short "why" comments, compact components, Tailwind tokens from `index.css`.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Checks before each commit: `cargo test --workspace`, `cargo clippy --workspace` (no new warnings); for frontend tasks also `cd app && npm test && npx tsc -b`.

## Review Focus

1. Monitor connected on the same serial port when "Check pack" is pressed: the monitor connection must be dropped before the port is opened again (otherwise "port busy"). Covered by `stop_monitor` being called first in `firmware_inspect`/`firmware_start` (Task 2) and by the manual check in Task 5.
2. Port or address changed after the pack was checked: the confirmation must not apply to the previous target. Pinned by `target_must_match_port_and_address` in Task 3.
3. Image file replaced between "check image" and "start": the backend re-reads and re-validates against the allowlist at start. Pinned by `image_info_rejects_unknown_code_hash` (Task 2) plus `firmware_start` calling `image_info` again.
4. Double click on "Start": only one flash may run. Pinned by `try_begin_allows_only_one_flash` in Task 2.
5. Sloppy confirmation (wrong address, lower case, extra words): rejected. Pinned by `confirmation_requires_exact_phrase` in Task 2 and `start_needs_exact_phrase` in Task 3.

---

### Task 1: Progress callback and post-flash device info in `openbms-flash`

**Files:**
- Modify: `crates/openbms-flash/src/session.rs` (struct `FlashResult`, fn `wait_for_app`, fn `flash`, new `Progress`, new `flash_with_progress`)
- Test: `crates/openbms-flash/tests/session.rs`

**Interfaces:**
- Produces:
  - `pub enum session::Progress { Preflight, Bootloader, Erase, Block { index: usize, total: usize }, Finalize, AppRestart, PostCheck }` (derives `Debug, Clone, Copy, PartialEq, Eq`)
  - `pub fn session::flash_with_progress(link: &mut impl Link, address: u8, image: &Image, log_file: &mut impl Write, options: Options, progress: &mut dyn FnMut(Progress)) -> Result<FlashResult>`
  - `FlashResult.after: DeviceInfo` (decoded 0x51 answer after the restart)
  - `session::flash` unchanged in signature; it calls `flash_with_progress` with a no-op closure.

- [ ] **Step 1: Write the failing tests** (append to `crates/openbms-flash/tests/session.rs`)

```rust
#[test]
fn flash_reports_progress_in_order_and_returns_device_after() {
    use session::Progress::*;
    let mut sim = BootSim::new();
    let mut seen = Vec::new();
    let result = session::flash_with_progress(
        &mut sim,
        0,
        &image(),
        &mut Vec::new(),
        fast(),
        &mut |p| seen.push(p),
    )
    .unwrap();
    assert_eq!(
        seen,
        vec![
            Preflight,
            Bootloader,
            Erase,
            Block { index: 0, total: 3 },
            Block { index: 1, total: 3 },
            Block { index: 2, total: 3 },
            Finalize,
            AppRestart,
            PostCheck,
        ]
    );
    assert_eq!(result.after.device_name, result.before.device_name);
    assert!(!result.after.firmware_version.is_empty());
}

#[test]
fn progress_stops_at_rejected_block() {
    let mut sim = BootSim::new();
    sim.reject_data_block(1);
    let mut seen = Vec::new();
    let err = session::flash_with_progress(
        &mut sim,
        0,
        &image(),
        &mut Vec::new(),
        fast(),
        &mut |p| seen.push(p),
    );
    assert!(err.is_err());
    assert_eq!(
        seen.last(),
        Some(&session::Progress::Block { index: 1, total: 3 })
    );
}

#[test]
fn flash_with_progress_sends_the_same_frames_as_flash() {
    fn tx(log: Vec<u8>) -> Vec<String> {
        String::from_utf8(log)
            .unwrap()
            .lines()
            .filter_map(|l| l.split_once(" TX ").map(|(_, hex)| hex.to_string()))
            .collect()
    }
    let mut plain = Vec::new();
    session::flash(&mut BootSim::new(), 0, &image(), &mut plain, fast()).unwrap();
    let mut with = Vec::new();
    session::flash_with_progress(&mut BootSim::new(), 0, &image(), &mut with, fast(), &mut |_| {})
        .unwrap();
    assert_eq!(tx(plain), tx(with));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p openbms-flash --test session`
Expected: compile error `cannot find function flash_with_progress` / `no field after`.

- [ ] **Step 3: Implement**

In `session.rs`, add after `ReplyTimes` impl:

```rust
/// Stage of a running flash, reported to the caller (the app shows it as progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Preflight,
    Bootloader,
    Erase,
    Block { index: usize, total: usize },
    Finalize,
    AppRestart,
    PostCheck,
}
```

Add the field to `FlashResult`:

```rust
pub struct FlashResult {
    pub before: DeviceInfo,
    /// 0x51 answer of the restarted application.
    pub after: DeviceInfo,
    pub blocks: usize,
    pub app_returned: bool,
    pub wfd_reply_times: ReplyTimes,
}
```

Change `wait_for_app` to return the decoded answer:

```rust
fn wait_for_app(
    link: &mut impl Link,
    address: u8,
    log_file: &mut impl Write,
    options: Options,
) -> Result<DeviceInfo> {
    let deadline = Instant::now() + options.post_timeout;
    while Instant::now() < deadline {
        match ascii_query(link, address, 0x51, log_file, Duration::from_millis(300)) {
            Ok(reply) => return Ok(DeviceInfo::parse(&reply.info)?),
            Err(Error::Timeout { .. }) => link.wait(Duration::from_millis(100)),
            Err(other) => return Err(other),
        }
    }
    Err(Error::Timeout {
        what: "application restart".into(),
        after: options.post_timeout,
    })
}
```

In `probe`, keep `wait_for_app(link, address, log_file, options)?;` (the value is ignored).

Replace `flash` by the pair:

```rust
/// Flash after the caller has validated the image and obtained explicit operator consent.
/// Every frame is precomputed before reset, including the unverified-checksum guard.
pub fn flash(
    link: &mut impl Link,
    address: u8,
    image: &Image,
    log_file: &mut impl Write,
    options: Options,
) -> Result<FlashResult> {
    flash_with_progress(link, address, image, log_file, options, &mut |_| {})
}

/// Like [`flash`], reporting each stage before it starts. Frames and timing are identical.
pub fn flash_with_progress(
    link: &mut impl Link,
    address: u8,
    image: &Image,
    log_file: &mut impl Write,
    options: Options,
    progress: &mut dyn FnMut(Progress),
) -> Result<FlashResult> {
    let frames = dry_run(image)?;
    let total = frames.len() - 3;
    progress(Progress::Preflight);
    let before = preflight(link, address, log_file, options)?;
    progress(Progress::Bootloader);
    enter_boot(link, address, log_file, options, &frames[0])?;
    note(
        log_file,
        "sending W F E; board is in update mode until application restart",
    )?;
    progress(Progress::Erase);
    boot_exchange(link, &frames[1], log_file, options).map_err(|e| uncertain("W F E", e))?;
    let mut wfd_times = Vec::with_capacity(total);
    for (index, frame) in frames[2..frames.len() - 1].iter().enumerate() {
        progress(Progress::Block { index, total });
        let reply = boot_exchange(link, frame, log_file, options)
            .map_err(|e| uncertain(&format!("W F D block {index}"), e))?;
        wfd_times.push(reply.elapsed);
    }
    progress(Progress::Finalize);
    boot_exchange(link, frames.last().unwrap(), log_file, options)
        .map_err(|e| uncertain("W F G", e))?;
    progress(Progress::AppRestart);
    let after = wait_for_app(link, address, log_file, options)
        .map_err(|e| uncertain("application restart", e))?;
    progress(Progress::PostCheck);
    let ascii_timeout = options.ack_timeout.min(Duration::from_millis(600));
    ascii_query(link, address, 0x42, log_file, ascii_timeout)
        .map_err(|e| uncertain("0x42 post-check", e))?;
    ascii_query(link, address, 0x44, log_file, ascii_timeout)
        .map_err(|e| uncertain("0x44 post-check", e))?;
    let wfd_reply_times = ReplyTimes::from_samples(wfd_times)?;
    note(
        log_file,
        &format!(
            "WFD response_ms min={:.1} median={:.1} max={:.1}",
            wfd_reply_times.min.as_secs_f64() * 1000.0,
            wfd_reply_times.median.as_secs_f64() * 1000.0,
            wfd_reply_times.max.as_secs_f64() * 1000.0,
        ),
    )?;
    note(
        log_file,
        "flash complete; application answered 0x51, 0x42 and 0x44",
    )?;
    Ok(FlashResult {
        before,
        after,
        blocks: total,
        app_returned: true,
        wfd_reply_times,
    })
}
```

In `crates/openbms-flash/src/main.rs`, in the `Command::Flash` success output, add the version after the update to the existing `println!` (field `result.after.firmware_version`), e.g. append ` | {} {}` with `lang.pick("Version jetzt", "version now"), result.after.firmware_version`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p openbms-flash && cargo clippy -p openbms-flash`
Expected: all pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/openbms-flash
git commit -m "openbms-flash: progress callback and device info after the flash

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Tauri backend module `firmware.rs`

**Files:**
- Create: `app/src-tauri/src/firmware.rs`
- Modify: `app/src-tauri/Cargo.toml` (dependency), `app/src-tauri/src/lib.rs` (AppState field, `stop_monitor`, guards in `connect`/`start_polling`, window close guard, command registration)

**Interfaces:**
- Consumes: `session::{inspect, flash_with_progress, dry_run, Options, Progress, FlashResult}`, `ehex::parse`, `known::identify`, `openbms_flash::{Lang, Error}` from Task 1.
- Produces (Tauri commands, camelCase args on the JS side):
  - `firmware_check_image(path: String, lang: String) -> ImageInfo { release, device, bytes, blocks, sha256 }`
  - `firmware_inspect(port: String, address: u8, lang: String) -> DeviceInfo`
  - `firmware_start(port: String, address: u8, path: String, confirmation: String, lang: String) -> ()`
  - `reveal_firmware_logs(path: Option<String>) -> ()`
  - Events: `firmware-progress` → `ProgressEvent { stage: &str, block: Option<usize>, total: Option<usize> }`; `firmware-done` → `FirmwareDone { ok, error: Option<String>, stage: Option<&str>, written: bool, before: Option<DeviceInfo>, after: Option<DeviceInfo>, log: String }`; `firmware-busy` → `()`
  - Stage strings: `"preflight" | "bootloader" | "erase" | "block" | "finalize" | "app_restart" | "post_check"`

- [ ] **Step 1: Add the dependency**

In `app/src-tauri/Cargo.toml` under `[dependencies]`:

```toml
openbms-flash = { path = "../../crates/openbms-flash" }
```

- [ ] **Step 2: Write the failing unit tests** (bottom of the new `app/src-tauri/src/firmware.rs`; the file starts with only the test module so it fails to compile)

```rust
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
```

Add `mod firmware;` next to `mod history;` in `lib.rs`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p openbms-monitor firmware`
Expected: compile errors for the missing functions.

- [ ] **Step 4: Implement `firmware.rs`** (above the test module)

```rust
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
```

Note: `openbms_flash::Error::from(openbms_transport::Error)` exists (`impl From` in `crates/openbms-flash/src/lib.rs`).

- [ ] **Step 5: Wire it into `lib.rs`**

1. `AppState` gets `flashing: Arc<AtomicBool>,` with a doc comment `/// Set while a firmware update runs; the monitor must not touch the bus then.`
2. Extract the body of `disconnect` into a shared helper and use it from `disconnect`:

```rust
/// Stop polling and drop the monitor's connection (also before a firmware update takes the port).
fn stop_monitor(state: &AppState) -> Result<(), String> {
    if let Ok(p) = state.polling.lock() {
        p.store(false, Ordering::SeqCst);
    }
    *lock(&state.conn)? = None;
    if let Ok(mut h) = state.history.lock() {
        h.disconnected();
    }
    Ok(())
}

#[tauri::command]
fn disconnect(state: State<AppState>) -> Result<(), String> {
    stop_monitor(&state)
}
```

3. First line of `connect` and of `start_polling`:

```rust
if state.flashing.load(Ordering::SeqCst) {
    return Err("a firmware update is running".into());
}
```

4. Make `reveal` and `AppState` visible to the child module: they already are (same crate, child module can use private items of its parent via `crate::`).
5. In `run()`, add before `.invoke_handler(...)`:

```rust
.on_window_event(|window, event| {
    // closing during a flash would cut it off and can leave the board without an application
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.app_handle().state::<AppState>().flashing.load(Ordering::SeqCst) {
            api.prevent_close();
            let _ = window.emit("firmware-busy", ());
        }
    }
})
```

6. Append to `generate_handler![...]`: `firmware::firmware_check_image, firmware::firmware_inspect, firmware::firmware_start, firmware::reveal_firmware_logs`.

- [ ] **Step 6: Run tests and checks**

Run: `cargo test --workspace && cargo clippy --workspace`
Expected: all pass, the six `firmware::tests` pass, no new warnings.

- [ ] **Step 7: Commit**

```bash
git add app/src-tauri
git commit -m "App backend: firmware commands (check image, inspect, flash with progress events)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Frontend API and step logic

**Files:**
- Modify: `app/src/api.ts`
- Create: `app/src/firmware.ts`, `app/src/firmware.test.ts`

**Interfaces:**
- Consumes: Tauri commands and events from Task 2.
- Produces:
  - Types `FirmwareImage`, `FirmwareStage`, `FirmwareProgress`, `FirmwareDone` in `api.ts`
  - `api.firmwareCheckImage(path, lang)`, `api.firmwareInspect(port, address, lang)`, `api.firmwareStart(port, address, path, confirmation, lang)`, `api.onFirmwareProgress(f)`, `api.onFirmwareDone(f)`, `api.onFirmwareBusy(f)`, `api.revealFirmwareLogs(path)`
  - In `firmware.ts`: `confirmationPhrase(address: number): string`, `type FwSetup`, `steps(s: FwSetup): { image: boolean; inspect: boolean; confirm: boolean; start: boolean }`, `progressFraction(p: FirmwareProgress | null): number`

- [ ] **Step 1: Write the failing tests** (`app/src/firmware.test.ts`)

```ts
import { describe, expect, it } from "vitest";
import { confirmationPhrase, progressFraction, steps, type FwSetup } from "./firmware";

const image = { release: "16.06.13", device: "EMU1101_10D_HW16", bytes: 53000, blocks: 209, sha256: "ab" };
const target = { device_name: "X", firmware_version: "16.04", can_protocol: "CAN" };
const base: FwSetup = { running: false, port: "/dev/x", address: 0, image, target, checkedFor: { port: "/dev/x", address: 0 }, typed: "" };

describe("firmware steps", () => {
  it("check pack needs port and image", () => {
    expect(steps({ ...base, port: "" }).inspect).toBe(false);
    expect(steps({ ...base, image: null }).inspect).toBe(false);
    expect(steps(base).inspect).toBe(true);
  });
  it("target_must_match_port_and_address", () => {
    expect(steps(base).confirm).toBe(true);
    expect(steps({ ...base, address: 1 }).confirm).toBe(false);
    expect(steps({ ...base, port: "/dev/y" }).confirm).toBe(false);
    expect(steps({ ...base, target: null }).confirm).toBe(false);
  });
  it("start_needs_exact_phrase", () => {
    expect(confirmationPhrase(3)).toBe("FLASH 3");
    expect(steps({ ...base, typed: "FLASH 0" }).start).toBe(true);
    expect(steps({ ...base, typed: " FLASH 0 " }).start).toBe(true);
    expect(steps({ ...base, typed: "flash 0" }).start).toBe(false);
    expect(steps({ ...base, typed: "FLASH 1" }).start).toBe(false);
  });
  it("nothing is editable while running", () => {
    const s = steps({ ...base, typed: "FLASH 0", running: true });
    expect(s).toEqual({ image: false, inspect: false, confirm: false, start: false });
  });
});

describe("progress", () => {
  it("counts blocks and treats the stages around them as 0 and 1", () => {
    expect(progressFraction(null)).toBe(0);
    expect(progressFraction({ stage: "erase", block: null, total: null })).toBe(0);
    expect(progressFraction({ stage: "block", block: 0, total: 4 })).toBe(0);
    expect(progressFraction({ stage: "block", block: 2, total: 4 })).toBe(0.5);
    expect(progressFraction({ stage: "finalize", block: null, total: null })).toBe(1);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd app && npx vitest run src/firmware.test.ts`
Expected: FAIL, cannot resolve `./firmware`.

- [ ] **Step 3: Add API types and calls** (`app/src/api.ts`)

After `ImportStats`:

```ts
export interface FirmwareImage { release: string; device: string; bytes: number; blocks: number; sha256: string }
export type FirmwareStage = "preflight" | "bootloader" | "erase" | "block" | "finalize" | "app_restart" | "post_check";
export interface FirmwareProgress { stage: FirmwareStage; block: number | null; total: number | null }
/** End of a flash. written = W F E was sent, so after an error the board may have no application. */
export interface FirmwareDone { ok: boolean; error: string | null; stage: FirmwareStage | null; written: boolean; before: DeviceInfo | null; after: DeviceInfo | null; log: string }
```

In `tauriApi`:

```ts
  firmwareCheckImage: (path: string, lang: string) => invoke<FirmwareImage>("firmware_check_image", { path, lang }),
  /** Disconnects the monitor, then reads the pack on the pack bus without resetting it. */
  firmwareInspect: (port: string, address: number, lang: string) => invoke<DeviceInfo>("firmware_inspect", { port, address, lang }),
  /** Returns at once; progress and result arrive as events. */
  firmwareStart: (port: string, address: number, path: string, confirmation: string, lang: string) =>
    invoke<void>("firmware_start", { port, address, path, confirmation, lang }),
  onFirmwareProgress: (f: (p: FirmwareProgress) => void) => listen<FirmwareProgress>("firmware-progress", (e) => f(e.payload)),
  onFirmwareDone: (f: (d: FirmwareDone) => void) => listen<FirmwareDone>("firmware-done", (e) => f(e.payload)),
  /** The window was not closed because a flash is running. */
  onFirmwareBusy: (f: () => void) => listen("firmware-busy", () => f()),
  revealFirmwareLogs: (path: string | null) => invoke<void>("reveal_firmware_logs", { path }),
```

In `demoApi()` return object (the type is `typeof tauriApi`, so all are required):

```ts
    firmwareCheckImage: async () => { throw new Error("desktop app only"); },
    firmwareInspect: async () => { throw new Error("desktop app only"); },
    firmwareStart: async () => { throw new Error("desktop app only"); },
    onFirmwareProgress: async () => () => {},
    onFirmwareDone: async () => () => {},
    onFirmwareBusy: async () => () => {},
    revealFirmwareLogs: async () => {},
```

- [ ] **Step 4: Implement `app/src/firmware.ts`**

```ts
import type { DeviceInfo, FirmwareImage, FirmwareProgress } from "./api";

export const confirmationPhrase = (address: number) => `FLASH ${address}`;

export interface FwSetup {
  running: boolean;
  port: string;
  address: number;
  image: FirmwareImage | null;
  /** result of "check pack" and the port/address it was read from */
  target: DeviceInfo | null;
  checkedFor: { port: string; address: number } | null;
  typed: string;
}

/** Which steps are usable. A checked pack counts only for the port and address it was read from. */
export function steps(s: FwSetup) {
  if (s.running) return { image: false, inspect: false, confirm: false, start: false };
  const inspect = s.port !== "" && s.image !== null;
  const confirm = inspect && s.target !== null && s.checkedFor?.port === s.port && s.checkedFor.address === s.address;
  return { image: true, inspect, confirm, start: confirm && s.typed.trim() === confirmationPhrase(s.address) };
}

/** 0..1 over the data blocks; erasing counts as start, everything after the last block as done. */
export function progressFraction(p: FirmwareProgress | null): number {
  if (!p) return 0;
  if (p.stage === "block" && p.block !== null && p.total) return p.block / p.total;
  return ["finalize", "app_restart", "post_check"].includes(p.stage) ? 1 : 0;
}
```

- [ ] **Step 5: Run tests and type check**

Run: `cd app && npm test && npx tsc -b`
Expected: all tests pass, no type errors.

- [ ] **Step 6: Commit**

```bash
git add app/src/api.ts app/src/firmware.ts app/src/firmware.test.ts
git commit -m "App: firmware API calls and step logic

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Firmware view, maintenance switch, navigation lock, texts

**Files:**
- Create: `app/src/views/Firmware.tsx`
- Modify: `app/src/App.tsx`, `app/src/views/Sidebar.tsx`, `app/src/i18n.ts`, `tools/gen_help.py` (+ regenerated `help/de|en/topic.firmware.md`)

**Interfaces:**
- Consumes: everything from Task 3; `Button` (`components/ui/button`), `Switch` (`components/PackPicker`), `InfoIcon` (`help`), `useNow` (`components/live`), `cn`.
- Produces: `<Firmware connected onMonitorStopped onRunning onBack />`; Sidebar props `locked: boolean`, `maintenance: boolean`, `setMaintenance(v: boolean)`.

- [ ] **Step 1: i18n strings** — in `app/src/i18n.ts` add to both `nav` objects `firmware: "Firmware"`, and a new `fw` block in each language next to `par`:

German:

```ts
      maint: "Wartungsfunktionen anzeigen",
      fw: {
        title: "Firmware", intro: "Spielt eine Original-Firmware von Seplos auf ein einzelnes Pack, über RS485-1/2 mit 19200 Baud. Nur freigegebene Versionen.",
        demo: "Firmware-Updates gehen nur in der Desktop-App.",
        s1: "Verbindung", port: "Port", choosePort: "Port wählen", address: "Pack-Adresse (DIP)",
        busHint: "Adapter an RS485-1 oder RS485-2 dieses Packs. Kein anderes Pack und kein anderes Programm am Bus.",
        monitorWillStop: "Die Monitor-Verbindung wird beim Prüfen getrennt.",
        s2: "Image", chooseImage: ".ehex-Datei wählen", imageInfo: "{{release}} · {{blocks}} Blöcke · {{bytes}} Byte", sha: "SHA-256 {{sha}}",
        s3: "Pack prüfen", inspect: "Pack prüfen", inspecting: "Prüfe (Bus still? Pack antwortet?) …", target: "{{name}}, Firmware {{version}}, {{can}}",
        s4: "Bestätigen", summary: "Pack {{address}}: Firmware {{from}} → {{to}}",
        risks: "Ab dem Löschen hat das Pack keine gültige Firmware, bis das Update fertig ist. Strom und Kabel nicht trennen, das Fenster bleibt offen. Blöcke werden nicht wiederholt.",
        typeToConfirm: "Zum Bestätigen {{phrase}} eintippen", start: "Firmware aufspielen",
        s5: "Ablauf", elapsed: "{{s}} s",
        stage: { preflight: "Prüfe Pack …", bootloader: "Starte Bootloader …", erase: "Lösche …", block: "Schreibe Block {{n}} von {{total}}", finalize: "Schließe ab …", app_restart: "Warte auf Neustart …", post_check: "Prüfe neue Firmware …" },
        busy: "Ein Firmware-Update läuft. Das Fenster bleibt offen, bis es fertig ist.",
        okTitle: "Update erfolgreich", okText: "Firmware {{from}} → {{to}}",
        failTitle: "Update fehlgeschlagen", failStage: "Abgebrochen bei: {{stage}}",
        failBefore: "Die alte Firmware läuft weiter; es wurde nichts geschrieben.",
        failAfter: "Das Pack ist vermutlich im Update-Modus. Nicht einfach neu starten: Log sichern und Hilfe lesen.",
        log: "Log", showLog: "Im Ordner zeigen", back: "Zurück zum Monitor", again: "Neues Update",
      },
```

English:

```ts
      maint: "Show maintenance functions",
      fw: {
        title: "Firmware", intro: "Writes an original Seplos firmware to a single pack over RS485-1/2 at 19200 baud. Released versions only.",
        demo: "Firmware updates need the desktop app.",
        s1: "Connection", port: "Port", choosePort: "Choose port", address: "Pack address (DIP)",
        busHint: "Adapter on RS485-1 or RS485-2 of this pack. No other pack and no other program on the bus.",
        monitorWillStop: "Checking the pack disconnects the monitor.",
        s2: "Image", chooseImage: "Choose .ehex file", imageInfo: "{{release}} · {{blocks}} blocks · {{bytes}} bytes", sha: "SHA-256 {{sha}}",
        s3: "Check pack", inspect: "Check pack", inspecting: "Checking (bus quiet? pack answers?) …", target: "{{name}}, firmware {{version}}, {{can}}",
        s4: "Confirm", summary: "Pack {{address}}: firmware {{from}} → {{to}}",
        risks: "From erasing on, the pack has no valid firmware until the update completes. Do not disconnect power or cable; the window stays open. Blocks are not retried.",
        typeToConfirm: "Type {{phrase}} to confirm", start: "Write firmware",
        s5: "Progress", elapsed: "{{s}} s",
        stage: { preflight: "Checking pack …", bootloader: "Starting bootloader …", erase: "Erasing …", block: "Writing block {{n}} of {{total}}", finalize: "Finishing …", app_restart: "Waiting for restart …", post_check: "Checking new firmware …" },
        busy: "A firmware update is running. The window stays open until it completes.",
        okTitle: "Update complete", okText: "Firmware {{from}} → {{to}}",
        failTitle: "Update failed", failStage: "Stopped at: {{stage}}",
        failBefore: "The old firmware keeps running; nothing was written.",
        failAfter: "The pack is probably in update mode. Do not just restart it: keep the log and read the help.",
        log: "Log", showLog: "Show in folder", back: "Back to monitor", again: "New update",
      },
```

- [ ] **Step 2: Help topic** — add to `TOPICS` in `tools/gen_help.py` (after `topic.buses`):

```python
"topic.firmware": ("topic.buses, topic.connection", (
 "Firmware-Update", "Spielt eine Original-Firmware von Seplos auf ein einzelnes Pack. Nur sichtbar, wenn die Wartungsfunktionen eingeschaltet sind.",
 "- **Warum RS485-1/2:** Der Bootloader des BMS hört nur auf diesem Bus (19200 Baud). Über die CAN-Buchse geht kein Update.\n- **Warum nur ein Pack am Bus:** Die Rahmen des Bootloaders tragen keine Pack-Adresse. Ein Master, der seine Slaves abfragt, oder ein zweites Programm stört das Update. Die App prüft vorher etwa 2 s lang, ob der Bus still ist.\n- **Ablauf:** Die App startet das Pack per Befehl neu. In den ersten etwa 0,7 s antwortet der Bootloader; die App meldet sich darin an, löscht, schreibt die Blöcke und schließt ab. Danach startet die neue Firmware, und die App liest Version und Werte zur Kontrolle.\n- **Nur freigegebene Images:** unveränderte Original-Firmware von Seplos (16.06.x), erkannt an der Prüfsumme. Die Parameter des Packs bleiben erhalten.\n- **Wenn etwas schiefgeht:** Ein Fehler vor dem Löschen ändert nichts, die alte Firmware läuft weiter. Ein Fehler danach kann das Pack im Update-Modus lassen. Dann das Log sichern (Ordner `firmware-logs` der App-Daten) und nicht einfach neu starten oder das Update blind wiederholen."),
 ("Firmware update", "Writes an original Seplos firmware to a single pack. Visible only when maintenance functions are switched on.",
 "- **Why RS485-1/2:** the BMS bootloader listens only on this bus (19200 baud). No update over the CAN socket.\n- **Why one pack on the bus:** the bootloader's frames carry no pack address. A master polling its slaves, or a second program, disturbs the update. The app first checks for about 2 s that the bus is quiet.\n- **Sequence:** the app restarts the pack by command. For about the first 0.7 s the bootloader answers; the app logs in, erases, writes the blocks and finishes. Then the new firmware starts, and the app reads version and values as a check.\n- **Released images only:** unmodified original Seplos firmware (16.06.x), recognised by its checksum. The pack's parameters are kept.\n- **If something goes wrong:** an error before erasing changes nothing, the old firmware keeps running. An error after that can leave the pack in update mode. Keep the log (folder `firmware-logs` in the app data) and do not just restart or blindly repeat the update.")),
```

Run: `python3 tools/gen_help.py`
Expected: prints the entry count (one more than before); `help/de/topic.firmware.md` and `help/en/topic.firmware.md` exist.

- [ ] **Step 3: View `app/src/views/Firmware.tsx`**

```tsx
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, FolderOpen, TriangleAlert } from "lucide-react";
import { api, isDemo, type DeviceInfo, type FirmwareDone, type FirmwareImage, type FirmwareProgress } from "../api";
import { Button } from "../components/ui/button";
import { useNow } from "../components/live";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { confirmationPhrase, progressFraction, steps } from "../firmware";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

function Step({ n, title, active, children }: { n: number; title: string; active: boolean; children: React.ReactNode }) {
  return (
    <section className={cn("space-y-3 border-t border-line pt-5", !active && "opacity-50")}>
      <h2 className="font-display text-lg font-semibold"><span className="mr-2 text-muted">{n}</span>{title}</h2>
      {children}
    </section>
  );
}

export function Firmware({ connected, onMonitorStopped, onRunning, onBack }: {
  connected: boolean;
  onMonitorStopped: () => void;
  onRunning: (running: boolean) => void;
  onBack: () => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [ports, setPorts] = useState<string[]>([]);
  const [port, setPort] = useState("");
  const [address, setAddress] = useState(0);
  const [path, setPath] = useState<string | null>(null);
  const [image, setImage] = useState<FirmwareImage | null>(null);
  const [target, setTarget] = useState<DeviceInfo | null>(null);
  const [checkedFor, setCheckedFor] = useState<{ port: string; address: number } | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<FirmwareProgress | null>(null);
  const [done, setDone] = useState<FirmwareDone | null>(null);
  const [closeRefused, setCloseRefused] = useState(false);
  const startedAt = useRef(0);
  const now = useNow(500);

  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);
  useEffect(() => {
    const subs = [
      api.onFirmwareProgress(setProgress),
      api.onFirmwareDone((d) => { setDone(d); setRunning(false); onRunning(false); }),
      api.onFirmwareBusy(() => setCloseRefused(true)),
    ];
    return () => { subs.forEach((s) => s.then((f) => f())); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const s = steps({ running, port, address, image, target, checkedFor, typed });
  const phrase = confirmationPhrase(address);

  const chooseImage = async () => {
    setError(null);
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, filters: [{ name: "EMU1101 firmware", extensions: ["ehex"] }] });
    if (typeof picked !== "string") return;
    setImage(null); setPath(null);
    try { setImage(await api.firmwareCheckImage(picked, lang)); setPath(picked); } catch (e) { setError(String(e)); }
  };

  const inspect = async () => {
    setBusy(true); setError(null); setTarget(null); setCheckedFor(null);
    // the backend drops the monitor connection first; the UI must forget it too
    if (connected) onMonitorStopped();
    try { setTarget(await api.firmwareInspect(port, address, lang)); setCheckedFor({ port, address }); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const start = async () => {
    if (!path) return;
    setError(null); setDone(null); setProgress(null); setCloseRefused(false);
    try {
      setRunning(true); onRunning(true); startedAt.current = Date.now();
      await api.firmwareStart(port, address, path, typed, lang);
    } catch (e) { setRunning(false); onRunning(false); setError(String(e)); }
  };

  const reset = () => { setDone(null); setProgress(null); setTyped(""); setTarget(null); setCheckedFor(null); };

  if (isDemo) return <div className="max-w-2xl space-y-3"><h1 className="font-display text-2xl font-semibold">{t("fw.title")}</h1><p className="text-muted">{t("fw.demo")}</p></div>;

  return (
    <div className="max-w-2xl space-y-6">
      <header className="space-y-2">
        <h1 className="flex items-center gap-2 font-display text-2xl font-semibold">{t("fw.title")}<InfoIcon id="topic.firmware" /></h1>
        <p className="text-muted">{t("fw.intro")}</p>
      </header>

      <Step n={1} title={t("fw.s1")} active={!running && !done}>
        <div className="flex flex-wrap gap-3">
          <select className={input} value={port} disabled={running} onChange={(e) => setPort(e.target.value)} aria-label={t("fw.port")}>
            <option value="">{t("fw.choosePort")}</option>
            {ports.map((p) => <option key={p}>{p}</option>)}
          </select>
          <label className="flex items-center gap-2 text-sm text-muted">{t("fw.address")}
            <select className={input} value={address} disabled={running} onChange={(e) => setAddress(Number(e.target.value))}>
              {Array.from({ length: 16 }, (_, a) => <option key={a} value={a}>{a}</option>)}
            </select>
          </label>
        </div>
        <p className="text-sm text-muted">{t("fw.busHint")}</p>
        {connected && <p className="text-sm text-discharge">{t("fw.monitorWillStop")}</p>}
      </Step>

      <Step n={2} title={t("fw.s2")} active={s.image}>
        <Button variant="ghost" disabled={!s.image} onClick={chooseImage}>{t("fw.chooseImage")}</Button>
        {image && <div className="text-sm"><p>{t("fw.imageInfo", { release: image.release, blocks: image.blocks, bytes: image.bytes })}</p><p className="break-all text-muted">{t("fw.sha", { sha: image.sha256 })}</p></div>}
      </Step>

      <Step n={3} title={t("fw.s3")} active={s.inspect}>
        <Button variant="ghost" disabled={!s.inspect || busy} onClick={inspect}>{busy ? t("fw.inspecting") : t("fw.inspect")}</Button>
        {target && s.confirm && <p className="text-sm">{t("fw.target", { name: target.device_name, version: target.firmware_version, can: target.can_protocol })}</p>}
      </Step>

      <Step n={4} title={t("fw.s4")} active={s.confirm}>
        {s.confirm && image && target && <p className="font-medium">{t("fw.summary", { address, from: target.firmware_version, to: image.release })}</p>}
        <p className="text-sm text-alarm">{t("fw.risks")}</p>
        <label className="block space-y-1 text-sm text-muted">{t("fw.typeToConfirm", { phrase })}
          <input className={cn(input, "block w-48 font-mono")} value={typed} disabled={!s.confirm} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} />
        </label>
        <Button variant="primary" disabled={!s.start} onClick={start}>{t("fw.start")}</Button>
      </Step>

      {(running || done) && (
        <Step n={5} title={t("fw.s5")} active>
          {running && (
            <>
              <p>{progress ? t(`fw.stage.${progress.stage}`, { n: (progress.block ?? 0) + 1, total: progress.total ?? 0 }) : t("fw.stage.preflight")}</p>
              <div className="h-2 overflow-hidden rounded bg-sunken"><div className="h-full bg-charge transition-all" style={{ width: `${progressFraction(progress) * 100}%` }} /></div>
              <p className="text-sm text-muted">{t("fw.elapsed", { s: Math.round((now - startedAt.current) / 1000) })}</p>
              {closeRefused && <p className="text-sm text-discharge">{t("fw.busy")}</p>}
            </>
          )}
          {done && (
            <div className="space-y-2">
              {done.ok ? (
                <p className="flex items-center gap-2 font-medium text-ok"><CircleCheck className="h-5 w-5" />{t("fw.okTitle")}: {t("fw.okText", { from: done.before?.firmware_version, to: done.after?.firmware_version })}</p>
              ) : (
                <>
                  <p className="flex items-center gap-2 font-medium text-alarm"><TriangleAlert className="h-5 w-5" />{t("fw.failTitle")}</p>
                  <p className="text-sm">{done.error}</p>
                  {done.stage && <p className="text-sm text-muted">{t("fw.failStage", { stage: t(`fw.stage.${done.stage}`, { n: (progress?.block ?? 0) + 1, total: progress?.total ?? 0 }) })}</p>}
                  <p className={cn("text-sm", done.written ? "text-alarm" : "text-muted")}>{done.written ? t("fw.failAfter") : t("fw.failBefore")}</p>
                </>
              )}
              <p className="break-all text-sm text-muted">{t("fw.log")}: {done.log}</p>
              <div className="flex gap-2">
                <Button variant="ghost" size="sm" onClick={() => api.revealFirmwareLogs(done.log)}><FolderOpen className="h-4 w-4" />{t("fw.showLog")}</Button>
                <Button variant="ghost" size="sm" onClick={reset}>{t("fw.again")}</Button>
                <Button variant="primary" size="sm" onClick={onBack}>{t("fw.back")}</Button>
              </div>
            </div>
          )}
        </Step>
      )}

      {error && <p className="text-sm text-alarm">{error}</p>}
    </div>
  );
}
```

`Button` variants used here (`primary`, `ghost`) and sizes (`sm`) exist in `app/src/components/ui/button.tsx`.

- [ ] **Step 4: Routing in `App.tsx`**

Add `import { Firmware } from "./views/Firmware";`, state `const [flashing, setFlashing] = useState(false);` and `const [maintenance, setMaintenance] = useState(() => localStorage.getItem("maint.enabled") === "1");`. Pass to `Sidebar`: `locked={flashing}`, `maintenance={maintenance}`, `setMaintenance={(v) => { localStorage.setItem("maint.enabled", v ? "1" : "0"); setMaintenance(v); if (!v && view === "firmware") setView("dashboard"); }}`. In `<main>`, as the first branch:

```tsx
          {view === "firmware" && maintenance ? (
            <Firmware connected={connected} onRunning={setFlashing} onBack={() => setView("dashboard")}
              onMonitorStopped={() => { live.reset(); setConnected(false); setPack(null); }} />
          ) : view === "cells" ? (
```

- [ ] **Step 5: Sidebar** (`app/src/views/Sidebar.tsx`)

- Props: add `locked: boolean; maintenance: boolean; setMaintenance: (v: boolean) => void;`.
- Import `Cpu` from `lucide-react` and `Switch` from `../components/PackPicker`.
- Nav list: `[...base, ...(maintenance ? [["firmware", Cpu]] : [])]` where `base` is the existing array; each nav button gets `disabled={locked}` and `disabled:opacity-50` in its class.
- Connect button: `disabled={busy || !target || locked}`; disconnect button `disabled={locked}`.
- Auto-reconnect effect: add `locked` to its guard (`if (lost && !busy && !connected && !locked && …)`), so the monitor does not grab the port during a flash.
- Above the language/theme row, add:

```tsx
      <label className="flex items-center gap-2 border-t border-line px-5 py-3 text-sm text-muted">
        <Switch checked={maintenance} onChange={setMaintenance} disabled={locked} />{t("maint")}
      </label>
```

- [ ] **Step 6: Checks**

Run: `cd app && npm test && npx tsc -b && cd .. && cargo clippy --workspace`
Expected: pass. Then `cd app && npm run dev` and open the browser preview: with the switch off there is no "Firmware" entry; switched on, the view shows the demo note.

- [ ] **Step 7: Commit**

```bash
git add app/src tools/gen_help.py help
git commit -m "App: firmware view behind the maintenance switch, navigation lock during a flash, help

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Documentation and hardware check

**Files:**
- Modify: `README.md`, `CLAUDE.md`

- [ ] **Step 1: README** — rename the section `## Firmware updater (separate CLI)` to `## Firmware updater` and add as first paragraph:

```markdown
**In the app:** switch on *Show maintenance functions* at the bottom of the sidebar; the *Firmware* view then guides through port and pack address, image check, pack check, confirmation (`FLASH <address>`) and progress. It accepts only the allowlisted original images, uses a serial adapter on RS485-1/2 (19200 baud) with a single pack on the bus, and writes a log to the app data folder (`firmware-logs`). While it runs, the monitor is disconnected and the window cannot be closed.
```

Also change the status line near the top from "The monitor is read only; the separate `openbms-flash` CLI can write firmware after explicit confirmation." to "The monitor is read only; firmware updates (CLI `openbms-flash`, or the app's maintenance view) write only after explicit confirmation."

- [ ] **Step 2: CLAUDE.md** — first paragraph: "The monitor is read only unless maintenance functions are switched on (firmware view)." Layout: add `app/src-tauri/src/firmware.rs` (firmware commands, progress events, flash guard) and `app/src/views/Firmware.tsx` + `app/src/firmware.ts` (view and step logic).

- [ ] **Step 3: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "Docs: firmware update in the app

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 4: Hardware check (done by the user at the bench board)**

`npm run tauri dev`, connect the monitor on the same serial port (pack bus), switch on maintenance functions, choose the 16.06.13 image, check pack (the monitor must disconnect without "port busy"), confirm, flash. Expected: progress over all blocks, success with version before/after, log in `firmware-logs`. Compare the log's TX lines with a CLI flash log of the same image: identical frames. Try closing the window during the run: refused with the notice.
