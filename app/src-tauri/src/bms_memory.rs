//! The BMS clock, the pack's internal history, calibration, the CAN inverter protocol, writing parameters and serial
//! numbers (RS485-1/2 only; parameters also on the CAN socket of a patched pack, `ParamPath`, and the clock of a pack with
//! the power and clock service, `power.rs`). Every write reserves
//! its pack for the whole exchange (`control::reserve_pack`), the history read too.
//!
//! A history read runs on its own thread and takes the connection lock per frame, through `with_conn` like every
//! other exchange; meanwhile `memory_busy` pauses polling, so the bus serves the read alone. Between two frames a
//! disconnect, a reconnect or a flash can still take the connection; the read then stops with that error.

use crate::bank::{check_target, Written};
use crate::history::{now_ms, SnapshotKind};
use crate::control::PackAction;
use crate::maint_store::Installation;
use crate::{for_link, for_link_checked, link_now, with_conn, with_link, AppState, Link};
use openbms_proto::access::{self, Action, Channel};
use openbms_proto::serial::Which;
use openbms_proto::{BmsTime, Calibration, DeviceInfo, HistoryRecord, Parameters, Refusal, SerialValue, Status, Telemetry, Values};
use openbms_transport::Bus;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use openbms_proto::uart1;
use tauri::{AppHandle, Emitter, State};

/// Guard against a device that never sets the last flag: the ring holds at most 500 records.
pub const MAX_RECORDS: usize = 520;

/// The refusal of a String 2 write whose read before failed (the UI names it).
pub const PRE_READ: &str = "pre-read: String 2 could not be read before the write; nothing was written";

pub(crate) const PACK_BUS_ONLY: &str = "this command exists only on RS485-1/2";
/// The refusal of a history read over the CAN socket of a build without read-capability bit 0 (the UI names it).
pub const HISTORY_NO_CAP: &str = "history-no-cap: over the CAN socket only with MAINT 1.5.4/1.6.4 or later; RS485-1/2 reads every pack";
const CAN_SOCKET_ONLY: &str = "this command exists only on the CAN socket";

/// The UART a bus reaches the packs on.
pub(crate) fn channel_of(bus: Bus) -> Channel {
    match bus {
        Bus::CanSocketBus => Channel::Uart1,
        Bus::PackBus => Channel::Uart0,
    }
}

/// The channels the app has a flow for, per action. The plain commands (`power::Act`) and the reads share one flow over
/// both; the others are written for one channel (their own checks and frames there). A catalog row the app marks as
/// implemented must find its channel here (`every_implemented_route_has_a_flow`): a new row for a plain command needs no
/// code, one for an action with its own flow needs that flow first.
pub(crate) fn flows(action: Action) -> &'static [Channel] {
    use Action as A;
    const BOTH: &[Channel] = &[Channel::Uart0, Channel::Uart1];
    match action {
        A::ReadTelemetry | A::ReadStatus | A::ReadInfo | A::ReadParameters | A::WriteParameters | A::ReadSerials | A::ReadClock => BOTH,
        A::SetClock | A::PowerOnOff | A::PackLockSet | A::PackLockRelease | A::Restart | A::ForceHeater | A::ClearHistory | A::SetRemaining
        | A::SetLearnedFull | A::Calibrate | A::SetCanProtocol => BOTH,
        // the hard stop choice rides in the parameter write (Group0 bits 6/7)
        A::HardStop => BOTH,
        // RS485-1/2 only: the name rides in the 169-byte ASCII A1, the serial strings and the history have their own
        // frames and checks there, the bootloader listens there
        A::WriteName | A::WriteSerials | A::Flash => &[Channel::Uart0],
        // the history: RS485-1/2, and the CAN socket of a build with read-capability bit 0 (MNT format 5)
        A::ReadHistory => BOTH,
        // RS485-1/2 with image flag bit 4: „Wartungs-Marker“ (`marker.rs`) and „Platine flashen“ (`bench.rs`)
        A::ReadIdentity | A::MaintStatus | A::MaintStop => BOTH,
        // the memory diagnosis (`diagnosis.rs`)
        A::ReadDiagnosis => BOTH,
        // the calibration pages and the stored SOC records / live energy (`diagnosis.rs`, MNT format 5)
        A::ReadCalibration | A::ReadSocStore => BOTH,
        A::MaintStart | A::ReadSystem => &[Channel::Uart1],
        A::DisplayTunnel => &[],
    }
}

/// The channel `action` takes on `bus`; refused when the catalog has no way for it there (`access::ROUTES`) or the app
/// no flow (`flows`).
pub(crate) fn on_bus(action: Action, bus: Bus) -> Result<Channel, String> {
    let channel = channel_of(bus);
    if access::routes(action).any(|r| r.channel == channel && r.implemented) && flows(action).contains(&channel) {
        return Ok(channel);
    }
    Err(if channel == Channel::Uart1 { PACK_BUS_ONLY } else { CAN_SOCKET_ONLY }.into())
}

/// The bus of connection `link`: the one its profile was read with for that generation, so bus and link always belong
/// to the same connection (never the global bus read at another moment, before a reconnect).
pub(crate) fn bus_of(state: &AppState, link: &Link) -> Result<Bus, String> {
    crate::bank::profile(state, link.gen).map(|p| p.bus).ok_or_else(|| "not connected".into())
}

/// A write's connection and its bus, taken together after the pack's reservation (no connect runs while a pack is
/// reserved); every frame then goes through `with_link` on exactly this link. On the CAN socket ein/aus, restart and
/// the clock take the patched pack's way (`power.rs`), never the RS485-1/2 one.
pub(crate) fn bound_bus(state: &AppState, installation: Option<&str>) -> Result<(Link, Bus), String> {
    let link = bound(state, installation)?;
    let bus = bus_of(state, &link)?;
    Ok((link, bus))
}

/// Writes to a pack belong to the connection's installation: none with „Nur ansehen“ (nothing would record the state
/// before, and the pack may belong to a foreign bank).
/// `installation`: the one the UI writes for; it must be the connection's, so a choice changed meanwhile never writes
/// to a pack of another bank.
pub(crate) fn bound(state: &AppState, installation: Option<&str>) -> Result<Link, String> {
    let l = link_now(state);
    if l.installation.is_none() {
        return Err("view only: writing to a pack needs a connection with an installation".into());
    }
    if l.installation.as_deref() != installation {
        return Err("the connection belongs to another installation than the one chosen; connect again".into());
    }
    Ok(l)
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// The pack flagged its last record.
    Last,
    /// The pack has no (more) records.
    Empty,
    Cancelled,
    /// MAX_RECORDS read without a last flag.
    Cap,
    Error,
}

/// Result of a history read: the records oldest first, as far as they came.
#[derive(Serialize, Clone, Debug)]
pub struct HistoryRead {
    pub address: u8,
    pub records: Vec<HistoryRecord>,
    pub end: End,
    pub error: Option<String>,
    /// with an error: "eeprom" (RTN 08: a memory error; over the CAN socket the pack restored its read index, over
    /// RS485-1/2 the original firmware promises nothing), "silent" (no answer: the index may have moved on; never
    /// continued automatically) or "other"
    pub error_kind: Option<&'static str>,
    /// how many of the records were new in the installation's archive; None: not archived (`archive_note` says why)
    pub archived: Option<usize>,
    /// "view-only", "unchecked" (the board was not proven for this read, or changed during it) or "error"
    pub archive_note: Option<&'static str>,
}

/// The kind of a history read error (`HistoryRead::error_kind`), from the transport's text.
fn error_kind(e: &str) -> &'static str {
    if e.contains("RTN 0x08") {
        "eeprom"
    } else if e.contains("no answer within") {
        "silent"
    } else {
        "other"
    }
}

/// Progress event "bms-history-progress": `n` records read so far.
#[derive(Serialize, Clone)]
struct Progress {
    address: u8,
    n: usize,
}

/// The read loop: the oldest record, then the next one until the last flag, an empty answer, an error, a
/// cancel or MAX_RECORDS.
pub fn read_all(
    address: u8,
    cancel: &AtomicBool,
    mut frame: impl FnMut(bool) -> Result<Option<(HistoryRecord, bool)>, String>,
    mut progress: impl FnMut(usize),
) -> HistoryRead {
    let mut records = Vec::new();
    let (end, error) = loop {
        if cancel.load(Ordering::SeqCst) {
            break (End::Cancelled, None);
        }
        if records.len() >= MAX_RECORDS {
            break (End::Cap, None);
        }
        match frame(!records.is_empty()) {
            Ok(None) => break (End::Empty, None),
            Ok(Some((r, last))) => {
                records.push(r);
                progress(records.len());
                if last {
                    break (End::Last, None);
                }
            }
            Err(e) => break (End::Error, Some(e)),
        }
    };
    let error_kind = error.as_deref().map(error_kind);
    HistoryRead { address, records, end, error, error_kind, archived: None, archive_note: None }
}

/// The clock (0x4D); over the CAN socket only of a pack with the power and clock service (`power::clock`).
#[tauri::command]
pub async fn bms_clock(state: State<'_, AppState>, address: u8) -> Result<BmsTime, String> {
    let link = link_now(&state);
    match crate::power::way(Action::ReadClock, bus_of(&state, &link)?)? {
        crate::power::Way::Power => crate::power::clock(&state, &link, address),
        crate::power::Way::Ascii => with_link(&state.conn, &state.flashing, &state.link, &link, |c| c.clock(address)),
    }
}

