# Maintenance Mode, Stage 1 (local) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Flash any pack of a multi-pack bank from the app with USB adapters on both buses (UART1 control, UART0 work). The master's polling is stopped either by maintenance mode of the patched master (CID2 0x64) or by DIP 5–8 OFF. Every risky action is unlocked only after a fresh measurement, and an open maintenance session stays visible until it has been ended and verified.

**Architecture:**
- **Protocol logic** goes into a new module `openbms_flash::maint` (Rust, no app dependencies). It holds the 0x64 dialog, the pack bus measurement, the end-of-maintenance listener and the pure master-flash decision table. The logic runs over the existing `Link` trait, so a fake link with a virtual clock tests it.
- **App backend** (`app/src-tauri`):
  - persists installations, master state, sessions and flash blocks in a small SQLite file;
  - routes control exchanges through the main connection when both point at the same endpoint;
  - gates `firmware_start`.
- **UI:**
  - pure state helpers in `app/src/maint.ts`;
  - a banner above every view;
  - a "Pack bus" panel at the top of the firmware view.

**Tech Stack:** Rust 2021 workspace (`openbms-proto`, `openbms-transport`, `openbms-flash`, Tauri 2 app crate with `rusqlite` 0.40), React 19 + TypeScript + Tailwind 4, vitest 3, react-i18next.

**Spec:** `docs/superpowers/specs/2026-10-12-maintenance-mode-design.md`. Read it before starting a task; this plan argues from it.

## Global Constraints

- The repo is public. No values, pack numbers, dates or files of a real installation in code, tests, help or comments. Tests use synthetic data.
- Code, comments, identifiers: English. UI text: German and English in `app/src/i18n.ts`. Help texts come from `tools/gen_help.py`; run `python3 tools/gen_help.py` after changing it.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Pushing to `origin/main` is routine for this repo.
- Frames from the spec, verbatim:
  - status `~20004664E00202FD31`
  - start `~20004664A00601C35AFC46`
  - stop `~20004664A00600C35AFC47`
  - reply INFO `[marker 0/1, effective slaves]`; format/token errors RTN 05; original firmware RTN 04.
- Time budgets (spec, step 2), both on a monotonic clock: connection setup ≤ 5 s; state change ≤ 3 s from the valid start/stop reply. For `N` = 0, wait at least 1.1 s after `stop`.
- Measurement (spec, step 3): no bytes for 2 s, then 0x47 answered by address 0. For patched masters it also needs a fresh UART1 status `1/0` taken just before. Wording in the UI: "bus quiet at hh:mm:ss", never "bus exclusive".
- Inverter confirmation: kept only in memory and only for the current work phase. It is never stored.
- `marker-unexplained`:
  - no `stop` is sent or offered, no "End", no hint to restore the DIPs, no inverter release;
  - the only way out is the recovery row (original image under DIP 5–8 OFF, no `stop`) or the user's statement that they set the marker earlier.
- An unclear update (`update-unclear`, `bootloader-unclear`) blocks every further flash for this installation. It is persistent and is cleared only when that pack answers 0x51 and 0x47 again, or when the user confirms it was isolated or recovered.
- Stage 2 (TCP flashing through the gateway) is **not** part of this plan. The firmware view keeps its serial-only pack port.
- Checks that must pass before every commit:
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cd app && npx vitest run`
  - `cd app && npx tsc -b`

## Decisions this plan takes where the spec leaves room

1. **Own database file `maintenance.sqlite`** (app data folder) instead of new tables in `history.sqlite`. `history.rs::open_db` moves the whole history file aside when its schema version changes, which would silently drop an open session or a flash block. The new file uses only `CREATE TABLE IF NOT EXISTS` and never bumps a version. *Update the spec's Backend section accordingly (Task 12).*
2. **No separate endpoint-lock registry.** All maintenance operations (dialogs, measurement, end check, firmware inspect/start) already serialize on `AppState.port_busy` (one "maintenance operation" lock). A control exchange whose endpoint equals the main connection's endpoint runs through the main connection while holding its mutex, so the polling thread simply waits. Together this keeps the spec's rule "one physical endpoint is never opened twice".
3. **Master-flash rows the table does not list:**
   - `no-0x64` → original: DIP 5–8 OFF, measure; afterwards RTN 04 expected.
   - `no-0x64` → custom: custom row without `stop`.
   - `marker-unexplained` → maintenance or custom: refused (locked).
   - `unknown` → anything: refused.
   - `patched-off` → custom: `stop` is kept, because the spec says "if the master was patched".
4. **The maintenance build counts as custom firmware for the opt-in.** It needs "Allow custom firmware" and the phrase `FLASH CUSTOM n`. It is still recognised by its code hash (`ImageKind::Maintenance`), so the decision table can tell it apart.
5. **Sequential way** (one adapter): the control endpoint and the firmware pack port are the same USB path. The UI then asks to move the cable before each dialog and before each measurement. No separate backend path.
6. **Parameter writing panel:** the parameter view has no write mode yet. The panel is added to the firmware view only, and the parameter view gets it when writing is built.

## Review Focus

- **Control endpoint equals the main connection.** Example: the sidebar is connected via USB to the CAN socket bus, and the same port is set as control channel. A dialog must not try to open the port a second time ("busy"). It runs over the main connection while polling waits. → Task 6, test `control_route_uses_main_connection_only_for_same_endpoint_on_can_bus`.
- **App restart or reconnect with an open session.**
  - The banner and `N` are still there.
  - The inverter tick is cleared.
  - A `1/0` with an open marker session is `patched-on`, not `marker-unexplained`.
  - → Task 5, test `session_and_block_survive_reopen`; Task 4, test `classify_uses_the_open_session`; Task 9, test `inverter_confirmation_is_not_part_of_the_overview`.
- **`stop` in a locked state.** Every path that could send `stop` must refuse while the state is `marker-unexplained`: the End button, a custom-image master flash and an API call from a stale UI. → Task 6, test `stop_is_refused_while_marker_unexplained`; Task 4, test `marker_unexplained_allows_only_recovery_to_original`.
- **Stale measurement.** A measurement taken minutes ago, or before a reconnect, must not unlock a flash. `firmware_start` measures again by itself right before `0x45`. → Task 8, test `flash_gate_refuses_without_inverter_or_with_block`. The gate does not trust a UI timestamp at all, and the fresh `measure` call inside `firmware_start` is covered by the bench step in Task 13.
- **Flash failure in each phase.** A failure after `R F L` or after `W F E` must leave a persistent block when the pack does not answer as an application again. A failure in preflight leaves none. → Task 8, test `block_reason_by_stage`.

---

### Task 0: Baseline

The work tree holds uncommitted changes of the same day (sidebar, pack count via 0x64, `maint.rs` in `openbms-proto`, `maint_status`). They are the base of this plan.

- [x] Done: baseline commit `cdca30a` (user approved; checks passed).

---

### Task 1: Image kind (original, maintenance build, custom)

**Files:**
- Modify: `crates/openbms-flash/src/known.rs`
- Modify: `app/src-tauri/src/firmware.rs` (struct `ImageInfo`, fn `image_info`, its test)
- Modify: `app/src/api.ts` (`FirmwareImage`)

**Interfaces:**
- Produces: `openbms_flash::known::{ImageKind, MAINT_BUILDS, kind_of_hash(&str) -> (ImageKind, Option<&'static Release>), kind(&Image) -> (ImageKind, Option<&'static Release>)}`. `ImageInfo` gains `kind: &'static str` (`"original" | "maintenance" | "custom"`).

