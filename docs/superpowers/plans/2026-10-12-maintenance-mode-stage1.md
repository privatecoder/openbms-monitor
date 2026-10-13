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
- Measurement (spec, step 3): no bytes for 2 s, then 0x47 answered by address 0. Before that, whenever the stored master state is known, a fresh UART1 status that matches it (`policy::fresh_status`: `1/0` for a set marker, `0/0` for a patched master without marker or after `stop`, RTN 04 for `no-0x64`); a mismatch re-checks the master and refuses. Wording in the UI: "bus quiet at hh:mm:ss", never "bus exclusive".
- Inverter confirmation: kept only in memory and only for the current work phase. It is never stored.
- `marker-unexplained`:
  - no `stop` is sent or offered, no "End", no hint to restore the DIPs, no inverter release;
  - the only way out is the recovery row (original image under DIP 5–8 OFF, no `stop`) or the user's statement that they set the marker earlier.
- An unclear update (`update-unclear`, `bootloader-unclear`, `update-interrupted`, `master-unclear`) blocks every further flash for this installation and the End of maintenance. It is persistent and is cleared only when that pack answers 0x51 and 0x47 again, or when the user confirms it was isolated or recovered. `update-interrupted` is written **before** the pack is restarted and replaced or cleared when the flash returns, so a crash or power loss of the computer leaves it behind.
- The backend is the authority for every gate. `firmware_start` finds the installation from the pack port itself; the optional UI argument only carries the operator's confirmations. An unassigned pack port is refused while any installation has an open session or block.
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
7. **The pack port decides the installation** (`firmware_start`, measurement, end, block check). A new connection never silently switches the installation in use; the user assigns endpoints explicitly (banner, setup, firmware view).
8. **`update-interrupted` before every restart into the bootloader**, replaced by the result once the flash returns. A crash leaves it, and it blocks like an unclear update.
9. **Sequential way and `stop`:** a master-flash row with `stop` needs `stop` right before the restart, which one adapter cannot do. It is refused there with a hint to use a separate control adapter.
10. **Unclear master update** is a flash block on pack 0 with reason `master-unclear` and the expected state, with or without a session. Like every block it stops further flashes and End. It is cleared by a master check that shows the expected state (not by 0x51/0x47, because the master's application runs), or by the user's "isolated or recovered".
11. **Endpoint ownership is guarded:** an endpoint or control adapter never moves between installations while the source or the target has open work.

## Review Focus

- **Control endpoint equals the main connection.** Example: the sidebar is connected via USB to the CAN socket bus, and the same port is set as control channel. A dialog must not try to open the port a second time ("busy"). It runs over the main connection while polling waits. → Task 7, test `control_route_uses_main_connection_only_for_same_endpoint_on_can_bus`.
- **App restart or reconnect with an open session.**
  - The banner and `N` are still there.
  - The inverter tick is cleared.
  - A `1/0` with an open marker session is `patched-on`, not `marker-unexplained`.
  - → Task 6, test `session_and_block_survive_reopen`; Task 4, test `classify_uses_the_open_session`; Task 9, test "the inverter confirmation is not part of the overview".
- **`stop` in a locked state.** Every path that could send `stop` must refuse while the state is `marker-unexplained`: the End button, a custom-image master flash and an API call from a stale UI. → Task 7, test `stop_is_refused_while_marker_unexplained`; Task 4, test `marker_unexplained_allows_only_recovery_to_original`.
- **Stale measurement.** A measurement taken minutes ago, or before a reconnect, must not unlock a flash. `firmware_start` measures again by itself right before `0x45`. → Task 8, test `flash_gate_refuses_without_inverter_or_with_block`. The gate does not trust a UI timestamp at all, and the fresh `measure` call inside `firmware_start` is covered by the bench step in Task 13.
- **Flash failure in each phase.** A failure after `R F L` or after `W F E` must leave a persistent block when the pack does not answer as an application again. A failure in preflight leaves none. → Task 8, test `block_reason_by_stage`.
- **App killed or computer off during a flash.** The `update-interrupted` block written before the restart stays and shows after the next start. → Task 6, test `interrupted_block_is_replaced_or_cleared`; Task 8, test `finish_flash_block`.
- **Flash without the UI's maintenance argument** (stale UI, overview failed to load, direct command call). The backend still finds the installation from the pack port and applies blocks, the locked state and the session. → Task 8, test `port_gate_is_fail_closed`.
- **Master-flash rows that change the master state before the restart** (`stop` in patched-on → original/custom). The fresh status must expect `0/0` after `stop`, not `1/0`. → Task 4, test `fresh_status_follows_the_state`; Task 8, test `master_changes_in_flash_order`.
- **The pack port belongs to another installation than the panel.** Flash and measurement refuse; the UI offers to assign the port. → Task 8, test `port_gate_is_fail_closed`; Task 9, test `unassigned_port_blocks_maintenance_flash`.

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
  - `maint::bus::watch_round(link, clock, log, slaves: u8, timeout: Duration) -> crate::Result<Round>`: one polling round of the master, counted from a poll of address 1 to the next; an answer counts only with 32 bytes INFO and directly after the master's poll of the same address. Returns the first complete round, or on timeout the round with the most answers.
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

    /// Inject one master poll and (optionally) the slave's answer 50 ms later.
    fn exchange(link: &mut FakeLink, at: Duration, a: u8, answer: Option<Vec<u8>>) {
        link.inject(at, &poll(a));
        if let Some(r) = answer {
            link.inject(at + ms(50), &r);
        }
    }

    #[test]
    fn a_round_is_complete_only_with_poll_and_answer_of_every_slave() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        exchange(&mut link, ms(100), 1, Some(record(1)));
        exchange(&mut link, ms(300), 2, Some(record(2)));
        exchange(&mut link, ms(500), 3, None); // slave 3 polled, never answers
        exchange(&mut link, ms(800), 1, Some(record(1))); // next round starts
        let r = watch_round(&mut link, &t, &mut Vec::new(), 3, ms(2000)).unwrap();
        assert!(!r.complete(3));
        assert_eq!(r.missing(3), vec![3]);
    }

    #[test]
    fn a_complete_round_returns_before_the_timeout() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        link.inject(ms(100), &poll(1));
        link.inject(ms(120), b"\x00garbage\r");
        link.inject(ms(150), &record(1));
        let r = watch_round(&mut link, &t, &mut Vec::new(), 1, ms(5000)).unwrap();
        assert!(r.complete(1));
        use crate::maint::Clock;
        assert!(t.now() < ms(1000));
    }

    #[test]
    fn answers_count_only_right_after_the_poll_of_the_same_address() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        link.inject(ms(50), &record(1)); // answer before any poll
        exchange(&mut link, ms(100), 1, None);
        exchange(&mut link, ms(300), 2, Some(record(1))); // address 1 answers the poll of 2
        exchange(&mut link, ms(500), 1, Some(answer(1, 0x5A, &[0u8; 8]))); // INFO not 32 bytes
        let r = watch_round(&mut link, &t, &mut Vec::new(), 2, ms(1500)).unwrap();
        assert!(r.answered.is_empty());
        assert!(!r.complete(2));
    }

    #[test]
    fn polls_and_answers_of_two_rounds_do_not_add_up() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        // round 1: slave 2 silent; round 2: slave 1 silent
        exchange(&mut link, ms(100), 1, Some(record(1)));
        exchange(&mut link, ms(300), 2, None);
        exchange(&mut link, ms(600), 1, None);
        exchange(&mut link, ms(800), 2, Some(record(2)));
        let r = watch_round(&mut link, &t, &mut Vec::new(), 2, ms(1500)).unwrap();
        assert!(!r.complete(2));
        assert_eq!(r.answered.len(), 1);
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

/// INFO length of a slave's answer to the master's 0x5A poll.
const RECORD_LEN: usize = 32;

/// Listen for one full polling round of the master. A round starts with a poll of address 1 and ends with
/// the next one (the master polls 1..=N in turn; from code, to be confirmed on the bench). An answer counts
/// only when it has 32 bytes INFO and follows the master's poll of the same address with no other frame in
/// between. Returns the first complete round; on timeout the round with the most answers, for `missing`.
pub fn watch_round(link: &mut impl Link, clock: &impl Clock, log: &mut impl Write, slaves: u8, timeout: Duration) -> Result<Round> {
    link.clear_input()?;
    let deadline = clock.now() + timeout;
    let mut best = Round::default();
    let mut round: Option<Round> = None; // None until the first poll of address 1
    let mut last_poll: Option<u8> = None;
    let mut pending = Vec::new();
    let mut buf = [0u8; 512];
    while clock.now() < deadline {
        let n = link.read_raw(&mut buf)?;
        if n > 0 {
            writeln!(log, "RX {}", buf[..n].iter().map(|b| format!("{b:02X}")).collect::<String>())?;
        }
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\r') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let Some(start) = line.iter().position(|b| *b == b'~') else { continue };
            let Ok(f) = frame::parse(&line[start..]) else {
                last_poll = None;
                continue;
            };
            if f.rtn != 0x5A {
                last_poll = None;
                continue;
            }
            if f.info.is_empty() {
                if f.address == 1 {
                    if let Some(done) = round.take() {
                        if done.answered.len() > best.answered.len() {
                            best = done;
                        }
                    }
                    round = Some(Round::default());
                }
                if let Some(r) = round.as_mut() {
                    r.polled.insert(f.address);
                }
                last_poll = Some(f.address);
            } else {
                if let (Some(r), Some(p)) = (round.as_mut(), last_poll) {
                    if p == f.address && f.info.len() == RECORD_LEN {
                        r.answered.insert(f.address);
                    }
                }
                last_poll = None;
            }
            if round.as_ref().is_some_and(|r| r.complete(slaves)) {
                return Ok(round.unwrap_or_default());
            }
        }
        if pending.len() > 4096 {
            pending.clear();
        }
    }
    Ok(match round {
        Some(r) if r.answered.len() > best.answered.len() => r,
        _ => best,
    })
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
  - `policy::{Fresh, fresh_status(MasterState) -> Fresh, fresh_matches(Fresh, Option<Probe>) -> bool}`
  - `policy::settled(Option<Probe>) -> bool` (post-flash polling stops on a settled answer)

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
        // a set marker counts only with 0 effective slaves; 1/N means the master still polls
        assert_eq!(classify(status(true, 3), true), M::Unknown);
        assert_eq!(classify(status(true, 3), false), M::Unknown);
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
        assert_eq!(after_flash(E::MarkerOn, status(true, 3)), Outcome::Unclear);
        assert_eq!(after_flash(E::ZeroOrUnexplained, status(true, 3)), Outcome::Unclear);
        assert_eq!(after_flash(E::MarkerOn, status(false, 11)), Outcome::Unclear);
        assert_eq!(after_flash(E::MarkerOn, None), Outcome::Unclear);
        assert_eq!(after_flash(E::Zero, status(false, 0)), Outcome::AsExpected);
        assert_eq!(after_flash(E::ZeroOrUnexplained, status(true, 0)), Outcome::MarkerUnexplained);
        assert_eq!(after_flash(E::ZeroOrUnexplained, status(false, 0)), Outcome::AsExpected);
        assert_eq!(after_flash(E::Unsupported, Some(Probe::Unsupported)), Outcome::AsExpected);
        assert_eq!(after_flash(E::Unsupported, status(false, 0)), Outcome::Unclear);
        assert_eq!(after_flash(E::NoAssumption, status(false, 0)), Outcome::UnknownImage);
    }

    #[test]
    fn fresh_status_follows_the_state() {
        // after `stop` the stored state is patched-off 0/0, so the fresh status expects 0/0, not 1/0
        assert_eq!(fresh_status(M::PatchedOff { slaves: 0 }), Fresh::Zero);
        // patched-off with N > 0 only reaches a flash in a DIP session: DIP 5–8 OFF reads 0/0
        assert_eq!(fresh_status(M::PatchedOff { slaves: 11 }), Fresh::Zero);
        assert_eq!(fresh_status(M::PatchedOn), Fresh::MarkerOn);
        assert_eq!(fresh_status(M::MarkerUnexplained), Fresh::MarkerOn);
        assert_eq!(fresh_status(M::No0x64), Fresh::Unsupported);
        assert_eq!(fresh_status(M::Unknown), Fresh::Unchecked);
        assert!(fresh_matches(Fresh::MarkerOn, status(true, 0)));
        assert!(!fresh_matches(Fresh::MarkerOn, status(true, 2)));
        assert!(!fresh_matches(Fresh::MarkerOn, status(false, 0)));
        assert!(fresh_matches(Fresh::Zero, status(false, 0)));
        assert!(!fresh_matches(Fresh::Zero, status(false, 1)));
        assert!(fresh_matches(Fresh::Unsupported, Some(Probe::Unsupported)));
        assert!(!fresh_matches(Fresh::Unsupported, Some(Probe::NoAnswer)));
        assert!(!fresh_matches(Fresh::Zero, None));
        assert!(fresh_matches(Fresh::Unchecked, None));
    }

    #[test]
    fn post_flash_polling_waits_for_a_settled_answer() {
        assert!(!settled(None));
        assert!(!settled(Some(Probe::NoAnswer)));
        assert!(!settled(status(true, 3))); // marker set, slave count not yet 0 (next 1 s cycle)
        assert!(settled(status(true, 0)));
        assert!(settled(status(false, 11)));
        assert!(settled(Some(Probe::Unsupported)));
        assert!(settled(Some(Probe::Rejected(5))));
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
        // marker set but still polling (1/N): not a state the app can act on
        Some(Probe::Status(s)) if s.effective_slaves > 0 => MasterState::Unknown,
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
        Expect::MarkerOn if s.is_some_and(|s| s.marker && s.effective_slaves == 0) => Outcome::AsExpected,
        Expect::Zero | Expect::ZeroOrUnexplained if s.is_some_and(|s| !s.marker && s.effective_slaves == 0) => Outcome::AsExpected,
        Expect::ZeroOrUnexplained if s.is_some_and(|s| s.marker && s.effective_slaves == 0) => Outcome::MarkerUnexplained,
        Expect::Unsupported if probe == Some(Probe::Unsupported) => Outcome::AsExpected,
        _ => Outcome::Unclear,
    }
}