/// Set the clock to `time` (the PC's local time, chosen by the UI) and read it back. Over the CAN socket with the
/// identity check and the lock bit first (`power::run`).
#[tauri::command]
pub async fn set_bms_clock(state: State<'_, AppState>, address: u8, installation: Option<String>, time: BmsTime) -> Result<Written<BmsTime>, String> {
    let PackAction { guard: _guard, link, bus } = PackAction::write(&state, installation.as_deref(), address, "clock")?;
    if crate::power::way(Action::SetClock, bus)? == crate::power::Way::Power {
        return match crate::power::run(&state, &link, address, crate::power::Act::SetClock(time))? {
            crate::power::Done::Clock(value) => Ok(Written { value, unchecked: false }),
            _ => Err("no result".into()),
        };
    }
    let checked = check_target(&state, &link, address)?;
    let value = with_link(&state.conn, &state.flashing, &state.link, &link, |c| {
        c.set_clock(address, time)?;
        c.clock(address)
    })?;
    Ok(Written { value, unchecked: checked.unchecked })
}

/// A write the BMS answered with RTN 06/04, as JSON for the UI (`refusal` in calibration.ts):
/// `{"kind":"bms-refused","rtn":"refused"}` (RTN 06), `"invalid"` (RTN 04) or `"locked"` (binary E3). Every other error stays plain text.
pub(crate) fn refusal_error(r: Refusal) -> String {
    serde_json::json!({ "kind": "bms-refused", "rtn": r }).to_string()
}

/// Run a write; a refusal by the BMS comes back as `Err(Refusal)` instead of a plain error.
pub(crate) fn refusable<T>(r: openbms_transport::Result<T>) -> openbms_transport::Result<Result<T, Refusal>> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(openbms_transport::Error::Proto(e)) => Refusal::from_error(&e).map(Err).ok_or_else(|| e.into()),
        Err(e) => Err(e),
    }
}

/// Telemetry and status of one pack, for the live values of the calibration and the control (both buses answer them;
/// the poll reads the same).
#[derive(Serialize)]
pub struct PackLive {
    pub telemetry: Telemetry,
    pub status: Status,
}

#[tauri::command]
pub async fn pack_live(state: State<'_, AppState>, address: u8) -> Result<PackLive, String> {
    // both frames under one lock: no calibration falls between the values and the flags shown beside them
    let (telemetry, status) = with_conn(&state.conn, &state.flashing, |c| Ok((c.telemetry(address)?, c.status(address)?)))?;
    Ok(PackLive { telemetry, status })
}

/// The learned full capacity of one pack (0x42 answers on both buses): a successful parameter write sets it to P58 when
/// they are 10 Ah or more apart, so the Parameters view asks it before every write, on RS485-1/2 and the CAN socket.
#[tauri::command]
pub async fn pack_full_capacity(state: State<'_, AppState>, address: u8) -> Result<f64, String> {
    full_capacity(&state, address)
}

fn full_capacity(state: &AppState, address: u8) -> Result<f64, String> {
    with_conn(&state.conn, &state.flashing, |c| c.telemetry(address)).map(|t| t.full_capacity_ah)
}

/// One calibration step (0xA0). The UI reads the live values again afterwards.
#[tauri::command]
pub async fn bms_calibrate(state: State<'_, AppState>, address: u8, installation: Option<String>, calibration: Calibration) -> Result<Written<()>, String> {
    crate::control::act_frame(&state, installation.as_deref(), address, "calibration", crate::power::Act::Calibrate(calibration), |_| ())
}

/// Result of a protocol set the BMS accepted: the device info read back, or why that read failed.
#[derive(Serialize, Debug)]
pub struct CanProtocolSet {
    pub device: Option<DeviceInfo>,
    pub read_error: Option<String>,
}

impl CanProtocolSet {
    fn from_read(r: Result<DeviceInfo, String>) -> Self {
        match r {
            Ok(d) => Self { device: Some(d), read_error: None },
            Err(e) => Self { device: None, read_error: Some(e) },
        }
    }
}

/// Set the CAN inverter protocol (0xA6, 1..=6) and read the device info (0x51) back for its new name. Once the BMS
/// has accepted, a failed read-back is still a success (the protocol is set), with `read_error`.
#[tauri::command]
pub async fn set_can_protocol(state: State<'_, AppState>, address: u8, installation: Option<String>, protocol: u8) -> Result<Written<CanProtocolSet>, String> {
    crate::control::act_frame(&state, installation.as_deref(), address, "can", crate::power::Act::CanProtocol(protocol), |link| {
        CanProtocolSet::from_read(with_link(&state.conn, &state.flashing, &state.link, link, |c| c.device_info(address)))
    })
}

/// Keep a parameter block for the place in the connection's installation (`before`: the link when the exchange
/// began). Nothing with „Nur ansehen“, nothing for a pack whose board is not confirmed (`bank.rs`); a failure must not
/// fail the exchange with the pack.
pub fn snapshot(state: &AppState, before: &Link, address: u8, kind: SnapshotKind, block: &[u8]) -> Option<i64> {
    if crate::bank::unconfirmed(state, before.gen, address) {
        return None;
    }
    for_link(state, before, "parameter snapshot", |s, inst| s.add_param_snapshot(inst, address, kind, block, now_ms()))
}

/// Keep with a kept read the patch it was taken under (`maint_store::set_snapshot_patch`); a failure only loses that.
pub fn snapshot_patch(state: &AppState, before: &Link, id: Option<i64>, patch: Option<&crate::profile::PackPatch>) {
    let (Some(id), Some(p)) = (id, patch) else { return };
    let Ok(json) = serde_json::to_string(p) else { return };
    for_link_checked(state, before, "snapshot patch", |s, _| s.set_snapshot_patch(id, &json));
}

/// How parameters are read and written on the connected bus: ASCII 0x47/0xA1 on RS485-1/2, the binary service of a
/// patched pack on the CAN socket (`uart1.rs`). Chosen by the bus, never one in place of the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamPath {
    Ascii,
    Uart1(Bus),
}

impl ParamPath {
    pub fn of(bus: Option<Bus>) -> Self {
        match bus {
            Some(b @ Bus::CanSocketBus) => ParamPath::Uart1(b),
            _ => ParamPath::Ascii,
        }
    }
}

/// The errors of the CAN socket's parameter service as JSON for the UI (`uart1Error` in control.ts): `no-answer` (no
/// answer within 3 s: without the patch, or not reachable; never a verdict on the firmware), `protocol` with the code
/// (81/87/83: the pack answered, nothing was taken), `pending` (an interrupted update of this address is open),
/// `other-master` (the master behind the CAN socket is not the installation's), `readback` (acknowledged, but the
/// read after it differs), the identity refusals of `uart1_id.rs` (`no-identity`, `unchecked`, `changed`,
/// `board-changed`, `duplicate`, `elsewhere`) and those of the power and clock service (`power.rs`: `power-unsupported`,
/// `power-locked`, `rtc-failed`, `power-no-answer`, `power-lost` (ein/aus sent, then the link broke), `no-board`; `cutoff-off-failed`/`cutoff-on-failed`: RTN 08 to ein/aus
/// of a build with the stored pack cutoff; `cutoff-changed`: the extended off differs from the confirmed one; RS485-1/2
/// has the same as `control::switch_refusal`).
pub(crate) fn uart1_refusal(error: &str) -> String {
    serde_json::json!({ "kind": "uart1", "error": error }).to_string()
}

pub(crate) fn uart1_error(e: openbms_transport::Error) -> String {
    match e {
        openbms_transport::Error::Timeout(_) => uart1_refusal("no-answer"),
        // cut off with a frame inside: no valid parameters
        openbms_transport::Error::Incomplete => uart1_refusal("no-answer"),
        openbms_transport::Error::Proto(openbms_proto::Error::Uart1(code)) => serde_json::json!({ "kind": "uart1", "error": "protocol", "code": code }).to_string(),
        e => e.to_string(),
    }
}

/// What came of sending a block, once it may have left: taken, refused by the pack, a protocol error of the pack
/// (nothing taken), or lost (no or a broken answer: open whether the pack took it).
#[derive(Debug, PartialEq, Eq)]
pub enum Sent {
    Ack,
    Refused { refusal: Refusal, switches_live: bool },
    Protocol(String),
    Lost(String),
}

/// The exchanges of one parameter write on its path; `write_checked` is the one logic over both.
pub(crate) trait ParamIo {
    /// a 0x47 read; `keep`: as the read before or after the write, kept as a `read` snapshot where the path stores
    fn read(&mut self, keep: bool) -> Result<Parameters, String>;
    /// Err: nothing was sent (connection changed, flash, not connected)
    fn send(&mut self, block: &[u8]) -> Result<Sent, String>;
    fn snapshot(&mut self, kind: SnapshotKind, block: &[u8]);
    /// the CAN socket's service: readback compared, nothing stored, the block before kept for the session
    fn uart1(&self) -> bool;
}

struct AsciiIo<'a> {
    state: &'a AppState,
    link: &'a Link,
    address: u8,
}

impl ParamIo for AsciiIo<'_> {
    fn read(&mut self, keep: bool) -> Result<Parameters, String> {
        let p = with_link(&self.state.conn, &self.state.flashing, &self.state.link, self.link, |c| c.parameters(self.address))?;
        if keep {
            let _ = snapshot(self.state, self.link, self.address, SnapshotKind::Read, &p.block);
        }
        Ok(p)
    }

    fn send(&mut self, block: &[u8]) -> Result<Sent, String> {
        // the exchange's own error inside: from then on the frame may have been sent (a lost or broken answer)
        let sent = with_link(&self.state.conn, &self.state.flashing, &self.state.link, self.link, |c| Ok(refusable(c.write_parameters(self.address, block))))?;
        Ok(match sent {
            Ok(Ok(())) => Sent::Ack,
            // after RTN 06 the switches sent are live
            Ok(Err(refusal)) => Sent::Refused { refusal, switches_live: true },
            // the bus never got quiet: the block did not go out
            Err(e @ openbms_transport::Error::BusBusy(_)) => return Err(e.to_string()),
            Err(e) => Sent::Lost(e.to_string()),
        })
    }

    fn snapshot(&mut self, kind: SnapshotKind, block: &[u8]) {
        let _ = snapshot(self.state, self.link, self.address, kind, block);
    }

    fn uart1(&self) -> bool {
        false
    }
}