- [ ] **Step 1: Write the failing test** (append to `known.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_of_hash_tells_originals_maintenance_builds_and_custom_apart() {
        let (k, r) = kind_of_hash(RELEASES[0].code_sha256);
        assert_eq!((k, r.map(|r| r.name)), (ImageKind::Original, Some(RELEASES[0].name)));
        let (k, r) = kind_of_hash(MAINT_BUILDS[0].code_sha256);
        assert_eq!((k, r.map(|r| r.name)), (ImageKind::Maintenance, Some("16.06.13 MAINT")));
        assert_eq!(kind_of_hash(&"0".repeat(64)), (ImageKind::Custom, None));
    }

    #[test]
    fn identify_still_accepts_only_vendor_releases() {
        let (_, r) = kind_of_hash(MAINT_BUILDS[0].code_sha256);
        assert!(RELEASES.iter().all(|v| v.code_sha256 != r.unwrap().code_sha256));
    }
}
```

`Release` needs `#[derive(Debug, PartialEq, Eq)]` for the `assert_eq!`.

- [ ] **Step 2: Run it.** `cargo test -p openbms-flash known` → FAIL (`kind_of_hash` not found).

- [ ] **Step 3: Implement** (in `known.rs`, after `RELEASES`)

```rust
/// Maintenance builds: patched 16.06.13 with CID2 0x64 on the CAN socket bus. Modified firmware, so
/// accepted only like custom images; known here so the app can plan a flash of the master.
pub const MAINT_BUILDS: &[Release] = &[Release {
    name: "16.06.13 MAINT",
    code_sha256: "4d76a96fa7f7ad0e1aa3de1ac4fbe51f0fc4ae03a19aea2d4e696c2812d8f55d",
}];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// unmodified vendor release (allowlist above)
    Original,
    /// a known maintenance build
    Maintenance,
    /// anything else that passes the format checks
    Custom,
}

pub fn kind_of_hash(code_sha256: &str) -> (ImageKind, Option<&'static Release>) {
    if let Some(r) = RELEASES.iter().find(|r| r.code_sha256 == code_sha256) {
        return (ImageKind::Original, Some(r));
    }
    if let Some(r) = MAINT_BUILDS.iter().find(|r| r.code_sha256 == code_sha256) {
        return (ImageKind::Maintenance, Some(r));
    }
    (ImageKind::Custom, None)
}

pub fn kind(image: &Image) -> (ImageKind, Option<&'static Release>) {
    kind_of_hash(&image.code_sha256)
}
```

- [ ] **Step 4: App side.** In `firmware.rs`:
  - add `kind: &'static str` to `ImageInfo`;
  - in `image_info`, after the `release` match, add:

```rust
    let (kind, known_build) = known::kind(&image);
    let kind = match kind {
        known::ImageKind::Original => "original",
        known::ImageKind::Maintenance => "maintenance",
        known::ImageKind::Custom => "custom",
    };
```

  - set `release: release.or(known_build.map(|r| r.name)).unwrap_or("custom").into()` and `kind`;
  - keep `custom: release.is_none()`, so the maintenance build stays opt-in;
  - extend the existing test `image_info_rejects_unknown_code_hash` with `assert_eq!(info.kind, "custom");`.
- [ ] **Step 5:** In `app/src/api.ts`, add `kind: "original" | "maintenance" | "custom"` to `FirmwareImage`.
- [ ] **Step 6:** Run `cargo test --workspace` and `cd app && npx tsc -b` → PASS.
- [ ] **Step 7: Commit.** `git commit -am "feat(flash): recognise the maintenance build by its code hash"` (with trailer).

---

### Task 2: 0x64 dialog over a link, with a virtual clock

**Files:**
- Create: `crates/openbms-flash/src/maint/mod.rs`, `crates/openbms-flash/src/maint/dialog.rs`, `crates/openbms-flash/src/maint/testkit.rs`
- Modify: `crates/openbms-flash/src/lib.rs` (`pub mod maint;`)

**Interfaces:**
- Produces:
  - `maint::Clock` (trait, `fn now(&self) -> Duration`) and `maint::SystemClock::new()`
  - `maint::dialog::{Probe, Action, Budgets, DialogError, probe(link, clock, Budgets) -> crate::Result<Probe>, dialog(link, clock, Action, Option<u8>, Budgets) -> Result<MaintStatus, DialogError>}`
  - test-only `maint::testkit::{VTime, FakeLink, answer(address, rtn, info) -> Vec<u8>}`

- [ ] **Step 1: Module skeleton** `maint/mod.rs`

```rust
//! Maintenance of a multi-pack bank: the master's CID2 0x64 dialog on the CAN socket bus, the pack bus
//! checks around it, and the decision which preparation a flash of the master needs.

pub mod bus;
pub mod dialog;
pub mod policy;
#[cfg(test)]
pub(crate) mod testkit;

use std::time::{Duration, Instant};

/// Monotonic time since an arbitrary start; tests use a virtual clock.
pub trait Clock {
    fn now(&self) -> Duration;
}

pub struct SystemClock(Instant);

impl SystemClock {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}
```

For this task, create `bus.rs` and `policy.rs` with only a `//!` line each, so the module compiles. Tasks 3 and 4 fill them.

- [ ] **Step 2: Test kit** `maint/testkit.rs`

```rust
//! Fake link with virtual time: a read with nothing due advances time by 100 ms (the real port's read
//! timeout), `wait` advances by its duration. A responder answers each written request.

use crate::{link::Link, Error, Result};
use openbms_proto::frame;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Default)]
pub struct VTime(Rc<Cell<Duration>>);

impl VTime {
    pub fn advance(&self, d: Duration) {
        self.0.set(self.0.get() + d);
    }
}

impl super::Clock for VTime {
    fn now(&self) -> Duration {
        self.0.get()
    }
}

type Responder = Box<dyn FnMut(&[u8], Duration) -> Vec<(Duration, Vec<u8>)>>;

pub struct FakeLink {
    pub time: VTime,
    incoming: Vec<(Duration, Vec<u8>)>,
    responder: Responder,
    pub sent: Vec<Vec<u8>>,
    /// every write fails (a closed port)
    pub broken: bool,
}

impl FakeLink {
    pub fn new(time: VTime, responder: impl FnMut(&[u8], Duration) -> Vec<(Duration, Vec<u8>)> + 'static) -> Self {
        Self { time, incoming: Vec::new(), responder: Box::new(responder), sent: Vec::new(), broken: false }
    }

    /// Bytes that arrive at `at` without a request (another sender on the bus).
    pub fn inject(&mut self, at: Duration, bytes: &[u8]) {
        self.incoming.push((at, bytes.to_vec()));
        self.incoming.sort_by_key(|(t, _)| *t);
    }

    fn now(&self) -> Duration {
        use super::Clock;
        self.time.now()
    }
}

impl Link for FakeLink {
    fn clear_input(&mut self) -> Result<()> {
        let now = self.now();
        self.incoming.retain(|(t, _)| *t > now);
        Ok(())
    }

    fn write_raw(&mut self, bytes: &[u8]) -> Result<()> {
        if self.broken {
            return Err(Error::Transport("port closed".into()));
        }
        self.sent.push(bytes.to_vec());
        let now = self.now();
        let replies = (self.responder)(bytes, now);
        self.incoming.extend(replies);
        self.incoming.sort_by_key(|(t, _)| *t);
        Ok(())
    }

    fn read_raw(&mut self, out: &mut [u8]) -> Result<usize> {
        let now = self.now();
        if let Some(pos) = self.incoming.iter().position(|(t, _)| *t <= now) {
            let (_, bytes) = self.incoming.remove(pos);
            let n = bytes.len().min(out.len());
            out[..n].copy_from_slice(&bytes[..n]);
            if n < bytes.len() {
                self.incoming.insert(0, (now, bytes[n..].to_vec()));
            }
            return Ok(n);
        }
        self.time.advance(Duration::from_millis(100));
        Ok(0)
    }

    fn wait(&mut self, d: Duration) {
        self.time.advance(d);
    }
}

/// An ASCII answer frame from `address` with `rtn` and binary `info`.
pub fn answer(address: u8, rtn: u8, info: &[u8]) -> Vec<u8> {
    let hex: String = info.iter().map(|b| format!("{b:02X}")).collect();
    let len = frame::length_field(hex.len()).unwrap();
    let body = format!("20{address:02X}46{rtn:02X}{len:04X}{hex}");
    format!("~{body}{:04X}\r", frame::checksum(body.as_bytes())).into_bytes()
}

pub fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
```

- [ ] **Step 3: Write the failing tests** (bottom of `maint/dialog.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::maint::testkit::{answer, ms, FakeLink, VTime};
    use crate::maint::Clock;
    use openbms_proto::frame;

    /// A patched master on UART1. A start or stop takes effect in the status `delay` later.
    struct Master { dip: u8, marker: bool, active: bool, switch_at: Option<Duration>, delay: Duration }

    impl Master {
        fn new(dip: u8, marker: bool, delay: Duration) -> Self {
            Self { dip, marker, active: marker, switch_at: None, delay }
        }
        fn reply(&mut self, req: &[u8], now: Duration) -> Vec<(Duration, Vec<u8>)> {
            if self.switch_at.is_some_and(|t| now >= t) {
                self.active = self.marker;
                self.switch_at = None;
            }
            let Ok(f) = frame::parse(req) else { return vec![] };
            if f.rtn != 0x64 {
                return vec![];
            }
            match f.info.as_slice() {
                [0x02] => {}
                [on @ (0 | 1), 0xC3, 0x5A] => {
                    self.marker = *on == 1;
                    self.switch_at = Some(now + self.delay);
                }
                _ => return vec![(now + ms(40), answer(0, 0x05, &[]))],
            }
            let slaves = if self.active { 0 } else { self.dip };
            vec![(now + ms(40), answer(0, 0x00, &[u8::from(self.marker), slaves]))]
        }
    }

    fn link_with(mut m: Master) -> (FakeLink, VTime) {
        let t = VTime::default();
        (FakeLink::new(t.clone(), move |req, now| m.reply(req, now)), t)
    }

    fn st(marker: bool, slaves: u8) -> MaintStatus {
        MaintStatus { marker, effective_slaves: slaves }
    }

    #[test]
    fn start_waits_until_the_master_reports_the_marker() {
        let (mut link, t) = link_with(Master::new(11, false, ms(1000)));
        let s = dialog(&mut link, &t, Action::Start, None, Budgets::default()).unwrap();
        assert_eq!(s, st(true, 0));
        assert!(t.now() >= ms(1000));
    }

    #[test]
    fn stop_waits_for_the_expected_slave_count() {
        let (mut link, t) = link_with(Master::new(11, true, ms(1000)));
        assert_eq!(dialog(&mut link, &t, Action::Stop, Some(11), Budgets::default()).unwrap(), st(false, 11));
    }

    #[test]
    fn stop_with_zero_slaves_waits_at_least_1_1_s() {
        let (mut link, t) = link_with(Master::new(0, true, ms(200)));
        assert_eq!(dialog(&mut link, &t, Action::Stop, Some(0), Budgets::default()).unwrap(), st(false, 0));
        assert!(t.now() >= ms(1100));
    }

    #[test]
    fn a_change_that_never_shows_times_out_with_the_last_state() {
        let (mut link, t) = link_with(Master::new(11, false, ms(10_000)));
        match dialog(&mut link, &t, Action::Start, None, Budgets::default()) {
            Err(DialogError::Timeout { last, .. }) => assert_eq!(last, st(true, 11)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rtn_04_is_unsupported_and_silence_is_no_answer() {
        let t = VTime::default();
        let mut orig = FakeLink::new(t.clone(), |_, now| vec![(now + ms(40), answer(0, 0x04, &[]))]);
        assert_eq!(probe(&mut orig, &t, Budgets::default()).unwrap(), Probe::Unsupported);
        assert!(matches!(dialog(&mut orig, &t, Action::Start, None, Budgets::default()), Err(DialogError::Unsupported)));
        let mut silent = FakeLink::new(t.clone(), |_, _| vec![]);
        assert_eq!(probe(&mut silent, &t, Budgets::default()).unwrap(), Probe::NoAnswer);
    }

    #[test]
    fn a_broken_port_is_a_connection_error_not_a_status() {
        let (mut link, t) = link_with(Master::new(11, false, ms(0)));
        link.broken = true;
        assert!(matches!(dialog(&mut link, &t, Action::Status, None, Budgets::default()), Err(DialogError::Link(_))));
    }

    #[test]
    fn an_echo_of_the_request_is_not_taken_as_the_answer() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |req, now| vec![(now, req.to_vec()), (now + ms(40), answer(0, 0, &[0, 3]))]);
        assert_eq!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Status(st(false, 3)));
    }
}
```

- [ ] **Step 4: Run them.** `cargo test -p openbms-flash maint::dialog` → FAIL (items missing).

- [ ] **Step 5: Implement** (top of `maint/dialog.rs`)

```rust
//! CID2 0x64 with the patched master on the CAN socket bus (UART1, 9600 baud): read the maintenance
//! state, set or clear the marker, and wait until the master's next cycles show the change.

use super::Clock;
use crate::{link::Link, Error, Result};
use openbms_proto::{frame, MaintStatus};
use std::time::Duration;

/// What the master at address 0 answered to a 0x64 status request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Status(MaintStatus),
    /// RTN 04: the firmware does not know 0x64 (original or another build; RTN 04 does not prove which).
    Unsupported,
    /// any other RTN (05: format or token)
    Rejected(u8),
    /// no frame from address 0 within the answer time
    NoAnswer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Status,
    Start,
    Stop,
}

#[derive(Debug, Clone, Copy)]
pub struct Budgets {
    /// wait for one answer
    pub answer: Duration,
    /// from the valid start/stop reply until the master shows the new state
    pub change: Duration,
    /// pause between two status reads while waiting
    pub interval: Duration,
}

impl Default for Budgets {
    fn default() -> Self {
        Self { answer: Duration::from_millis(600), change: Duration::from_secs(3), interval: Duration::from_millis(300) }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DialogError {
    #[error("connection: {0}")]
    Link(#[from] Error),
    #[error("the master does not know CID2 0x64 (RTN 04)")]
    Unsupported,
    #[error("the master rejected the request (RTN {0:02X})")]
    Rejected(u8),
    #[error("the master at address 0 does not answer")]
    NoAnswer,
    #[error("the master did not reach the requested state within {after:?}")]
    Timeout { after: Duration, last: MaintStatus },
}

/// Minimum time after `stop` when the slave count does not change (N = 0): one full master cycle.
const ZERO_SLAVES_SETTLE: Duration = Duration::from_millis(1100);

/// Send one request and return the first frame from address 0, or None after `timeout`.
/// A frame that carries CID2 0x64 is our own request echoed by the adapter, not an answer.
fn exchange(link: &mut impl Link, clock: &impl Clock, request: &[u8], timeout: Duration) -> Result<Option<frame::Frame>> {
    link.clear_input()?;
    link.write_raw(request)?;
    let deadline = clock.now() + timeout;
    let mut pending = Vec::new();
    let mut buf = [0u8; 256];
    while clock.now() < deadline {
        let n = link.read_raw(&mut buf)?;
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\r') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let Some(start) = line.iter().position(|b| *b == b'~') else { continue };
            if let Ok(f) = frame::parse(&line[start..]) {
                if f.address == 0 && f.rtn != 0x64 {
                    return Ok(Some(f));
                }
            }
        }
        if pending.len() > 1024 {
            pending.clear();
        }
    }
    Ok(None)
}

fn to_probe(answer: Option<frame::Frame>) -> Result<Probe> {
    Ok(match answer {
        None => Probe::NoAnswer,
        Some(f) if f.rtn == 0x00 => Probe::Status(MaintStatus::parse(&f.info)?),
        Some(f) if f.rtn == 0x04 => Probe::Unsupported,
        Some(f) => Probe::Rejected(f.rtn),
    })
}

/// Read the state once. `Err` only for a link failure; every master reaction is a `Probe`.
pub fn probe(link: &mut impl Link, clock: &impl Clock, b: Budgets) -> Result<Probe> {
    let request = MaintStatus::request()?;
    to_probe(exchange(link, clock, &request, b.answer)?)
}

fn status_of(p: Probe) -> std::result::Result<MaintStatus, DialogError> {
    match p {
        Probe::Status(s) => Ok(s),
        Probe::Unsupported => Err(DialogError::Unsupported),
        Probe::Rejected(r) => Err(DialogError::Rejected(r)),
        Probe::NoAnswer => Err(DialogError::NoAnswer),
    }
}

/// Read the state (`Status`), or set (`Start`) / clear (`Stop`) the marker and wait until the master shows
/// it. `expected_slaves` (Stop only): the DIP slave count the master must report again; None = any.
pub fn dialog(
    link: &mut impl Link,
    clock: &impl Clock,
    action: Action,
    expected_slaves: Option<u8>,
    b: Budgets,
) -> std::result::Result<MaintStatus, DialogError> {
    let request = match action {
        Action::Status => return status_of(probe(link, clock, b)?),
        Action::Start => MaintStatus::set_request(true).map_err(Error::from)?,
        Action::Stop => MaintStatus::set_request(false).map_err(Error::from)?,
    };
    let mut last = status_of(to_probe(exchange(link, clock, &request, b.answer)?)?)?;
    let since = clock.now();
    let reached = |s: MaintStatus| match action {
        Action::Start => s.marker && s.effective_slaves == 0,
        _ => !s.marker && expected_slaves.is_none_or(|n| s.effective_slaves == n),
    };
    loop {
        if clock.now().saturating_sub(since) > b.change {
            return Err(DialogError::Timeout { after: b.change, last });
        }
        link.wait(b.interval);
        match status_of(probe(link, clock, b)?) {
            Ok(s) => {
                last = s;
                if reached(s) {
                    if action == Action::Stop && s.effective_slaves == 0 {
                        link.wait(ZERO_SLAVES_SETTLE.saturating_sub(clock.now().saturating_sub(since)));
                    }
                    return Ok(s);
                }
            }
            // one lost answer while waiting is not final; the budget decides
            Err(DialogError::NoAnswer) => {}
            Err(e) => return Err(e),
        }
    }
}
```

- [ ] **Step 6: Run them.** `cargo test -p openbms-flash maint::dialog` → PASS (7 tests). Then run `cargo clippy -p openbms-flash --all-targets -- -D warnings`.
- [ ] **Step 7: Commit.** `feat(flash): CID2 0x64 dialog with budgets and typed errors`

---

### Task 3: Pack bus measurement, end-of-maintenance round, application answer

**Files:**
- Modify: `crates/openbms-flash/src/maint/bus.rs`
- Modify: `crates/openbms-flash/src/session.rs` (make `quiet` and `ascii_query` `pub(crate)`)
- Modify: `crates/openbms-flash/src/lib.rs` (new `Error::MasterSilent` with German text)

**Interfaces:**
- Consumes: `maint::Clock`, testkit (Task 2).
- Produces:
  - `maint::bus::measure(link, log) -> crate::Result<()>`
  - `maint::bus::Round { polled: BTreeSet<u8>, answered: BTreeSet<u8> }` with `complete(n) -> bool` and `missing(n) -> Vec<u8>`
  - `maint::bus::watch_round(link, clock, log, slaves: u8, timeout: Duration) -> crate::Result<Round>`
  - `maint::bus::app_answers(link, log, address, within: Duration) -> bool`
  - `Error::MasterSilent`

- [ ] **Step 1: Write the failing tests** (bottom of `bus.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::maint::testkit::{answer, ms, FakeLink, VTime};
    use openbms_proto::frame;

    fn poll(a: u8) -> Vec<u8> {
        frame::encode_request(a, 0x5A, &[]).unwrap()
    }

    fn record(a: u8) -> Vec<u8> {
        answer(a, 0x5A, &[0u8; 32])
    }

    /// A quiet master that answers 0x47 (and 0x51) at address 0.
    fn quiet_master(req: &[u8], now: Duration) -> Vec<(Duration, Vec<u8>)> {
        match frame::parse(req) {
            Ok(f) if f.address == 0 && (f.rtn == 0x47 || f.rtn == 0x51) => vec![(now + ms(60), answer(0, 0, &[1, 2, 3]))],
            _ => vec![],
        }
    }

    #[test]
    fn measure_passes_on_a_quiet_bus_with_the_master_answering() {
        let mut link = FakeLink::new(VTime::default(), quiet_master);
        assert!(measure(&mut link, &mut Vec::new()).is_ok());
    }

    #[test]
    fn measure_fails_when_the_master_still_polls() {
        let mut link = FakeLink::new(VTime::default(), quiet_master);
        link.inject(ms(500), &poll(1));
        assert!(matches!(measure(&mut link, &mut Vec::new()), Err(Error::BusBusy(_))));
    }

    #[test]
    fn measure_fails_when_only_0x51_is_answered() {
        // the CAN socket bus: 0x51 yes, 0x47 no
        let mut link = FakeLink::new(VTime::default(), |req, now| match frame::parse(req) {
            Ok(f) if f.rtn == 0x51 => vec![(now + ms(60), answer(0, 0, &[1]))],
            _ => vec![],
        });
        assert!(matches!(measure(&mut link, &mut Vec::new()), Err(Error::MasterSilent)));
    }

    #[test]
    fn a_round_is_complete_only_with_poll_and_answer_of_every_slave() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        for (i, a) in [1u8, 2].iter().enumerate() {
            let at = ms(100 + 200 * i as u64);
            link.inject(at, &poll(*a));
            link.inject(at + ms(50), &record(*a));
        }
        link.inject(ms(700), &poll(3)); // slave 3 polled, never answers
        let r = watch_round(&mut link, &t, &mut Vec::new(), 3, ms(2000)).unwrap();
        assert!(!r.complete(3));
        assert_eq!(r.missing(3), vec![3]);
        assert!(r.complete(2));
    }

    #[test]
    fn a_complete_round_returns_before_the_timeout() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        link.inject(ms(100), &poll(1));
        link.inject(ms(150), b"\x00garbage\r");
        link.inject(ms(200), &record(1));
        let r = watch_round(&mut link, &t, &mut Vec::new(), 1, ms(5000)).unwrap();
        assert!(r.complete(1));
        use crate::maint::Clock;
        assert!(t.now() < ms(1000));
    }
}
```

- [ ] **Step 2: Run them.** `cargo test -p openbms-flash maint::bus` → FAIL.

- [ ] **Step 3: Implement.** In `lib.rs`, add the variant and its German text:

```rust
    #[error("the master (address 0) does not answer 0x47 here: not RS485-1/2, or the master is not quiet")]
    MasterSilent,
```

```rust
            Error::MasterSilent => "Der Master (Adresse 0) antwortet hier nicht auf 0x47: nicht RS485-1/2, oder der Master ist nicht still".into(),
```

In `session.rs`, change `fn quiet` and `fn ascii_query` to `pub(crate) fn`. Then write `bus.rs`:

```rust
//! Pack bus (RS485-1/2) checks around maintenance: before an action the bus is quiet and the master
//! answers there; at the end every slave is polled by the master and answers again.

use super::Clock;
use crate::{link::Link, session, Error, Result};
use openbms_proto::frame;
use std::collections::BTreeSet;
use std::io::Write;
use std::time::Duration;

const ASCII_TIMEOUT: Duration = Duration::from_millis(600);

/// Fresh measurement: about 2 s without any byte, then 0x47 answered by address 0. 0x47 exists only on
/// UART0, so this also proves the channel is the pack bus and the master's dispatcher runs.
pub fn measure(link: &mut impl Link, log: &mut impl Write) -> Result<()> {
    session::quiet(link, log)?;
    session::ascii_query(link, 0, 0x47, log, ASCII_TIMEOUT).map_err(|e| match e {
        Error::Timeout { .. } => Error::MasterSilent,
        other => other,
    })?;
    Ok(())
}

/// Intra-pack traffic seen on the pack bus: polls of the master (CID2 0x5A, empty INFO) and answers of
/// the slaves (0x5A in the RTN field, 32 bytes INFO), by address.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Round {
    pub polled: BTreeSet<u8>,
    pub answered: BTreeSet<u8>,
}

impl Round {
    pub fn complete(&self, slaves: u8) -> bool {
        self.missing(slaves).is_empty()
    }

    /// Slave addresses without a poll or without an answer.
    pub fn missing(&self, slaves: u8) -> Vec<u8> {
        (1..=slaves).filter(|a| !self.polled.contains(a) || !self.answered.contains(a)).collect()
    }
}

/// Listen until every address 1..=slaves was polled and answered, or `timeout` passed.
pub fn watch_round(link: &mut impl Link, clock: &impl Clock, log: &mut impl Write, slaves: u8, timeout: Duration) -> Result<Round> {
    link.clear_input()?;
    let deadline = clock.now() + timeout;
    let mut round = Round::default();
    let mut pending = Vec::new();
    let mut buf = [0u8; 512];
    while clock.now() < deadline && !round.complete(slaves) {
        let n = link.read_raw(&mut buf)?;
        if n > 0 {
            writeln!(log, "RX {}", buf[..n].iter().map(|b| format!("{b:02X}")).collect::<String>())?;
        }
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\r') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let Some(start) = line.iter().position(|b| *b == b'~') else { continue };
            if let Ok(f) = frame::parse(&line[start..]) {
                if f.rtn == 0x5A {
                    if f.info.is_empty() { round.polled.insert(f.address); } else { round.answered.insert(f.address); }
                }
            }
        }
        if pending.len() > 4096 {
            pending.clear();
        }
    }
    Ok(round)
}

/// Whether the pack answers as an application (0x51 and 0x47) within `within`. Used after a failed
/// flash: a pack that answers is not stuck in its bootloader.
pub fn app_answers(link: &mut impl Link, log: &mut impl Write, address: u8, within: Duration) -> bool {
    let deadline = std::time::Instant::now() + within;
    while std::time::Instant::now() < deadline {
        if session::ascii_query(link, address, 0x51, log, Duration::from_millis(300)).is_ok() {
            return session::ascii_query(link, address, 0x47, log, ASCII_TIMEOUT).is_ok();
        }
        link.wait(Duration::from_millis(200));
    }
    false
}
```

Note: `writeln!` returns `std::io::Error`; `Error::Io` has `#[from]`, so `?` works.

- [ ] **Step 4: Run them.** `cargo test -p openbms-flash` → PASS. Then clippy.
- [ ] **Step 5: Commit.** `feat(flash): pack bus measurement and polling-round check for maintenance`

---

### Task 4: Master state and master-flash decision table

**Files:**
- Modify: `crates/openbms-flash/src/maint/policy.rs`

**Interfaces:**
- Consumes: `maint::dialog::Probe`, `known::ImageKind`.
- Produces:
  - `policy::{MasterState, classify(Option<Probe>, marker_expected: bool) -> MasterState}`
  - `policy::{Step, Expect, Plan, Refusal, master_flash_plan(MasterState, ImageKind) -> Result<Plan, Refusal>}`
  - `policy::{Outcome, after_flash(Expect, Option<Probe>) -> Outcome}`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::known::ImageKind as K;
    use openbms_proto::MaintStatus;
    use MasterState as M;
    use Step as S;

    fn status(marker: bool, slaves: u8) -> Option<Probe> {
        Some(Probe::Status(MaintStatus { marker, effective_slaves: slaves }))
    }

    #[test]
    fn classify_uses_the_open_session() {
        assert_eq!(classify(status(false, 11), false), M::PatchedOff { slaves: 11 });
        assert_eq!(classify(status(true, 0), true), M::PatchedOn);
        assert_eq!(classify(status(true, 0), false), M::MarkerUnexplained);
        assert_eq!(classify(Some(Probe::Unsupported), false), M::No0x64);
        // never shown as no-0x64 without a verified RTN 04
        for p in [None, Some(Probe::NoAnswer), Some(Probe::Rejected(5))] {
            assert_eq!(classify(p, false), M::Unknown);
        }
    }

    #[test]
    fn every_combination_has_a_defined_row() {
        use Expect as E;
        let plan = |s, k| master_flash_plan(s, k);
        let ok = |before: &[Step], after| Ok(Plan { before: before.to_vec(), after, recovery: false });
        for k in [K::Original, K::Maintenance, K::Custom] {
            assert_eq!(plan(M::Unknown, k), Err(Refusal::Unknown));
        }
        assert_eq!(plan(M::PatchedOn, K::Maintenance), ok(&[S::Measure], E::MarkerOn));
        assert_eq!(plan(M::PatchedOn, K::Original), ok(&[S::DipOff, S::StopToZero, S::Measure], E::Unsupported));
        assert_eq!(plan(M::PatchedOff { slaves: 11 }, K::Original), Err(Refusal::QuietFirst));
        assert_eq!(plan(M::PatchedOff { slaves: 11 }, K::Maintenance), Err(Refusal::QuietFirst));
        assert_eq!(plan(M::PatchedOff { slaves: 0 }, K::Maintenance), ok(&[S::Measure], E::Zero));
        assert_eq!(plan(M::PatchedOff { slaves: 0 }, K::Original), ok(&[S::Measure], E::Unsupported));
        assert_eq!(plan(M::No0x64, K::Maintenance), ok(&[S::DipOff, S::Measure], E::ZeroOrUnexplained));
        assert_eq!(plan(M::No0x64, K::Original), ok(&[S::DipOff, S::Measure], E::Unsupported));
    }

    #[test]
    fn the_image_kind_decides_first_for_custom_images() {
        let custom = [S::DipOff, S::StopToZero, S::Measure, S::IsolatedOrReleased];
        for s in [M::PatchedOn, M::PatchedOff { slaves: 0 }, M::PatchedOff { slaves: 11 }] {
            assert_eq!(master_flash_plan(s, K::Custom).unwrap().before, custom.to_vec());
        }
        assert_eq!(master_flash_plan(M::No0x64, K::Custom).unwrap().before, vec![S::DipOff, S::Measure, S::IsolatedOrReleased]);
        assert_eq!(master_flash_plan(M::No0x64, K::Custom).unwrap().after, Expect::NoAssumption);
    }

    #[test]
    fn marker_unexplained_allows_only_recovery_to_original() {
        let r = master_flash_plan(M::MarkerUnexplained, K::Original).unwrap();
        assert!(r.recovery);
        assert_eq!(r.before, vec![S::DipOff, S::Measure]);
        assert!(!r.before.contains(&S::StopToZero));
        assert_eq!(master_flash_plan(M::MarkerUnexplained, K::Maintenance), Err(Refusal::Locked));
        assert_eq!(master_flash_plan(M::MarkerUnexplained, K::Custom), Err(Refusal::Locked));
        // the normal patched-on -> original change always clears the marker first
        assert!(master_flash_plan(M::PatchedOn, K::Original).unwrap().before.contains(&S::StopToZero));
    }

    #[test]
    fn after_flash_outcomes() {
        use Expect as E;
        assert_eq!(after_flash(E::MarkerOn, status(true, 0)), Outcome::AsExpected);
        assert_eq!(after_flash(E::MarkerOn, status(false, 11)), Outcome::Unclear);
        assert_eq!(after_flash(E::MarkerOn, None), Outcome::Unclear);
        assert_eq!(after_flash(E::Zero, status(false, 0)), Outcome::AsExpected);
        assert_eq!(after_flash(E::ZeroOrUnexplained, status(true, 0)), Outcome::MarkerUnexplained);
        assert_eq!(after_flash(E::ZeroOrUnexplained, status(false, 0)), Outcome::AsExpected);
        assert_eq!(after_flash(E::Unsupported, Some(Probe::Unsupported)), Outcome::AsExpected);
        assert_eq!(after_flash(E::Unsupported, status(false, 0)), Outcome::Unclear);
        assert_eq!(after_flash(E::NoAssumption, status(false, 0)), Outcome::UnknownImage);
    }
}
```

- [ ] **Step 2: Run them.** `cargo test -p openbms-flash maint::policy` → FAIL.

- [ ] **Step 3: Implement**

```rust
//! What the app knows about the master, and which preparation a flash of the master needs (design:
//! "Master state as seen by the app", "Flashing the master itself"). Pure, so every combination is tested.

use super::dialog::Probe;
use crate::known::ImageKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterState {
    /// patched firmware, marker 0, `slaves` effective (= DIP count)
    PatchedOff { slaves: u8 },
    /// patched firmware, marker 1, and the marker is accounted for
    PatchedOn,
    /// verified RTN 04
    No0x64,
    /// no answer, connection error, other RTN
    Unknown,
    /// marker 1 that nobody can account for: locked
    MarkerUnexplained,
}

/// `probe` None = the control channel could not be used. `marker_expected`: an open marker session
/// exists, the app has just set the marker, or the user stated where it comes from.
pub fn classify(probe: Option<Probe>, marker_expected: bool) -> MasterState {
    match probe {
        Some(Probe::Status(s)) if !s.marker => MasterState::PatchedOff { slaves: s.effective_slaves },
        Some(Probe::Status(_)) if marker_expected => MasterState::PatchedOn,
        Some(Probe::Status(_)) => MasterState::MarkerUnexplained,
        Some(Probe::Unsupported) => MasterState::No0x64,
        _ => MasterState::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// the user sets DIP 5–8 OFF and confirms
    DipOff,
    /// send `stop` and wait for 0/0 (after DipOff)
    StopToZero,
    /// quiet bus + 0x47 at address 0 (+ fresh 1/0 while the marker is set)
    Measure,
    /// not a known build: the user confirms it is released for a shared bus, or the master is isolated
    IsolatedOrReleased,
}

/// What 0x64 must show after the flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    MarkerOn,
    Zero,
    /// 0/0 expected; 1/0 means an old marker or other content in 0x23000
    ZeroOrUnexplained,
    /// RTN 04 (original firmware)
    Unsupported,
    NoAssumption,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub before: Vec<Step>,
    pub after: Expect,
    /// recovery row for `marker-unexplained`: no `stop`, sector content stays unexplained
    pub recovery: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// the master still polls: activate maintenance mode or set DIP 5–8 OFF first
    QuietFirst,
    /// nothing is unlocked while the state is unknown
    Unknown,
    /// marker of unexplained origin: only the recovery to the original image
    Locked,
}

fn row(before: &[Step], after: Expect) -> Result<Plan, Refusal> {
    Ok(Plan { before: before.to_vec(), after, recovery: false })
}

pub fn master_flash_plan(state: MasterState, image: ImageKind) -> Result<Plan, Refusal> {
    use MasterState as M;
    use Step::*;
    match (state, image) {
        (M::Unknown, _) => Err(Refusal::Unknown),
        (M::MarkerUnexplained, ImageKind::Original) => Ok(Plan { before: vec![DipOff, Measure], after: Expect::Unsupported, recovery: true }),
        (M::MarkerUnexplained, _) => Err(Refusal::Locked),
        // the image kind decides first: an unknown image may ignore the marker
        (M::PatchedOn | M::PatchedOff { .. }, ImageKind::Custom) => row(&[DipOff, StopToZero, Measure, IsolatedOrReleased], Expect::NoAssumption),
        (M::No0x64, ImageKind::Custom) => row(&[DipOff, Measure, IsolatedOrReleased], Expect::NoAssumption),
        (M::PatchedOn, ImageKind::Maintenance) => row(&[Measure], Expect::MarkerOn),
        // the original ignores the marker: clear it under DIP OFF so no hidden state stays in flash
        (M::PatchedOn, ImageKind::Original) => row(&[DipOff, StopToZero, Measure], Expect::Unsupported),
        (M::PatchedOff { slaves: 1.. }, _) => Err(Refusal::QuietFirst),
        (M::PatchedOff { slaves: 0 }, ImageKind::Maintenance) => row(&[Measure], Expect::Zero),
        (M::PatchedOff { slaves: 0 }, ImageKind::Original) => row(&[Measure], Expect::Unsupported),
        (M::No0x64, ImageKind::Maintenance) => row(&[DipOff, Measure], Expect::ZeroOrUnexplained),
        (M::No0x64, ImageKind::Original) => row(&[DipOff, Measure], Expect::Unsupported),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    AsExpected,
    /// keep the session open, no inverter release, show DIP 5–8 OFF and the recovery hint
    Unclear,
    MarkerUnexplained,
    /// custom image: show what answers, DIP 5–8 stay OFF
    UnknownImage,
}

pub fn after_flash(expect: Expect, probe: Option<Probe>) -> Outcome {
    let s = match probe {
        Some(Probe::Status(s)) => Some(s),
        _ => None,
    };
    match expect {
        Expect::NoAssumption => Outcome::UnknownImage,
        Expect::MarkerOn if s.is_some_and(|s| s.marker) => Outcome::AsExpected,
        Expect::Zero | Expect::ZeroOrUnexplained if s.is_some_and(|s| !s.marker && s.effective_slaves == 0) => Outcome::AsExpected,
        Expect::ZeroOrUnexplained if s.is_some_and(|s| s.marker) => Outcome::MarkerUnexplained,
        Expect::Unsupported if probe == Some(Probe::Unsupported) => Outcome::AsExpected,
        _ => Outcome::Unclear,
    }
}
```

- [ ] **Step 4: Run them.** `cargo test -p openbms-flash` → PASS, then clippy.
- [ ] **Step 5: Commit.** `feat(flash): master state and master-flash decision table`

---

### Task 5: TCP connection setup within one overall deadline

**Files:**
- Modify: `crates/openbms-transport/src/lib.rs` (`connect_tcp`)

**Interfaces:**
- Produces: `fn connect_tcp_within(addr: &str, budget: Duration) -> Result<TcpStream>` (private). `connect_tcp(addr)` calls it with `CONNECT_TIMEOUT`.

- [ ] **Step 1: Write the failing test** (new `#[cfg(test)] mod tests` in `transport/src/lib.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_stays_within_one_budget_across_addresses() {
        // a non-routable address: each attempt would otherwise wait for the system's own timeout
        let started = Instant::now();
        assert!(connect_tcp_within("10.255.255.1:9", Duration::from_millis(300)).is_err());
        assert!(started.elapsed() < Duration::from_millis(1500));
    }
}
```

- [ ] **Step 2: Run it.** `cargo test -p openbms-transport` → FAIL (fn missing).

- [ ] **Step 3: Implement** (replace `connect_tcp`)

```rust
/// Connect to the first address of `addr` that answers within CONNECT_TIMEOUT, name resolution included.
fn connect_tcp(addr: &str) -> Result<TcpStream> {
    connect_tcp_within(addr, CONNECT_TIMEOUT)
}

/// Resolution runs on a helper thread, so a hanging name server cannot stretch the budget; every address
/// gets only what is left of it.
fn connect_tcp_within(addr: &str, budget: Duration) -> Result<TcpStream> {
    let deadline = Instant::now() + budget;
    let (tx, rx) = std::sync::mpsc::channel();
    let name = addr.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(name.to_socket_addrs().map(|a| a.collect::<Vec<_>>()));
    });
    let timed_out = || std::io::Error::new(std::io::ErrorKind::TimedOut, format!("{addr}: no connection within {budget:?}"));
    let addrs = rx.recv_timeout(budget).map_err(|_| timed_out())??;
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("{addr}: no address"));
    for a in addrs {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(timed_out().into());
        }
        match TcpStream::connect_timeout(&a, left) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last.into())
}
```

- [ ] **Step 4: Run it.** `cargo test -p openbms-transport` → PASS.
- [ ] **Step 5: Commit.** `fix(transport): one deadline for name resolution and all addresses`

---

### Task 6: Maintenance store (installations, master state, sessions, blocks)

**Files:**
- Create: `app/src-tauri/src/maint_store.rs`
- Modify: `app/src-tauri/src/lib.rs` (`mod maint_store;`)

**Interfaces:**
- Produces (all `Result<_, String>`):
  - `endpoint_key(tcp: bool, s: &str) -> String`
  - `Installation { id, name, endpoints: Vec<String>, control: Option<ControlEndpoint> }`
  - `ControlEndpoint::{Serial{path}, Tcp{addr}}` with `key()`
  - `StoredMasterState` (serde tag `state`, kebab names incl. `"no-0x64"`), with `From<policy::MasterState>` and `Into<policy::MasterState>`
  - `SessionKind::{Marker, Dip}`, `MaintSession`, `FlashBlock`
  - `Store::{open, installation_for, installations, save_installation, master_state, set_master_state, open_session, start_session, set_session_kind, note_done, set_failing, end_session, add_block, blocks, clear_block, note_sector, sector_note}`

- [ ] **Step 1: Write the failing tests** (bottom of `maint_store.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("openbms-maint-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("maintenance.sqlite")
    }

    #[test]
    fn endpoint_keys_name_the_physical_endpoint() {
        assert_eq!(endpoint_key(true, " Gateway.local:4196 "), "tcp:gateway.local:4196");
        assert_eq!(endpoint_key(false, "/dev/cu.usbserial-X"), "serial:/dev/cu.usbserial-X");
    }

    #[test]
    fn one_installation_per_endpoint_and_explicit_reassignment() {
        let s = Store::open(&temp("inst")).unwrap();
        let a = s.installation_for("tcp:gw:4196").unwrap();
        assert_eq!(s.installation_for("tcp:gw:4196").unwrap().id, a.id);
        let mut a2 = a.clone();
        a2.name = "Bank A".into();
        a2.endpoints.push("serial:/dev/x".into());
        a2.control = Some(ControlEndpoint::Serial { path: "/dev/y".into() });
        s.save_installation(&a2).unwrap();
        assert_eq!(s.installation_for("serial:/dev/x").unwrap(), a2);
        assert_eq!(s.installations().unwrap().len(), 1);
    }

    #[test]
    fn session_and_block_survive_reopen() {
        let path = temp("reopen");
        let id = {
            let s = Store::open(&path).unwrap();
            let i = s.installation_for("tcp:gw:4196").unwrap();
            s.set_master_state(&i.id, &StoredMasterState::PatchedOn, 1).unwrap();
            let ses = s.start_session(&i.id, SessionKind::Marker, 11, 2).unwrap();
            assert!(s.start_session(&i.id, SessionKind::Dip, 11, 3).is_err(), "only one open session");
            s.note_done(ses.id, "flash 3").unwrap();
            s.add_block(&i.id, 3, "update-unclear", 4).unwrap();
            i.id
        };
        let s = Store::open(&path).unwrap();
        let ses = s.open_session(&id).unwrap().unwrap();
        assert_eq!((ses.kind, ses.expected_slaves, ses.done.clone()), (SessionKind::Marker, 11, vec!["flash 3".to_string()]));
        assert_eq!(s.master_state(&id).unwrap().map(|(m, _)| m), Some(StoredMasterState::PatchedOn));
        let b = s.blocks(&id).unwrap();
        assert_eq!((b.len(), b[0].pack, b[0].reason.as_str()), (1, 3, "update-unclear"));
        s.clear_block(b[0].id, "answered", 5).unwrap();
        assert!(s.blocks(&id).unwrap().is_empty());
        s.end_session(ses.id, 6).unwrap();
        assert!(s.open_session(&id).unwrap().is_none());
    }

    #[test]
    fn master_state_serializes_with_the_spec_names() {
        let v = serde_json::to_value(StoredMasterState::No0x64).unwrap();
        assert_eq!(v, serde_json::json!({ "state": "no-0x64" }));
        let v = serde_json::to_value(StoredMasterState::PatchedOff { slaves: 3 }).unwrap();
        assert_eq!(v, serde_json::json!({ "state": "patched-off", "slaves": 3 }));
    }
}
```

- [ ] **Step 2: Run them.** `cargo test -p openbms-monitor maint_store` → FAIL.

- [ ] **Step 3: Implement**

```rust
//! Maintenance state per installation (`maintenance.sqlite` in the app data folder): installations with
//! their endpoints and control channel, the last confirmed master state, the maintenance session and
//! flash blocks after an unclear update. A file of its own: the history database is moved aside when its
//! schema changes, and an open session must never vanish that way. Tables are only ever added.