/// What 0x64 must read on the control channel right before a flash (design, step 3). Derived from the
/// stored state just before the measurement, i.e. after any `stop` of the master-flash plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fresh {
    /// 1/0
    MarkerOn,
    /// 0/0 (patched, no marker; with N > 0 stored this means DIP 5–8 are OFF now)
    Zero,
    /// RTN 04
    Unsupported,
    /// state unknown (manual DIP way): only the pack bus measurement counts
    Unchecked,
}

pub fn fresh_status(state: MasterState) -> Fresh {
    match state {
        MasterState::PatchedOn | MasterState::MarkerUnexplained => Fresh::MarkerOn,
        MasterState::PatchedOff { .. } => Fresh::Zero,
        MasterState::No0x64 => Fresh::Unsupported,
        MasterState::Unknown => Fresh::Unchecked,
    }
}

pub fn fresh_matches(f: Fresh, probe: Option<Probe>) -> bool {
    match (f, probe) {
        (Fresh::Unchecked, _) => true,
        (Fresh::MarkerOn, Some(Probe::Status(s))) => s.marker && s.effective_slaves == 0,
        (Fresh::Zero, Some(Probe::Status(s))) => !s.marker && s.effective_slaves == 0,
        (Fresh::Unsupported, Some(Probe::Unsupported)) => true,
        _ => false,
    }
}