/// What the answer to a binary write says. A timeout, an incomplete answer or a broken link leave it open (lost).
pub(crate) fn uart1_sent(r: openbms_transport::Result<()>) -> Sent {
    match r {
        Ok(()) => Sent::Ack,
        // E2: the patch takes neither the values nor the switches
        Err(openbms_transport::Error::Proto(openbms_proto::Error::Uart1(0xE2))) => Sent::Refused { refusal: Refusal::Refused, switches_live: false },
        // E3: writing locked there, or the start release bit would change; checked before sending, so the pack changed
        // since the read
        Err(openbms_transport::Error::Proto(openbms_proto::Error::Uart1(0xE3))) => Sent::Refused { refusal: Refusal::Locked, switches_live: false },
        Err(e @ openbms_transport::Error::Proto(openbms_proto::Error::Uart1(_))) => Sent::Protocol(uart1_error(e)),
        // the bus never got quiet: nothing went out
        Err(e @ openbms_transport::Error::BusBusy(_)) => Sent::Protocol(e.to_string()),
        Err(e) => Sent::Lost(e.to_string()),
    }
}

/// Outcome of a parameter write.
#[derive(Serialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParamWrite {
    /// The pack changed a value the user did not touch since the edit's read: nothing was written; `fresh` is the
    /// pack now. `lock_changed`: its lock state (Group0 bit 5, MAINT 1.6) is among the changes.
    Stale { fresh: Parameters, lock_changed: bool },
    /// Written; the block read back, or why that read failed. `unanswered`: the BMS's answer to the write was missing
    /// or broken, the fresh read showed the values sent. `before` (CAN socket only): the read right before the write,
    /// „Vorheriger Stand“ for this session, since nothing read there is stored.
    Written {
        read_back: Option<Parameters>,
        read_error: Option<String>,
        unanswered: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        before: Option<Parameters>,
    },
    /// The answer to the write was missing or broken and a fresh read showed neither the values sent nor those before
    /// (or failed): whether the pack took the block is open; read the parameters again.
    Unclear { error: String },
    /// CAN socket: the outcome is open (acknowledged but the read-back differs from the block sent, or the answer lost
    /// and the read shows neither block, or an unchanged block sent), with what the pack holds now (`read_back`, None
    /// when that read failed) and the read before the write.
    #[serde(rename = "unclear")]
    UnclearRead { error: String, read_back: Option<Parameters>, before: Parameters },
    /// The BMS did not take the block (RTN 06 plausibility, RTN 04 markers; CAN socket: E2). `stored` is the read right
    /// before the write. `switches_live`: after RTN 06 the switches sent are live, these are the stored ones; after E2
    /// nothing was taken.
    Refused { refusal: Refusal, stored: Parameters, switches_live: bool },
}

/// Write one pack's parameters (0xA1): read 0x47 again, put the user's changes (`edited` against `base`, the read
/// the edit started from) on top of it, send, read back. `restore` sends `edited` as it is on top of the fresh read
/// (the stored block after a refusal); the rebase would take the live switches for a change of the pack.
///
/// CAN socket: the same steps over the binary service in one port session (`uart1_id::write`), only for a target whose
/// board identity is strictly known (a fresh canonical String 2 equal to `serial`, the String 2 read with the
/// edited column). Host polling pauses from the identity read to the read-back.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn write_parameters(state: State<'_, AppState>, address: u8, installation: Option<String>, base: Values, edited: Values, restore: bool, serial: Option<String>) -> Result<Written<ParamWrite>, String> {
    // fresh read, write and read-back belong together: no restart or other control of this pack in between
    // the path by the bus of this very link, so a reconnect meanwhile cannot send ASCII over the CAN socket
    let PackAction { guard: _guard, link, bus } = PackAction::write(&state, installation.as_deref(), address, "params")?;
    let r = match ParamPath::of(Some(bus)) {
        ParamPath::Ascii => (|| {
            // the right bank before anything else, also before the fresh 0x47 and its snapshot
            let checked = check_target(&state, &link, address)?;
            let value = write_checked(&mut AsciiIo { state: &state, link: &link, address }, address, base, edited, restore, crate::profile::group0_rule(false, None))?;
            Ok(Written { value, unchecked: checked.unchecked })
        })(),
        ParamPath::Uart1(bus) => crate::uart1_id::write(&state, &link, bus, address, serial.as_deref(), base, edited, restore),
    };
    // a write to the master: the release bits the profile shows follow it (on both paths)
    crate::profile::after_master_write(&state, &link, address, r.as_ref().ok().map(|w| &w.value));
    r
}

/// Outcome of a device name write.
#[derive(Serialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NameWrite {
    /// The pack holds another name than the one the user saw (`fresh`): nothing was written.
    Stale { fresh: Parameters },
    /// The pack already holds the name: nothing was sent (an A1 may reset the learned full capacity).
    Unchanged { fresh: Parameters },
    /// Acknowledged; the block read back or why that read failed.
    Written { read_back: Option<Parameters>, read_error: Option<String> },
    /// The BMS refused the block (RTN 06 for a stored value, RTN 04): the new name may be live until a restart, it was
    /// not stored. `stored` is the read right before.
    Refused { refusal: Refusal, stored: Parameters },
    /// The answer was lost: whether the BMS stored the block is open. `read_back`: the fresh read (None when it failed);
    /// even one equal to the block sent proves nothing, since the BMS takes the name into RAM before it checks the block.
    Unclear { error: String, read_back: Option<Parameters> },
}

/// Write the device name (bytes 159..169 of the 0xA1 block; RS485-1/2 only, the CAN socket's binary write has no
/// name): read 0x47 again, refuse when the pack's name is no longer `expected`, send that block with only the name
/// replaced, read back.
#[tauri::command]
pub async fn write_name(state: State<'_, AppState>, address: u8, installation: Option<String>, expected: String, name: String) -> Result<Written<NameWrite>, String> {
    let PackAction { guard: _guard, link, bus } = PackAction::write(&state, installation.as_deref(), address, "name")?;
    on_bus(Action::WriteName, bus)?;
    let checked = check_target(&state, &link, address)?;
    let value = name_checked(&mut AsciiIo { state: &state, link: &link, address }, address, &expected, &name)?;
    Ok(Written { value, unchecked: checked.unchecked })
}

pub(crate) fn name_checked(io: &mut impl ParamIo, address: u8, expected: &str, name: &str) -> Result<NameWrite, String> {
    // nothing goes out for a name the block cannot hold
    openbms_proto::params::name_bytes(name).map_err(|e| e.to_string())?;
    let fresh = io.read(true)?;
    if fresh.address != address {
        return Err(format!("the pack at address {address} answered with address {}", fresh.address));
    }
    if fresh.device_name != expected {
        return Ok(NameWrite::Stale { fresh });
    }
    if fresh.device_name == name {
        return Ok(NameWrite::Unchanged { fresh });
    }
    let block = openbms_proto::params::with_name(&fresh.block, name).map_err(|e| e.to_string())?;
    match io.send(&block)? {
        Sent::Refused { refusal, .. } => {
            io.snapshot(SnapshotKind::Refused, &block);
            Ok(NameWrite::Refused { refusal, stored: fresh })
        }
        Sent::Protocol(e) => Err(e),
        Sent::Lost(e) => match io.read(false) {
            // the block before: the write did not arrive
            Ok(p) if p.block == fresh.block => Err(e),
            // the new name in RAM also follows a refusal (RTN 06 copies it before the check), and the values sent are the
            // ones before: accepted and refused look the same, so nothing counts as written
            read => Ok(NameWrite::Unclear { error: e, read_back: read.ok() }),
        },
        Sent::Ack => {
            io.snapshot(SnapshotKind::Written, &block);
            Ok(match io.read(true) {
                Ok(p) => NameWrite::Written { read_back: Some(p), read_error: None },
                Err(e) => NameWrite::Written { read_back: None, read_error: Some(e) },
            })
        }
    }
}

/// An open bootloader record of the installation (one of its endpoints) at this address: the pack there may not run its
/// application, or another board may answer; the record is cleared by the existing way first.
pub fn pending_blocks(pending: &[crate::maint_store::Pending], endpoints: &[String], address: u8) -> bool {
    pending.iter().any(|p| p.pack == address && endpoints.contains(&p.endpoint))
}

/// The master's battery serial read now (Modbus FC04) is a real one and not the installation's stored one: the gateway
/// is at another bank. Missing, empty, a placeholder or nothing stored to compare decides nothing.
pub fn master_blocks(stored: Option<&str>, read: &SerialValue) -> bool {
    matches!(read, SerialValue::Set { value, .. } if !value.is_empty()) && stored.is_some() && !crate::notify::master_matches(stored, read)
}

/// Before a write over the CAN socket: no open interrupted update of this address (`pending_blocks`).
pub(crate) fn uart1_pending(state: &AppState, link: &Link, address: u8) -> Result<(), String> {
    let inst = link.installation.as_deref().ok_or("view only: writing to a pack needs a connection with an installation")?;
    let pending = crate::maint::with_store(state, |s| Ok(pending_blocks(&s.all_pending()?, &s.installation(inst)?.endpoints, address)))
        .map_err(|e| format!("{e:?}"))?;
    if pending { Err(uart1_refusal("pending")) } else { Ok(()) }
}