use openbms_flash::maint::policy::MasterState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS installations(id TEXT PRIMARY KEY, name TEXT NOT NULL, control TEXT, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS installation_endpoints(endpoint TEXT PRIMARY KEY, installation TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS master_state(installation TEXT PRIMARY KEY, state TEXT NOT NULL, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS maint_sessions(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, kind TEXT NOT NULL,
  started INTEGER NOT NULL, expected_slaves INTEGER NOT NULL, done TEXT NOT NULL DEFAULT '[]', failing TEXT, ended INTEGER);
CREATE TABLE IF NOT EXISTS flash_blocks(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, pack INTEGER NOT NULL,
  reason TEXT NOT NULL, since INTEGER NOT NULL, cleared INTEGER, how TEXT);
CREATE TABLE IF NOT EXISTS sector_notes(installation TEXT PRIMARY KEY, note TEXT NOT NULL, at INTEGER NOT NULL);
";

/// Physical endpoint, independent of the bus setting: `tcp:host:port` (lower case) or `serial:<path>`.
pub fn endpoint_key(tcp: bool, s: &str) -> String {
    if tcp { format!("tcp:{}", s.trim().to_ascii_lowercase()) } else { format!("serial:{}", s.trim()) }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ControlEndpoint {
    Serial { path: String },
    Tcp { addr: String },
}

impl ControlEndpoint {
    pub fn key(&self) -> String {
        match self {
            ControlEndpoint::Serial { path } => endpoint_key(false, path),
            ControlEndpoint::Tcp { addr } => endpoint_key(true, addr),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    pub id: String,
    pub name: String,
    pub endpoints: Vec<String>,
    pub control: Option<ControlEndpoint>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "state")]
pub enum StoredMasterState {
    #[serde(rename = "patched-off")]
    PatchedOff { slaves: u8 },
    #[serde(rename = "patched-on")]
    PatchedOn,
    #[serde(rename = "no-0x64")]
    No0x64,
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "marker-unexplained")]
    MarkerUnexplained,
}

impl From<MasterState> for StoredMasterState {
    fn from(m: MasterState) -> Self {
        match m {
            MasterState::PatchedOff { slaves } => Self::PatchedOff { slaves },
            MasterState::PatchedOn => Self::PatchedOn,
            MasterState::No0x64 => Self::No0x64,
            MasterState::Unknown => Self::Unknown,
            MasterState::MarkerUnexplained => Self::MarkerUnexplained,
        }
    }
}

impl From<&StoredMasterState> for MasterState {
    fn from(m: &StoredMasterState) -> Self {
        match m {
            StoredMasterState::PatchedOff { slaves } => Self::PatchedOff { slaves: *slaves },
            StoredMasterState::PatchedOn => Self::PatchedOn,
            StoredMasterState::No0x64 => Self::No0x64,
            StoredMasterState::Unknown => Self::Unknown,
            StoredMasterState::MarkerUnexplained => Self::MarkerUnexplained,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Marker,
    Dip,
}

impl SessionKind {
    fn as_str(self) -> &'static str {
        match self { SessionKind::Marker => "marker", SessionKind::Dip => "dip" }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MaintSession {
    pub id: i64,
    pub installation: String,
    pub kind: SessionKind,
    pub started: i64,
    pub expected_slaves: u8,
    /// what was done (e.g. "flash 3 16.06.13"), shown in the banner and the end dialog
    pub done: Vec<String>,
    /// the end check that failed last, if any
    pub failing: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FlashBlock {
    pub id: i64,
    pub pack: u8,
    /// "update-unclear" or "bootloader-unclear"
    pub reason: String,
    pub since: i64,
}

pub struct Store {
    db: Connection,
}

fn e(err: rusqlite::Error) -> String {
    err.to_string()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        }
        let db = Connection::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
        db.execute_batch(SCHEMA).map_err(e)?;
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(e)?;
        Ok(Self { db })
    }

    fn installation(&self, id: &str) -> Result<Installation, String> {
        let (name, control): (String, Option<String>) = self
            .db
            .query_row("SELECT name, control FROM installations WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(e)?;
        let mut stmt = self.db.prepare("SELECT endpoint FROM installation_endpoints WHERE installation = ?1 ORDER BY endpoint").map_err(e)?;
        let endpoints = stmt.query_map([id], |r| r.get(0)).map_err(e)?.collect::<rusqlite::Result<Vec<String>>>().map_err(e)?;
        let control = control.map(|c| serde_json::from_str(&c)).transpose().map_err(|err| err.to_string())?;
        Ok(Installation { id: id.into(), name, endpoints, control })
    }

    /// The installation an endpoint belongs to; a new one ("Installation n") if it is not assigned yet.
    pub fn installation_for(&self, endpoint: &str) -> Result<Installation, String> {
        let id: Option<String> = self
            .db
            .query_row("SELECT installation FROM installation_endpoints WHERE endpoint = ?1", [endpoint], |r| r.get(0))
            .optional()
            .map_err(e)?;
        if let Some(id) = id {
            return self.installation(&id);
        }
        let now = crate::history::now_ms();
        let count: i64 = self.db.query_row("SELECT COUNT(*) FROM installations", [], |r| r.get(0)).map_err(e)?;
        let id = format!("inst-{now:x}-{:x}", std::process::id());
        self.db.execute("INSERT INTO installations(id, name, control, created) VALUES (?1, ?2, NULL, ?3)", params![id, format!("Installation {}", count + 1), now]).map_err(e)?;
        self.db.execute("INSERT INTO installation_endpoints(endpoint, installation) VALUES (?1, ?2)", params![endpoint, id]).map_err(e)?;
        self.installation(&id)
    }

    pub fn installations(&self) -> Result<Vec<Installation>, String> {
        let mut stmt = self.db.prepare("SELECT id FROM installations ORDER BY created").map_err(e)?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>().map_err(e)?;
        ids.iter().map(|id| self.installation(id)).collect()
    }

    /// Name, control channel and endpoints; an endpoint listed here moves away from any other installation.
    pub fn save_installation(&self, i: &Installation) -> Result<(), String> {
        let control = i.control.as_ref().map(serde_json::to_string).transpose().map_err(|err| err.to_string())?;
        self.db.execute("UPDATE installations SET name = ?2, control = ?3 WHERE id = ?1", params![i.id, i.name, control]).map_err(e)?;
        self.db.execute("DELETE FROM installation_endpoints WHERE installation = ?1", [&i.id]).map_err(e)?;
        for ep in &i.endpoints {
            self.db.execute("INSERT OR REPLACE INTO installation_endpoints(endpoint, installation) VALUES (?1, ?2)", params![ep, i.id]).map_err(e)?;
        }
        Ok(())
    }

    pub fn master_state(&self, inst: &str) -> Result<Option<(StoredMasterState, i64)>, String> {
        let row: Option<(String, i64)> = self.db.query_row("SELECT state, at FROM master_state WHERE installation = ?1", [inst], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(e)?;
        row.map(|(s, at)| serde_json::from_str(&s).map(|m| (m, at)).map_err(|err| err.to_string())).transpose()
    }

    pub fn set_master_state(&self, inst: &str, s: &StoredMasterState, at: i64) -> Result<(), String> {
        let json = serde_json::to_string(s).map_err(|err| err.to_string())?;
        self.db.execute("INSERT OR REPLACE INTO master_state(installation, state, at) VALUES (?1, ?2, ?3)", params![inst, json, at]).map_err(e)?;
        Ok(())
    }

    pub fn open_session(&self, inst: &str) -> Result<Option<MaintSession>, String> {
        self.db
            .query_row(
                "SELECT id, kind, started, expected_slaves, done, failing FROM maint_sessions WHERE installation = ?1 AND ended IS NULL",
                [inst],
                |r| {
                    let kind: String = r.get(1)?;
                    let done: String = r.get(4)?;
                    Ok(MaintSession {
                        id: r.get(0)?,
                        installation: inst.into(),
                        kind: if kind == "dip" { SessionKind::Dip } else { SessionKind::Marker },
                        started: r.get(2)?,
                        expected_slaves: r.get(3)?,
                        done: serde_json::from_str(&done).unwrap_or_default(),
                        failing: r.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(e)
    }

    pub fn start_session(&self, inst: &str, kind: SessionKind, expected_slaves: u8, now: i64) -> Result<MaintSession, String> {
        if self.open_session(inst)?.is_some() {
            return Err("a maintenance session is already open".into());
        }
        self.db.execute("INSERT INTO maint_sessions(installation, kind, started, expected_slaves) VALUES (?1, ?2, ?3, ?4)", params![inst, kind.as_str(), now, expected_slaves]).map_err(e)?;
        self.open_session(inst)?.ok_or_else(|| "session not stored".into())
    }

    /// After the master was flashed back to the original image, a marker session ends as a DIP session.
    pub fn set_session_kind(&self, id: i64, kind: SessionKind) -> Result<(), String> {
        self.db.execute("UPDATE maint_sessions SET kind = ?2 WHERE id = ?1", params![id, kind.as_str()]).map_err(e)?;
        Ok(())
    }

    pub fn note_done(&self, id: i64, what: &str) -> Result<(), String> {
        let done: String = self.db.query_row("SELECT done FROM maint_sessions WHERE id = ?1", [id], |r| r.get(0)).map_err(e)?;
        let mut list: Vec<String> = serde_json::from_str(&done).unwrap_or_default();
        list.push(what.into());
        self.db.execute("UPDATE maint_sessions SET done = ?2 WHERE id = ?1", params![id, serde_json::to_string(&list).map_err(|err| err.to_string())?]).map_err(e)?;
        Ok(())
    }

    pub fn set_failing(&self, id: i64, failing: Option<&str>) -> Result<(), String> {
        self.db.execute("UPDATE maint_sessions SET failing = ?2 WHERE id = ?1", params![id, failing]).map_err(e)?;
        Ok(())
    }

    pub fn end_session(&self, id: i64, now: i64) -> Result<(), String> {
        self.db.execute("UPDATE maint_sessions SET ended = ?2, failing = NULL WHERE id = ?1", params![id, now]).map_err(e)?;
        Ok(())
    }

    pub fn add_block(&self, inst: &str, pack: u8, reason: &str, now: i64) -> Result<i64, String> {
        self.db.execute("INSERT INTO flash_blocks(installation, pack, reason, since) VALUES (?1, ?2, ?3, ?4)", params![inst, pack, reason, now]).map_err(e)?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn blocks(&self, inst: &str) -> Result<Vec<FlashBlock>, String> {
        let mut stmt = self.db.prepare("SELECT id, pack, reason, since FROM flash_blocks WHERE installation = ?1 AND cleared IS NULL ORDER BY id").map_err(e)?;
        let rows = stmt.query_map([inst], |r| Ok(FlashBlock { id: r.get(0)?, pack: r.get(1)?, reason: r.get(2)?, since: r.get(3)? })).map_err(e)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
    }

    /// `how`: "answered" (pack answered 0x51/0x47) or "isolated" (the user confirmed isolation or recovery).
    pub fn clear_block(&self, id: i64, how: &str, now: i64) -> Result<(), String> {
        self.db.execute("UPDATE flash_blocks SET cleared = ?2, how = ?3 WHERE id = ?1", params![id, now, how]).map_err(e)?;
        Ok(())
    }

    pub fn note_sector(&self, inst: &str, note: &str, now: i64) -> Result<(), String> {
        self.db.execute("INSERT OR REPLACE INTO sector_notes(installation, note, at) VALUES (?1, ?2, ?3)", params![inst, note, now]).map_err(e)?;
        Ok(())
    }

    pub fn sector_note(&self, inst: &str) -> Result<Option<String>, String> {
        self.db.query_row("SELECT note FROM sector_notes WHERE installation = ?1", [inst], |r| r.get(0)).optional().map_err(e)
    }
}
```

- [ ] **Step 4: Run them.** `cargo test -p openbms-monitor maint_store` → PASS (4 tests). Then clippy.
- [ ] **Step 5: Commit.** `feat(app): maintenance store for installations, sessions and flash blocks`

---

### Task 7: Backend maintenance commands (control channel, check, activate, sessions)

**Files:**
- Create: `app/src-tauri/src/maint.rs`
- Modify: `app/src-tauri/src/lib.rs`:
  - `AppState`: add `main: Mutex<Option<(String, Bus)>>` (endpoint key and bus of the main connection) and `maint: Arc<Mutex<Option<maint_store::Store>>>`;
  - `connect`: set `main`;
  - `stop_monitor`: clear `main`;
  - `setup`: open the store at `app_data_dir()/maintenance.sqlite`;
  - register the new commands.

**Interfaces:**
- Consumes: Tasks 2, 4, 6.
- Produces:
  - Tauri commands (args camelCase from JS):
    - `maint_installation_for(endpoint: EndpointArg) -> Installation`
    - `maint_save_installation(installation: Installation) -> Installation`
    - `maint_overview(installation: String) -> Overview`
    - `maint_check(installation) -> Overview`
    - `maint_activate(installation, inverterConfirmed: bool) -> Overview`
    - `maint_start_dip(installation, expectedSlaves: u8) -> Overview`
    - `maint_account_marker(installation, expectedSlaves: u8) -> Overview`
  - Helpers:
    - `MaintError` (serde tag `kind`)
    - `Overview { installation, state: Option<StoredMasterState>, state_at: Option<i64>, session: Option<MaintSession>, blocks: Vec<FlashBlock>, sector_note: Option<String> }`
    - `uses_main_connection(main: Option<&(String, Bus)>, control_key: &str) -> bool`
    - `with_control<T>(state, inst, f: impl FnOnce(&mut Connection) -> T) -> Result<T, MaintError>`
    - `stop_allowed(state: Option<&StoredMasterState>) -> Result<(), MaintError>`

- [ ] **Step 1: Write the failing tests** (bottom of `maint.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_route_uses_main_connection_only_for_same_endpoint_on_can_bus() {
        let can = ("serial:/dev/a".to_string(), Bus::CanSocketBus);
        let pack = ("serial:/dev/a".to_string(), Bus::PackBus);
        assert!(uses_main_connection(Some(&can), "serial:/dev/a"));
        assert!(!uses_main_connection(Some(&can), "serial:/dev/b"));
        // same adapter on the pack bus: the control exchange would go to the wrong bus
        assert!(!uses_main_connection(Some(&pack), "serial:/dev/a"));
        assert!(!uses_main_connection(None, "serial:/dev/a"));
    }

    #[test]
    fn same_adapter_on_the_pack_bus_is_refused_not_opened_twice() {
        let pack = ("serial:/dev/a".to_string(), Bus::PackBus);
        assert!(matches!(route(Some(&pack), "serial:/dev/a"), Route::Refused));
        assert!(matches!(route(Some(&pack), "serial:/dev/b"), Route::Open));
    }

    #[test]
    fn stop_is_refused_while_marker_unexplained() {
        assert!(matches!(stop_allowed(Some(&StoredMasterState::MarkerUnexplained)), Err(MaintError::Refused { .. })));
        assert!(stop_allowed(Some(&StoredMasterState::PatchedOn)).is_ok());
    }

    #[test]
    fn dialog_errors_map_to_typed_ui_errors() {
        use openbms_flash::maint::dialog::DialogError as D;
        assert!(matches!(MaintError::from(D::Unsupported), MaintError::Unsupported));
        assert!(matches!(MaintError::from(D::NoAnswer), MaintError::NoAnswer));
        assert!(matches!(MaintError::from(D::Link(openbms_flash::Error::Transport("x".into()))), MaintError::Connection { .. }));
        let v = serde_json::to_value(MaintError::Rejected { rtn: 5 }).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "rejected", "rtn": 5 }));
    }
}
```

- [ ] **Step 2: Run them.** `cargo test -p openbms-monitor maint::tests` → FAIL.

- [ ] **Step 3: Implement** `maint.rs`

```rust
//! Maintenance of a multi-pack bank (design: docs/superpowers/specs/2026-10-12-maintenance-mode-design.md):
//! the 0x64 dialog over the control channel, the master state and the session per installation.
//! All operations hold the one maintenance-operation lock (`port_busy`); a control exchange on the
//! endpoint of the main connection runs through that connection, so no endpoint is opened twice.

use crate::maint_store::{ControlEndpoint, FlashBlock, Installation, MaintSession, SessionKind, Store, StoredMasterState};
use crate::{lock, reserve_port, AppState, EndpointArg};
use openbms_flash::maint::dialog::{self, Action, Budgets, DialogError, Probe};
use openbms_flash::maint::{policy, SystemClock};
use openbms_transport::{Bus, Connection, Endpoint};
use serde::Serialize;
use tauri::State;

#[derive(Serialize, Debug)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MaintError {
    /// opening or using the control/pack bus channel failed
    Connection { message: String },
    Unsupported,
    NoAnswer,
    Rejected { rtn: u8 },
    Timeout { marker: bool, slaves: u8 },
    /// not allowed in the current state; `reason` is a stable key for the UI text
    Refused { reason: String },
    Busy,
    Measure { message: String },
    Store { message: String },
}

impl From<DialogError> for MaintError {
    fn from(e: DialogError) -> Self {
        match e {
            DialogError::Link(e) => MaintError::Connection { message: e.to_string() },
            DialogError::Unsupported => MaintError::Unsupported,
            DialogError::NoAnswer => MaintError::NoAnswer,
            DialogError::Rejected(rtn) => MaintError::Rejected { rtn },
            DialogError::Timeout { last, .. } => MaintError::Timeout { marker: last.marker, slaves: last.effective_slaves },
        }
    }
}

fn store_err(message: String) -> MaintError {
    MaintError::Store { message }
}

fn refused(reason: &str) -> MaintError {
    MaintError::Refused { reason: reason.into() }
}

#[derive(Serialize)]
pub struct Overview {
    installation: Installation,
    state: Option<StoredMasterState>,
    state_at: Option<i64>,
    session: Option<MaintSession>,
    blocks: Vec<FlashBlock>,
    sector_note: Option<String>,
}

pub fn with_store<T>(state: &AppState, f: impl FnOnce(&Store) -> Result<T, String>) -> Result<T, MaintError> {
    let guard = state.maint.lock().map_err(|e| store_err(e.to_string()))?;
    let s = guard.as_ref().ok_or_else(|| store_err("maintenance database not open".into()))?;
    f(s).map_err(store_err)
}

pub fn overview(state: &AppState, inst: &str) -> Result<Overview, MaintError> {
    with_store(state, |s| {
        let installation = s.installations()?.into_iter().find(|i| i.id == inst).ok_or("unknown installation")?;
        let ms = s.master_state(inst)?;
        Ok(Overview {
            installation,
            state_at: ms.as_ref().map(|(_, at)| *at),
            state: ms.map(|(m, _)| m),
            session: s.open_session(inst)?,
            blocks: s.blocks(inst)?,
            sector_note: s.sector_note(inst)?,
        })
    })
}

pub fn uses_main_connection(main: Option<&(String, Bus)>, control_key: &str) -> bool {
    matches!(main, Some((key, Bus::CanSocketBus)) if key == control_key)
}

pub enum Route {
    Main,
    Open,
    /// the adapter is held by the main connection on the pack bus: the user must switch the bus first
    Refused,
}

pub fn route(main: Option<&(String, Bus)>, control_key: &str) -> Route {
    match main {
        _ if uses_main_connection(main, control_key) => Route::Main,
        Some((key, _)) if key == control_key => Route::Refused,
        _ => Route::Open,
    }
}

fn control_endpoint(c: &ControlEndpoint) -> Endpoint {
    match c {
        ControlEndpoint::Serial { path } => Endpoint::Serial { path: path.clone(), baud: Bus::CanSocketBus.baud() },
        ControlEndpoint::Tcp { addr } => Endpoint::Tcp { addr: addr.clone() },
    }
}

/// Run `f` on the control channel: through the main connection when it is that endpoint on the CAN socket
/// bus (the polling thread waits for the mutex meanwhile), otherwise on a connection opened for this dialog.
pub fn with_control<T>(state: &AppState, inst: &Installation, f: impl FnOnce(&mut Connection) -> T) -> Result<T, MaintError> {
    let control = inst.control.as_ref().ok_or_else(|| refused("no-control-channel"))?;
    let main = state.main.lock().map_err(|e| store_err(e.to_string()))?.clone();
    match route(main.as_ref(), &control.key()) {
        Route::Main => {
            let mut guard = lock(&state.conn).map_err(|m| MaintError::Connection { message: m })?;
            match guard.as_mut() {
                Some(c) => Ok(f(c)),
                None => Err(MaintError::Connection { message: "not connected".into() }),
            }
        }
        Route::Refused => Err(refused("control-on-pack-bus")),
        Route::Open => {
            let mut c = Connection::open(&control_endpoint(control)).map_err(|e| MaintError::Connection { message: e.to_string() })?;
            Ok(f(&mut c))
        }
    }
}

pub fn stop_allowed(state: Option<&StoredMasterState>) -> Result<(), MaintError> {
    if state == Some(&StoredMasterState::MarkerUnexplained) {
        return Err(refused("marker-unexplained"));
    }
    Ok(())
}

fn installation(state: &AppState, id: &str) -> Result<Installation, MaintError> {
    with_store(state, |s| s.installations()?.into_iter().find(|i| i.id == id).ok_or_else(|| "unknown installation".into()))
}

/// Read 0x64 and store the classified state. A connection failure stores `unknown`, never `no-0x64`.
pub fn check(state: &AppState, inst: &Installation) -> Result<StoredMasterState, MaintError> {
    let session = with_store(state, |s| s.open_session(&inst.id))?;
    let probe = with_control(state, inst, |c| dialog::probe(c, &SystemClock::new(), Budgets::default()).ok()).unwrap_or(None);
    let expected = session.is_some_and(|s| s.kind == SessionKind::Marker);
    let m: StoredMasterState = policy::classify(probe, expected).into();
    with_store(state, |s| s.set_master_state(&inst.id, &m, crate::history::now_ms()))?;
    Ok(m)
}

#[tauri::command]
pub async fn maint_installation_for(state: State<'_, AppState>, endpoint: EndpointArg) -> Result<Installation, MaintError> {
    let key = endpoint.key();
    with_store(&state, |s| s.installation_for(&key))
}

#[tauri::command]
pub async fn maint_save_installation(state: State<'_, AppState>, installation: Installation) -> Result<Installation, MaintError> {
    with_store(&state, |s| {
        s.save_installation(&installation)?;
        Ok(installation.clone())
    })
}

#[tauri::command]
pub async fn maint_overview(state: State<'_, AppState>, installation: String) -> Result<Overview, MaintError> {
    overview(&state, &installation)
}

#[tauri::command]
pub async fn maint_check(state: State<'_, AppState>, installation: String) -> Result<Overview, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    check(&state, &inst)?;
    overview(&state, &installation)
}

/// `patched-off` → session (kind marker, N = DIP count) → `start` → wait for 1/0. The session is stored
/// before `start`, so an interruption leaves a visible open session; a failed start re-reads 0x64.
#[tauri::command]
pub async fn maint_activate(state: State<'_, AppState>, installation: String, inverter_confirmed: bool) -> Result<Overview, MaintError> {
    if !inverter_confirmed {
        return Err(refused("inverter"));
    }
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    let n = match check(&state, &inst)? {
        StoredMasterState::PatchedOff { slaves } => slaves,
        _ => return Err(refused("not-patched-off")),
    };
    let now = crate::history::now_ms();
    with_store(&state, |s| s.start_session(&inst.id, SessionKind::Marker, n, now))?;
    let r = with_control(&state, &inst, |c| dialog::dialog(c, &SystemClock::new(), Action::Start, None, Budgets::default()))?;
    check(&state, &inst)?;
    r?;
    overview(&state, &installation)
}

/// The user confirmed DIP 5–8 OFF (and noted the positions): a DIP session with the slave count they entered.
#[tauri::command]
pub async fn maint_start_dip(state: State<'_, AppState>, installation: String, expected_slaves: u8) -> Result<Overview, MaintError> {
    let now = crate::history::now_ms();
    with_store(&state, |s| s.start_session(&installation, SessionKind::Dip, expected_slaves, now).map(|_| ()))?;
    overview(&state, &installation)
}

/// The user states where an unexplained marker comes from ("I set it earlier"): a marker session with `N`.
#[tauri::command]
pub async fn maint_account_marker(state: State<'_, AppState>, installation: String, expected_slaves: u8) -> Result<Overview, MaintError> {
    let now = crate::history::now_ms();
    with_store(&state, |s| {
        if s.open_session(&installation)?.is_none() {
            s.start_session(&installation, SessionKind::Marker, expected_slaves, now)?;
        }
        s.set_master_state(&installation, &StoredMasterState::PatchedOn, now)
    })?;
    overview(&state, &installation)
}

// tests: see Step 1
```

Do not delete the `// tests: see Step 1` line until the test module from Step 1 is below it. Also unused `Probe`/`Store` imports should not remain once the code compiles; remove any that clippy reports.

In `lib.rs`:
- make `EndpointArg` and `lock` `pub(crate)`;
- add `impl EndpointArg { pub fn key(&self) -> String { match self { EndpointArg::Serial { path, .. } => maint_store::endpoint_key(false, path), EndpointArg::Tcp { addr, .. } => maint_store::endpoint_key(true, addr) } } }`;
- add `#[derive(Clone)]` on `EndpointArg` and `BusArg` (already `Copy`).

In `connect`:
- compute `let key = endpoint.key();` before the match moves `endpoint`;
- after storing `bus`, add `*state.main.lock().map_err(|e| e.to_string())? = Some((key, bus));`.

In `stop_monitor`, add `if let Ok(mut m) = state.main.lock() { *m = None; }`.

In `setup`:

```rust
            match maint_store::Store::open(&app.path().app_data_dir()?.join("maintenance.sqlite")) {
                Ok(s) => if let Ok(mut m) = state.maint.lock() { *m = Some(s); },
                Err(e) => eprintln!("maintenance: {e}"),
            }
```

Add `maint::maint_installation_for, maint::maint_save_installation, maint::maint_overview, maint::maint_check, maint::maint_activate, maint::maint_start_dip, maint::maint_account_marker` to `generate_handler!`.

- [ ] **Step 4: Run.** `cargo test -p openbms-monitor` and clippy → PASS.
- [ ] **Step 5: Commit.** `feat(app): maintenance commands for master check, activation and sessions`

---

### Task 8: Measurement, end check, and the flash gate

**Files:**
- Modify: `app/src-tauri/src/maint.rs` (commands `maint_measure`, `maint_end`, `maint_clear_block`, `maint_master_plan`; pure fns `flash_gate`, `block_reason`, `end_timeout`)
- Modify: `app/src-tauri/src/firmware.rs`:
  - `firmware_start` takes `maint: Option<FlashMaint>`;
  - the gate, the fresh measurement, the master plan, the block after a failure, the post-flash 0x64 check;
  - `FirmwareDone` gains `block: Option<String>` and `master: Option<String>`;
  - `firmware_check_image` returns `kind` (done in Task 1).
- Modify: `app/src-tauri/src/lib.rs` (register commands)

**Interfaces:**
- Consumes: Tasks 3, 4, 6, 7.
- Produces:
  - `maint_measure(installation, port) -> i64` (ms timestamp of the passed measurement)
  - `maint_end(installation, port) -> EndResult { ok: bool, missing: Vec<u8>, failing: Option<String> }`
  - `maint_clear_block(installation, id: i64, how: "answered" | "isolated", port: Option<String>) -> Overview`
  - `maint_master_plan(installation, kind: "original" | "maintenance" | "custom") -> PlanView { before: Vec<&str>, after: &str, recovery: bool }`
  - `FlashMaint { installation: String, inverter_confirmed: bool, dip_off_confirmed: bool, isolated_confirmed: bool }`
  - `flash_gate(&GateInput) -> Result<(), MaintError>`
  - `block_reason(last: Option<Progress>) -> Option<&'static str>`
  - `end_timeout(n: u8) -> Duration`

- [ ] **Step 1: Write the failing tests** (add to `maint.rs` tests)

```rust
    use openbms_flash::session::Progress;

    fn block(pack: u8) -> FlashBlock {
        FlashBlock { id: 1, pack, reason: "update-unclear".into(), since: 0 }
    }

    #[test]
    fn flash_gate_refuses_without_inverter_or_with_block() {
        fn g(session: Option<&MaintSession>, state: Option<&StoredMasterState>, blocks: &[FlashBlock], inverter: bool) -> Result<(), MaintError> {
            flash_gate(&GateInput { session, state, blocks, inverter_confirmed: inverter })
        }
        let ses = MaintSession { id: 1, installation: "i".into(), kind: SessionKind::Marker, started: 0, expected_slaves: 11, done: vec![], failing: None };
        let on = StoredMasterState::PatchedOn;
        assert!(g(Some(&ses), Some(&on), &[], true).is_ok());
        assert!(matches!(g(Some(&ses), Some(&on), &[], false), Err(MaintError::Refused { reason }) if reason == "inverter"));
        assert!(matches!(g(Some(&ses), Some(&on), &[block(3)], true), Err(MaintError::Refused { reason }) if reason == "update-unclear"));
        assert!(matches!(g(None, Some(&StoredMasterState::MarkerUnexplained), &[], true), Err(MaintError::Refused { .. })));
        // a single pack without maintenance: nothing to confirm
        assert!(g(None, Some(&StoredMasterState::PatchedOff { slaves: 0 }), &[], false).is_ok());
        assert!(g(None, None, &[], false).is_ok());
    }

    #[test]
    fn block_reason_by_stage() {
        assert_eq!(block_reason(None), None);
        assert_eq!(block_reason(Some(Progress::Preflight)), None);
        assert_eq!(block_reason(Some(Progress::Bootloader)), Some("bootloader-unclear"));
        assert_eq!(block_reason(Some(Progress::Erase)), Some("update-unclear"));
        assert_eq!(block_reason(Some(Progress::Block { index: 3, total: 200 })), Some("update-unclear"));
        assert_eq!(block_reason(Some(Progress::PostCheck)), Some("update-unclear"));
    }

    #[test]
    fn end_round_waits_long_enough_for_a_full_bank() {
        // 150 ms per slave answer (code 0x13F6/0x14AE), two rounds, plus margin
        assert!(end_timeout(11) >= std::time::Duration::from_millis(3300 + 2000));
        assert!(end_timeout(1) >= std::time::Duration::from_secs(4));
    }
```

- [ ] **Step 2: Run them.** `cargo test -p openbms-monitor maint::tests` → FAIL.

- [ ] **Step 3: Implement the pure parts** (in `maint.rs`)

```rust
use openbms_flash::session::Progress;
use std::time::Duration;

pub struct GateInput<'a> {
    pub session: Option<&'a MaintSession>,
    pub state: Option<&'a StoredMasterState>,
    pub blocks: &'a [FlashBlock],
    pub inverter_confirmed: bool,
}

/// Checks before every flash of an installation (the fresh measurement follows in `firmware_start`).
pub fn flash_gate(g: &GateInput) -> Result<(), MaintError> {
    if let Some(b) = g.blocks.first() {
        return Err(refused(&b.reason));
    }
    if g.state == Some(&StoredMasterState::MarkerUnexplained) {
        return Err(refused("marker-unexplained"));
    }
    let maintenance = g.session.is_some() || g.state == Some(&StoredMasterState::PatchedOn);
    if maintenance && !g.inverter_confirmed {
        return Err(refused("inverter"));
    }
    Ok(())
}

/// Which block a failed flash leaves until the pack answers as an application again. A failure in the
/// preflight changes nothing; after the restart the bootloader may hold the bus.
pub fn block_reason(last: Option<Progress>) -> Option<&'static str> {
    match last {
        None | Some(Progress::Preflight) => None,
        Some(Progress::Bootloader) => Some("bootloader-unclear"),
        Some(_) => Some("update-unclear"),
    }
}

/// Listening time for the end round: two full polling rounds (150 ms per slave) plus 2 s, at least 4 s.
pub fn end_timeout(n: u8) -> Duration {
    (Duration::from_millis(300) * u32::from(n) + Duration::from_secs(2)).max(Duration::from_secs(4))
}
```

- [ ] **Step 4: Run the tests.** `cargo test -p openbms-monitor maint::tests` → PASS.

- [ ] **Step 5: Implement the commands** (in `maint.rs`)

```rust
use openbms_flash::maint::bus;
use openbms_flash::known::ImageKind;

const PACK_BUS_BAUD: u32 = 19_200;

fn open_pack(port: &str) -> Result<Connection, MaintError> {
    Connection::open(&Endpoint::Serial { path: port.into(), baud: PACK_BUS_BAUD }).map_err(|e| MaintError::Connection { message: e.to_string() })
}

/// The measurement of step 3: for a set marker first a fresh 1/0 over the control channel, then on the pack
/// bus 2 s quiet and 0x47 from address 0. Stops the monitor when it uses the pack bus.
pub fn measure(state: &AppState, inst: &Installation, port: &str) -> Result<i64, MaintError> {
    let (session, ms) = with_store(state, |s| Ok((s.open_session(&inst.id)?, s.master_state(&inst.id)?)))?;
    let marker = session.as_ref().is_some_and(|s| s.kind == SessionKind::Marker) || matches!(ms, Some((StoredMasterState::PatchedOn, _)));
    if marker {
        let s = with_control(state, inst, |c| dialog::dialog(c, &SystemClock::new(), Action::Status, None, Budgets::default()))??;
        if !(s.marker && s.effective_slaves == 0) {
            check(state, inst)?;
            return Err(refused("marker-not-active"));
        }
    }
    if matches!(*state.main.lock().map_err(|e| store_err(e.to_string()))?, Some((_, Bus::PackBus))) {
        crate::stop_monitor(state).map_err(|m| MaintError::Connection { message: m })?;
    }
    let mut link = open_pack(port)?;
    bus::measure(&mut link, &mut std::io::sink()).map_err(|e| MaintError::Measure { message: e.to_string() })?;
    Ok(crate::history::now_ms())
}

#[tauri::command]
pub async fn maint_measure(state: State<'_, AppState>, installation: String, port: String) -> Result<i64, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    measure(&state, &inst, &port)
}

#[derive(Serialize)]
pub struct EndResult {
    ok: bool,
    missing: Vec<u8>,
    failing: Option<String>,
}

/// End (step 5): marker → `stop` and wait for 0/N; then the pack bus check against N; then two Modbus reads
/// ≥ 2 s apart over the control channel. The session closes only when all of it passed.
#[tauri::command]
pub async fn maint_end(state: State<'_, AppState>, installation: String, port: String) -> Result<EndResult, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    let (session, ms) = with_store(&state, |s| Ok((s.open_session(&inst.id)?, s.master_state(&inst.id)?)))?;
    let session = session.ok_or_else(|| refused("no-session"))?;
    stop_allowed(ms.as_ref().map(|(m, _)| m))?;
    let n = session.expected_slaves;
    let fail = |what: &str, missing: Vec<u8>| -> Result<EndResult, MaintError> {
        with_store(&state, |s| s.set_failing(session.id, Some(what)))?;
        Ok(EndResult { ok: false, missing, failing: Some(what.into()) })
    };
    if session.kind == SessionKind::Marker {
        let r = with_control(&state, &inst, |c| dialog::dialog(c, &SystemClock::new(), Action::Stop, Some(n), Budgets::default()))?;
        check(&state, &inst)?;
        if let Err(e) = r {
            return fail(&format!("stop: {}", serde_json::to_string(&MaintError::from(e)).unwrap_or_default()), vec![]);
        }
    }
    let mut link = open_pack(&port)?;
    if n > 0 {
        let round = bus::watch_round(&mut link, &SystemClock::new(), &mut std::io::sink(), n, end_timeout(n))
            .map_err(|e| MaintError::Connection { message: e.to_string() })?;
        if !round.complete(n) {
            return fail("pack-communication-incomplete", round.missing(n));
        }
    } else if bus::measure(&mut link, &mut std::io::sink()).is_err() {
        return fail("master-not-answering", vec![]);
    }
    drop(link);
    for i in 0..2 {
        if i == 1 {
            std::thread::sleep(Duration::from_secs(2));
        }
        if with_control(&state, &inst, |c| c.system_values().is_ok())? == false {
            return fail("system-values", vec![]);
        }
    }
    with_store(&state, |s| s.end_session(session.id, crate::history::now_ms()))?;
    Ok(EndResult { ok: true, missing: vec![], failing: None })
}

/// Clear a flash block: "answered" checks the pack on the pack bus (0x51 and 0x47), "isolated" is the
/// user's confirmation that the pack was isolated or recovered.
#[tauri::command]
pub async fn maint_clear_block(state: State<'_, AppState>, installation: String, id: i64, how: String, port: Option<String>) -> Result<Overview, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let blocks = with_store(&state, |s| s.blocks(&installation))?;
    let b = blocks.iter().find(|b| b.id == id).ok_or_else(|| refused("no-block"))?;
    match how.as_str() {
        "answered" => {
            let mut link = open_pack(port.as_deref().ok_or_else(|| refused("no-port"))?)?;
            if !bus::app_answers(&mut link, &mut std::io::sink(), b.pack, Duration::from_secs(2)) {
                return Err(refused("pack-not-answering"));
            }
        }
        "isolated" => {}
        _ => return Err(refused("how")),
    }
    with_store(&state, |s| s.clear_block(id, &how, crate::history::now_ms()))?;
    overview(&state, &installation)
}

#[derive(Serialize)]
pub struct PlanView {
    before: Vec<&'static str>,
    after: &'static str,
    recovery: bool,
}

pub fn image_kind(kind: &str) -> ImageKind {
    match kind {
        "original" => ImageKind::Original,
        "maintenance" => ImageKind::Maintenance,
        _ => ImageKind::Custom,
    }
}

pub fn plan_view(p: &policy::Plan) -> PlanView {
    use policy::{Expect, Step};
    PlanView {
        before: p.before.iter().map(|s| match s {
            Step::DipOff => "dip-off",
            Step::StopToZero => "stop-to-zero",
            Step::Measure => "measure",
            Step::IsolatedOrReleased => "isolated-or-released",
        }).collect(),
        after: match p.after {
            Expect::MarkerOn => "marker-on",
            Expect::Zero => "zero",
            Expect::ZeroOrUnexplained => "zero-or-unexplained",
            Expect::Unsupported => "unsupported",
            Expect::NoAssumption => "no-assumption",
        },
        recovery: p.recovery,
    }
}

pub fn refusal_key(r: policy::Refusal) -> &'static str {
    match r {
        policy::Refusal::QuietFirst => "quiet-first",
        policy::Refusal::Unknown => "state-unknown",
        policy::Refusal::Locked => "marker-unexplained",
    }
}

#[tauri::command]
pub async fn maint_master_plan(state: State<'_, AppState>, installation: String, kind: String) -> Result<PlanView, MaintError> {
    let ms = with_store(&state, |s| s.master_state(&installation))?;
    let m: policy::MasterState = ms.as_ref().map(|(m, _)| m.into()).unwrap_or(policy::MasterState::Unknown);
    policy::master_flash_plan(m, image_kind(&kind)).map(|p| plan_view(&p)).map_err(|r| refused(refusal_key(r)))
}
```

`if … == false` in `maint_end` fails clippy (`bool_comparison`). Write it as `if !with_control(&state, &inst, |c| c.system_values().is_ok())? {`.

- [ ] **Step 6: Gate in `firmware_start`** (`firmware.rs`). Add:

```rust
#[derive(serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlashMaint {
    installation: String,
    inverter_confirmed: bool,
    dip_off_confirmed: bool,
    isolated_confirmed: bool,
}
```

Then change the signature to `firmware_start(app, state, port, address, path, allow_custom, expected_sha256, confirmation, lang, maint: Option<FlashMaint>)`. After `let port_slot = reserve_port(...)` and before `try_begin`, insert:

```rust
    // maintenance gate: blocks, locked state, inverter; for the master the decision table; then a fresh measurement
    let mut master_plan = None;
    let mut inst = None;
    if let Some(m) = &maint {
        let i = crate::maint::installation_by_id(&state, &m.installation).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
        let (session, ms, blocks) = crate::maint::with_store(&state, |s| Ok((s.open_session(&i.id)?, s.master_state(&i.id)?, s.blocks(&i.id)?)))
            .map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
        let ms_ref = ms.as_ref().map(|(m, _)| m);
        if address == 0 {
            let state_now: openbms_flash::maint::policy::MasterState = ms_ref.map(Into::into).unwrap_or(openbms_flash::maint::policy::MasterState::Unknown);
            let plan = openbms_flash::maint::policy::master_flash_plan(state_now, crate::maint::image_kind(info.kind))
                .map_err(|r| crate::maint::refusal_key(r).to_string())?;
            use openbms_flash::maint::policy::Step;
            if plan.before.contains(&Step::DipOff) && (!m.dip_off_confirmed || session.is_none()) {
                return Err("dip-off".into());
            }
            if plan.before.contains(&Step::IsolatedOrReleased) && !m.isolated_confirmed {
                return Err("isolated-or-released".into());
            }
            if !plan.recovery {
                crate::maint::flash_gate(&crate::maint::GateInput { session: session.as_ref(), state: ms_ref, blocks: &blocks, inverter_confirmed: m.inverter_confirmed })
                    .map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
            } else if !blocks.is_empty() || !m.inverter_confirmed {
                return Err("update-unclear-or-inverter".into());
            }
            if plan.before.contains(&Step::StopToZero) {
                crate::maint::stop_to_zero(&state, &i).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
            }
            master_plan = Some(plan);
        } else {
            crate::maint::flash_gate(&crate::maint::GateInput { session: session.as_ref(), state: ms_ref, blocks: &blocks, inverter_confirmed: m.inverter_confirmed })
                .map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
        }
        crate::maint::measure(&state, &i, &port).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
        inst = Some(i);
    }
```

Add these to `maint.rs`:
- `pub fn installation_by_id(state: &AppState, id: &str) -> Result<Installation, MaintError> { installation(state, id) }`
- `stop_to_zero`, which sends `stop` and waits for 0/0, refusing in the locked state:

```rust
pub fn stop_to_zero(state: &AppState, inst: &Installation) -> Result<(), MaintError> {
    let ms = with_store(state, |s| s.master_state(&inst.id))?;
    stop_allowed(ms.as_ref().map(|(m, _)| m))?;
    let r = with_control(state, inst, |c| dialog::dialog(c, &SystemClock::new(), Action::Stop, Some(0), Budgets::default()))?;
    check(state, inst)?;
    r.map(|_| ()).map_err(Into::into)
}
```

`check` stores the state found after `stop` (expected `patched-off` 0/0).

`ImageInfo.kind` is `&'static str`; `image_kind` takes `&str`, so this works. Errors from these paths reach the UI as a JSON string of `MaintError` or a key. Task 9's `errorText` parses both.

- [ ] **Step 7: After the flash** (in the spawned thread of `firmware_start`)
  - Clone what the thread needs before spawning:
    - `let store = state.maint.clone();`
    - `let inst2 = inst.clone();`
    - `let plan2 = master_plan.clone();`
  - The thread cannot use `State`. Write two helpers in `maint.rs` that take `&Arc<Mutex<Option<Store>>>` and the installation, so they need no `AppState`:
    - `record_after_flash(store, inst, address, ok, last, link: &mut Connection) -> (Option<String> /*block*/, Option<String> /*master outcome*/)`
    - `post_master_check(store, inst, plan) -> Option<String>`
  - **Failure** (`ok == false`): if `block_reason(last)` is `Some(r)` and `!bus::app_answers(link, &mut sink, address, Duration::from_secs(6))` → `store.add_block(&inst.id, address, r, now)`; return the reason.
  - **Success with a session open:** `store.note_done(session.id, &format!("flash {address} {}", info.release))`.
  - **Success at address 0 with a plan:**
    1. Open the control channel directly with `Connection::open(&control_endpoint(..))`. The main connection was stopped before the flash, so the backend must not assume it exists.
    2. Read 0x64 with up to 6 s of retries: `dialog::probe` every 500 ms until something other than `NoAnswer`.
    3. `policy::after_flash(plan.after, probe)` → store the state:

       | Outcome | Store | Session |
       |---|---|---|
       | `AsExpected` + `MarkerOn` | `PatchedOn` | — |
       | `AsExpected` + `Zero` / `ZeroOrUnexplained` | `PatchedOff { slaves: 0 }` | — |
       | `AsExpected` + `Unsupported` | `No0x64` | `set_session_kind(Dip)` |
       | `MarkerUnexplained` | `MarkerUnexplained`; `note_sector(.., "marker after first flash of a maintenance build", now)` | — |
       | `Unclear` | `Unknown` | `set_failing(.., Some("update-unclear"))` |
       | `UnknownImage` | `Unknown`; `note_sector(.., "unknown image on the master", now)` | — |

    4. If `plan.recovery`, also `note_sector(.., "0x23000 unexplained, recovered to original without stop", now)`.
  - Put the results into `FirmwareDone { block, master }` (both `Option<String>`, serialized; the outcome as `"as-expected" | "unclear" | "marker-unexplained" | "unknown-image"`).

Write a unit test for the outcome-to-state mapping as a pure fn `state_after(outcome: Outcome, expect: Expect) -> StoredMasterState` in `maint.rs`:

```rust
    #[test]
    fn state_after_master_flash() {
        use openbms_flash::maint::policy::{Expect as E, Outcome as O};
        assert_eq!(state_after(O::AsExpected, E::MarkerOn), StoredMasterState::PatchedOn);
        assert_eq!(state_after(O::AsExpected, E::Zero), StoredMasterState::PatchedOff { slaves: 0 });
        assert_eq!(state_after(O::AsExpected, E::Unsupported), StoredMasterState::No0x64);
        assert_eq!(state_after(O::MarkerUnexplained, E::ZeroOrUnexplained), StoredMasterState::MarkerUnexplained);
        assert_eq!(state_after(O::Unclear, E::MarkerOn), StoredMasterState::Unknown);
        assert_eq!(state_after(O::UnknownImage, E::NoAssumption), StoredMasterState::Unknown);
    }
```

```rust
pub fn state_after(outcome: policy::Outcome, expect: policy::Expect) -> StoredMasterState {
    use policy::{Expect as E, Outcome as O};
    match (outcome, expect) {
        (O::AsExpected, E::MarkerOn) => StoredMasterState::PatchedOn,
        (O::AsExpected, E::Zero | E::ZeroOrUnexplained) => StoredMasterState::PatchedOff { slaves: 0 },
        (O::AsExpected, E::Unsupported) => StoredMasterState::No0x64,
        (O::MarkerUnexplained, _) => StoredMasterState::MarkerUnexplained,
        _ => StoredMasterState::Unknown,
    }
}
```

`AppState.maint` is `Arc<Mutex<Option<Store>>>`. Add a variant of `with_store` that takes the `Arc` directly (`with_store_arc`), so the thread can use it. `rusqlite::Connection` is `Send`, so `Mutex<Option<Store>>` is `Sync`.

- [ ] **Step 8:** Register `maint_measure, maint_end, maint_clear_block, maint_master_plan` in `lib.rs`. Then run `cargo test --workspace` and clippy → PASS.
- [ ] **Step 9: Commit.** `feat(app): measurement, end check and maintenance gate for firmware updates`

---

### Task 9: UI state helpers and API

**Files:**
- Create: `app/src/maint.ts`, `app/src/maint.test.ts`
- Modify: `app/src/api.ts` (types, tauri calls, demo stubs)

**Interfaces:**
- Consumes: the backend commands of Tasks 7–8.
- Produces:
  - TS types `MasterStateName`, `MasterState`, `ControlEndpoint`, `Installation`, `MaintSession`, `FlashBlock`, `MaintOverview`, `MaintError`, `EndResult`, `PlanView`
  - api methods `maintInstallationFor`, `maintSaveInstallation`, `maintOverview`, `maintCheck`, `maintActivate`, `maintStartDip`, `maintAccountMarker`, `maintMeasure`, `maintEnd`, `maintClearBlock`, `maintMasterPlan`
  - `firmwareStart` gains a last argument `maint: FlashMaint | null`
  - `maint.ts`: `bannerOf(o) -> Banner | null`, `needsReplug(control, packPort) -> boolean`, `errorKey(e) -> { key: string; params?: Record<string, unknown> }`, `canFlash({ overview, inverter, measuredAt }) -> { ok: boolean; reason?: string }`

- [ ] **Step 1: Types and API** (append to `api.ts` types)

```ts
export type MasterStateName = "patched-off" | "patched-on" | "no-0x64" | "unknown" | "marker-unexplained";
export interface MasterState { state: MasterStateName; slaves?: number }
export type ControlEndpoint = { kind: "serial"; path: string } | { kind: "tcp"; addr: string };
export interface Installation { id: string; name: string; endpoints: string[]; control: ControlEndpoint | null }
export interface MaintSession { id: number; installation: string; kind: "marker" | "dip"; started: number; expected_slaves: number; done: string[]; failing: string | null }
export interface FlashBlock { id: number; pack: number; reason: "update-unclear" | "bootloader-unclear"; since: number }
export interface MaintOverview { installation: Installation; state: MasterState | null; state_at: number | null; session: MaintSession | null; blocks: FlashBlock[]; sector_note: string | null }
export type MaintError =
  | { kind: "connection"; message: string } | { kind: "unsupported" } | { kind: "no-answer" } | { kind: "rejected"; rtn: number }
  | { kind: "timeout"; marker: boolean; slaves: number } | { kind: "refused"; reason: string } | { kind: "busy" }
  | { kind: "measure"; message: string } | { kind: "store"; message: string };
export interface EndResult { ok: boolean; missing: number[]; failing: string | null }
export type PlanStep = "dip-off" | "stop-to-zero" | "measure" | "isolated-or-released";
export interface PlanView { before: PlanStep[]; after: "marker-on" | "zero" | "zero-or-unexplained" | "unsupported" | "no-assumption"; recovery: boolean }
export interface FlashMaint { installation: string; inverterConfirmed: boolean; dipOffConfirmed: boolean; isolatedConfirmed: boolean }
```

Extend `FirmwareDone` with `block: string | null; master: string | null`. Add to `tauriApi`:

```ts
  maintInstallationFor: (endpoint: Endpoint) => invoke<Installation>("maint_installation_for", { endpoint }),
  maintSaveInstallation: (installation: Installation) => invoke<Installation>("maint_save_installation", { installation }),
  maintOverview: (installation: string) => invoke<MaintOverview>("maint_overview", { installation }),
  /** reads 0x64 over the control channel and stores the classified state */
  maintCheck: (installation: string) => invoke<MaintOverview>("maint_check", { installation }),
  maintActivate: (installation: string, inverterConfirmed: boolean) => invoke<MaintOverview>("maint_activate", { installation, inverterConfirmed }),
  maintStartDip: (installation: string, expectedSlaves: number) => invoke<MaintOverview>("maint_start_dip", { installation, expectedSlaves }),
  maintAccountMarker: (installation: string, expectedSlaves: number) => invoke<MaintOverview>("maint_account_marker", { installation, expectedSlaves }),
  /** returns the time of the passed measurement (ms) */
  maintMeasure: (installation: string, port: string) => invoke<number>("maint_measure", { installation, port }),
  maintEnd: (installation: string, port: string) => invoke<EndResult>("maint_end", { installation, port }),
  maintClearBlock: (installation: string, id: number, how: "answered" | "isolated", port: string | null) => invoke<MaintOverview>("maint_clear_block", { installation, id, how, port }),
  maintMasterPlan: (installation: string, kind: FirmwareImage["kind"]) => invoke<PlanView>("maint_master_plan", { installation, kind }),
```

Change `firmwareStart` to add `maint: FlashMaint | null` and pass `{ ..., maint }`. In `demoApi`, add stubs that throw `new Error("desktop app only")`. Exceptions:
- `maintInstallationFor` returns `{ id: "demo", name: "Demo", endpoints: ["tcp:demo:4196"], control: null }`;
- `maintOverview` / `maintCheck` return `{ installation: <that>, state: null, state_at: null, session: null, blocks: [], sector_note: null }`.

Update the existing `firmwareStart` call in `Firmware.tsx` to pass `null` for now.

- [ ] **Step 2: Write the failing tests** `app/src/maint.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { bannerOf, canFlash, errorKey, needsReplug } from "./maint";
import type { MaintOverview, MaintSession } from "./api";

const inst = { id: "i", name: "Bank", endpoints: [], control: { kind: "serial" as const, path: "/dev/a" } };
const base: MaintOverview = { installation: inst, state: null, state_at: null, session: null, blocks: [], sector_note: null };
const session: MaintSession = { id: 1, installation: "i", kind: "marker", started: 1_000, expected_slaves: 3, done: [], failing: null };

describe("banner", () => {
  it("is hidden without session and without marker", () => {
    expect(bannerOf(base)).toBeNull();
    expect(bannerOf({ ...base, state: { state: "patched-off", slaves: 3 } })).toBeNull();
  });
  it("shows an open session with End, also after the connection was lost", () => {
    expect(bannerOf({ ...base, session, state: { state: "patched-on" } })).toMatchObject({ kind: "open", end: true, how: "marker" });
    expect(bannerOf({ ...base, session, state: { state: "unknown" } })).toMatchObject({ kind: "unconfirmed", end: true });
  });
  it("shows a marker without session, and never End for an unexplained marker", () => {
    expect(bannerOf({ ...base, state: { state: "patched-on" } })).toMatchObject({ kind: "open", end: true });
    expect(bannerOf({ ...base, state: { state: "marker-unexplained" } })).toMatchObject({ kind: "unexplained", end: false });
    expect(bannerOf({ ...base, session, state: { state: "marker-unexplained" } })).toMatchObject({ kind: "unexplained", end: false });
  });
});

describe("flash unlock", () => {
  const ok = { overview: { ...base, session, state: { state: "patched-on" as const } }, inverter: true, measuredAt: 5_000 };
  it("needs the inverter tick and a measurement in a session", () => {
    expect(canFlash(ok).ok).toBe(true);
    expect(canFlash({ ...ok, inverter: false })).toEqual({ ok: false, reason: "inverter" });
    expect(canFlash({ ...ok, measuredAt: null })).toEqual({ ok: false, reason: "measure" });
  });
  it("is blocked by an unclear update and by an unexplained marker", () => {
    expect(canFlash({ ...ok, overview: { ...ok.overview, blocks: [{ id: 1, pack: 2, reason: "update-unclear", since: 0 }] } }).reason).toBe("update-unclear");
    expect(canFlash({ ...ok, overview: { ...base, state: { state: "marker-unexplained" } } }).reason).toBe("marker-unexplained");
  });
  it("a measurement older than the last state check does not count", () => {
    expect(canFlash({ ...ok, overview: { ...ok.overview, state_at: 6_000 }, measuredAt: 5_000 }).reason).toBe("measure");
  });
  it("a single pack without maintenance needs neither", () => {
    expect(canFlash({ overview: { ...base, state: { state: "patched-off", slaves: 0 } }, inverter: false, measuredAt: null }).ok).toBe(true);
  });
});

describe("helpers", () => {
  it("asks to move the cable when control and pack port are one adapter", () => {
    expect(needsReplug({ kind: "serial", path: "/dev/a" }, "/dev/a")).toBe(true);
    expect(needsReplug({ kind: "serial", path: "/dev/a" }, "/dev/b")).toBe(false);
    expect(needsReplug({ kind: "tcp", addr: "gw:4196" }, "/dev/a")).toBe(false);
    expect(needsReplug(null, "/dev/a")).toBe(false);
  });
  it("turns backend errors into text keys", () => {
    expect(errorKey({ kind: "refused", reason: "inverter" })).toEqual({ key: "maint.err.refused.inverter" });
    expect(errorKey('{"kind":"no-answer"}')).toEqual({ key: "maint.err.no-answer" });
    expect(errorKey("dip-off")).toEqual({ key: "maint.err.refused.dip-off" });
    expect(errorKey("some plain text")).toEqual({ key: "maint.err.text", params: { text: "some plain text" } });
    expect(errorKey({ kind: "timeout", marker: true, slaves: 11 })).toEqual({ key: "maint.err.timeout", params: { marker: 1, slaves: 11 } });
  });
  it("the inverter confirmation is not part of the overview", () => {
    expect(Object.keys(base)).not.toContain("inverter");
  });
});
```

- [ ] **Step 3: Run them.** `cd app && npx vitest run src/maint.test.ts` → FAIL (module missing).

- [ ] **Step 4: Implement** `app/src/maint.ts`

```ts
import type { ControlEndpoint, MaintError, MaintOverview } from "./api";

/** What the banner above every view shows; null = nothing open. */
export interface Banner {
  kind: "open" | "unconfirmed" | "unexplained";
  /** "End" is offered (never for an unexplained marker) */
  end: boolean;
  how: "marker" | "dip";
  since: number | null;
}

export function bannerOf(o: MaintOverview | null): Banner | null {
  if (!o) return null;
  const s = o.state?.state;
  if (s === "marker-unexplained") return { kind: "unexplained", end: false, how: "marker", since: o.session?.started ?? null };
  if (o.session) return { kind: s === "unknown" ? "unconfirmed" : "open", end: true, how: o.session.kind, since: o.session.started };
  if (s === "patched-on") return { kind: "open", end: true, how: "marker", since: null };
  return null;
}

/** Control channel and pack port on one USB adapter: the cable must be moved between the steps. */
export function needsReplug(control: ControlEndpoint | null, packPort: string): boolean {
  return control?.kind === "serial" && control.path === packPort;
}

export function canFlash({ overview: o, inverter, measuredAt }: { overview: MaintOverview | null; inverter: boolean; measuredAt: number | null }): { ok: boolean; reason?: string } {
  if (!o) return { ok: true };
  if (o.blocks.length) return { ok: false, reason: o.blocks[0].reason };
  if (o.state?.state === "marker-unexplained") return { ok: false, reason: "marker-unexplained" };
  const maintenance = !!o.session || o.state?.state === "patched-on";
  if (!maintenance) return { ok: true };
  if (!inverter) return { ok: false, reason: "inverter" };
  // a measurement counts only if it is newer than the last confirmed state
  if (measuredAt === null || (o.state_at !== null && measuredAt < o.state_at)) return { ok: false, reason: "measure" };
  return { ok: true };
}

const KINDS = ["connection", "unsupported", "no-answer", "rejected", "timeout", "refused", "busy", "measure", "store"];

/** Backend errors arrive as a MaintError object, as its JSON text, as a bare refusal key, or as plain text. */
export function errorKey(e: unknown): { key: string; params?: Record<string, unknown> } {
  let v: unknown = e;
  if (typeof e === "string") {
    try { v = JSON.parse(e); } catch { v = /^[a-z0-9-]+$/.test(e) ? { kind: "refused", reason: e } : e; }
  }
  if (v && typeof v === "object" && "kind" in v && KINDS.includes(String((v as MaintError).kind))) {
    const m = v as MaintError;
    switch (m.kind) {
      case "refused": return { key: `maint.err.refused.${m.reason}` };
      case "timeout": return { key: "maint.err.timeout", params: { marker: Number(m.marker), slaves: m.slaves } };
      case "rejected": return { key: "maint.err.rejected", params: { rtn: m.rtn.toString(16).padStart(2, "0") } };
      case "connection": case "measure": case "store": return { key: `maint.err.${m.kind}`, params: { text: m.message } };
      default: return { key: `maint.err.${m.kind}` };
    }
  }
  return { key: "maint.err.text", params: { text: String(e) } };
}
```

- [ ] **Step 5: Run them.** `cd app && npx vitest run && npx tsc -b` → PASS.
- [ ] **Step 6: Commit.** `feat(ui): maintenance API and state helpers`

---

### Task 10: Installation setup, banner, re-check on start and connect

**Files:**
- Create: `app/src/views/MaintBanner.tsx`, `app/src/views/MaintSetup.tsx`, `app/src/maintStore.ts`
- Modify: `app/src/App.tsx` (hold the overview, render the banner above `main`'s content), `app/src/views/Sidebar.tsx` (tell App the endpoint after connecting), `app/src/i18n.ts` (keys `maint.*` DE/EN)

**Interfaces:**
- Consumes: Task 9.
- Produces:
  - `useMaint()` returns `{ overview, setOverview, installation, attach(endpoint: Endpoint, bus: Bus), refresh(), check() }`; `installation` is the id, persisted in `localStorage["maint.installation"]`.
  - `<MaintBanner overview onEnd />`
  - `<MaintSetup overview onSaved />`
  - `Sidebar` prop `onEndpoint(endpoint: Endpoint)`

- [ ] **Step 1: Hook** `app/src/maintStore.ts`

```ts
import { useCallback, useEffect, useState } from "react";
import { api, isDemo, type Bus, type Endpoint, type MaintOverview } from "./api";

/** The installation in use and its maintenance overview. The banner reads from here, so it survives view
 *  changes, reconnects and (through the backend database) app restarts. */
export function useMaint() {
  const [installation, setInstallation] = useState<string | null>(() => localStorage.getItem("maint.installation"));
  const [overview, setOverview] = useState<MaintOverview | null>(null);

  const refresh = useCallback(async (id = installation) => {
    if (!id || isDemo) return;
    setOverview(await api.maintOverview(id).catch(() => null));
  }, [installation]);

  /** Re-read 0x64 (start-up, every new connection on the CAN socket bus); errors leave the stored state. */
  const check = useCallback(async (id = installation) => {
    if (!id || isDemo) return;
    try { setOverview(await api.maintCheck(id)); } catch { await refresh(id); }
  }, [installation, refresh]);

  const attach = useCallback(async (endpoint: Endpoint, bus: Bus) => {
    const inst = await api.maintInstallationFor(endpoint).catch(() => null);
    if (!inst) return;
    localStorage.setItem("maint.installation", inst.id);
    setInstallation(inst.id);
    if (bus === "can" && inst.control) await check(inst.id); else await refresh(inst.id);
  }, [check, refresh]);

  // start-up: show the last confirmed state at once, then re-check if a control channel is set
  useEffect(() => {
    (async () => {
      await refresh();
      const o = installation && !isDemo ? await api.maintOverview(installation).catch(() => null) : null;
      if (o?.installation.control) await check();
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return { installation, overview, setOverview, attach, refresh, check };
}
```

- [ ] **Step 2: Banner** `app/src/views/MaintBanner.tsx`

```tsx
import { useTranslation } from "react-i18next";
import { TriangleAlert, Wrench } from "lucide-react";
import type { MaintOverview } from "../api";
import { bannerOf } from "../maint";
import { Button } from "../components/ui/button";
import { cn } from "../lib/utils";

export function MaintBanner({ overview, onEnd }: { overview: MaintOverview | null; onEnd: () => void }) {
  const { t } = useTranslation();
  const b = bannerOf(overview);
  if (!b) return null;
  const since = b.since ? new Date(b.since).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : null;
  return (
    <div role="status" className={cn("mb-6 flex items-start gap-3 rounded-md border p-3 text-sm", b.kind === "open" ? "border-discharge" : "border-alarm text-alarm")}>
      {b.kind === "open" ? <Wrench className="mt-0.5 h-4 w-4 shrink-0" /> : <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />}
      <div className="flex-1 space-y-1">
        <p className="font-medium">{t(`maint.banner.${b.kind}`, { since: since ?? "?", how: t(`maint.how.${b.how}`) })}</p>
        <p className="text-muted">{t(b.kind === "unexplained" ? "maint.banner.unexplainedHint" : "maint.banner.hint")}</p>
        {overview?.session?.failing && <p className="text-alarm">{t("maint.banner.failing", { what: overview.session.failing })}</p>}
      </div>
      {b.end && <Button variant="ghost" size="sm" onClick={onEnd}>{t("maint.end")}</Button>}
    </div>
  );
}
```

- [ ] **Step 3: Setup** `app/src/views/MaintSetup.tsx`. It is a small form for name and control channel (kind TCP/USB, address or port). The main endpoints are listed read-only; the assignment of the current endpoint happens through `attach`.

```tsx
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type ControlEndpoint, type MaintOverview } from "../api";
import { Button } from "../components/ui/button";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function MaintSetup({ overview, onSaved }: { overview: MaintOverview; onSaved: (o: MaintOverview) => void }) {
  const { t } = useTranslation();
  const inst = overview.installation;
  const [name, setName] = useState(inst.name);
  const [kind, setKind] = useState<ControlEndpoint["kind"]>(inst.control?.kind ?? "serial");
  const [value, setValue] = useState(inst.control ? (inst.control.kind === "tcp" ? inst.control.addr : inst.control.path) : "");
  const [ports, setPorts] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);

  const save = async () => {
    setError(null);
    const control: ControlEndpoint | null = !value ? null : kind === "tcp" ? { kind, addr: value } : { kind, path: value };
    try {
      await api.maintSaveInstallation({ ...inst, name, control });
      onSaved(await api.maintOverview(inst.id));
    } catch (e) { setError(String(e)); }
  };

  return (
    <div className="space-y-3 text-sm">
      <label className="flex items-center gap-3">{t("maint.setup.name")}<input className={input} value={name} onChange={(e) => setName(e.target.value)} /></label>
      <div className="flex flex-wrap items-center gap-3">
        {t("maint.setup.control")}
        <select className={input} value={kind} onChange={(e) => { setKind(e.target.value as ControlEndpoint["kind"]); setValue(""); }}>
          <option value="serial">{t("conn.serial")}</option>
          <option value="tcp">{t("conn.tcp")}</option>
        </select>
        {kind === "tcp"
          ? <input className={input} value={value} onChange={(e) => setValue(e.target.value)} placeholder="192.168.1.10:4196" />
          : <select className={input} value={value} onChange={(e) => setValue(e.target.value)}><option value="">{t("conn.port")}</option>{ports.map((p) => <option key={p}>{p}</option>)}</select>}
      </div>
      <p className="text-muted">{t("maint.setup.controlHint")}</p>
      <p className="text-muted">{t("maint.setup.endpoints", { list: inst.endpoints.join(", ") || "–" })}</p>
      <Button variant="ghost" size="sm" onClick={save}>{t("maint.setup.save")}</Button>
      {error && <p className="text-alarm">{error}</p>}
    </div>
  );
}
```

- [ ] **Step 4: Wire App and Sidebar.**
  - `App.tsx`:
    - `const maint = useMaint();`
    - render `<MaintBanner overview={maint.overview} onEnd={() => setView("firmware")} />` as the first child inside `<main>` (End lives in the Pack bus panel);
    - pass `maint` to `<Firmware maint={maint} …/>`;
    - pass `onEndpoint={(ep) => maint.attach(ep, ep.bus)}` to `Sidebar`.
  - `Sidebar.tsx`:
    - add the prop `onEndpoint: (endpoint: Endpoint) => void` and import `type Endpoint`;
    - in `connect`, after `await api.connect(ep)`, call `onEndpoint(ep)` without awaiting, where `const ep: Endpoint = kind === "tcp" ? { kind, addr: target, bus } : { kind, path: target, bus };`;
    - pass `ep` to `api.connect` too.
- [ ] **Step 5: i18n.** Add a `maint` block next to `fw` in both languages. DE:

```ts
      maint: {
        end: "Beenden",
        how: { marker: "Wartungsmodus", dip: "DIP 5–8 AUS" },
        banner: {
          open: "Wartung des Masters offen seit {{since}} ({{how}}). Der Master meldet dem Wechselrichter nur sein eigenes Pack.",
          unconfirmed: "Wartung offen seit {{since}} ({{how}}), nicht bestätigt: Verbindung verloren.",
          unexplained: "Der Master meldet einen Wartungsmarker, dessen Herkunft unklar ist.",
          hint: "Zuletzt bestätigter Zustand, keine Garantie.",
          unexplainedHint: "Kein Beenden, kein „stop“: Das würde den ganzen Sektor 0x23000 löschen. Weg zurück: Original-Firmware unter DIP 5–8 AUS (Firmware-Ansicht) oder den Sektor zuerst untersuchen.",
          failing: "Letzte Prüfung beim Beenden fehlgeschlagen: {{what}}",
        },
        setup: {
          name: "Name der Anlage", control: "Steuerkanal (CAN-Buchse, 9600 Baud)",
          controlHint: "Für den Wartungsmodus des Masters (Befehl 0x64). Am selben Adapter wie die Verbindung links läuft der Austausch über diese Verbindung.",
          endpoints: "Verbindungen dieser Anlage: {{list}}", save: "Speichern",
        },
        err: {
          connection: "Verbindung: {{text}}", unsupported: "Der Master kennt den Wartungsbefehl nicht (RTN 04).",
          "no-answer": "Der Master antwortet nicht auf dem Steuerkanal.", rejected: "Der Master hat die Anfrage abgelehnt (RTN {{rtn}}).",
          timeout: "Der Master hat den neuen Zustand nicht rechtzeitig gemeldet (zuletzt Marker {{marker}}, {{slaves}} Slaves).",
          busy: "Eine andere Wartungsaktion läuft.", measure: "Messung: {{text}}", store: "Datenbank: {{text}}", text: "{{text}}",
          refused: {
            inverter: "Erst den Wechselrichter bestätigen.", "update-unclear": "Ein unklares Update sperrt weitere Updates an diesem Bus.",
            "bootloader-unclear": "Ein Pack ist eventuell noch im Bootloader; weitere Updates sind gesperrt.",
            "marker-unexplained": "Gesperrt: Herkunft des Markers ungeklärt.", "no-control-channel": "Kein Steuerkanal eingerichtet.",
            "control-on-pack-bus": "Der Adapter des Steuerkanals ist gerade am Pack-Bus verbunden.", "not-patched-off": "Der Master ist nicht im Zustand „gepatcht, Marker aus“.",
            "marker-not-active": "Der Master meldet keinen aktiven Marker mehr.", "no-session": "Keine offene Wartung.",
            "quiet-first": "Der Master fragt noch ab: erst Wartungsmodus aktivieren oder DIP 5–8 AUS.", "state-unknown": "Zustand des Masters unbekannt.",
            "dip-off": "Erst DIP 5–8 AUS bestätigen.", "isolated-or-released": "Erst bestätigen: Build für den gemeinsamen Bus freigegeben oder Master getrennt.",
            "pack-not-answering": "Das Pack antwortet noch nicht als Anwendung.", "no-port": "Erst den Port wählen.", "no-block": "Sperre nicht gefunden.",
            how: "Ungültige Angabe.", "update-unclear-or-inverter": "Wiederherstellung: Wechselrichter bestätigen; keine offene Update-Sperre.",
          },
        },
      },
```

EN (same keys):

```ts
      maint: {
        end: "End",
        how: { marker: "maintenance mode", dip: "DIP 5–8 OFF" },
        banner: {
          open: "Maintenance of the master open since {{since}} ({{how}}). The master reports only its own pack to the inverter.",
          unconfirmed: "Maintenance open since {{since}} ({{how}}), not confirmed: connection lost.",
          unexplained: "The master reports a maintenance marker of unexplained origin.",
          hint: "Last confirmed state, not a guarantee.",
          unexplainedHint: "No End, no “stop”: it would erase the whole sector 0x23000. Way back: original firmware under DIP 5–8 OFF (firmware view), or examine the sector first.",
          failing: "Last end check failed: {{what}}",
        },
        setup: {
          name: "Installation name", control: "Control channel (CAN socket, 9600 baud)",
          controlHint: "For the master's maintenance mode (command 0x64). On the same adapter as the connection on the left, the exchange uses that connection.",
          endpoints: "Connections of this installation: {{list}}", save: "Save",
        },
        err: {
          connection: "Connection: {{text}}", unsupported: "The master does not know the maintenance command (RTN 04).",
          "no-answer": "The master does not answer on the control channel.", rejected: "The master rejected the request (RTN {{rtn}}).",
          timeout: "The master did not report the new state in time (last marker {{marker}}, {{slaves}} slaves).",
          busy: "Another maintenance action is running.", measure: "Measurement: {{text}}", store: "Database: {{text}}", text: "{{text}}",
          refused: {
            inverter: "Confirm the inverter first.", "update-unclear": "An unclear update blocks further updates on this bus.",
            "bootloader-unclear": "A pack may still be in its bootloader; further updates are blocked.",
            "marker-unexplained": "Locked: origin of the marker unexplained.", "no-control-channel": "No control channel set up.",
            "control-on-pack-bus": "The control channel's adapter is connected to the pack bus right now.", "not-patched-off": "The master is not in the state “patched, marker off”.",
            "marker-not-active": "The master no longer reports an active marker.", "no-session": "No open maintenance.",
            "quiet-first": "The master still polls: activate maintenance mode or set DIP 5–8 OFF first.", "state-unknown": "State of the master unknown.",
            "dip-off": "Confirm DIP 5–8 OFF first.", "isolated-or-released": "Confirm first: build released for the shared bus, or master isolated.",
            "pack-not-answering": "The pack does not answer as an application yet.", "no-port": "Choose the port first.", "no-block": "Block not found.",
            how: "Invalid choice.", "update-unclear-or-inverter": "Recovery: confirm the inverter; no open update block.",
          },
        },
      },
```

Task 11 adds the panel keys under `maint.panel`. Note that the existing top-level key `maint: "Wartungsfunktionen anzeigen"` (line 46 / 235) collides. Rename it to `maintToggle` and update `Sidebar.tsx` (`t("maint")` → `t("maintToggle")`, two places).

- [ ] **Step 6: Check.** `cd app && npx vitest run && npx tsc -b` → PASS. Then start the dev app (`npx tauri dev`) and see that:
  - the banner is hidden on a fresh database;
  - nothing breaks on connect.
- [ ] **Step 7: Commit.** `feat(ui): maintenance banner, installation setup, re-check on start and connect`

---

### Task 11: "Pack bus" panel in the firmware view (check, inverter, activate, measure, end, blocks)

**Files:**
- Create: `app/src/views/PackBusPanel.tsx`
- Modify: `app/src/views/Firmware.tsx` (render the panel above step 1, pass `port`, gate the start button with `canFlash`, pass `FlashMaint` to `firmwareStart`, master-flash plan)
- Modify: `app/src/i18n.ts` (keys `maint.panel.*`)
- Modify: `app/src/firmware.ts` + `app/src/firmware.test.ts` (`steps()` takes `unlocked: boolean`)

**Interfaces:**
- Consumes: Tasks 9, 10.
- Produces: `<PackBusPanel maint port inverter setInverter measuredAt setMeasuredAt />`; `steps({..., unlocked})`.

- [ ] **Step 1: Write the failing test** (add to `firmware.test.ts`)

```ts
it("start stays locked while maintenance does not unlock it", () => {
  const s = { running: false, done: false, port: "/dev/a", address: 2, image: { release: "16.06.13", device: "d", bytes: 1, blocks: 1, sha256: "x", custom: false, kind: "original" as const },
    target: { device_name: "n", firmware_version: "16.06.12", can_protocol: "c" }, checkedFor: { port: "/dev/a", address: 2 }, typed: "FLASH 2" };
  expect(steps({ ...s, unlocked: true }).start).toBe(true);
  expect(steps({ ...s, unlocked: false }).start).toBe(false);
});
```

- [ ] **Step 2: Run it.** `npx vitest run src/firmware.test.ts` → FAIL (TS: `unlocked` not in `FwSetup`).
- [ ] **Step 3: Implement.** In `firmware.ts`, add `unlocked: boolean;` to `FwSetup` and `&& s.unlocked` to `start`. Update the existing tests' setup objects with `unlocked: true`.
- [ ] **Step 4: Panel** `PackBusPanel.tsx`

```tsx
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, TriangleAlert } from "lucide-react";
import { api, type MaintOverview } from "../api";
import { errorKey, needsReplug } from "../maint";
import type { useMaint } from "../maintStore";
import { Button } from "../components/ui/button";
import { MaintSetup } from "./MaintSetup";

const input = "h-9 w-20 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function PackBusPanel({ maint, port, inverter, setInverter, measuredAt, setMeasuredAt }: {
  maint: ReturnType<typeof useMaint>;
  port: string;
  inverter: boolean;
  setInverter: (v: boolean) => void;
  measuredAt: number | null;
  setMeasuredAt: (v: number | null) => void;
}) {
  const { t } = useTranslation();
  const o = maint.overview;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [n, setN] = useState(0);
  const [dipConfirmed, setDipConfirmed] = useState(false);
  const [setup, setSetup] = useState(false);
  const [endNote, setEndNote] = useState<string | null>(null);

  const run = async (f: () => Promise<MaintOverview | void>) => {
    setBusy(true); setError(null);
    try { const r = await f(); if (r) maint.setOverview(r); }
    catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); await maint.refresh(); }
    finally { setBusy(false); }
  };

  if (!o) return <section className="space-y-2 rounded-md border border-line p-4 text-sm text-muted">{t("maint.panel.noInstallation")}</section>;
  const s = o.state?.state;
  const replug = needsReplug(o.installation.control, port);

  return (
    <section className="space-y-3 rounded-md border border-line p-4 text-sm">
      <div className="flex items-center justify-between">
        <h2 className="font-display text-lg font-semibold">{t("maint.panel.title", { name: o.installation.name })}</h2>
        <Button variant="ghost" size="sm" onClick={() => setSetup(!setup)}>{t("maint.panel.setup")}</Button>
      </div>
      {setup && <MaintSetup overview={o} onSaved={(x) => { maint.setOverview(x); setSetup(false); }} />}

      {/* 1. check master */}
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="sm" disabled={busy || !o.installation.control} onClick={() => run(() => api.maintCheck(o.installation.id))}>{t("maint.panel.check")}</Button>
        <span>{s ? t(`maint.state.${s}`, { slaves: o.state?.slaves ?? 0 }) : t("maint.state.none")}</span>
        {replug && <span className="text-discharge">{t("maint.panel.replugControl")}</span>}
      </div>

      {s === "patched-off" && (o.state?.slaves ?? 0) > 0 && !o.session && (
        <Button variant="primary" size="sm" disabled={busy || !inverter} onClick={() => run(() => api.maintActivate(o.installation.id, inverter))}>{t("maint.panel.activate")}</Button>
      )}
      {(s === "no-0x64" || s === "unknown") && !o.session && (
        <div className="space-y-2">
          <p>{t(s === "unknown" ? "maint.panel.unknownHint" : "maint.panel.dipHint")}</p>
          <label className="flex items-center gap-2"><input type="checkbox" checked={dipConfirmed} onChange={(e) => setDipConfirmed(e.target.checked)} />{t("maint.panel.dipConfirm")}</label>
          <label className="flex items-center gap-2">{t("maint.panel.slaves")}<input type="number" min={0} max={15} className={input} value={n} onChange={(e) => setN(Number(e.target.value))} /></label>
          <Button variant="ghost" size="sm" disabled={busy || !dipConfirmed} onClick={() => run(() => api.maintStartDip(o.installation.id, n))}>{t("maint.panel.startDip")}</Button>
        </div>
      )}
      {s === "marker-unexplained" && (
        <div className="space-y-2">
          <p className="text-alarm">{t("maint.banner.unexplainedHint")}</p>
          <label className="flex items-center gap-2">{t("maint.panel.slaves")}<input type="number" min={0} max={15} className={input} value={n} onChange={(e) => setN(Number(e.target.value))} /></label>
          <Button variant="ghost" size="sm" disabled={busy} onClick={() => run(() => api.maintAccountMarker(o.installation.id, n))}>{t("maint.panel.accountMarker")}</Button>
        </div>
      )}

      {/* 2. inverter: in memory only, cleared with every new work phase */}
      {(o.session || s === "patched-on" || (s === "patched-off" && (o.state?.slaves ?? 0) > 0)) && s !== "marker-unexplained" && (
        <label className="flex items-start gap-2">
          <input type="checkbox" className="mt-1" checked={inverter} onChange={(e) => setInverter(e.target.checked)} />
          <span>{t("maint.panel.inverter")}<span className="block text-muted">{t("maint.panel.inverterHint")}</span></span>
        </label>
      )}

      {/* 3. measure */}
      {(o.session || s === "patched-on") && s !== "marker-unexplained" && (
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="ghost" size="sm" disabled={busy || !port} onClick={() => run(async () => { setMeasuredAt(null); setMeasuredAt(await api.maintMeasure(o.installation.id, port)); })}>{t("maint.panel.measure")}</Button>
          {replug && <span className="text-discharge">{t("maint.panel.replugPack")}</span>}
          {measuredAt && <span className="flex items-center gap-1 text-ok"><CircleCheck className="h-4 w-4" />{t("maint.panel.quietAt", { time: new Date(measuredAt).toLocaleTimeString() })}</span>}
        </div>
      )}

      {/* unclear updates */}
      {o.blocks.map((b) => (
        <div key={b.id} className="space-y-1 rounded-md border border-alarm p-2 text-alarm">
          <p className="flex gap-2"><TriangleAlert className="h-4 w-4 shrink-0" />{t(`maint.panel.block.${b.reason}`, { pack: b.pack })}</p>
          <div className="flex gap-2">
            <Button variant="ghost" size="sm" disabled={busy || !port} onClick={() => run(() => api.maintClearBlock(o.installation.id, b.id, "answered", port))}>{t("maint.panel.blockCheck")}</Button>
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => run(() => api.maintClearBlock(o.installation.id, b.id, "isolated", null))}>{t("maint.panel.blockIsolated")}</Button>
          </div>
        </div>
      ))}

      {/* 5. end */}
      {o.session && s !== "marker-unexplained" && (
        <div className="space-y-1">
          <Button variant="primary" size="sm" disabled={busy || !port} onClick={() => run(async () => {
            setEndNote(null);
            const r = await api.maintEnd(o.installation.id, port);
            setInverter(false); setMeasuredAt(null);
            setEndNote(r.ok ? t("maint.panel.endOk") : r.missing.length ? t("maint.panel.endMissing", { list: r.missing.join(", ") }) : t("maint.panel.endFailed", { what: r.failing }));
            return api.maintOverview(o.installation.id);
          })}>{t("maint.end")}</Button>
          {o.session.kind === "dip" && <p className="text-muted">{t("maint.panel.dipBack")}</p>}
          {endNote && <p>{endNote}</p>}
        </div>
      )}
      <p className="text-muted">{t("maint.panel.residual")}</p>
      {error && <p className="text-alarm">{error}</p>}
    </section>
  );
}
```

- [ ] **Step 5: Firmware view wiring** (`Firmware.tsx`)
  - Add the prop `maint: ReturnType<typeof useMaint>`.
  - State: `inverter` (false) and `measuredAt` (null). Reset both whenever `maint.overview?.state_at` or `maint.overview?.session?.id` changes: `useEffect(() => { setMeasuredAt(null); }, [maint.overview?.state_at]); useEffect(() => { setInverter(false); }, [maint.overview?.session?.id]);`
  - Render `<PackBusPanel maint={maint} port={port} inverter={inverter} setInverter={setInverter} measuredAt={measuredAt} setMeasuredAt={setMeasuredAt} />` between the header and step 1.
  - `const gate = canFlash({ overview: maint.overview, inverter, measuredAt });` → pass `unlocked: gate.ok` to `steps()`. When `!gate.ok`, show `t(errorKey(gate.reason).key)` under the start button.
  - **Master (address 0):** when `image` and `maint.overview` are set and `address === 0`, call `api.maintMasterPlan(id, image.kind)`.
    - Show the steps as a checklist: `maint.plan.<step>` and `maint.plan.after.<after>`.
    - Show a checkbox for `dip-off` and one for `isolated-or-released`, plus the recovery note if `plan.recovery`.
    - When the call is refused, show `errorKey(e)` and keep start locked.
  - `start`: pass `maint.overview ? { installation: maint.overview.installation.id, inverterConfirmed: inverter, dipOffConfirmed, isolatedConfirmed } : null`.
  - **In `onFirmwareDone`:**
    - call `maint.refresh()`;
    - reset `setMeasuredAt(null)` (one measurement, one flash);
    - if `d.block`, show `t("maint.panel.block." + d.block, { pack: address })`;
    - if `d.master`, show `t("maint.master." + d.master)`.
- [ ] **Step 6: i18n.** Add to `maint` in DE:

```ts
        state: {
          none: "Noch nicht geprüft.", "patched-off": "Gepatchter Master, Marker aus, {{slaves}} Slaves.", "patched-on": "Wartungsmodus aktiv (Marker an).",
          "no-0x64": "Master ohne Wartungsbefehl (RTN 04).", unknown: "Zustand unbekannt (keine Antwort oder Verbindungsfehler).",
          "marker-unexplained": "Marker gesetzt, Herkunft ungeklärt: gesperrt.",
        },
        panel: {
          title: "Pack-Bus: {{name}}", setup: "Einrichten", noInstallation: "Erst links verbinden, dann ist die Anlage bekannt.",
          check: "Master prüfen", activate: "Wartungsmodus aktivieren",
          dipHint: "Dieser Master kennt den Wartungsmodus nicht. Stellungen von DIP 5–8 notieren und auf AUS stellen.",
          unknownHint: "Der Master antwortet nicht auf dem Steuerkanal. Verbindung prüfen, oder von Hand: DIP 5–8 notieren und auf AUS stellen.",
          dipConfirm: "DIP 5–8 stehen auf AUS, die alten Stellungen sind notiert.", slaves: "Slaves laut DIP (N)", startDip: "Wartung per DIP beginnen",
          accountMarker: "Ich habe den Marker selbst gesetzt",
          inverter: "Wechselrichter: kein Laden und kein Entladen der Batterie.",
          inverterHint: "Wartungsmodus und DIP 5–8 AUS halten den Wechselrichter nicht an. Der Master meldet nur noch sein eigenes Pack: DCL fällt sofort auf ein Pack, CCL sinkt nur mit 1 A/s. Während der Master selbst geflasht wird, endet CAN ganz. Das ist eine Angabe des Bedieners, keine technische Prüfung.",
          measure: "Bus messen", quietAt: "Bus still um {{time}}",
          replugControl: "Kabel an die CAN-Buchse stecken.", replugPack: "Kabel an RS485-1/2 stecken.",
          block: { "update-unclear": "Update an Pack {{pack}} unklar: weitere Updates gesperrt.", "bootloader-unclear": "Pack {{pack}} ist eventuell im Bootloader: weitere Updates gesperrt." },
          blockCheck: "Pack prüfen", blockIsolated: "Pack getrennt oder wiederhergestellt",
          endOk: "Beendet: alle Packs werden wieder abgefragt. Jetzt den Wechselrichter wieder freigeben.",
          endMissing: "Zurückgeschaltet, Pack-Kommunikation unvollständig (fehlt: {{list}}). Die Wartung bleibt offen.",
          endFailed: "Prüfung fehlgeschlagen ({{what}}). Die Wartung bleibt offen.",
          dipBack: "Vorher DIP 5–8 auf die notierten Stellungen zurückstellen.",
          residual: "Die Messung sieht kein Pack, das still in seinem Bootloader hängt. Ergebnis heißt „Bus still um …“, nicht „Bus exklusiv“.",
        },
        plan: {
          "dip-off": "DIP 5–8 AUS (bestätigen)", "stop-to-zero": "Marker löschen (stop) und auf 0/0 warten", measure: "Bus messen",
          "isolated-or-released": "Build für den gemeinsamen Bus freigegeben oder Master getrennt (bestätigen)",
          recovery: "Wiederherstellung ohne stop: Der Inhalt von 0x23000 bleibt ungeklärt und wird vermerkt.",
          after: { "marker-on": "Danach erwartet: Marker an (1/0).", zero: "Danach erwartet: 0/0.", "zero-or-unexplained": "Danach erwartet: 0/0.", unsupported: "Danach erwartet: RTN 04 (Original).", "no-assumption": "Danach: keine Annahme, DIP 5–8 bleiben AUS." },
        },
        master: {
          "as-expected": "Master meldet den erwarteten Zustand.", unclear: "Update unklar: Wartung bleibt offen, kein Freigeben des Wechselrichters. DIP 5–8 AUS hält den Bus still.",
          "marker-unexplained": "Master meldet einen Marker unklarer Herkunft: gesperrt.", "unknown-image": "Eigenes Image: Zustand unbekannt, DIP 5–8 bleiben AUS.",
        },
```

EN:

```ts
        state: {
          none: "Not checked yet.", "patched-off": "Patched master, marker off, {{slaves}} slaves.", "patched-on": "Maintenance mode active (marker on).",
          "no-0x64": "Master without the maintenance command (RTN 04).", unknown: "State unknown (no answer or connection error).",
          "marker-unexplained": "Marker set, origin unexplained: locked.",
        },
        panel: {
          title: "Pack bus: {{name}}", setup: "Set up", noInstallation: "Connect on the left first; then the installation is known.",
          check: "Check master", activate: "Activate maintenance mode",
          dipHint: "This master does not know maintenance mode. Note the positions of DIP 5–8 and set them OFF.",
          unknownHint: "The master does not answer on the control channel. Check the connection, or by hand: note DIP 5–8 and set them OFF.",
          dipConfirm: "DIP 5–8 are OFF, the old positions are noted.", slaves: "Slaves per DIP (N)", startDip: "Start maintenance by DIP",
          accountMarker: "I set the marker myself",
          inverter: "Inverter: no battery charging or discharging.",
          inverterHint: "Maintenance mode and DIP 5–8 OFF do not stop the inverter. The master then reports only its own pack: DCL drops to one pack at once, CCL only ramps down 1 A/s. While the master itself is flashed, CAN stops entirely. This is an operator statement, not a technical check.",
          measure: "Measure bus", quietAt: "Bus quiet at {{time}}",
          replugControl: "Plug the cable into the CAN socket.", replugPack: "Plug the cable into RS485-1/2.",
          block: { "update-unclear": "Update of pack {{pack}} unclear: further updates blocked.", "bootloader-unclear": "Pack {{pack}} may be in its bootloader: further updates blocked." },
          blockCheck: "Check pack", blockIsolated: "Pack isolated or recovered",
          endOk: "Ended: every pack is polled again. Now give the inverter its battery power back.",
          endMissing: "Switched back, pack communication incomplete (missing: {{list}}). Maintenance stays open.",
          endFailed: "Check failed ({{what}}). Maintenance stays open.",
          dipBack: "First set DIP 5–8 back to the noted positions.",
          residual: "The measurement cannot see a pack that sits silently in its bootloader. The result reads “bus quiet at …”, not “bus exclusive”.",
        },
        plan: {
          "dip-off": "DIP 5–8 OFF (confirm)", "stop-to-zero": "Clear the marker (stop) and wait for 0/0", measure: "Measure bus",
          "isolated-or-released": "Build released for the shared bus, or master isolated (confirm)",
          recovery: "Recovery without stop: the content of 0x23000 stays unexplained and is recorded.",
          after: { "marker-on": "Expected afterwards: marker on (1/0).", zero: "Expected afterwards: 0/0.", "zero-or-unexplained": "Expected afterwards: 0/0.", unsupported: "Expected afterwards: RTN 04 (original).", "no-assumption": "Afterwards: no assumption, DIP 5–8 stay OFF." },
        },
        master: {
          "as-expected": "The master reports the expected state.", unclear: "Update unclear: maintenance stays open, do not release the inverter. DIP 5–8 OFF keeps the bus quiet.",
          "marker-unexplained": "The master reports a marker of unexplained origin: locked.", "unknown-image": "Custom image: state unknown, DIP 5–8 stay OFF.",
        },
```

- [ ] **Step 7: Check.** `cd app && npx vitest run && npx tsc -b` → PASS. Then open the firmware view in the dev app without a control channel. Expected:
  - the panel shows "Not checked yet";
  - the start button keeps working for a single pack;
  - with a configured control channel, "Check master" shows the state.
- [ ] **Step 8: Commit.** `feat(ui): pack bus panel with master check, inverter, measurement, end and blocks`

---

### Task 12: Help, README, spec status

**Files:**
- Modify: `tools/gen_help.py` (topic `topic.firmware` updated, new topic `topic.maintenance`), then run `python3 tools/gen_help.py`
- Modify: `README.md` (maintenance section)
- Modify: `docs/superpowers/specs/2026-10-12-maintenance-mode-design.md`:
  - Backend: own file `maintenance.sqlite` instead of the history database, with the reason;
  - master-flash table: rows `no-0x64 → original` and `no-0x64 → custom`;
  - status: "stage 1 implemented".
- Modify: `app/src/views/PackBusPanel.tsx` (`<InfoIcon id="topic.maintenance" />` next to the title)

- [ ] **Step 1: Help text.** Add `topic.maintenance` to `gen_help.py`, related `"topic.firmware, topic.buses"`. Keep the style of the neighbouring entries: German title/intro/body, then English.
  - **DE title** „Wartung am Pack-Bus“
  - **DE intro:** „Hält den Master an, damit ein Pack einer Mehrpack-Bank geflasht werden kann.“
  - **DE body:** one bullet each for:
    - warum der Master stört (er fragt RS485-1/2 ständig ab und führt dabei keinen Neustart-Befehl aus);
    - Wartungsmodus (gepatchte Master-Firmware, Befehl über die CAN-Buchse, Marker übersteht Neustarts);
    - DIP 5–8 AUS als Weg für jede Firmware;
    - Steuerkanal;
    - Wechselrichter (Bedienerangabe; DCL/CCL-Verhalten);
    - Messung („Bus still um …“, was sie nicht sieht);
    - Beenden (alle Slaves abgefragt und geantwortet, Systemwerte zweimal);
    - unklares Update und Sperre;
    - unklarer Marker (kein stop, Weg zurück).

  Write the English entry the same way. Also extend `topic.firmware`:
  - **DE:** „Bei mehreren Packs zuerst die Wartung im Feld *Pack-Bus* vorbereiten.“
  - **EN:** "With several packs, prepare maintenance in the *Pack bus* panel first."
- [ ] **Step 2:** `python3 tools/gen_help.py` → help files written. Check that `git status` lists `help/de/topic.maintenance.md` and `help/en/topic.maintenance.md`.
- [ ] **Step 3: README.** Add a "Maintenance (multi-pack banks)" section of 5–8 lines: what it does, the two ways, the control channel, and that stage 2 (remote flashing via gateway) is pending bench tests. Keep it generic, with no installation data.
- [ ] **Step 4: Spec.** Apply the three edits listed under Files.
- [ ] **Step 5: Check and commit.** Run the full checks from Global Constraints, then commit: `docs: maintenance help topic, README section, spec status`

---

### Task 13: Bench verification (manual, single board with DIP 5 ON)

No code. The results go into the lab notebook of the knowledge base (`Firmware/Labor-Platine/README.md`, results + chronicle). That is outside this repo.

- [ ] **Step 1:** Prepare the board: patched master, DIP 5 ON, marker off. USB on UART1 (control) and UART0 (pack bus).
- [ ] **Step 2: Full local flow in the app.**
  1. Firmware view → "Check master" → `patched-off, 1 slave`.
  2. Tick the inverter → "Activate maintenance mode" → banner open.
  3. "Measure bus" → "bus quiet at …".
  4. Flash address 0 with the maintenance build: plan `[measure]`, expect `1/0`. Note whether `0x23000` survived (`master: as-expected` = kept).
  5. "End" → `stop` → 0/1 → the end round shows the poll to address 1. *Without a second pack the round stays incomplete; expected result: "pack communication incomplete (missing: 1)", session stays open.* Note it in the lab notebook. Then end by setting DIP 5 OFF plus a DIP session, or simulate a slave on the bus with the lab script `reset_watch.py`, whichever is available.
- [ ] **Step 3: Restart persistence.** With the marker set, quit and restart the app. The banner must be shown at once, the inverter tick cleared, and `patched-on` re-checked.
- [ ] **Step 4: Power cycle with the marker set.** The marker must survive: 0x64 reads `1/0` after power-up, and there is no 0x5A on UART0.
- [ ] **Step 5: patched → original in DIP-first order.**
  1. DIP 5 OFF and confirm.
  2. Check that the app sends `stop` → 0/0.
  3. Measure.
  4. Flash the original → RTN 04, session kind `dip`.
- [ ] **Step 6: Locked state.**
  1. Construct `marker-unexplained`: clear the database file `maintenance.sqlite` while the marker is set, then check the master.
  2. Check that the UI offers neither End nor `stop`.
  3. The recovery row to the original runs without `stop`.
- [ ] **Step 7: Sequential way.** Set the control channel to the same USB adapter as the pack port. The app must ask to move the cable before the check and before the measurement, and no step may fail with "busy".
- [ ] **Step 8:** Write results and open points into the lab notebook. Bench items that need two real packs or the gateway stay open for stage 2.

---

## Self-Review notes

- **Spec coverage:**

  | Spec section | Task |
  |---|---|
  | Goal, stage 1 | 1–11 |
  | Installation identity | 6, 7, 10 |
  | Master states | 4, 7 |
  | Session | 6, 7 |
  | `marker-unexplained` lock | 4, 7, 8, 9, 11 |
  | Connections and channel ownership | 7 (route), 8 |
  | Sequential way | 9 (`needsReplug`), 11, 13 |
  | Flow steps 1–5 | 7, 8, 11 |
  | Banner | 9, 10 |
  | Master flash table | 4, 8, 11 |
  | Abort cases | 8 (`block_reason`, `app_answers`) |
  | Transport | 5 (TCP deadline; TCP flashing is stage 2) |
  | Backend | 6–8 |
  | Tests | 2–4, 6–9, 11, 13 |

  Parameter writing and snapshots are out of scope, as in the spec.
- **Known gap left to stage 2:** TCP pack port in the firmware view; bench tests through the gateway and with several real packs.