/// After a master flash: keep reading 0x64 until the answer is final. A set marker takes effect from the
/// master's next one-second cycle, so 1/N is still in transition.
pub fn settled(probe: Option<Probe>) -> bool {
    match probe {
        None | Some(Probe::NoAnswer) => false,
        Some(Probe::Status(s)) => !(s.marker && s.effective_slaves > 0),
        Some(_) => true,
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
- Produces (private): `fn connect_tcp_within(addr, budget, resolve, connect) -> Result<TcpStream>` with the resolver and the per-address connect injected, so tests need no network. `connect_tcp(addr)` calls it with `CONNECT_TIMEOUT`, `ToSocketAddrs` and `TcpStream::connect_timeout`.

- [ ] **Step 1: Write the failing tests** (new `#[cfg(test)] mod tests` in `transport/src/lib.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    fn two() -> Vec<SocketAddr> {
        vec!["192.0.2.1:9".parse().unwrap(), "192.0.2.2:9".parse().unwrap()]
    }

    #[test]
    fn a_hanging_name_server_does_not_stretch_the_budget() {
        let started = Instant::now();
        let r = connect_tcp_within(
            "slow.example:9",
            Duration::from_millis(300),
            |_| {
                std::thread::sleep(Duration::from_secs(3));
                Ok(two())
            },
            |_, _| unreachable!("no address resolved in time"),
        );
        assert!(r.is_err());
        assert!(started.elapsed() < Duration::from_millis(1000));
    }

    #[test]
    fn all_addresses_share_one_budget() {
        let given = Arc::new(Mutex::new(Vec::new()));
        let g = given.clone();
        let started = Instant::now();
        let r = connect_tcp_within("gw:9", Duration::from_millis(400), |_| Ok(two()), move |_, left| {
            g.lock().unwrap().push(left);
            std::thread::sleep(left.min(Duration::from_millis(250)));
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "test"))
        });
        assert!(r.is_err());
        let given = given.lock().unwrap();
        assert_eq!(given.len(), 2);
        assert!(given[1] < Duration::from_millis(200), "second address gets only what is left: {:?}", given[1]);
        assert!(started.elapsed() < Duration::from_millis(700));
    }
}
```

- [ ] **Step 2: Run them.** `cargo test -p openbms-transport` → FAIL (fn missing).

- [ ] **Step 3: Implement** (replace `connect_tcp`)

```rust
/// Connect to the first address of `addr` that answers within CONNECT_TIMEOUT, name resolution included.
fn connect_tcp(addr: &str) -> Result<TcpStream> {
    connect_tcp_within(
        addr,
        CONNECT_TIMEOUT,
        |name| name.to_socket_addrs().map(|a| a.collect()),
        |a, left| TcpStream::connect_timeout(a, left),
    )
}

/// Resolution runs on a helper thread, so a hanging name server cannot stretch the budget; every address
/// gets only what is left of it.
fn connect_tcp_within(
    addr: &str,
    budget: Duration,
    resolve: impl FnOnce(String) -> std::io::Result<Vec<std::net::SocketAddr>> + Send + 'static,
    mut connect: impl FnMut(&std::net::SocketAddr, Duration) -> std::io::Result<TcpStream>,
) -> Result<TcpStream> {
    let deadline = Instant::now() + budget;
    let (tx, rx) = std::sync::mpsc::channel();
    let name = addr.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(resolve(name));
    });
    let timed_out = || std::io::Error::new(std::io::ErrorKind::TimedOut, format!("{addr}: no connection within {budget:?}"));
    let addrs = rx.recv_timeout(budget).map_err(|_| timed_out())??;
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("{addr}: no address"));
    for a in addrs {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(timed_out().into());
        }
        match connect(&a, left) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last.into())
}
```

- [ ] **Step 4: Run them.** `cargo test -p openbms-transport` → PASS.
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
  - `Store::{open, installation_for, installation_of, any_open, open_work, installations, save_installation, add_master_block, master_state, set_master_state, open_session, start_session, set_session_kind, note_done, set_failing, end_session, add_block, set_block_reason, blocks, clear_block, note_sector, sector_note}`
  - `installation_of(endpoint) -> Option<Installation>` never creates one; it also finds the installation whose control channel is that endpoint (sequential way: one adapter for both buses). `installation_for` creates one for an unassigned endpoint.
  - `any_open() -> bool`: some installation has an open session, an uncleared block, or the last state `patched-on` / `marker-unexplained`. `open_work(inst) -> bool`: the same for one installation.
  - `save_installation` refuses (`Err("endpoint-in-use")`) to move an endpoint or the control adapter between installations while the source or the target has open work; it runs in one transaction.
  - `add_master_block(inst, expect, now) -> i64`: block on pack 0 with reason `master-unclear` and the expected state.

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
    fn lookup_without_creating_and_open_work_anywhere() {
        let s = Store::open(&temp("lookup")).unwrap();
        assert_eq!(s.installation_of("serial:/dev/p").unwrap(), None);
        assert!(s.installations().unwrap().is_empty(), "a lookup must not create an installation");
        assert!(!s.any_open().unwrap());
        let i = s.installation_for("tcp:gw:4196").unwrap();
        assert!(!s.any_open().unwrap());
        let b = s.add_block(&i.id, 2, "update-interrupted", 1).unwrap();
        assert!(s.any_open().unwrap());
        s.clear_block(b, "flashed", 2).unwrap();
        assert!(!s.any_open().unwrap());
        s.set_master_state(&i.id, &StoredMasterState::PatchedOn, 3).unwrap();
        assert!(s.any_open().unwrap(), "a set marker is open work too");
        s.set_master_state(&i.id, &StoredMasterState::PatchedOff { slaves: 3 }, 4).unwrap();
        assert!(!s.any_open().unwrap());
        s.start_session(&i.id, SessionKind::Dip, 3, 5).unwrap();
        assert!(s.any_open().unwrap());
        // the control adapter of an installation counts as its endpoint
        let mut i2 = i.clone();
        i2.control = Some(ControlEndpoint::Serial { path: "/dev/c".into() });
        s.save_installation(&i2).unwrap();
        assert_eq!(s.installation_of("serial:/dev/c").unwrap().map(|x| x.id), Some(i.id.clone()));
    }

    #[test]
    fn an_endpoint_with_open_work_cannot_move() {
        let s = Store::open(&temp("move")).unwrap();
        let a = s.installation_for("serial:/dev/p").unwrap();
        let b = s.installation_for("tcp:gw:4196").unwrap();
        s.add_block(&a.id, 3, "bootloader-unclear", 1).unwrap();
        // moving the blocked bus's port to another installation would hide the block
        let mut b2 = b.clone();
        b2.endpoints.push("serial:/dev/p".into());
        assert_eq!(s.save_installation(&b2), Err("endpoint-in-use".to_string()));
        assert_eq!(s.installation_of("serial:/dev/p").unwrap().map(|i| i.id), Some(a.id.clone()));
        // nor may a target with open work take over a foreign endpoint
        let c = s.installation_for("serial:/dev/q").unwrap();
        let mut a2 = a.clone();
        a2.endpoints.push("serial:/dev/q".into());
        assert_eq!(s.save_installation(&a2), Err("endpoint-in-use".to_string()));
        assert_eq!(s.installation_of("serial:/dev/q").unwrap().map(|i| i.id), Some(c.id.clone()));
        // without open work the move is allowed
        let mut b3 = b.clone();
        b3.endpoints.push("serial:/dev/q".into());
        s.save_installation(&b3).unwrap();
        assert_eq!(s.installation_of("serial:/dev/q").unwrap().map(|i| i.id), Some(b.id.clone()));
        assert!(s.open_work(&a.id).unwrap());
        assert!(!s.open_work(&b.id).unwrap());
    }

    #[test]
    fn master_block_keeps_its_expectation() {
        let s = Store::open(&temp("masterblock")).unwrap();
        let i = s.installation_for("serial:/dev/p").unwrap();
        s.add_master_block(&i.id, "marker-on", 1).unwrap();
        let b = &s.blocks(&i.id).unwrap()[0];
        assert_eq!((b.pack, b.reason.as_str(), b.expect.as_deref()), (0, "master-unclear", Some("marker-on")));
    }

    #[test]
    fn interrupted_block_is_replaced_or_cleared() {
        let s = Store::open(&temp("interrupted")).unwrap();
        let i = s.installation_for("serial:/dev/p").unwrap();
        let b = s.add_block(&i.id, 4, "update-interrupted", 1).unwrap();
        s.set_block_reason(b, "bootloader-unclear").unwrap();
        assert_eq!(s.blocks(&i.id).unwrap()[0].reason, "bootloader-unclear");
        s.clear_block(b, "answered", 2).unwrap();
        assert!(s.blocks(&i.id).unwrap().is_empty());
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
  reason TEXT NOT NULL, expect TEXT, since INTEGER NOT NULL, cleared INTEGER, how TEXT);
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
    /// "update-interrupted" (written before the restart, still there after a crash), "update-unclear",
    /// "bootloader-unclear", or "master-unclear" (the master's application runs, 0x64 shows not what the
    /// master-flash table expected)
    pub reason: String,
    /// for "master-unclear": the expected state after the flash ("marker-on", "zero", "unsupported" …)
    pub expect: Option<String>,
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

    /// The installation an endpoint is assigned to (as a main endpoint or as its control channel), without
    /// creating one.
    pub fn installation_of(&self, endpoint: &str) -> Result<Option<Installation>, String> {
        let id: Option<String> = self
            .db
            .query_row("SELECT installation FROM installation_endpoints WHERE endpoint = ?1", [endpoint], |r| r.get(0))
            .optional()
            .map_err(e)?;
        if let Some(id) = id {
            return self.installation(&id).map(Some);
        }
        Ok(self.installations()?.into_iter().find(|i| i.control.as_ref().is_some_and(|c| c.key() == endpoint)))
    }

    /// Some installation has an open session or an uncleared flash block.
    pub fn any_open(&self) -> Result<bool, String> {
        let n: i64 = self
            .db
            .query_row(
                "SELECT (SELECT COUNT(*) FROM maint_sessions WHERE ended IS NULL)
                      + (SELECT COUNT(*) FROM flash_blocks WHERE cleared IS NULL)
                      + (SELECT COUNT(*) FROM master_state WHERE state LIKE '%\"patched-on\"%' OR state LIKE '%\"marker-unexplained\"%')",
                [],
                |r| r.get(0),
            )
            .map_err(e)?;
        Ok(n > 0)
    }

    pub fn installations(&self) -> Result<Vec<Installation>, String> {
        let mut stmt = self.db.prepare("SELECT id FROM installations ORDER BY created").map_err(e)?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>().map_err(e)?;
        ids.iter().map(|id| self.installation(id)).collect()
    }

    /// Name, control channel and endpoints; an endpoint listed here moves away from any other installation.
    /// A move is refused while the source or the target has open work: a block or a session must never be
    /// separated from the bus it belongs to.
    pub fn save_installation(&self, i: &Installation) -> Result<(), String> {
        let mut keys: Vec<String> = i.endpoints.clone();
        keys.extend(i.control.as_ref().map(ControlEndpoint::key));
        for key in &keys {
            if let Some(owner) = self.installation_of(key)? {
                if owner.id != i.id && (self.open_work(&owner.id)? || self.open_work(&i.id)?) {
                    return Err("endpoint-in-use".into());
                }
            }
        }
        let control = i.control.as_ref().map(serde_json::to_string).transpose().map_err(|err| err.to_string())?;
        let tx = self.db.unchecked_transaction().map_err(e)?;
        tx.execute("UPDATE installations SET name = ?2, control = ?3 WHERE id = ?1", params![i.id, i.name, control]).map_err(e)?;
        tx.execute("DELETE FROM installation_endpoints WHERE installation = ?1", [&i.id]).map_err(e)?;
        for ep in &i.endpoints {
            tx.execute("INSERT OR REPLACE INTO installation_endpoints(endpoint, installation) VALUES (?1, ?2)", params![ep, i.id]).map_err(e)?;
        }
        tx.commit().map_err(e)
    }

    /// The installation has an open session, an uncleared block, or the last state is a set marker.
    pub fn open_work(&self, inst: &str) -> Result<bool, String> {
        let state = self.master_state(inst)?.map(|(m, _)| m);
        Ok(self.open_session(inst)?.is_some()
            || !self.blocks(inst)?.is_empty()
            || matches!(state, Some(StoredMasterState::PatchedOn | StoredMasterState::MarkerUnexplained)))
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

    /// Unclear master update: the application runs, 0x64 did not show `expect` after the flash.
    pub fn add_master_block(&self, inst: &str, expect: &str, now: i64) -> Result<i64, String> {
        self.db.execute("INSERT INTO flash_blocks(installation, pack, reason, expect, since) VALUES (?1, 0, 'master-unclear', ?2, ?3)", params![inst, expect, now]).map_err(e)?;
        Ok(self.db.last_insert_rowid())
    }

    /// An `update-interrupted` block becomes the reason found when the flash returned.
    pub fn set_block_reason(&self, id: i64, reason: &str) -> Result<(), String> {
        self.db.execute("UPDATE flash_blocks SET reason = ?2 WHERE id = ?1", params![id, reason]).map_err(e)?;
        Ok(())
    }

    pub fn blocks(&self, inst: &str) -> Result<Vec<FlashBlock>, String> {
        let mut stmt = self.db.prepare("SELECT id, pack, reason, expect, since FROM flash_blocks WHERE installation = ?1 AND cleared IS NULL ORDER BY id").map_err(e)?;
        let rows = stmt.query_map([inst], |r| Ok(FlashBlock { id: r.get(0)?, pack: r.get(1)?, reason: r.get(2)?, expect: r.get(3)?, since: r.get(4)? })).map_err(e)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
    }

    /// `how`: "answered" (pack answered 0x51/0x47), "isolated" (the user confirmed isolation or recovery),
    /// "flashed" (the flash and its post-check passed) or "preflight" (failed before the restart).
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

- [ ] **Step 4: Run them.** `cargo test -p openbms-monitor maint_store` → PASS (8 tests). Then clippy.
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
    - `maint_installation_for(endpoint: EndpointArg) -> Installation` (creates one for an unassigned endpoint)
    - `maint_installation_of(endpoint: EndpointArg) -> Option<Installation>` (lookup only)
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
pub async fn maint_installation_of(state: State<'_, AppState>, endpoint: EndpointArg) -> Result<Option<Installation>, MaintError> {
    let key = endpoint.key();
    with_store(&state, |s| s.installation_of(&key))
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
    // Task 8 adds here: account_allowed(with_store(&state, |s| s.open_session(&installation))?.as_ref())?;
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

Add `maint::maint_installation_for, maint::maint_installation_of, maint::maint_save_installation, maint::maint_overview, maint::maint_check, maint::maint_activate, maint::maint_start_dip, maint::maint_account_marker` to `generate_handler!`.

- [ ] **Step 4: Run.** `cargo test -p openbms-monitor` and clippy → PASS.
- [ ] **Step 5: Commit.** `feat(app): maintenance commands for master check, activation and sessions`

---

### Task 8: Measurement, end check, and the flash gate

**Files:**
- Modify: `app/src-tauri/src/maint.rs`:
  - commands `maint_measure`, `maint_end`, `maint_clear_block`, `maint_master_plan`;
  - pure fns `flash_gate`, `port_gate`, `must_stop_monitor`, `account_allowed`, `block_reason`, `finish_block`, `end_allowed`, `clears_master_unclear`, `end_timeout`, `state_after`;
  - the trait `Prep` with `prepare_flash` (everything before the restart, in order) and its app implementation `AppPrep`;
  - `check` (Task 7) clears a resolved `master-unclear` block; `maint_account_marker` (Task 7) refuses while a DIP session is open.
- Modify: `app/src-tauri/src/firmware.rs`:
  - `firmware_start` takes `maint: Option<FlashMaint>`;
  - the port gate, `prepare_flash`, the `update-interrupted` block before the restart, the block after a failure, the post-flash 0x64 check;
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
  - `FlashMaint { installation: String, inverter_confirmed: bool, dip_off_confirmed: bool, isolated_confirmed: bool }` (`Default`)
  - `flash_gate(&GateInput) -> Result<(), MaintError>`
  - `port_gate(port_inst: Option<&str>, ui_inst: Option<&str>, any_open: bool) -> Result<(), MaintError>`
  - `same_adapter(inst: &Installation, port: &str) -> bool`
  - `Snapshot { session, state, blocks }`, trait `Prep`, `prepare_flash(p: &mut impl Prep, address: u8, image: ImageKind, c: &FlashMaint) -> Result<Option<policy::Plan>, MaintError>`
  - `block_reason(last: Option<Progress>) -> Option<&'static str>`
  - `BlockAction::{Clear(&'static str), Set(&'static str)}`, `finish_block(ok: bool, last: Option<Progress>, answers: impl FnOnce() -> bool) -> BlockAction`
  - `end_allowed(blocks: &[FlashBlock]) -> Result<(), MaintError>`
  - `clears_master_unclear(expect: &str, probe: Option<Probe>) -> bool`
  - `must_stop_monitor(main: Option<&(String, Bus)>, pack_key: &str) -> bool`, `open_pack_for(state, port) -> Result<Connection, MaintError>` (every pack-bus open goes through it)
  - `account_allowed(session: Option<&MaintSession>) -> Result<(), MaintError>`
  - `end_timeout(n: u8) -> Duration`
  - `state_after(outcome: Outcome, expect: Expect) -> StoredMasterState`
  - refusal keys added by this task (Task 10 has the texts): `installation-mismatch`, `port-not-assigned`, `state-changed`, `stop-needs-control-channel`, `master-unclear`, `update-interrupted`, `dip-session-open`

- [ ] **Step 1: Write the failing tests** (add to `maint.rs` tests)

```rust
    use openbms_flash::known::ImageKind;
    use openbms_flash::maint::dialog::Probe;
    use openbms_flash::maint::policy::{self, Expect};
    use openbms_flash::session::Progress;
    use openbms_proto::MaintStatus;

    fn block(pack: u8) -> FlashBlock {
        FlashBlock { id: 1, pack, reason: "update-unclear".into(), expect: None, since: 0 }
    }

    fn session(kind: SessionKind) -> MaintSession {
        MaintSession { id: 1, installation: "i".into(), kind, started: 0, expected_slaves: 11, done: vec![], failing: None }
    }

    fn refused_with(r: Result<impl std::fmt::Debug, MaintError>, key: &str) -> bool {
        matches!(r, Err(MaintError::Refused { reason }) if reason == key)
    }

    #[test]
    fn flash_gate_refuses_without_inverter_or_with_block() {
        fn g(session: Option<&MaintSession>, state: Option<&StoredMasterState>, blocks: &[FlashBlock], inverter: bool) -> Result<(), MaintError> {
            flash_gate(&GateInput { session, state, blocks, inverter_confirmed: inverter })
        }
        let ses = session(SessionKind::Marker);
        let on = StoredMasterState::PatchedOn;
        assert!(g(Some(&ses), Some(&on), &[], true).is_ok());
        assert!(refused_with(g(Some(&ses), Some(&on), &[], false), "inverter"));
        assert!(refused_with(g(Some(&ses), Some(&on), &[block(3)], true), "update-unclear"));
        assert!(refused_with(g(None, Some(&StoredMasterState::MarkerUnexplained), &[], true), "marker-unexplained"));
        // a single pack without maintenance: nothing to confirm
        assert!(g(None, Some(&StoredMasterState::PatchedOff { slaves: 0 }), &[], false).is_ok());
        assert!(g(None, None, &[], false).is_ok());
    }

    #[test]
    fn port_gate_is_fail_closed() {
        assert!(port_gate(Some("a"), Some("a"), true).is_ok());
        // no UI argument: the port's own installation is used, the gate runs on it
        assert!(port_gate(Some("a"), None, true).is_ok());
        assert!(refused_with(port_gate(Some("a"), Some("b"), false), "installation-mismatch"));
        // an unassigned port while maintenance is open anywhere
        assert!(refused_with(port_gate(None, Some("a"), true), "port-not-assigned"));
        assert!(refused_with(port_gate(None, None, true), "port-not-assigned"));
        // a single pack, nothing open: allowed (the port gets an installation of its own)
        assert!(port_gate(None, None, false).is_ok());
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
    fn finish_flash_block() {
        assert_eq!(finish_block(true, Some(Progress::PostCheck), || unreachable!()), BlockAction::Clear("flashed"));
        assert_eq!(finish_block(false, Some(Progress::Preflight), || unreachable!()), BlockAction::Clear("preflight"));
        assert_eq!(finish_block(false, Some(Progress::Erase), || true), BlockAction::Clear("answered"));
        assert_eq!(finish_block(false, Some(Progress::Erase), || false), BlockAction::Set("update-unclear"));
        assert_eq!(finish_block(false, Some(Progress::Bootloader), || false), BlockAction::Set("bootloader-unclear"));
    }

    #[test]
    fn end_refused_with_open_block_or_unclear_master() {
        assert!(end_allowed(&[]).is_ok());
        assert!(refused_with(end_allowed(&[block(2)]), "update-unclear"));
        let master = FlashBlock { id: 2, pack: 0, reason: "master-unclear".into(), expect: Some("marker-on".into()), since: 0 };
        assert!(refused_with(end_allowed(&[master]), "master-unclear"));
    }

    #[test]
    fn master_unclear_clears_only_on_the_expected_state() {
        let st = |marker, n| Some(Probe::Status(MaintStatus { marker, effective_slaves: n }));
        assert!(clears_master_unclear("marker-on", st(true, 0)));
        assert!(!clears_master_unclear("marker-on", st(true, 2)));
        assert!(!clears_master_unclear("marker-on", None));
        assert!(clears_master_unclear("unsupported", Some(Probe::Unsupported)));
        assert!(!clears_master_unclear("no-assumption", st(false, 0)));
    }

    #[test]
    fn every_pack_bus_open_frees_the_port_first() {
        let pack = |k: &str| (k.to_string(), Bus::PackBus);
        let can = |k: &str| (k.to_string(), Bus::CanSocketBus);
        // monitor on the pack bus, any adapter: it would see (and disturb) the bus
        assert!(must_stop_monitor(Some(&pack("serial:/dev/b")), "serial:/dev/a"));
        // monitor on the CAN socket bus, same adapter (sequential way): the port is held
        assert!(must_stop_monitor(Some(&can("serial:/dev/a")), "serial:/dev/a"));
        assert!(!must_stop_monitor(Some(&can("tcp:gw:4196")), "serial:/dev/a"));
        assert!(!must_stop_monitor(None, "serial:/dev/a"));
    }

    #[test]
    fn account_marker_never_rewrites_a_dip_recovery() {
        assert!(account_allowed(None).is_ok());
        assert!(account_allowed(Some(&session(SessionKind::Marker))).is_ok());
        assert!(refused_with(account_allowed(Some(&session(SessionKind::Dip))), "dip-session-open"));
    }

    #[test]
    fn end_round_waits_long_enough_for_a_full_bank() {
        // 150 ms per slave answer (code 0x13F6/0x14AE), two rounds, plus margin
        assert!(end_timeout(11) >= std::time::Duration::from_millis(3300 + 2000));
        assert!(end_timeout(1) >= std::time::Duration::from_secs(4));
    }

    #[test]
    fn state_after_master_flash() {
        use policy::{Expect as E, Outcome as O};
        assert_eq!(state_after(O::AsExpected, E::MarkerOn), StoredMasterState::PatchedOn);
        assert_eq!(state_after(O::AsExpected, E::Zero), StoredMasterState::PatchedOff { slaves: 0 });
        assert_eq!(state_after(O::AsExpected, E::Unsupported), StoredMasterState::No0x64);
        assert_eq!(state_after(O::MarkerUnexplained, E::ZeroOrUnexplained), StoredMasterState::MarkerUnexplained);
        assert_eq!(state_after(O::Unclear, E::MarkerOn), StoredMasterState::Unknown);
        assert_eq!(state_after(O::UnknownImage, E::NoAssumption), StoredMasterState::Unknown);
    }

    /// A simulated master behind `Prep`: `check` reads the real state, `stop_to_zero` clears the marker and
    /// turns the session into a DIP session, `measure` compares the fresh status with the stored state.
    struct FakePrep {
        master: StoredMasterState,
        snap: Snapshot,
        control: bool,
        sequential: bool,
        calls: Vec<&'static str>,
    }

    fn probe_of(m: &StoredMasterState) -> Option<Probe> {
        let st = |marker, n| Some(Probe::Status(MaintStatus { marker, effective_slaves: n }));
        match m {
            StoredMasterState::PatchedOn | StoredMasterState::MarkerUnexplained => st(true, 0),
            StoredMasterState::PatchedOff { slaves } => st(false, *slaves),
            StoredMasterState::No0x64 => Some(Probe::Unsupported),
            StoredMasterState::Unknown => None,
        }
    }

    impl Prep for FakePrep {
        fn has_control(&self) -> bool { self.control }
        fn sequential(&self) -> bool { self.sequential }
        fn snapshot(&mut self) -> Result<Snapshot, MaintError> { Ok(self.snap.clone()) }
        fn check(&mut self) -> Result<(), MaintError> {
            self.calls.push("check");
            self.snap.state = Some(self.master.clone());
            Ok(())
        }
        fn stop_to_zero(&mut self) -> Result<(), MaintError> {
            self.calls.push("stop");
            self.master = StoredMasterState::PatchedOff { slaves: 0 };
            self.snap.state = Some(self.master.clone());
            if let Some(s) = self.snap.session.as_mut() { s.kind = SessionKind::Dip; }
            Ok(())
        }
        fn measure(&mut self) -> Result<(), MaintError> {
            self.calls.push("measure");
            let stored: policy::MasterState = self.snap.state.as_ref().map(Into::into).unwrap_or(policy::MasterState::Unknown);
            if policy::fresh_matches(policy::fresh_status(stored), probe_of(&self.master)) { Ok(()) } else { Err(refused("state-changed")) }
        }
    }

    fn fake(master: StoredMasterState, stored: Option<StoredMasterState>, session: Option<MaintSession>) -> FakePrep {
        FakePrep { master, snap: Snapshot { session, state: stored, blocks: vec![] }, control: true, sequential: false, calls: vec![] }
    }

    const ALL: FlashMaint = FlashMaint { installation: String::new(), inverter_confirmed: true, dip_off_confirmed: true, isolated_confirmed: true };

    #[test]
    fn master_changes_in_flash_order() {
        use StoredMasterState as S;
        // patched-on -> original: check, stop (0/0 now), then the measurement expects 0/0, not 1/0
        let mut p = fake(S::PatchedOn, Some(S::PatchedOn), Some(session(SessionKind::Marker)));
        let plan = prepare_flash(&mut p, 0, ImageKind::Original, &ALL).unwrap().unwrap();
        assert_eq!(plan.after, Expect::Unsupported);
        assert_eq!(p.calls, ["check", "stop", "measure"]);
        assert_eq!(p.snap.session.unwrap().kind, SessionKind::Dip);
        // patched-on -> custom: the same order
        let mut p = fake(S::PatchedOn, Some(S::PatchedOn), Some(session(SessionKind::Marker)));
        assert!(prepare_flash(&mut p, 0, ImageKind::Custom, &ALL).is_ok());
        assert_eq!(p.calls, ["check", "stop", "measure"]);
        // patched-on -> maintenance build: no stop
        let mut p = fake(S::PatchedOn, Some(S::PatchedOn), Some(session(SessionKind::Marker)));
        assert!(prepare_flash(&mut p, 0, ImageKind::Maintenance, &ALL).is_ok());
        assert_eq!(p.calls, ["check", "measure"]);
    }

    #[test]
    fn recovery_runs_without_stop_and_only_to_the_original() {
        use StoredMasterState as S;
        let mut p = fake(S::MarkerUnexplained, Some(S::MarkerUnexplained), Some(session(SessionKind::Dip)));
        let plan = prepare_flash(&mut p, 0, ImageKind::Original, &ALL).unwrap().unwrap();
        assert!(plan.recovery);
        assert_eq!(p.calls, ["check", "measure"]);
        // without the DIP session the DIP step is not confirmed
        let mut p = fake(S::MarkerUnexplained, Some(S::MarkerUnexplained), None);
        assert!(refused_with(prepare_flash(&mut p, 0, ImageKind::Original, &ALL), "dip-off"));
        let mut p = fake(S::MarkerUnexplained, Some(S::MarkerUnexplained), Some(session(SessionKind::Dip)));
        assert!(refused_with(prepare_flash(&mut p, 0, ImageKind::Maintenance, &ALL), "marker-unexplained"));
        // a slave is never flashed while the marker is unexplained
        let mut p = fake(S::MarkerUnexplained, Some(S::MarkerUnexplained), Some(session(SessionKind::Dip)));
        assert!(refused_with(prepare_flash(&mut p, 3, ImageKind::Original, &ALL), "marker-unexplained"));
    }

    #[test]
    fn a_state_change_before_the_restart_refuses() {
        use StoredMasterState as S;
        // slave flash in a marker session, but someone cleared the marker meanwhile
        let mut p = fake(S::PatchedOff { slaves: 11 }, Some(S::PatchedOn), Some(session(SessionKind::Marker)));
        assert!(refused_with(prepare_flash(&mut p, 3, ImageKind::Original, &ALL), "state-changed"));
        // the master check re-reads the state first and refuses with the decision table
        let mut p = fake(S::PatchedOff { slaves: 11 }, Some(S::PatchedOn), Some(session(SessionKind::Marker)));
        assert!(refused_with(prepare_flash(&mut p, 0, ImageKind::Maintenance, &ALL), "quiet-first"));
    }

    #[test]
    fn single_pack_and_sequential_way() {
        use StoredMasterState as S;
        // single pack, nothing known, no control channel: the flash's own preflight decides
        let mut p = fake(S::Unknown, None, None);
        p.control = false;
        assert_eq!(prepare_flash(&mut p, 0, ImageKind::Original, &FlashMaint::default()).unwrap(), None);
        assert!(p.calls.is_empty());
        // no-0x64 -> original needs the DIP session
        let mut p = fake(S::No0x64, Some(S::No0x64), None);
        assert!(refused_with(prepare_flash(&mut p, 0, ImageKind::Original, &ALL), "dip-off"));
        let mut p = fake(S::No0x64, Some(S::No0x64), Some(session(SessionKind::Dip)));
        assert!(prepare_flash(&mut p, 0, ImageKind::Original, &ALL).is_ok());
        assert_eq!(p.calls, ["check", "measure"]);
        // sequential way: no UART1 exchange inside the flash, so a row with `stop` is refused
        let mut p = fake(S::PatchedOn, Some(S::PatchedOn), Some(session(SessionKind::Marker)));
        p.sequential = true;
        assert!(refused_with(prepare_flash(&mut p, 0, ImageKind::Original, &ALL), "stop-needs-control-channel"));
        assert!(p.calls.is_empty());
    }
```

- [ ] **Step 2: Run them.** `cargo test -p openbms-monitor maint::tests` → FAIL.

- [ ] **Step 3: Implement the pure parts** (in `maint.rs`)

```rust
use openbms_flash::known::ImageKind;
use openbms_flash::maint::policy::{Expect, Outcome, Step};
use openbms_flash::session::Progress;
use std::time::Duration;

pub struct GateInput<'a> {
    pub session: Option<&'a MaintSession>,
    pub state: Option<&'a StoredMasterState>,
    pub blocks: &'a [FlashBlock],
    pub inverter_confirmed: bool,
}

/// Checks before every flash of an installation (the fresh measurement follows in `prepare_flash`).
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

/// The pack port decides the installation, not the UI. `port_inst`: installation the port is assigned to;
/// `ui_inst`: the installation the UI shows; `any_open`: some installation has open maintenance work.
pub fn port_gate(port_inst: Option<&str>, ui_inst: Option<&str>, any_open: bool) -> Result<(), MaintError> {
    match (port_inst, ui_inst) {
        (Some(p), Some(u)) if p != u => Err(refused("installation-mismatch")),
        (None, _) if any_open => Err(refused("port-not-assigned")),
        _ => Ok(()),
    }
}

/// Control channel and pack port are one USB adapter (sequential way).
pub fn same_adapter(inst: &Installation, port: &str) -> bool {
    inst.control.as_ref().is_some_and(|c| c.key() == crate::maint_store::endpoint_key(false, port))
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

#[derive(Debug, PartialEq, Eq)]
pub enum BlockAction {
    /// clear the `update-interrupted` block, with this `how`
    Clear(&'static str),
    /// keep it, with this reason
    Set(&'static str),
}

/// What happens to the `update-interrupted` block written before the restart, once the flash returned.
/// `answers` asks the pack for 0x51/0x47 and is called only when that decides.
pub fn finish_block(ok: bool, last: Option<Progress>, answers: impl FnOnce() -> bool) -> BlockAction {
    if ok {
        return BlockAction::Clear("flashed");
    }
    match block_reason(last) {
        None => BlockAction::Clear("preflight"),
        Some(_) if answers() => BlockAction::Clear("answered"),
        Some(r) => BlockAction::Set(r),
    }
}

/// End is refused while an update is unclear (any block, the master's included): the inverter must not
/// get its battery back then.
pub fn end_allowed(blocks: &[FlashBlock]) -> Result<(), MaintError> {
    match blocks.first() {
        Some(b) => Err(refused(&b.reason)),
        None => Ok(()),
    }
}

/// Before the pack bus is opened: the monitor holds that adapter, or it polls the pack bus.
pub fn must_stop_monitor(main: Option<&(String, Bus)>, pack_key: &str) -> bool {
    main.is_some_and(|(key, bus)| matches!(bus, Bus::PackBus) || key == pack_key)
}

/// "I set the marker myself" turns an unexplained marker into a marker session. A DIP session (the
/// recovery) stays what it is: its End sends no `stop` and expects the polls of a quiet master.
pub fn account_allowed(session: Option<&MaintSession>) -> Result<(), MaintError> {
    match session {
        Some(s) if s.kind == SessionKind::Dip => Err(refused("dip-session-open")),
        _ => Ok(()),
    }
}

pub fn expect_key(e: Expect) -> &'static str {
    match e {
        Expect::MarkerOn => "marker-on",
        Expect::Zero => "zero",
        Expect::ZeroOrUnexplained => "zero-or-unexplained",
        Expect::Unsupported => "unsupported",
        Expect::NoAssumption => "no-assumption",
    }
}

/// A later master check that shows the expected state (`expect_key` of the plan) resolves an unclear
/// master update.
pub fn clears_master_unclear(expect: &str, probe: Option<Probe>) -> bool {
    let expect = match expect {
        "marker-on" => Expect::MarkerOn,
        "zero" => Expect::Zero,
        "zero-or-unexplained" => Expect::ZeroOrUnexplained,
        "unsupported" => Expect::Unsupported,
        _ => return false,
    };
    policy::after_flash(expect, probe) == Outcome::AsExpected
}

/// Listening time for the end round: two full polling rounds (150 ms per slave) plus 2 s, at least 4 s.
pub fn end_timeout(n: u8) -> Duration {
    (Duration::from_millis(300) * u32::from(n) + Duration::from_secs(2)).max(Duration::from_secs(4))
}

pub fn state_after(outcome: Outcome, expect: Expect) -> StoredMasterState {
    use policy::{Expect as E, Outcome as O};
    match (outcome, expect) {
        (O::AsExpected, E::MarkerOn) => StoredMasterState::PatchedOn,
        (O::AsExpected, E::Zero | E::ZeroOrUnexplained) => StoredMasterState::PatchedOff { slaves: 0 },
        (O::AsExpected, E::Unsupported) => StoredMasterState::No0x64,
        (O::MarkerUnexplained, _) => StoredMasterState::MarkerUnexplained,
        _ => StoredMasterState::Unknown,
    }
}

#[derive(serde::Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FlashMaint {
    pub installation: String,
    pub inverter_confirmed: bool,
    pub dip_off_confirmed: bool,
    pub isolated_confirmed: bool,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub session: Option<MaintSession>,
    pub state: Option<StoredMasterState>,
    pub blocks: Vec<FlashBlock>,
}

/// The side effects `prepare_flash` needs, so its order can be tested against a simulated master.
pub trait Prep {
    fn has_control(&self) -> bool;
    /// one adapter for both buses: no UART1 exchange is possible inside the flash
    fn sequential(&self) -> bool;
    fn snapshot(&mut self) -> Result<Snapshot, MaintError>;
    /// read 0x64 and store the classified state
    fn check(&mut self) -> Result<(), MaintError>;
    /// `stop`, wait for 0/0, store it, and turn an open session into a DIP session
    fn stop_to_zero(&mut self) -> Result<(), MaintError>;
    /// fresh status matching the stored state, then the pack bus measurement
    fn measure(&mut self) -> Result<(), MaintError>;
}

/// Everything before the pack is restarted into its bootloader, in the order of the design: fresh master
/// state, gate, master-flash plan (DIP, `stop`), fresh measurement. Returns the master plan for the
/// post-check. Without any maintenance information (a single pack) nothing is done here; the flash's own
/// preflight (quiet bus, 0x51/0x47) still refuses a polling master.
pub fn prepare_flash(p: &mut impl Prep, address: u8, image: ImageKind, c: &FlashMaint) -> Result<Option<policy::Plan>, MaintError> {
    let uart1 = p.has_control() && !p.sequential();
    if address == 0 && uart1 {
        p.check()?;
    }
    let snap = p.snapshot()?;
    let state = snap.state.as_ref();
    let plan = if address == 0 && (snap.session.is_some() || state.is_some()) {
        let m: policy::MasterState = state.map(Into::into).unwrap_or(policy::MasterState::Unknown);
        Some(policy::master_flash_plan(m, image).map_err(|r| refused(refusal_key(r)))?)
    } else {
        None
    };
    let recovery = plan.as_ref().is_some_and(|p| p.recovery);
    if recovery {
        // the recovery row is the one way out of the locked state; blocks and the inverter still count
        if let Some(b) = snap.blocks.first() {
            return Err(refused(&b.reason));
        }
        if !c.inverter_confirmed {
            return Err(refused("inverter"));
        }
    } else {
        flash_gate(&GateInput { session: snap.session.as_ref(), state, blocks: &snap.blocks, inverter_confirmed: c.inverter_confirmed })?;
    }
    if let Some(plan) = &plan {
        if plan.before.contains(&Step::DipOff) && (!c.dip_off_confirmed || snap.session.is_none()) {
            return Err(refused("dip-off"));
        }
        if plan.before.contains(&Step::IsolatedOrReleased) && !c.isolated_confirmed {
            return Err(refused("isolated-or-released"));
        }
        if plan.before.contains(&Step::StopToZero) {
            if !uart1 {
                return Err(refused("stop-needs-control-channel"));
            }
            p.stop_to_zero()?;
        }
    }
    if plan.is_some() || snap.session.is_some() || state == Some(&StoredMasterState::PatchedOn) {
        p.measure()?;
    }
    Ok(plan)
}
```

Note on the sequential check: `stop-needs-control-channel` is tested before any side effect, because `uart1` is known up front. In `single_pack_and_sequential_way` the call list is therefore empty.

`refusal_key` is defined in Step 5; keep both in `maint.rs`.

- [ ] **Step 4: Run the tests.** `cargo test -p openbms-monitor maint::tests` → PASS (after Step 5 adds `refusal_key`; write Steps 3 and 5 together, then run).

- [ ] **Step 5: Implement the side effects and commands** (in `maint.rs`)

```rust
use openbms_flash::maint::bus;

const PACK_BUS_BAUD: u32 = 19_200;

fn open_pack(port: &str) -> Result<Connection, MaintError> {
    Connection::open(&Endpoint::Serial { path: port.into(), baud: PACK_BUS_BAUD }).map_err(|e| MaintError::Connection { message: e.to_string() })
}

/// The measurement of step 3. First, whenever the stored state is known and the control channel is a
/// separate adapter, a fresh 0x64 status that must match it (`policy::fresh_status`); a mismatch re-checks
/// the master and refuses. Then on the pack bus 2 s quiet and 0x47 from address 0. Stops the monitor when
/// it holds the pack port or polls the pack bus.
pub fn measure(state: &AppState, inst: &Installation, port: &str) -> Result<i64, MaintError> {
    let ms = with_store(state, |s| s.master_state(&inst.id))?;
    let fresh = ms.as_ref().map(|(m, _)| policy::fresh_status(m.into())).unwrap_or(policy::Fresh::Unchecked);
    if fresh != policy::Fresh::Unchecked && inst.control.is_some() && !same_adapter(inst, port) {
        let probe = with_control(state, inst, |c| dialog::probe(c, &SystemClock::new(), Budgets::default()).ok())?;
        if !policy::fresh_matches(fresh, probe) {
            check(state, inst)?;
            return Err(refused("state-changed"));
        }
    }
    let mut link = open_pack_for(state, port)?;
    bus::measure(&mut link, &mut std::io::sink()).map_err(|e| MaintError::Measure { message: e.to_string() })?;
    Ok(crate::history::now_ms())
}

/// Open the pack bus port; first stop the monitor when it holds that adapter or polls the pack bus.
pub fn open_pack_for(state: &AppState, port: &str) -> Result<Connection, MaintError> {
    let main = state.main.lock().map_err(|e| store_err(e.to_string()))?.clone();
    if must_stop_monitor(main.as_ref(), &crate::maint_store::endpoint_key(false, port)) {
        crate::stop_monitor(state).map_err(|m| MaintError::Connection { message: m })?;
    }
    open_pack(port)
}

/// `stop` and wait for 0/0, refusing in the locked state. The master is now quiet because of DIP 5–8 OFF,
/// so an open session continues (and later ends) as a DIP session.
pub fn stop_to_zero(state: &AppState, inst: &Installation) -> Result<(), MaintError> {
    let ms = with_store(state, |s| s.master_state(&inst.id))?;
    stop_allowed(ms.as_ref().map(|(m, _)| m))?;
    let r = with_control(state, inst, |c| dialog::dialog(c, &SystemClock::new(), Action::Stop, Some(0), Budgets::default()))?;
    check(state, inst)?;
    r?;
    with_store(state, |s| match s.open_session(&inst.id)? {
        Some(ses) => s.set_session_kind(ses.id, SessionKind::Dip),
        None => Ok(()),
    })
}

pub struct AppPrep<'a> {
    pub state: &'a AppState,
    pub inst: &'a Installation,
    pub port: &'a str,
}

impl Prep for AppPrep<'_> {
    fn has_control(&self) -> bool {
        self.inst.control.is_some()
    }
    fn sequential(&self) -> bool {
        same_adapter(self.inst, self.port)
    }
    fn snapshot(&mut self) -> Result<Snapshot, MaintError> {
        let id = &self.inst.id;
        with_store(self.state, |s| {
            Ok(Snapshot { session: s.open_session(id)?, state: s.master_state(id)?.map(|(m, _)| m), blocks: s.blocks(id)? })
        })
    }
    fn check(&mut self) -> Result<(), MaintError> {
        check(self.state, self.inst).map(|_| ())
    }
    fn stop_to_zero(&mut self) -> Result<(), MaintError> {
        stop_to_zero(self.state, self.inst)
    }
    fn measure(&mut self) -> Result<(), MaintError> {
        measure(self.state, self.inst, self.port).map(|_| ())
    }
}

/// The pack port must belong to the installation (as a main endpoint or as the control adapter).
fn port_of(state: &AppState, inst: &Installation, port: &str) -> Result<(), MaintError> {
    let key = crate::maint_store::endpoint_key(false, port);
    let (owner, any_open) = with_store(state, |s| Ok((s.installation_of(&key)?, s.any_open()?)))?;
    port_gate(owner.as_ref().map(|i| i.id.as_str()), Some(&inst.id), any_open)?;
    if owner.is_none() {
        return Err(refused("port-not-assigned"));
    }
    Ok(())
}

#[tauri::command]
pub async fn maint_measure(state: State<'_, AppState>, installation: String, port: String) -> Result<i64, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    port_of(&state, &inst, &port)?;
    measure(&state, &inst, &port)
}

#[derive(Serialize)]
pub struct EndResult {
    ok: bool,
    missing: Vec<u8>,
    failing: Option<String>,
}

/// End (step 5): refused while an update is unclear; marker → `stop` and wait for 0/N; then the pack bus
/// check against N; then two Modbus reads ≥ 2 s apart over the control channel. The session closes only
/// when all of it passed.
#[tauri::command]
pub async fn maint_end(state: State<'_, AppState>, installation: String, port: String) -> Result<EndResult, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    port_of(&state, &inst, &port)?;
    let (session, ms, blocks) = with_store(&state, |s| Ok((s.open_session(&inst.id)?, s.master_state(&inst.id)?, s.blocks(&inst.id)?)))?;
    let session = session.ok_or_else(|| refused("no-session"))?;
    stop_allowed(ms.as_ref().map(|(m, _)| m))?;
    end_allowed(&blocks)?;
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
    let mut link = open_pack_for(&state, &port)?;
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
        if !with_control(&state, &inst, |c| c.system_values().is_ok())? {
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
        // the master's application runs; only a 0x64 check that shows the expected state resolves it
        "answered" if b.reason == "master-unclear" => {
            let inst = self::installation(&state, &installation)?;
            check(&state, &inst)?;
            if with_store(&state, |s| s.blocks(&installation))?.iter().any(|x| x.id == id) {
                return Err(refused("master-unclear"));
            }
            return overview(&state, &installation);
        }
        "answered" => {
            let port = port.as_deref().ok_or_else(|| refused("no-port"))?;
            let inst = self::installation(&state, &installation)?;
            port_of(&state, &inst, port)?;
            let mut link = open_pack_for(&state, port)?;
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
    PlanView {
        before: p.before.iter().map(|s| match s {
            Step::DipOff => "dip-off",
            Step::StopToZero => "stop-to-zero",
            Step::Measure => "measure",
            Step::IsolatedOrReleased => "isolated-or-released",
        }).collect(),
        after: expect_key(p.after),
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

pub fn installation_by_id(state: &AppState, id: &str) -> Result<Installation, MaintError> {
    installation(state, id)
}
```

Also extend `check` (Task 7): keep the probe, and after storing the state clear a resolved unclear master update. And replace the placeholder comment in `maint_account_marker` (Task 7) with the line it names: `account_allowed(with_store(&state, |s| s.open_session(&installation))?.as_ref())?;`.

```rust
pub fn check(state: &AppState, inst: &Installation) -> Result<StoredMasterState, MaintError> {
    let session = with_store(state, |s| s.open_session(&inst.id))?;
    let probe = with_control(state, inst, |c| dialog::probe(c, &SystemClock::new(), Budgets::default()).ok()).unwrap_or(None);
    let expected = session.as_ref().is_some_and(|s| s.kind == SessionKind::Marker);
    let m: StoredMasterState = policy::classify(probe, expected).into();
    let now = crate::history::now_ms();
    with_store(state, |s| {
        s.set_master_state(&inst.id, &m, now)?;
        for b in s.blocks(&inst.id)? {
            if b.reason == "master-unclear" && b.expect.as_deref().is_some_and(|x| clears_master_unclear(x, probe)) {
                s.clear_block(b.id, "checked", now)?;
            }
        }
        Ok(())
    })?;
    Ok(m)
}
```

- [ ] **Step 6: Gate in `firmware_start`** (`firmware.rs`)

Change the signature to `firmware_start(app, state, port, address, path, allow_custom, expected_sha256, confirmation, lang, maint: Option<crate::maint::FlashMaint>)`. After `let port_slot = reserve_port(...)` and before `try_begin`, insert the gate. It always runs; `maint` only carries the operator's confirmations and the installation the UI shows. A missing maintenance database refuses (fail closed).

```rust
    // maintenance gate: the pack port decides the installation; then everything before the restart
    let to_text = |e: crate::maint::MaintError| serde_json::to_string(&e).unwrap_or_default();
    let confirm = maint.unwrap_or_default();
    let key = crate::maint_store::endpoint_key(false, &port);
    let (owner, any_open) = crate::maint::with_store(&state, |s| Ok((s.installation_of(&key)?, s.any_open()?))).map_err(to_text)?;
    let ui = (!confirm.installation.is_empty()).then_some(confirm.installation.as_str());
    crate::maint::port_gate(owner.as_ref().map(|i| i.id.as_str()), ui, any_open).map_err(to_text)?;
    let inst = match owner {
        Some(i) => i,
        None => crate::maint::with_store(&state, |s| s.installation_for(&key)).map_err(to_text)?,
    };
    let master_plan = crate::maint::prepare_flash(
        &mut crate::maint::AppPrep { state: &state, inst: &inst, port: &port },
        address,
        crate::maint::image_kind(info.kind),
        &confirm,
    )
    .map_err(to_text)?;
    // written before the restart, so a crash or power loss leaves it behind
    let pending = crate::maint::with_store(&state, |s| s.add_block(&inst.id, address, "update-interrupted", crate::history::now_ms())).map_err(to_text)?;
```

`ImageInfo.kind` is `&'static str`; `image_kind` takes `&str`. Errors reach the UI as a JSON string of `MaintError`; Task 9's `errorKey` parses it.

The gate runs before `try_begin`, so a refusal leaves no `update-interrupted` block. Any `return Err` between `add_block` and the thread spawn (opening the port, the log file) must clear `pending` with `how = "preflight"` first; wrap the `prepared` closure's error path accordingly:

```rust
    let (mut link, mut log, log_path) = match prepared {
        Ok(p) => p,
        Err(e) => {
            let _ = crate::maint::with_store(&state, |s| s.clear_block(pending, "preflight", crate::history::now_ms()));
            return Err(e);
        }
    };
```

- [ ] **Step 7: After the flash** (in the spawned thread of `firmware_start`)
  - Clone what the thread needs before spawning:
    - `let store = state.maint.clone();`
    - `let inst2 = inst.clone();`
    - `let plan2 = master_plan.clone();`
  - The thread cannot use `State`. Add `with_store_arc(store: &Arc<Mutex<Option<Store>>>, f)` to `maint.rs` (same body as `with_store`), and two helpers that take the `Arc` and the installation:
    - `record_after_flash(store, inst, pending: i64, address, release: &str, ok, last, link: &mut Connection) -> Option<String>` (the block reason, if one stays; `release` from `info.release`, cloned into the thread)
    - `post_master_check(store, inst, plan) -> Option<String>` (the outcome key)
  - **Order:** call `record_after_flash` **before** `drop(link)`; today the thread drops the link right after the flash. Move `drop(link)` below it.
  - **`record_after_flash`:** `finish_block(ok, last, || bus::app_answers(link, &mut std::io::sink(), address, Duration::from_secs(6)))`:
    - `Clear(how)` → `clear_block(pending, how, now)`, return `None`;
    - `Set(reason)` → `set_block_reason(pending, reason)`, return `Some(reason)`.
    - On success with a session open: `note_done(session.id, &format!("flash {address} {}", info.release))`.
  - **Success at address 0 with a plan (`post_master_check`):**
    1. Open the control channel directly with `Connection::open(&control_endpoint(..))`. The main connection was stopped before the flash, so the backend must not assume it exists. In the sequential way (`same_adapter`) skip the check: store `Unknown` and `add_master_block(.., expect_key(plan.after), now)`; the user re-checks after moving the cable, and `check` clears it.
    2. Read 0x64 every 500 ms for up to 6 s until `policy::settled(probe)`; keep the last probe.
    3. `policy::after_flash(plan.after, probe)` → store the state with `state_after`:

       | Outcome | Store | Session |
       |---|---|---|
       | `AsExpected` + `MarkerOn` | `PatchedOn` | — |
       | `AsExpected` + `Zero` / `ZeroOrUnexplained` | `PatchedOff { slaves: 0 }` | — |
       | `AsExpected` + `Unsupported` | `No0x64` | `set_session_kind(Dip)` |
       | `MarkerUnexplained` | `MarkerUnexplained`; `note_sector(.., "marker after first flash of a maintenance build", now)` | — |
       | `Unclear` | `Unknown`; `add_master_block(.., expect_key(plan.after), now)` (with or without a session) | — |
       | `UnknownImage` | `Unknown`; `note_sector(.., "unknown image on the master", now)` | — |

    4. If `plan.recovery`, also `note_sector(.., "0x23000 unexplained, recovered to original without stop", now)`.
  - Put the results into `FirmwareDone { block, master }` (both `Option<String>`; the outcome as `"as-expected" | "unclear" | "marker-unexplained" | "unknown-image"`).
  - A panic inside the flash (`Err(_)` of `catch_unwind`) counts as `ok = false` with the last progress.

`AppState.maint` is `Arc<Mutex<Option<Store>>>`. `rusqlite::Connection` is `Send`, so `Mutex<Option<Store>>` is `Sync`.

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
  - api methods `maintInstallationFor`, `maintInstallationOf`, `maintSaveInstallation`, `maintOverview`, `maintCheck`, `maintActivate`, `maintStartDip`, `maintAccountMarker`, `maintMeasure`, `maintEnd`, `maintClearBlock`, `maintMasterPlan`
  - `firmwareStart` gains a last argument `maint: FlashMaint | null`
  - `maint.ts`: `bannerOf(o) -> Banner | null`, `openWork(o) -> boolean`, `attachDecision(current, known) -> Attach`, `endpointKey(ep: Endpoint) -> string`, `needsReplug(control, packPort) -> boolean`, `errorKey(e) -> { key: string; params?: Record<string, unknown> }`, `canFlash({ overview, inverter, measuredAt, address, imageKind, portAssigned }) -> { ok: boolean; reason?: string }`. The backend repeats every check; `canFlash` only keeps the start button honest.

- [ ] **Step 1: Types and API** (append to `api.ts` types)

```ts
export type MasterStateName = "patched-off" | "patched-on" | "no-0x64" | "unknown" | "marker-unexplained";
export interface MasterState { state: MasterStateName; slaves?: number }
export type ControlEndpoint = { kind: "serial"; path: string } | { kind: "tcp"; addr: string };
export interface Installation { id: string; name: string; endpoints: string[]; control: ControlEndpoint | null }
export interface MaintSession { id: number; installation: string; kind: "marker" | "dip"; started: number; expected_slaves: number; done: string[]; failing: string | null }
export interface FlashBlock { id: number; pack: number; reason: "update-interrupted" | "update-unclear" | "bootloader-unclear" | "master-unclear"; expect: string | null; since: number }
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
  /** lookup only; null = the endpoint is not assigned to any installation */
  maintInstallationOf: (endpoint: Endpoint) => invoke<Installation | null>("maint_installation_of", { endpoint }),
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
- `maintInstallationFor` and `maintInstallationOf` return `{ id: "demo", name: "Demo", endpoints: ["tcp:demo:4196"], control: null }`;
- `maintOverview` / `maintCheck` return `{ installation: <that>, state: null, state_at: null, session: null, blocks: [], sector_note: null }`.

Update the existing `firmwareStart` call in `Firmware.tsx` to pass `null` for now.

- [ ] **Step 2: Write the failing tests** `app/src/maint.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { attachDecision, bannerOf, canFlash, endpointKey, errorKey, needsReplug, openWork } from "./maint";
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
  const ok = { overview: { ...base, session, state: { state: "patched-on" as const } }, inverter: true, measuredAt: 5_000, address: 3, imageKind: "original" as const, portAssigned: true };
  it("needs the inverter tick and a measurement in a session", () => {
    expect(canFlash(ok).ok).toBe(true);
    expect(canFlash({ ...ok, inverter: false })).toEqual({ ok: false, reason: "inverter" });
    expect(canFlash({ ...ok, measuredAt: null })).toEqual({ ok: false, reason: "measure" });
  });
  it("is blocked by an unclear update and by an unexplained marker", () => {
    expect(canFlash({ ...ok, overview: { ...ok.overview, blocks: [{ id: 1, pack: 2, reason: "update-unclear", expect: null, since: 0 }] } }).reason).toBe("update-unclear");
    expect(canFlash({ ...ok, overview: { ...base, state: { state: "marker-unexplained" } } }).reason).toBe("marker-unexplained");
  });
  it("a measurement older than the last state check does not count", () => {
    expect(canFlash({ ...ok, overview: { ...ok.overview, state_at: 6_000 }, measuredAt: 5_000 }).reason).toBe("measure");
  });
  it("a single pack without maintenance needs neither", () => {
    expect(canFlash({ ...ok, overview: { ...base, state: { state: "patched-off", slaves: 0 } }, inverter: false, measuredAt: null, portAssigned: false }).ok).toBe(true);
    expect(canFlash({ ...ok, overview: null, inverter: false, measuredAt: null, portAssigned: false }).ok).toBe(true);
  });
  it("unassigned_port_blocks_maintenance_flash", () => {
    expect(canFlash({ ...ok, portAssigned: false }).reason).toBe("port-not-assigned");
  });
  it("an interrupted update blocks like an unclear one", () => {
    expect(canFlash({ ...ok, overview: { ...ok.overview, blocks: [{ id: 2, pack: 4, reason: "update-interrupted", expect: null, since: 0 }] } }).reason).toBe("update-interrupted");
  });
  it("an unexplained marker allows only the recovery: master, original image, DIP session", () => {
    const locked = { ...base, state: { state: "marker-unexplained" as const } };
    const dip: MaintSession = { ...session, kind: "dip" };
    expect(canFlash({ ...ok, overview: locked, address: 0 }).reason).toBe("dip-off");
    expect(canFlash({ ...ok, overview: { ...locked, session: dip }, address: 0 }).ok).toBe(true);
    expect(canFlash({ ...ok, overview: { ...locked, session: dip }, address: 0, imageKind: "maintenance" }).reason).toBe("marker-unexplained");
    expect(canFlash({ ...ok, overview: { ...locked, session: dip }, address: 3 }).reason).toBe("marker-unexplained");
    expect(canFlash({ ...ok, overview: { ...locked, session: dip }, address: 0, inverter: false }).reason).toBe("inverter");
  });
});

describe("helpers", () => {
  it("asks to move the cable when control and pack port are one adapter", () => {
    expect(needsReplug({ kind: "serial", path: "/dev/a" }, "/dev/a")).toBe(true);
    expect(needsReplug({ kind: "serial", path: "/dev/a" }, "/dev/b")).toBe(false);
    expect(needsReplug({ kind: "tcp", addr: "gw:4196" }, "/dev/a")).toBe(false);
    expect(needsReplug(null, "/dev/a")).toBe(false);
  });
  it("a known endpoint of another installation never hides open work", () => {
    const other = { ...inst, id: "j", name: "Other" };
    const open = { ...base, session };
    expect(openWork(open)).toBe(true);
    expect(openWork(base)).toBe(false);
    expect(attachDecision(open, other)).toEqual({ do: "offer-switch", id: "j", name: "Other" });
    expect(attachDecision(base, other)).toEqual({ do: "use", id: "j" });
    expect(attachDecision(open, inst)).toEqual({ do: "use", id: "i" });
    expect(attachDecision(null, other)).toEqual({ do: "use", id: "j" });
    expect(attachDecision(open, null)).toEqual({ do: "offer-assign" });
    expect(attachDecision(null, null)).toEqual({ do: "create" });
  });
  it("names endpoints like the backend", () => {
    expect(endpointKey({ kind: "tcp", addr: " Gateway.local:4196 ", bus: "can" })).toBe("tcp:gateway.local:4196");
    expect(endpointKey({ kind: "serial", path: "/dev/cu.usbserial-X", bus: "pack" })).toBe("serial:/dev/cu.usbserial-X");
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
import type { ControlEndpoint, Endpoint, FirmwareImage, Installation, MaintError, MaintOverview } from "./api";

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

/** Open maintenance work: a session, a block, or a set marker (same rule as the backend's `open_work`). */
export function openWork(o: MaintOverview | null): boolean {
  const s = o?.state?.state;
  return !!o && (!!o.session || o.blocks.length > 0 || s === "patched-on" || s === "marker-unexplained");
}

export type Attach = { do: "use"; id: string } | { do: "create" } | { do: "offer-assign" } | { do: "offer-switch"; id: string; name: string };

/** What a new connection does to the installation in use: switch only when nothing open would vanish. */
export function attachDecision(current: MaintOverview | null, known: Installation | null): Attach {
  if (known && (!current || known.id === current.installation.id || !openWork(current))) return { do: "use", id: known.id };
  if (known) return { do: "offer-switch", id: known.id, name: known.name };
  return current ? { do: "offer-assign" } : { do: "create" };
}

/** Same key as the backend's `endpoint_key`: the physical endpoint, independent of the bus. */
export function endpointKey(ep: Endpoint): string {
  return ep.kind === "tcp" ? `tcp:${ep.addr.trim().toLowerCase()}` : `serial:${ep.path.trim()}`;
}

/** Control channel and pack port on one USB adapter: the cable must be moved between the steps. */
export function needsReplug(control: ControlEndpoint | null, packPort: string): boolean {
  return control?.kind === "serial" && control.path === packPort;
}

export interface FlashCheck {
  overview: MaintOverview | null;
  inverter: boolean;
  measuredAt: number | null;
  address: number;
  imageKind: FirmwareImage["kind"];
  /** the pack port belongs to the overview's installation */
  portAssigned: boolean;
}

/** Mirrors the backend gate so the start button stays honest; the backend checks everything again. */
export function canFlash({ overview: o, inverter, measuredAt, address, imageKind, portAssigned }: FlashCheck): { ok: boolean; reason?: string } {
  if (!o) return { ok: true };
  if (o.blocks.length) return { ok: false, reason: o.blocks[0].reason };
  const s = o.state?.state;
  // the recovery row: the master back to the original image under DIP 5–8 OFF, without stop
  const recovery = s === "marker-unexplained" && address === 0 && imageKind === "original";
  if (s === "marker-unexplained" && !recovery) return { ok: false, reason: "marker-unexplained" };
  if (recovery && !o.session) return { ok: false, reason: "dip-off" };
  const maintenance = recovery || !!o.session || s === "patched-on";
  if (!maintenance) return { ok: true };
  if (!portAssigned) return { ok: false, reason: "port-not-assigned" };
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
  - `useMaint()` returns `{ overview, setOverview, installation, unassigned, other, attach(endpoint: Endpoint), assign(endpoint: Endpoint), switchTo(id: string), refresh(), check() }`; `installation` is the id, persisted in `localStorage["maint.installation"]`. `unassigned`: the last connected endpoint that belongs to no installation while another one is in use. `other`: `{ id, name }` of a known installation the connection belongs to, not switched to because the current one has open work.
  - `<MaintBanner overview unassigned other onAssign onSwitch onEnd />`
  - `<MaintSetup overview onSaved />` (name, control channel, and the pack-bus ports of the installation)
  - `Sidebar` prop `onEndpoint(endpoint: Endpoint)`

- [ ] **Step 1: Hook** `app/src/maintStore.ts`

```ts
import { useCallback, useEffect, useState } from "react";
import { api, isDemo, type Endpoint, type MaintOverview } from "./api";
import { attachDecision, endpointKey } from "./maint";

/** The installation in use and its maintenance overview. The banner reads from here, so it survives view
 *  changes, reconnects and (through the backend database) app restarts. */
export function useMaint() {
  const [installation, setInstallation] = useState<string | null>(() => localStorage.getItem("maint.installation"));
  const [overview, setOverview] = useState<MaintOverview | null>(null);
  const [unassigned, setUnassigned] = useState<Endpoint | null>(null);
  const [other, setOther] = useState<{ id: string; name: string } | null>(null);

  const refresh = useCallback(async (id = installation) => {
    if (!id || isDemo) return;
    setOverview(await api.maintOverview(id).catch(() => null));
  }, [installation]);

  /** Re-read 0x64 (start-up, every new connection on the CAN socket bus); errors leave the stored state. */
  const check = useCallback(async (id = installation) => {
    if (!id || isDemo) return;
    try { setOverview(await api.maintCheck(id)); } catch { await refresh(id); }
  }, [installation, refresh]);

  const use = useCallback(async (id: string, recheck: boolean) => {
    localStorage.setItem("maint.installation", id);
    setInstallation(id);
    if (recheck) await check(id); else await refresh(id);
  }, [check, refresh]);

  /** A new connection (`attachDecision`): never silently replaces an installation with open work (its
   *  session and blocks would vanish from view); the banner offers to assign or to switch instead. */
  const attach = useCallback(async (endpoint: Endpoint) => {
    if (isDemo) return;
    const known = await api.maintInstallationOf(endpoint).catch(() => null);
    const d = attachDecision(installation ? overview : null, known);
    setUnassigned(d.do === "offer-assign" ? endpoint : null);
    setOther(d.do === "offer-switch" ? { id: d.id, name: d.name } : null);
    if (d.do === "use") await use(d.id, endpoint.bus === "can" && !!known?.control);
    if (d.do === "create") {
      const created = await api.maintInstallationFor(endpoint).catch(() => null);
      if (created) await use(created.id, false);
    }
  }, [installation, overview, use]);

  /** Explicit switch from the banner. */
  const switchTo = useCallback(async (id: string) => {
    setOther(null);
    await use(id, false);
  }, [use]);

  /** Add an endpoint to the installation in use (explicit, from the banner or the firmware view). */
  const assign = useCallback(async (endpoint: Endpoint) => {
    if (!overview) return;
    const inst = overview.installation;
    const key = endpointKey(endpoint);
    if (!inst.endpoints.includes(key)) await api.maintSaveInstallation({ ...inst, endpoints: [...inst.endpoints, key] });
    setUnassigned(null);
    await refresh(inst.id);
  }, [overview, refresh]);

  // start-up: show the last confirmed state at once, then re-check if a control channel is set
  useEffect(() => {
    (async () => {
      await refresh();
      const o = installation && !isDemo ? await api.maintOverview(installation).catch(() => null) : null;
      if (o?.installation.control) await check();
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return { installation, overview, setOverview, unassigned, other, attach, assign, switchTo, refresh, check };
}
```

- [ ] **Step 2: Banner** `app/src/views/MaintBanner.tsx`

```tsx
import { useTranslation } from "react-i18next";
import { TriangleAlert, Wrench } from "lucide-react";
import type { Endpoint, MaintOverview } from "../api";
import { bannerOf } from "../maint";
import { Button } from "../components/ui/button";
import { cn } from "../lib/utils";

export function MaintBanner({ overview, unassigned, other, onAssign, onSwitch, onEnd }: {
  overview: MaintOverview | null;
  unassigned: Endpoint | null;
  other: { id: string; name: string } | null;
  onAssign: () => void;
  onSwitch: (id: string) => void;
  onEnd: () => void;
}) {
  const { t } = useTranslation();
  const b = bannerOf(overview);
  const assign = overview && (unassigned || other) && (
    <div role="status" className="mb-6 flex items-center gap-3 rounded-md border border-discharge p-3 text-sm">
      {unassigned && <>
        <p className="flex-1">{t("maint.banner.unassigned", { endpoint: unassigned.kind === "tcp" ? unassigned.addr : unassigned.path, name: overview.installation.name })}</p>
        <Button variant="ghost" size="sm" onClick={onAssign}>{t("maint.banner.assign")}</Button>
      </>}
      {other && <>
        <p className="flex-1">{t("maint.banner.other", { other: other.name, name: overview.installation.name })}</p>
        <Button variant="ghost" size="sm" onClick={() => onSwitch(other.id)}>{t("maint.banner.switch", { other: other.name })}</Button>
      </>}
    </div>
  );
  if (!b) return assign || null;
  const since = b.since ? new Date(b.since).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : null;
  return (
    <>
    {assign}
    <div role="status" className={cn("mb-6 flex items-start gap-3 rounded-md border p-3 text-sm", b.kind === "open" ? "border-discharge" : "border-alarm text-alarm")}>
      {b.kind === "open" ? <Wrench className="mt-0.5 h-4 w-4 shrink-0" /> : <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />}
      <div className="flex-1 space-y-1">
        <p className="font-medium">{t(`maint.banner.${b.kind}`, { since: since ?? "?", how: t(`maint.how.${b.how}`) })}</p>
        <p className="text-muted">{t(b.kind === "unexplained" ? "maint.banner.unexplainedHint" : "maint.banner.hint")}</p>
        {overview?.session?.failing && <p className="text-alarm">{t("maint.banner.failing", { what: overview.session.failing })}</p>}
      </div>
      {b.end && <Button variant="ghost" size="sm" onClick={onEnd}>{t("maint.end")}</Button>}
    </div>
    </>
  );
}
```

- [ ] **Step 3: Setup** `app/src/views/MaintSetup.tsx`. It is a small form for name, control channel (kind TCP/USB, address or port) and the endpoints of the installation. The user adds the USB port of the pack bus (RS485-1/2) here explicitly; every endpoint can be removed. Saving moves an endpoint away from any other installation (backend `save_installation`).

```tsx
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type ControlEndpoint, type MaintOverview } from "../api";
import { endpointKey } from "../maint";
import { Button } from "../components/ui/button";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function MaintSetup({ overview, onSaved }: { overview: MaintOverview; onSaved: (o: MaintOverview) => void }) {
  const { t } = useTranslation();
  const inst = overview.installation;
  const [name, setName] = useState(inst.name);
  const [kind, setKind] = useState<ControlEndpoint["kind"]>(inst.control?.kind ?? "serial");
  const [value, setValue] = useState(inst.control ? (inst.control.kind === "tcp" ? inst.control.addr : inst.control.path) : "");
  const [ports, setPorts] = useState<string[]>([]);
  const [endpoints, setEndpoints] = useState<string[]>(inst.endpoints);
  const [packPort, setPackPort] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);

  const addPackPort = () => {
    const key = endpointKey({ kind: "serial", path: packPort, bus: "pack" });
    if (packPort && !endpoints.includes(key)) setEndpoints([...endpoints, key]);
    setPackPort("");
  };

  const save = async () => {
    setError(null);
    const control: ControlEndpoint | null = !value ? null : kind === "tcp" ? { kind, addr: value } : { kind, path: value };
    try {
      await api.maintSaveInstallation({ ...inst, name, control, endpoints });
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
      <div className="space-y-1">
        <p>{t("maint.setup.endpoints")}</p>
        {endpoints.length === 0 && <p className="text-muted">–</p>}
        {endpoints.map((ep) => (
          <p key={ep} className="flex items-center gap-2"><span className="font-mono text-xs">{ep}</span>
            <Button variant="ghost" size="sm" onClick={() => setEndpoints(endpoints.filter((x) => x !== ep))}>{t("maint.setup.remove")}</Button></p>
        ))}
        <div className="flex flex-wrap items-center gap-3">
          {t("maint.setup.packPort")}
          <select className={input} value={packPort} onChange={(e) => setPackPort(e.target.value)}><option value="">{t("conn.port")}</option>{ports.map((p) => <option key={p}>{p}</option>)}</select>
          <Button variant="ghost" size="sm" disabled={!packPort} onClick={addPackPort}>{t("maint.setup.add")}</Button>
        </div>
      </div>
      <Button variant="ghost" size="sm" onClick={save}>{t("maint.setup.save")}</Button>
      {error && <p className="text-alarm">{error}</p>}
    </div>
  );
}
```

- [ ] **Step 4: Wire App and Sidebar.**
  - `App.tsx`:
    - `const maint = useMaint();`
    - render `<MaintBanner overview={maint.overview} unassigned={maint.unassigned} other={maint.other} onAssign={() => maint.unassigned && maint.assign(maint.unassigned)} onSwitch={maint.switchTo} onEnd={() => setView("firmware")} />` as the first child inside `<main>` (End lives in the Pack bus panel);
    - pass `maint` to `<Firmware maint={maint} …/>`;
    - pass `onEndpoint={(ep) => maint.attach(ep)}` to `Sidebar`.
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
          unassigned: "Die Verbindung {{endpoint}} gehört zu keiner Anlage. Angezeigt wird weiter „{{name}}“.",
          assign: "Dieser Anlage zuordnen",
          other: "Die Verbindung gehört zu „{{other}}“. Weiter angezeigt wird „{{name}}“, weil dort eine Wartung oder Sperre offen ist.",
          switch: "Zu „{{other}}“ wechseln",
        },
        setup: {
          name: "Name der Anlage", control: "Steuerkanal (CAN-Buchse, 9600 Baud)",
          controlHint: "Für den Wartungsmodus des Masters (Befehl 0x64). Am selben Adapter wie die Verbindung links läuft der Austausch über diese Verbindung.",
          endpoints: "Verbindungen dieser Anlage", packPort: "USB-Port am Pack-Bus (RS485-1/2)", add: "Hinzufügen", remove: "Entfernen", save: "Speichern",
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
            "state-changed": "Der Master meldet nicht mehr den gespeicherten Zustand. Er wurde neu geprüft; bitte erneut vorbereiten.", "no-session": "Keine offene Wartung.",
            "quiet-first": "Der Master fragt noch ab: erst Wartungsmodus aktivieren oder DIP 5–8 AUS.", "state-unknown": "Zustand des Masters unbekannt.",
            "dip-off": "Erst DIP 5–8 AUS bestätigen.", "isolated-or-released": "Erst bestätigen: Build für den gemeinsamen Bus freigegeben oder Master getrennt.",
            "pack-not-answering": "Das Pack antwortet noch nicht als Anwendung.", "no-port": "Erst den Port wählen.", "no-block": "Sperre nicht gefunden.",
            how: "Ungültige Angabe.",
            "update-interrupted": "Ein Update wurde unterbrochen (App beendet oder Rechner aus). Weitere Updates sind gesperrt, bis das Pack geprüft ist.",
            "installation-mismatch": "Der Pack-Port gehört zu einer anderen Anlage als der angezeigten.",
            "port-not-assigned": "Der Pack-Port ist keiner Anlage zugeordnet, während eine Wartung offen ist. Erst zuordnen.",
            "stop-needs-control-channel": "Dieser Schritt braucht „stop“ direkt vor dem Flashen und damit einen eigenen Adapter für den Steuerkanal.",
            "master-unclear": "Das Update des Masters ist unklar. Erst den Master prüfen; Beenden ist bis dahin gesperrt.",
            "dip-session-open": "Eine Wiederherstellung per DIP läuft; die Herkunft des Markers lässt sich dabei nicht nachtragen.",
            "endpoint-in-use": "Dieser Anschluss gehört zu einer Anlage mit offener Wartung oder Sperre und kann nicht umziehen.",
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
          unassigned: "The connection {{endpoint}} belongs to no installation. Still showing “{{name}}”.",
          assign: "Assign to this installation",
          other: "The connection belongs to “{{other}}”. Still showing “{{name}}” because maintenance or a block is open there.",
          switch: "Switch to “{{other}}”",
        },
        setup: {
          name: "Installation name", control: "Control channel (CAN socket, 9600 baud)",
          controlHint: "For the master's maintenance mode (command 0x64). On the same adapter as the connection on the left, the exchange uses that connection.",
          endpoints: "Connections of this installation", packPort: "USB port on the pack bus (RS485-1/2)", add: "Add", remove: "Remove", save: "Save",
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
            "state-changed": "The master no longer reports the stored state. It was checked again; prepare again.", "no-session": "No open maintenance.",
            "quiet-first": "The master still polls: activate maintenance mode or set DIP 5–8 OFF first.", "state-unknown": "State of the master unknown.",
            "dip-off": "Confirm DIP 5–8 OFF first.", "isolated-or-released": "Confirm first: build released for the shared bus, or master isolated.",
            "pack-not-answering": "The pack does not answer as an application yet.", "no-port": "Choose the port first.", "no-block": "Block not found.",
            how: "Invalid choice.",
            "update-interrupted": "An update was interrupted (app closed or computer off). Further updates are blocked until the pack is checked.",
            "installation-mismatch": "The pack port belongs to another installation than the one shown.",
            "port-not-assigned": "The pack port belongs to no installation while maintenance is open. Assign it first.",
            "stop-needs-control-channel": "This step needs “stop” right before the flash, so a separate adapter for the control channel.",
            "master-unclear": "The master's update is unclear. Check the master first; End is blocked until then.",
            "dip-session-open": "A recovery by DIP is running; the marker's origin cannot be stated now.",
            "endpoint-in-use": "This connection belongs to an installation with open maintenance or a block and cannot move.",
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
          {o.session?.kind !== "dip" && <Button variant="ghost" size="sm" disabled={busy} onClick={() => run(() => api.maintAccountMarker(o.installation.id, n))}>{t("maint.panel.accountMarker")}</Button>}
          {/* the recovery row: DIP session first, then the original image on address 0 (firmware view below) */}
          {!o.session && (
            <div className="space-y-2 border-t border-line pt-2">
              <p>{t("maint.panel.recoveryHint")}</p>
              <label className="flex items-center gap-2"><input type="checkbox" checked={dipConfirmed} onChange={(e) => setDipConfirmed(e.target.checked)} />{t("maint.panel.dipConfirm")}</label>
              <Button variant="ghost" size="sm" disabled={busy || !dipConfirmed} onClick={() => run(() => api.maintStartDip(o.installation.id, n))}>{t("maint.panel.startRecovery")}</Button>
            </div>
          )}
        </div>
      )}

      {/* 2. inverter: in memory only, cleared with every new work phase */}
      {(o.session || s === "patched-on" || (s === "patched-off" && (o.state?.slaves ?? 0) > 0)) && (
        <label className="flex items-start gap-2">
          <input type="checkbox" className="mt-1" checked={inverter} onChange={(e) => setInverter(e.target.checked)} />
          <span>{t("maint.panel.inverter")}<span className="block text-muted">{t("maint.panel.inverterHint")}</span></span>
        </label>
      )}

      {/* 3. measure */}
      {(o.session || s === "patched-on") && (
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
  - **Pack port assignment:** whenever `port` or `maint.overview` changes, `api.maintInstallationOf({ kind: "serial", path: port, bus: "pack" })` → `portAssigned = result?.id === maint.overview?.installation.id`. When it is not assigned, show under the port select `t("maint.panel.portUnassigned")` with a button `t("maint.banner.assign")` that calls `maint.assign({ kind: "serial", path: port, bus: "pack" })` and re-runs the lookup. When it belongs to another installation, show `t("maint.err.refused.installation-mismatch")` and no button (the user changes it in the setup).
  - `const gate = canFlash({ overview: maint.overview, inverter, measuredAt, address, imageKind: image?.kind ?? "custom", portAssigned });` → pass `unlocked: gate.ok` to `steps()`. When `!gate.ok`, show `t(errorKey(gate.reason).key)` under the start button.
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
          block: { "master-unclear": "Update des Masters unklar: Er meldet nicht den erwarteten Zustand. „Pack prüfen“ fragt den Master erneut ab.", "update-unclear": "Update an Pack {{pack}} unklar: weitere Updates gesperrt.", "bootloader-unclear": "Pack {{pack}} ist eventuell im Bootloader: weitere Updates gesperrt.", "update-interrupted": "Update an Pack {{pack}} läuft oder wurde unterbrochen (App beendet, Rechner aus): weitere Updates gesperrt, bis das Pack geprüft ist." },
          portUnassigned: "Dieser Port gehört noch zu keiner Anlage.",
          recoveryHint: "Weg zurück ohne „stop“: DIP 5–8 notieren und auf AUS stellen, dann unten den Master (Adresse 0) mit der Original-Firmware flashen.",
          startRecovery: "Wiederherstellung per DIP beginnen",
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
          block: { "master-unclear": "Master update unclear: it does not report the expected state. “Check pack” asks the master again.", "update-unclear": "Update of pack {{pack}} unclear: further updates blocked.", "bootloader-unclear": "Pack {{pack}} may be in its bootloader: further updates blocked.", "update-interrupted": "Update of pack {{pack}} running or interrupted (app closed, computer off): further updates blocked until the pack is checked." },
          portUnassigned: "This port belongs to no installation yet.",
          recoveryHint: "Way back without “stop”: note DIP 5–8 and set them OFF, then flash the master (address 0) below with the original firmware.",
          startRecovery: "Start recovery by DIP",
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
  - decisions 7–11 of this plan (pack port decides the installation, `update-interrupted`, sequential way and `stop`, unclear master update as a block, guarded endpoint ownership);
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
- [ ] **Step 4: Spec.** Apply the edits listed under Files.
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
  5. "End" → `stop` → 0/1 → the end round shows the poll to address 1. *Without a second pack the round stays incomplete; expected result: "pack communication incomplete (missing: 1)", session stays open.* Note it in the lab notebook.
  6. Close the session for the bench only, outside the app: quit the app, then `sqlite3 "<app data>/maintenance.sqlite" "UPDATE maint_sessions SET ended = strftime('%s','now')*1000 WHERE ended IS NULL"`. Restart the app; the banner is gone and the master reads `patched-off, 1 slave`. This is a lab step; the product path ends only through a passed end check.
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
- [ ] **Step 7: Sequential way.** Set the control channel to the same USB adapter as the pack port. The app must ask to move the cable before the check and before the measurement, and no step may fail with "busy". A master flash that needs `stop` is refused with "needs a separate adapter".
- [ ] **Step 7a: Interrupted update.** Flash a slave address that does not exist on the board (preflight fails): no block remains. Then start a real flash of address 0 and quit the app during the block transfer. After the restart the panel shows "update running or interrupted" for address 0, further flashes are refused, and "Check pack" clears it only once the master answers 0x51/0x47 again (after a recovery flash).
- [ ] **Step 7b: Pack port assignment.** With an open session, connect the sidebar to a second, unassigned port: the banner offers to assign it and the shown installation stays. Starting a flash on an unassigned pack port is refused with "port not assigned".
- [ ] **Step 8:** Write results and open points into the lab notebook. Bench items that need two real packs or the gateway stay open for stage 2, among them: the master polls 1..N in turn and a round starts with address 1 (`watch_round` relies on it), and a complete end round with real slaves.

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
  | Connections and channel ownership | 7 (route), 8 (`port_gate`, `same_adapter`) |
  | Sequential way | 9 (`needsReplug`), 11, 13 |
  | Flow steps 1–5 | 7, 8 (`prepare_flash` order, fresh status), 11 |
  | Banner | 9, 10 |
  | Master flash table | 4, 8, 11 |
  | Abort cases | 8 (`block_reason`, `finish_block`, `update-interrupted`, `app_answers`) |
  | Transport | 5 (TCP deadline; TCP flashing is stage 2) |
  | Backend | 6–8 |
  | Tests | 2–4, 6–9, 11, 13 |

  Parameter writing and snapshots are out of scope, as in the spec.
- **Known gap left to stage 2:** TCP pack port in the firmware view; bench tests through the gateway and with several real packs.