/// `group0`: what the write may do with the release bits of Group0 (`profile::apply_group0`); a refusal there sends nothing.
pub(crate) fn write_checked(io: &mut impl ParamIo, address: u8, base: Values, edited: Values, restore: bool, group0: crate::profile::Group0) -> Result<ParamWrite, String> {
    let fresh = io.read(true)?;
    // byte 0 is the pack's own address: a block from another pack must never be written here
    if fresh.address != address {
        return Err(format!("the pack at address {address} answered with address {}", fresh.address));
    }
    let lock_changed = crate::profile::lock_changed(base.switches[0], fresh.function_switches[0]);
    // a changed lock state stops the write even where the edit carries the bit as changed (a restore sends the stored
    // block on top of the pack, its lock state kept)
    let values = if restore { edited } else {
        match Values::rebase(&base, &edited, &fresh.values()).filter(|_| !lock_changed) {
            Some(v) => v,
            None => return Ok(ParamWrite::Stale { fresh, lock_changed }),
        }
    };
    let mut block = values.apply(&fresh.block).map_err(|e| e.to_string())?;
    crate::profile::apply_group0(group0, &fresh.block, &mut block)?;
    let uart1 = io.uart1();
    match io.send(&block)? {
        Sent::Refused { refusal, switches_live } => {
            io.snapshot(SnapshotKind::Refused, &block);
            return Ok(ParamWrite::Refused { refusal, stored: fresh, switches_live });
        }
        // the pack answered that it took nothing
        Sent::Protocol(e) => return Err(e),
        Sent::Lost(e) => {
            // the comparison read keeps no `read` snapshot: „Vorherigen Stand“ stays the block read before the send
            let read = io.read(false);
            let mut decided = after_lost(read.as_ref().ok().map(|p| p.values()).as_ref(), &values, &fresh.values());
            // an unchanged block cannot tell a write from none, and an A1 that arrived may have reset the full capacity
            if uart1 && values == fresh.values() {
                decided = AfterLost::Unclear;
            }
            if let Some(kind) = decided.snapshot() {
                io.snapshot(kind, &block);
            }
            // the CAN socket (a checked board): a read that shows neither block is what the pack holds now
            if let (true, AfterLost::Unclear, Ok(p)) = (uart1, &decided, &read) {
                io.snapshot(SnapshotKind::Read, &p.block);
            }
            return match decided {
                AfterLost::Written => Ok(ParamWrite::Written { read_back: read.ok(), read_error: None, unanswered: true, before: uart1.then_some(fresh) }),
                AfterLost::Unreached => Err(e),
                AfterLost::Unclear if uart1 => Ok(ParamWrite::UnclearRead { error: e, read_back: read.ok(), before: fresh }),
                AfterLost::Unclear => Ok(ParamWrite::Unclear { error: e }),
            };
        }
        Sent::Ack => {}
    }
    if !uart1 {
        io.snapshot(SnapshotKind::Written, &block);
        return Ok(match io.read(true) {
            Ok(p) => ParamWrite::Written { read_back: Some(p), read_error: None, unanswered: false, before: None },
            Err(e) => ParamWrite::Written { read_back: None, read_error: Some(e), unanswered: false, before: None },
        });
    }
    // the CAN socket: `written` only once the read-back shows the first 158 data bytes sent; another read-back is kept
    // as what was read, the attempt stays open
    Ok(match io.read(false) {
        Ok(p) if uart1::write_data(&p.block) != uart1::write_data(&block) => {
            io.snapshot(SnapshotKind::Read, &p.block);
            ParamWrite::UnclearRead { error: uart1_refusal("readback"), read_back: Some(p), before: fresh }
        }
        Ok(p) => {
            io.snapshot(SnapshotKind::Written, &block);
            io.snapshot(SnapshotKind::Read, &p.block);
            ParamWrite::Written { read_back: Some(p), read_error: None, unanswered: false, before: Some(fresh) }
        }
        // acknowledged, but nothing confirms it (a late answer of an earlier exchange looks the same): open, the series
        // stops
        Err(e) => ParamWrite::UnclearRead { error: e, read_back: None, before: fresh },
    })
}

/// What a fresh 0x47 read says after a write whose answer was lost or broken (`read`: None when it failed).
#[derive(Debug, PartialEq, Eq)]
pub enum AfterLost {
    /// the values sent: the pack took the block
    Written,
    /// the values before: the block did not arrive, as a write that reached nothing
    Unreached,
    /// anything else, or not readable
    Unclear,
}

impl AfterLost {
    /// The one snapshot a lost answer keeps: the block sent, once the read confirmed it. Never a `read` of the
    /// comparison, which would become „Vorheriger Stand“ (`last_write`: the newest read before the write).
    pub fn snapshot(&self) -> Option<SnapshotKind> {
        matches!(self, AfterLost::Written).then_some(SnapshotKind::Written)
    }
}

pub fn after_lost(read: Option<&Values>, sent: &Values, before: &Values) -> AfterLost {
    match read {
        Some(r) if r == sent => AfterLost::Written,
        Some(r) if r == before => AfterLost::Unreached,
        _ => AfterLost::Unclear,
    }
}

/// The last write of a pack at this installation and the parameters it replaced, for „Vorherigen Stand“.
#[derive(Serialize, Debug)]
pub struct ParamHistory {
    pub last_written: Option<i64>,
    pub previous: Option<Parameters>,
}

#[tauri::command]
pub fn param_history(state: State<AppState>, address: u8) -> Result<ParamHistory, String> {
    let l = link_now(&state);
    let Some(inst) = l.installation else { return Ok(ParamHistory { last_written: None, previous: None }) };
    let w = crate::maint::with_store(&state, |s| s.last_write(&inst, address)).map_err(|e| format!("{e:?}"))?;
    Ok(ParamHistory { last_written: w.at, previous: w.previous.and_then(|b| Parameters::parse(&b).ok()) })
}

/// Installations whose stored master serial is the master's old BMS serial number: after a successful write of a
/// new one they follow it, so the bank check before flashing still matches. Only set values count on both sides.
pub fn follow_master_serial<'a>(installations: &'a [Installation], old: &SerialValue, new: &SerialValue) -> Vec<&'a Installation> {
    let (SerialValue::Set { value: old, .. }, SerialValue::Set { value: new, .. }) = (old, new) else { return vec![] };
    if old == new {
        return vec![];
    }
    installations.iter().filter(|i| i.master_serial.as_deref() == Some(old.as_str())).collect()
}

/// A serial number write the BMS accepted: the value read back (or why that read failed) and the installations
/// whose stored master serial followed it. `master_unchecked`: String 2 of the master, but the old value (or the
/// store, or the new value) could not be read, so no installation's stored master serial was updated.
/// `board_unchecked`: String 1 of a known board, but the database could not keep the new value for it (the check of
/// the master behind the CAN socket then compares the old one).
#[derive(Serialize, Debug)]
pub struct SerialSet {
    pub value: Option<SerialValue>,
    pub read_error: Option<String>,
    pub master_updated: Vec<String>,
    pub master_unchecked: bool,
    pub board_unchecked: bool,
}

/// Write a serial number (0xA3 battery, 0xA5 BMS) and read it back. String 2 of the master (address 0): an
/// installation of this endpoint that stored the old value gets the new one. String 1 of a known board: the board
/// keeps the value read back, in the same action (`same_master` compares it). Device and database are not updated
/// atomically; a crash in between shows as a bank mismatch, which „Seriennummer übernehmen“ resolves.
#[tauri::command]
pub async fn set_serial(state: State<'_, AppState>, address: u8, installation: Option<String>, which: Which, text: String) -> Result<Written<SerialSet>, String> {
    let PackAction { guard: _guard, link, bus } = PackAction::write(&state, installation.as_deref(), address, "serial")?;
    on_bus(Action::WriteSerials, bus)?;
    // the right bank first (a silent pack is refused there); its String 2 read is the value before the write
    let checked = check_target(&state, &link, address)?;
    let unchecked = checked.unchecked;
    let master = which == Which::Bms && address == 0;
    // String 2 before the write: whether the board keeps its identity under the new serial depends on it. Unread, the
    // app's own write could not be told from a board swap: no write then.
    let pre = checked.read;
    let old = if which == Which::Bms {
        match &pre {
            SerialValue::NoAnswer | SerialValue::Unavailable => return Err(PRE_READ.into()),
            v => Some(v.clone()),
        }
    } else {
        None
    };
    with_link(&state.conn, &state.flashing, &state.link, &link, |c| refusable(c.set_serial(address, which, &text)))?.map_err(refusal_error)?;
    let read_raw = with_link(&state.conn, &state.flashing, &state.link, &link, |c| c.serial_canonical(address, which));
    let read = read_raw.as_ref().map(|(v, _)| v.clone()).map_err(Clone::clone);
    // the app's own String 2 write: the board keeps its identity under the new serial, its provenance from the raw
    // read-back
    if let (Which::Bms, Ok((new, canonical))) = (which, &read_raw) {
        let before = old.clone().unwrap_or(SerialValue::NoAnswer);
        // the board was checked just before: recorded also while another pack of the connection is foreign
        for_link_checked(&state, &link, "board", |s, inst| s.board_written_canonical(inst, address, &before, new, *canonical, now_ms()));
    }
    // the app's own String 1 write on a known board: kept for it, a store failure is reported
    let mut board_unchecked = false;
    if let (Which::Battery, false, Ok(new), Some(inst)) = (which, unchecked, &read, link.installation.as_deref()) {
        if let Err(e) = crate::maint::with_store(&state, |s| s.battery_written(inst, address, &pre, new, now_ms())) {
            eprintln!("board battery serial: {e:?}");
            board_unchecked = true;
        }
    }
    let mut master_updated = vec![];
    let mut master_unchecked = master && (old.is_none() || read.is_err());
    if let (true, Some(old), Ok(new)) = (master, &old, &read) {
        let key = state.main.lock().map_err(|e| e.to_string())?.as_ref().map(|m| m.0.clone());
        if let (Some(key), SerialValue::Set { value, .. }) = (key, new) {
            let r = crate::maint::with_store(&state, |s| {
                let all = s.installations_of(&key, false)?;
                let follow = follow_master_serial(&all, old, new);
                for i in &follow {
                    s.set_master_serial(&i.id, value, now_ms())?;
                }
                Ok(follow.iter().map(|i| i.name.clone()).collect::<Vec<_>>())
            });
            match r {
                Ok(names) => master_updated = names,
                Err(e) => {
                    eprintln!("master serial: {e:?}");
                    master_unchecked = true;
                }
            }
        }
    }
    let value = match read {
        Ok(v) => SerialSet { value: Some(v), read_error: None, master_updated, master_unchecked, board_unchecked },
        Err(e) => SerialSet { value: None, read_error: Some(e), master_updated, master_unchecked, board_unchecked },
    };
    Ok(Written { value, unchecked })
}

/// Read the whole history of one pack; progress arrives as "bms-history-progress" events. One read at a time.
#[tauri::command]
pub async fn bms_history(app: AppHandle, state: State<'_, AppState>, address: u8) -> Result<HistoryRead, String> {
    let busy = crate::reserve_port(&state.memory_busy).ok_or("a history read is already running")?;
    // clearing the same pack meanwhile would cut the read short
    // records of another connection's pack must not continue this read
    let PackAction { guard: pack, link, bus } = PackAction::read(&state, address, "history")?;
    let links = state.link.clone();
    on_bus(Action::ReadHistory, bus)?;
    // over the CAN socket only a build that announces it in its own 64/03 (read-capability bit 0)
    if bus == Bus::CanSocketBus {
        let caps = with_link(&state.conn, &state.flashing, &state.link, &link, |c| Ok(c.maint_info(address).ok().filter(|i| i.supported()).map_or(0, |i| i.read_caps)))?;
        if caps & (1 << openbms_proto::maint::CAP_HISTORY) == 0 {
            return Err(HISTORY_NO_CAP.into());
        }
    }
    // the board the records belong to, proven now under the pack's reservation (the parameter read's rule); without an
    // installation nothing is archived anyway
    let before = link.installation.is_some().then(|| archive_identity(&state, &link, bus, address));
    state.memory_cancel.store(false, Ordering::SeqCst);
    let (conn, flashing, cancel) = (state.conn.clone(), state.flashing.clone(), state.memory_cancel.clone());
    let read_link = link.clone();
    // the reservations come back with the result: they stay held until the records are archived
    let (mut read, _busy, _pack) = tauri::async_runtime::spawn_blocking(move || {
        let r = read_all(
            address,
            &cancel,
            |next| with_link(&conn, &flashing, &links, &read_link, |c| c.history_record(address, next)),
            |n| {
                let _ = app.emit("bms-history-progress", Progress { address, n });
            },
        );
        (r, busy, pack)
    })
    .await
    .map_err(|e| e.to_string())?;
    if read.records.is_empty() {
        return Ok(read);
    }
    let Some(before) = before else {
        read.archive_note = Some("view-only");
        return Ok(read);
    };
    // the same board after the read as before it (a swap during a long read is not archived under the old one)
    let after = before.as_ref().and_then(|_| archive_identity(&state, &link, bus, address));
    let Some(serial) = archive_serial(before, after) else {
        read.archive_note = Some("unchecked");
        return Ok(read);
    };
    let raws: Vec<&[u8]> = read.records.iter().map(|r| r.raw.as_slice()).collect();
    match for_link_checked(&state, &link, "history archive", |s, inst| s.add_bms_records(inst, address, serial.as_deref(), &raws, now_ms())) {
        Some(n) => read.archived = Some(n),
        None => read.archive_note = Some(if link_now(&state).gen != link.gen { "unchecked" } else { "error" }),
    }
    Ok(read)
}

/// The serial a read is archived under: the identity proven before and after it, only when both agree (None: not
/// archived). An inner None is a pack without a serial to compare (RS485-1/2).
fn archive_serial(before: Option<Option<String>>, after: Option<Option<String>>) -> Option<Option<String>> {
    match (before, after) {
        (Some(b), Some(a)) if b == a => Some(b),
        _ => None,
    }
}

/// The board a history read of `address` may be archived for: Some(serial) for a board proven now, Some(None) for a
/// pack on RS485-1/2 without a serial to compare (the parameter read keeps its snapshot then too), None otherwise
/// (unconfirmed, another board, no answer). RS485-1/2: one 0xA4 (`bank::check`); CAN socket: the binary String 2 of
/// a strictly known board (`uart1_id::known_serial`).
fn archive_identity(state: &AppState, link: &Link, bus: Bus, address: u8) -> Option<Option<String>> {
    match bus {
        Bus::CanSocketBus => crate::uart1_id::known_serial(state, link, bus, address).map(Some),
        Bus::PackBus => {
            let checked = crate::bank::check(state, link, address);
            crate::bank::snapshot_allowed(&checked).then(|| checked.ok().and_then(|(_, read)| crate::maint_store::real(&read).map(String::from)))
        }
    }
}

/// One archived record: the record, the String 2 it was kept for and when it was first read (host time, ms).
#[derive(Serialize, Clone, Debug)]
pub struct ArchivedRecord {
    #[serde(flatten)]
    pub record: HistoryRecord,
    pub bms_serial: Option<String>,
    pub read_at: i64,
}

/// The places of `installation` with archived history (no connection needed).
#[tauri::command]
pub async fn bms_archive_packs(state: State<'_, AppState>, installation: String) -> Result<Vec<crate::maint_store::ArchivePack>, String> {
    crate::maint::with_store(&state, |s| s.bms_archive_packs(&installation)).map_err(|e| format!("{e:?}"))
}

/// The archived history of one place of `installation`, sorted by the BMS clock in the records (ties: as kept); a
/// stored row that does not parse is left out (`skipped`), never the whole place.
#[tauri::command]
pub async fn bms_archive(state: State<'_, AppState>, installation: String, address: u8) -> Result<Archive, String> {
    let rows = crate::maint::with_store(&state, |s| s.bms_archive(&installation, address)).map_err(|e| format!("{e:?}"))?;
    Ok(archive_of(rows))
}

#[derive(Serialize, Debug)]
pub struct Archive {
    pub records: Vec<ArchivedRecord>,
    pub skipped: usize,
}

fn archive_of(rows: Vec<crate::maint_store::ArchivedRow>) -> Archive {
    let mut skipped = 0;
    let mut records: Vec<ArchivedRecord> = rows
        .into_iter()
        .filter_map(|(bms_serial, raw, read_at)| match openbms_proto::memory::record_from_raw(&raw) {
            Ok(record) => Some(ArchivedRecord { record, bms_serial, read_at }),
            Err(_) => {
                skipped += 1;
                None
            }
        })
        .collect();
    // stable: equal clock fields keep the order kept
    records.sort_by_key(|r| { let t = r.record.time; (t.year, t.month, t.day, t.hour, t.minute, t.second) });
    Archive { records, skipped }
}

#[tauri::command]
pub fn bms_history_cancel(state: State<AppState>) {
    state.memory_cancel.store(true, Ordering::SeqCst);
}

/// Write a CSV built by the UI to the path the user picked in the save dialog.
#[tauri::command]
pub fn save_bms_history(path: String, content: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("csv")) != Some(true) {
        return Err(format!("not a .csv file: {path}"));
    }
    std::fs::write(p, content).map_err(|e| format!("{path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbms_proto::memory;

    fn record(last: bool) -> (HistoryRecord, bool) {
        let mut info = vec![0x01, u8::from(last)];
        info.extend(2026u16.to_be_bytes());
        info.resize(memory::HISTORY_LEN, 0);
        memory::parse_history(&info).unwrap().unwrap()
    }

    #[test]
    fn a_read_is_archived_only_for_the_same_proven_board_before_and_after() {
        let s = |v: &str| Some(Some(v.to_string()));
        assert_eq!(archive_serial(s("SYN-1"), s("SYN-1")), s("SYN-1"));
        assert_eq!(archive_serial(Some(None), Some(None)), Some(None), "no serial to compare, both times");
        // swapped during the read, not proven before or after, unconfirmed: nothing kept
        assert_eq!(archive_serial(s("SYN-1"), s("SYN-2")), None);
        assert_eq!(archive_serial(None, s("SYN-1")), None);
        assert_eq!(archive_serial(s("SYN-1"), None), None);
        assert_eq!(archive_serial(Some(None), s("SYN-1")), None);
    }

    #[test]
    fn the_archive_is_sorted_by_the_bms_clock_and_skips_unreadable_rows() {
        let rec = |month: u8, minute: u8| {
            let mut r = vec![0x01, 0x07, 0xEA, month, 9, 7, minute, 0];
            r.resize(openbms_proto::memory::RECORD_LEN, 0);
            r
        };
        let a = archive_of(vec![(None, rec(4, 0), 1), (Some("SYN-1".into()), rec(3, 5), 2), (None, vec![1, 2, 3], 3), (None, rec(3, 5), 4)]);
        assert_eq!(a.skipped, 1);
        let order: Vec<(u8, i64)> = a.records.iter().map(|r| (r.record.time.month, r.read_at)).collect();
        assert_eq!(order, vec![(3, 2), (3, 4), (4, 1)], "ties keep the order kept");
    }

    #[test]
    fn refusals_become_json_other_errors_stay() {
        use openbms_transport::Error as E;
        let refused = refusable::<()>(Err(E::Proto(openbms_proto::Error::Rtn(0x06)))).unwrap();
        assert_eq!(refused.map_err(refusal_error), Err(r#"{"kind":"bms-refused","rtn":"refused"}"#.to_string()));
        let invalid = refusable::<()>(Err(E::Proto(openbms_proto::Error::Rtn(0x04)))).unwrap();
        assert_eq!(invalid.map_err(refusal_error), Err(r#"{"kind":"bms-refused","rtn":"invalid"}"#.to_string()));
        assert!(refusable::<()>(Err(E::Proto(openbms_proto::Error::Rtn(0x02)))).is_err());
        assert!(refusable::<()>(Err(E::Closed)).is_err());
        assert_eq!(refusable(Ok(5)).unwrap(), Ok(5));
    }

    #[test]
    fn a_lost_write_answer_is_decided_by_a_fresh_read() {
        let v = |p0: u16| Values { raws: vec![p0, 2], switches: [0; 8] };
        let (sent, before) = (v(3650), v(3600));
        assert_eq!(after_lost(Some(&v(3650)), &sent, &before), AfterLost::Written);
        assert_eq!(after_lost(Some(&v(3600)), &sent, &before), AfterLost::Unreached);
        assert_eq!(after_lost(Some(&v(3500)), &sent, &before), AfterLost::Unclear);
        assert_eq!(after_lost(None, &sent, &before), AfterLost::Unclear);
        // only a confirmed write keeps a snapshot, and never a `read`
        assert_eq!(AfterLost::Written.snapshot(), Some(SnapshotKind::Written));
        assert_eq!((AfterLost::Unreached.snapshot(), AfterLost::Unclear.snapshot()), (None, None));
        let j = serde_json::to_value(ParamWrite::Unclear { error: "timeout".into() }).unwrap();
        assert_eq!(j, serde_json::json!({ "kind": "unclear", "error": "timeout" }));
    }

    /// A synthetic block of `address`: P0..P59 = 3000 + i, P60.. = i, switches 0, name "SYN".
    fn block(address: u8) -> Vec<u8> {
        let mut b = vec![address, 0x3C];
        for i in 0..60u16 {
            b.extend((3000 + i).to_be_bytes());
        }
        b.push(0x1B);
        b.extend(60..87u8);
        b.push(0x08);
        b.extend([0u8; 8]);
        b.extend(b"SYN       ");
        b
    }

    /// The exchanges of a write as a script: the reads in order, the send's outcome, what was kept.
    struct Fake {
        uart1: bool,
        reads: Vec<Result<Parameters, String>>,
        sent: Option<Sent>,
        blocks: Vec<Vec<u8>>,
        kept: Vec<SnapshotKind>,
    }

    impl Fake {
        fn new(uart1: bool, reads: Vec<Result<Vec<u8>, &str>>, sent: Sent) -> Self {
            let reads = reads.into_iter().rev().map(|r| r.map(|b| Parameters::parse(&b).unwrap()).map_err(String::from)).collect();
            Fake { uart1, reads, sent: Some(sent), blocks: vec![], kept: vec![] }
        }
    }

    impl ParamIo for Fake {
        fn read(&mut self, keep: bool) -> Result<Parameters, String> {
            let r = self.reads.pop().expect("an unexpected read");
            if let (true, Ok(p)) = (keep, &r) {
                self.snapshot(SnapshotKind::Read, &p.block.clone());
            }
            r
        }
        fn send(&mut self, block: &[u8]) -> Result<Sent, String> {
            self.blocks.push(block.to_vec());
            Ok(self.sent.take().expect("sent twice"))
        }
        fn snapshot(&mut self, kind: SnapshotKind, _block: &[u8]) {
            self.kept.push(kind);
        }
        fn uart1(&self) -> bool {
            self.uart1
        }
    }

    fn edit(b: &[u8], p0: u16) -> (Values, Values) {
        let base = Parameters::parse(b).unwrap().values();
        let mut edited = base.clone();
        edited.raws[0] = p0;
        (base, edited)
    }

    fn with_p0(b: &[u8], p0: u16) -> Vec<u8> {
        let mut out = b.to_vec();
        out[2..4].copy_from_slice(&p0.to_be_bytes());
        out
    }

    #[test]
    fn a_name_write_sends_the_fresh_block_with_only_the_name() {
        let b = block(4);
        let named = openbms_proto::params::with_name(&b, "PACK-X").unwrap();
        let mut f = Fake::new(false, vec![Ok(b.clone()), Ok(named.clone())], Sent::Ack);
        let r = name_checked(&mut f, 4, "SYN", "PACK-X").unwrap();
        assert!(matches!(&r, NameWrite::Written { read_back: Some(p), .. } if p.device_name == "PACK-X"));
        assert_eq!(f.blocks, std::slice::from_ref(&named));
        assert_eq!(f.kept, [SnapshotKind::Read, SnapshotKind::Written, SnapshotKind::Read]);
        // another name on the pack than the one shown, the same name, a name the block cannot hold: nothing sent
        let mut f = Fake::new(false, vec![Ok(b.clone())], Sent::Ack);
        assert!(matches!(name_checked(&mut f, 4, "OTHER", "PACK-X").unwrap(), NameWrite::Stale { .. }));
        let mut f = Fake::new(false, vec![Ok(b.clone())], Sent::Ack);
        assert!(matches!(name_checked(&mut f, 4, "SYN", "SYN").unwrap(), NameWrite::Unchanged { .. }));
        let mut f = Fake::new(false, vec![], Sent::Ack);
        assert!(name_checked(&mut f, 4, "SYN", "ZU-LANGER-NAME").is_err());
        let mut f = Fake::new(false, vec![Ok(block(5))], Sent::Ack);
        assert!(name_checked(&mut f, 4, "SYN", "PACK-X").is_err());
        assert!(f.blocks.is_empty());
    }

    #[test]
    fn a_lost_name_answer_is_decided_by_the_whole_block() {
        let b = block(4);
        let named = openbms_proto::params::with_name(&b, "PACK-X").unwrap();
        let lost = || Sent::Lost("timeout".into());
        // the new name read back after a lost answer: a refusal leaves the same RAM, so open and no `written` kept
        let mut f = Fake::new(false, vec![Ok(b.clone()), Ok(named.clone())], lost());
        assert!(matches!(name_checked(&mut f, 4, "SYN", "PACK-X").unwrap(), NameWrite::Unclear { read_back: Some(p), .. } if p.device_name == "PACK-X"));
        assert_eq!(f.kept, [SnapshotKind::Read]);
        let mut f = Fake::new(false, vec![Ok(b.clone()), Ok(b.clone())], lost());
        assert!(name_checked(&mut f, 4, "SYN", "PACK-X").is_err());
        let mut f = Fake::new(false, vec![Ok(b.clone()), Ok(with_p0(&named, 3001))], lost());
        assert!(matches!(name_checked(&mut f, 4, "SYN", "PACK-X").unwrap(), NameWrite::Unclear { read_back: Some(_), .. }));
        let mut f = Fake::new(false, vec![Ok(b.clone()), Err("timeout")], lost());
        assert!(matches!(name_checked(&mut f, 4, "SYN", "PACK-X").unwrap(), NameWrite::Unclear { read_back: None, .. }));
        let mut f = Fake::new(false, vec![Ok(b.clone())], Sent::Refused { refusal: Refusal::Refused, switches_live: true });
        assert!(matches!(name_checked(&mut f, 4, "SYN", "PACK-X").unwrap(), NameWrite::Refused { .. }));
        assert_eq!(f.kept, [SnapshotKind::Read, SnapshotKind::Refused]);
    }

    #[test]
    fn both_paths_write_and_read_back_uart1_keeps_written_only_after_the_read_back() {
        for adr in [0u8, 1, 11] {
            let b = block(adr);
            let (base, edited) = edit(&b, 3650);
            let mut f = Fake::new(false, vec![Ok(b.clone()), Ok(with_p0(&b, 3650))], Sent::Ack);
            let r = write_checked(&mut f, adr, base.clone(), edited.clone(), false, crate::profile::group0_rule(false, None)).unwrap();
            assert!(matches!(&r, ParamWrite::Written { read_back: Some(p), unanswered: false, before: None, .. } if p.address == adr));
            assert_eq!(f.kept, [SnapshotKind::Read, SnapshotKind::Written, SnapshotKind::Read]);
            assert_eq!(f.blocks, [with_p0(&b, 3650)]);
            let mut f = Fake::new(true, vec![Ok(b.clone()), Ok(with_p0(&b, 3650))], Sent::Ack);
            let r = write_checked(&mut f, adr, base, edited, false, crate::profile::group0_rule(false, None)).unwrap();
            assert!(matches!(&r, ParamWrite::Written { read_back: Some(_), unanswered: false, before: Some(p), .. } if p.block == b));
            assert_eq!(f.kept, [SnapshotKind::Read, SnapshotKind::Written, SnapshotKind::Read]);
            // the block sent carries the pack's own address
            assert_eq!(f.blocks[0][0], adr);
        }
    }

    #[test]
    fn a_changed_lock_state_stops_the_write_and_a_restore_keeps_it() {
        let with_g0 = |g0: u8| {
            let mut b = block(2);
            b[crate::profile::GROUP0] = g0;
            b
        };
        // the column read without a cutoff (bit 5 at 1), the pack locked meanwhile; the edit switches the extended off on
        // and even carries bit 5 as changed: nothing goes out, the answer names the lock
        let base = Parameters::parse(&with_g0(0xFF)).unwrap().values();
        for g0 in [0xEF, 0xCF] {
            let mut edited = base.clone();
            edited.switches[0] = g0;
            let mut f = Fake::new(true, vec![Ok(with_g0(0xDF))], Sent::Ack);
            let r = write_checked(&mut f, 2, base.clone(), edited, false, crate::profile::group0_rule(false, None)).unwrap();
            assert!(matches!(&r, ParamWrite::Stale { lock_changed: true, fresh } if fresh.function_switches[0] == 0xDF), "{g0:02X}");
            assert!(f.blocks.is_empty());
            assert_eq!(serde_json::to_value(&r).unwrap()["lock_changed"], true);
        }
        // another change of the pack is stale without the lock
        let mut edited = base.clone();
        edited.raws[0] = 3650;
        let mut f = Fake::new(false, vec![Ok(with_p0(&with_g0(0xFE), 3001))], Sent::Ack);
        assert!(matches!(write_checked(&mut f, 2, base.clone(), edited, false, crate::profile::group0_rule(false, None)).unwrap(), ParamWrite::Stale { lock_changed: false, .. }));
        // restoring an older block (bit 5 at 1) over a locked pack: sent, with the lock as the pack has it
        let mut f = Fake::new(false, vec![Ok(with_g0(0xDF)), Ok(with_g0(0xDF))], Sent::Ack);
        let stored = base.clone();
        write_checked(&mut f, 2, stored.clone(), stored, true, crate::profile::group0_rule(false, None)).unwrap();
        assert_eq!(f.blocks[0][crate::profile::GROUP0], 0xDF);
    }

    #[test]
    fn e2_is_a_refusal_without_live_switches_rtn06_keeps_them() {
        let b = block(3);
        let (base, edited) = edit(&b, 3650);
        let mut f = Fake::new(true, vec![Ok(b.clone())], Sent::Refused { refusal: Refusal::Refused, switches_live: false });
        let r = write_checked(&mut f, 3, base.clone(), edited.clone(), false, crate::profile::group0_rule(false, None)).unwrap();
        assert!(matches!(r, ParamWrite::Refused { refusal: Refusal::Refused, switches_live: false, .. }));
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!((j["kind"].as_str(), j["switches_live"].as_bool()), (Some("refused"), Some(false)));
        let mut f = Fake::new(false, vec![Ok(b.clone())], Sent::Refused { refusal: Refusal::Refused, switches_live: true });
        let r = write_checked(&mut f, 3, base, edited, false, crate::profile::group0_rule(false, None)).unwrap();
        assert!(matches!(r, ParamWrite::Refused { switches_live: true, .. }));
        assert_eq!(f.kept, [SnapshotKind::Read, SnapshotKind::Refused]);
    }

    #[test]
    fn a_protocol_error_reports_and_reads_nothing_more() {
        let b = block(3);
        let (base, edited) = edit(&b, 3650);
        let e = r#"{"code":135,"error":"protocol","kind":"uart1"}"#.to_string();
        let mut f = Fake::new(true, vec![Ok(b)], Sent::Protocol(e.clone()));
        assert_eq!(write_checked(&mut f, 3, base, edited, false, crate::profile::group0_rule(false, None)).unwrap_err(), e);
        assert!(f.reads.is_empty());
    }

    #[test]
    fn uart1_acknowledged_but_another_read_back_is_unclear_with_what_the_pack_holds() {
        let b = block(2);
        let (base, edited) = edit(&b, 3650);
        let mut f = Fake::new(true, vec![Ok(b.clone()), Ok(with_p0(&b, 3640))], Sent::Ack);
        let r = write_checked(&mut f, 2, base, edited, false, crate::profile::group0_rule(false, None)).unwrap();
        match &r {
            ParamWrite::UnclearRead { error, read_back: Some(p), before } => {
                assert_eq!(error, &uart1_refusal("readback"));
                assert_eq!(p.parameters[0].raw, 3640);
                assert_eq!(before.block, b);
            }
            other => panic!("{other:?}"),
        }
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["kind"], "unclear");
        // a different name alone is no difference: it is not written
        let mut other_name = with_p0(&b, 3650);
        other_name[160] = b'X';
        let (base, edited) = edit(&b, 3650);
        let mut f = Fake::new(true, vec![Ok(b.clone()), Ok(other_name)], Sent::Ack);
        assert!(matches!(write_checked(&mut f, 2, base, edited, false, crate::profile::group0_rule(false, None)).unwrap(), ParamWrite::Written { .. }));
    }

    #[test]
    fn uart1_acknowledged_without_a_read_back_is_unclear_ascii_stays_written() {
        let b = block(6);
        let (base, edited) = edit(&b, 3650);
        let mut f = Fake::new(true, vec![Ok(b.clone()), Err("no answer")], Sent::Ack);
        let r = write_checked(&mut f, 6, base.clone(), edited.clone(), false, crate::profile::group0_rule(false, None)).unwrap();
        assert!(matches!(&r, ParamWrite::UnclearRead { read_back: None, before, .. } if before.block == b), "{r:?}");
        assert_eq!(f.kept, [SnapshotKind::Read], "no written");
        let mut f = Fake::new(false, vec![Ok(b.clone()), Err("timeout")], Sent::Ack);
        assert!(matches!(write_checked(&mut f, 6, base, edited, false, crate::profile::group0_rule(false, None)).unwrap(), ParamWrite::Written { read_back: None, .. }));
    }

    #[test]
    fn a_lost_uart1_answer_and_another_read_keeps_that_read() {
        let b = block(7);
        let (base, edited) = edit(&b, 3650);
        let mut f = Fake::new(true, vec![Ok(b.clone()), Ok(with_p0(&b, 3640))], Sent::Lost("timeout".into()));
        let r = write_checked(&mut f, 7, base.clone(), edited.clone(), false, crate::profile::group0_rule(false, None)).unwrap();
        assert!(matches!(&r, ParamWrite::UnclearRead { read_back: Some(p), .. } if p.parameters[0].raw == 3640));
        assert_eq!(f.kept, [SnapshotKind::Read, SnapshotKind::Read], "the read before and the one seen now, no written");
        // RS485-1/2 as before: the comparison read is not kept
        let mut f = Fake::new(false, vec![Ok(b.clone()), Ok(with_p0(&b, 3640))], Sent::Lost("timeout".into()));
        assert!(matches!(write_checked(&mut f, 7, base, edited, false, crate::profile::group0_rule(false, None)).unwrap(), ParamWrite::Unclear { .. }));
        assert_eq!(f.kept, [SnapshotKind::Read]);
    }

    #[test]
    fn a_lost_uart1_answer_with_an_unchanged_block_is_unclear() {
        let b = block(4);
        let same = Parameters::parse(&b).unwrap().values();
        // restore of the block the pack holds: a fresh read cannot tell whether the A1 arrived
        let mut f = Fake::new(true, vec![Ok(b.clone()), Ok(b.clone())], Sent::Lost("no answer within 3s".into()));
        let r = write_checked(&mut f, 4, same.clone(), same.clone(), true, crate::profile::group0_rule(false, None)).unwrap();
        assert!(matches!(r, ParamWrite::UnclearRead { read_back: Some(_), .. }), "{r:?}");
        // the same on RS485-1/2 stays as it was
        let mut f = Fake::new(false, vec![Ok(b.clone()), Ok(b.clone())], Sent::Lost("timeout".into()));
        assert!(matches!(write_checked(&mut f, 4, same.clone(), same, true, crate::profile::group0_rule(false, None)).unwrap(), ParamWrite::Written { unanswered: true, .. }));
        // a changed block: the read decides as before; the values before mean the write did not arrive
        let (base, edited) = edit(&b, 3650);
        let mut f = Fake::new(true, vec![Ok(b.clone()), Ok(with_p0(&b, 3650))], Sent::Lost("timeout".into()));
        assert!(matches!(write_checked(&mut f, 4, base.clone(), edited.clone(), false, crate::profile::group0_rule(false, None)).unwrap(), ParamWrite::Written { unanswered: true, before: Some(_), .. }));
        let mut f = Fake::new(true, vec![Ok(b.clone()), Ok(b.clone())], Sent::Lost("timeout".into()));
        assert_eq!(write_checked(&mut f, 4, base.clone(), edited.clone(), false, crate::profile::group0_rule(false, None)).unwrap_err(), "timeout");
        let mut f = Fake::new(true, vec![Ok(b.clone()), Err("no answer")], Sent::Lost("timeout".into()));
        assert!(matches!(write_checked(&mut f, 4, base, edited, false, crate::profile::group0_rule(false, None)).unwrap(), ParamWrite::UnclearRead { read_back: None, .. }));
    }

    #[test]
    fn a_binary_write_answer_is_taken_refused_a_protocol_error_or_lost() {
        use openbms_transport::Error as E;
        assert_eq!(uart1_sent(Ok(())), Sent::Ack);
        assert_eq!(uart1_sent(Err(E::Proto(openbms_proto::Error::Uart1(0xE2)))), Sent::Refused { refusal: Refusal::Refused, switches_live: false });
        assert_eq!(uart1_sent(Err(E::Proto(openbms_proto::Error::Uart1(0x87)))), Sent::Protocol(r#"{"code":135,"error":"protocol","kind":"uart1"}"#.into()));
        // an answer cut off with a frame inside it is never a refusal: re-read like a lost answer
        assert_eq!(uart1_sent(Err(E::Incomplete)), Sent::Lost("answer incomplete".into()));
        assert!(matches!(uart1_sent(Err(E::Timeout(std::time::Duration::from_secs(3)))), Sent::Lost(_)));
        assert!(matches!(uart1_sent(Err(E::Closed)), Sent::Lost(_)));
    }

    #[test]
    fn the_path_follows_the_bus() {
        assert_eq!(ParamPath::of(Some(Bus::CanSocketBus)), ParamPath::Uart1(Bus::CanSocketBus));
        assert_eq!(ParamPath::of(Some(Bus::PackBus)), ParamPath::Ascii);
        assert_eq!(ParamPath::of(None), ParamPath::Ascii);
    }

    #[test]
    fn an_open_update_of_the_same_installation_and_address_blocks() {
        let p = |endpoint: &str, pack: u8| crate::maint_store::Pending { id: 1, endpoint: endpoint.into(), pack, release: "16.06.13".into(), since: 0, bms_serial: None };
        let eps = vec!["tcp:192.0.2.11:4196".to_string()];
        assert!(pending_blocks(&[p("tcp:192.0.2.11:4196", 3)], &eps, 3));
        assert!(!pending_blocks(&[p("tcp:192.0.2.11:4196", 3)], &eps, 4), "another pack");
        assert!(!pending_blocks(&[p("serial:/dev/cu.other", 3)], &eps, 3), "another installation's port");
        assert!(!pending_blocks(&[], &eps, 3));
    }

    #[test]
    fn only_a_known_other_master_blocks() {
        let set = |v: &str| SerialValue::Set { value: v.into(), truncated: false };
        assert!(master_blocks(Some("SYN-A"), &set("SYN-B")));
        assert!(!master_blocks(Some("SYN-A"), &set("SYN-A")));
        assert!(!master_blocks(Some("SYN-A-LONGER"), &SerialValue::Set { value: "SYN-A".into(), truncated: true }));
        assert!(!master_blocks(None, &set("SYN-B")), "nothing stored to compare");
        for v in [SerialValue::NoAnswer, SerialValue::Unavailable, SerialValue::Empty, SerialValue::Placeholder { value: "EMU110110S-XXXX".into() }, set("")] {
            assert!(!master_blocks(Some("SYN-A"), &v), "{v:?}");
        }
    }

    #[test]
    fn the_full_capacity_is_read_on_both_buses() {
        let state = AppState::default();
        *state.bus.lock().unwrap() = Some(Bus::CanSocketBus);
        // not refused for the bus: it fails here only for the missing connection
        assert_eq!(full_capacity(&state, 2).unwrap_err(), "not connected");
        // the history has a CAN socket way now (read-capability bit 0, checked by `bms_history` from the pack's 64/03)
        assert_eq!(on_bus(Action::ReadHistory, Bus::CanSocketBus), Ok(Channel::Uart1));
        for a in [Action::ReadCalibration, Action::ReadSocStore] {
            assert_eq!((on_bus(a, Bus::CanSocketBus), on_bus(a, Bus::PackBus)), (Ok(Channel::Uart1), Ok(Channel::Uart0)), "{a:?}");
        }
        assert_eq!(on_bus(Action::MaintStart, Bus::PackBus).unwrap_err(), CAN_SOCKET_ONLY);
        for a in [Action::ForceHeater, Action::ClearHistory, Action::SetRemaining, Action::SetLearnedFull, Action::Calibrate, Action::SetCanProtocol, Action::WriteSerials, Action::WriteName] {
            assert_eq!(on_bus(a, Bus::CanSocketBus).unwrap_err(), PACK_BUS_ONLY, "{a:?}");
            assert_eq!(on_bus(a, Bus::PackBus), Ok(Channel::Uart0), "{a:?}");
        }
    }

    #[test]
    fn every_implemented_route_has_a_flow() {
        for r in access::ROUTES.iter().filter(|r| r.implemented) {
            assert!(flows(r.action).contains(&r.channel), "the catalog marks {:?} over {:?} as implemented, the app has no flow for it", r.action, r.channel);
        }
    }

    #[test]
    fn a_write_takes_the_bus_of_its_own_link_never_the_global_one() {
        let state = AppState::default();
        crate::new_link(&state, Some("A".into()));
        let link = link_now(&state);
        crate::bank::begin(&state, link.gen, false, None);
        crate::bank::set_profile(&state, link.gen, crate::profile::of("tcp:gw:4196", Bus::CanSocketBus, None));
        // the global bus still says RS485-1/2 (read before a reconnect): the link's own connection decides
        *state.bus.lock().unwrap() = Some(Bus::PackBus);
        let (l, bus) = bound_bus(&state, Some("A")).unwrap();
        assert_eq!((l.gen, bus), (link.gen, Bus::CanSocketBus));
        for a in [Action::PowerOnOff, Action::Restart, Action::ReadClock, Action::SetClock] {
            assert_eq!(crate::power::way(a, bus), Ok(crate::power::Way::Power), "ein/aus, restart and the clock go through power::run");
        }
        assert_eq!(on_bus(Action::ForceHeater, bus).unwrap_err(), PACK_BUS_ONLY);
        // a new connection whose profile is not set yet: nothing is decided for it
        crate::new_link(&state, Some("A".into()));
        assert_eq!(bound_bus(&state, Some("A")).unwrap_err(), "not connected");
        let gen = link_now(&state).gen;
        crate::bank::begin(&state, gen, true, None);
        crate::bank::set_profile(&state, gen, crate::profile::of("serial:/dev/cu.synthetic", Bus::PackBus, None));
        *state.bus.lock().unwrap() = Some(Bus::CanSocketBus);
        assert_eq!(bound_bus(&state, Some("A")).unwrap().1, Bus::PackBus);
        assert_eq!(crate::power::way(Action::Restart, Bus::PackBus), Ok(crate::power::Way::Ascii));
        // „Nur ansehen“ or another installation writes nothing
        assert!(bound_bus(&state, Some("B")).is_err());
    }

    #[test]
    fn a_failed_read_back_is_still_a_set() {
        let s = CanProtocolSet::from_read(Err("timeout".into()));
        assert!(s.device.is_none());
        assert_eq!(s.read_error.as_deref(), Some("timeout"));
        let d = DeviceInfo { device_name: "SYN".into(), firmware_version: "16.06".into(), can_protocol: "CAN:Victron".into() };
        let s = CanProtocolSet::from_read(Ok(d.clone()));
        assert_eq!((s.device, s.read_error), (Some(d), None));
    }

    #[test]
    fn the_stored_master_serial_follows_only_from_the_old_value() {
        let inst = |id: &str, serial: Option<&str>| Installation { id: id.into(), name: id.into(), endpoints: vec![], control: None, master_serial: serial.map(Into::into), archived: false, legacy: None, last_connect: None };
        let set = |v: &str| SerialValue::Set { value: v.into(), truncated: false };
        let all = [inst("a", Some("SN-OLD")), inst("b", Some("SN-OTHER")), inst("c", None)];
        let ids = |v: Vec<&Installation>| v.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(follow_master_serial(&all, &set("SN-OLD"), &set("SN-NEW"))), ["a"]);
        assert!(follow_master_serial(&all, &set("SN-OLD"), &set("SN-OLD")).is_empty());
        assert!(follow_master_serial(&all, &SerialValue::Empty, &set("SN-NEW")).is_empty());
        assert!(follow_master_serial(&all, &set("SN-OLD"), &SerialValue::Empty).is_empty());
        assert!(follow_master_serial(&all, &SerialValue::Placeholder { value: "EMU110110S-XXXX".into() }, &set("SN-NEW")).is_empty());
    }

    #[test]
    fn stops_on_the_last_flag_and_asks_oldest_then_next() {
        let mut asked = Vec::new();
        let mut seen = 0;
        let r = read_all(1, &AtomicBool::new(false), |next| { asked.push(next); Ok(Some(record(asked.len() == 3))) }, |n| seen = n);
        assert_eq!((r.end, r.records.len(), seen), (End::Last, 3, 3));
        assert_eq!(asked, [false, true, true]);
    }

    #[test]
    fn empty_error_cancel_and_cap() {
        let r = read_all(1, &AtomicBool::new(false), |_| Ok(None), |_| {});
        assert_eq!((r.end, r.records.len()), (End::Empty, 0));

        let mut n = 0;
        let r = read_all(1, &AtomicBool::new(false), |_| { n += 1; if n < 3 { Ok(Some(record(false))) } else { Err("timeout".into()) } }, |_| {});
        assert_eq!((r.end, r.records.len(), r.error.as_deref(), r.error_kind), (End::Error, 2, Some("timeout"), Some("other")));
        // the pack's EEPROM error (its read index restored) and silence (the index may have moved) are told apart; the
        // loop stops at the first error and never asks again
        let mut asked = 0;
        let r = read_all(1, &AtomicBool::new(false), |_| { asked += 1; Err("protocol: BMS returned RTN 0x08".into()) }, |_| {});
        assert_eq!((r.error_kind, asked), (Some("eeprom"), 1));
        let r = read_all(1, &AtomicBool::new(false), |_| Err("no answer within 1.5s".into()), |_| {});
        assert_eq!(r.error_kind, Some("silent"));

        let cancel = AtomicBool::new(false);
        let r = read_all(1, &cancel, |_| Ok(Some(record(false))), |n| if n == 4 { cancel.store(true, Ordering::SeqCst) });
        assert_eq!((r.end, r.records.len()), (End::Cancelled, 4));

        let r = read_all(1, &AtomicBool::new(false), |_| Ok(Some(record(false))), |_| {});
        assert_eq!((r.end, r.records.len()), (End::Cap, MAX_RECORDS));
    }
}
