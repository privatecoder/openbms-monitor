//! The BMS clock, the pack's internal history, calibration, the CAN inverter protocol, writing parameters and serial
//! numbers (RS485-1/2 only, like reading the parameters).
//!
//! A history read runs on its own thread and takes the connection lock per frame, through `with_conn` like every
//! other exchange; meanwhile `memory_busy` pauses polling, so the bus serves the read alone. Between two frames a
//! disconnect, a reconnect or a flash can still take the connection; the read then stops with that error.

use crate::history::{now_ms, SnapshotKind};
use crate::maint_store::Installation;
use crate::{with_conn, AppState, PortSlot};
use openbms_proto::serial::Which;
use openbms_proto::{BmsTime, Calibration, DeviceInfo, HistoryRecord, Parameters, Refusal, SerialValue, Status, Telemetry, Values};
use openbms_transport::Bus;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, State};

/// Guard against a device that never sets the last flag: the ring holds at most 500 records.
pub const MAX_RECORDS: usize = 520;

const PACK_BUS_ONLY: &str = "this command exists only on RS485-1/2";

fn pack_bus_only(state: &AppState) -> Result<(), String> {
    if *state.bus.lock().map_err(|e| e.to_string())? == Some(Bus::CanSocketBus) {
        return Err(PACK_BUS_ONLY.into());
    }
    Ok(())
}

/// Parameter and serial number writes wait while the history database is being prepared: the snapshot of the state
/// before a write would be lost. Reads go on without a snapshot.
fn history_ready(state: &AppState) -> Result<(), String> {
    if state.history.lock().map_err(|e| e.to_string())?.preparing() {
        return Err("the recording is being updated; writing is possible once that is done".into());
    }
    Ok(())
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
    HistoryRead { address, records, end, error }
}

#[tauri::command]
pub async fn bms_clock(state: State<'_, AppState>, address: u8) -> Result<BmsTime, String> {
    pack_bus_only(&state)?;
    with_conn(&state.conn, &state.flashing, |c| c.clock(address))
}

/// Set the clock to `time` (the PC's local time, chosen by the UI) and read it back.
#[tauri::command]
pub async fn set_bms_clock(state: State<'_, AppState>, address: u8, time: BmsTime) -> Result<BmsTime, String> {
    pack_bus_only(&state)?;
    with_conn(&state.conn, &state.flashing, |c| {
        c.set_clock(address, time)?;
        c.clock(address)
    })
}

/// A write the BMS answered with RTN 06/04, as JSON for the UI (`refusal` in calibration.ts):
/// `{"kind":"refused"}` or `{"kind":"invalid"}`. Every other error stays plain text.
fn refusal_error(r: Refusal) -> String {
    serde_json::json!({ "kind": r }).to_string()
}

/// Run a write; a refusal by the BMS comes back as `Err(Refusal)` instead of a plain error.
fn refusable<T>(r: openbms_transport::Result<T>) -> openbms_transport::Result<Result<T, Refusal>> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(openbms_transport::Error::Proto(e)) => Refusal::from_error(&e).map(Err).ok_or_else(|| e.into()),
        Err(e) => Err(e),
    }
}

/// Telemetry and status of one pack, for the live values of the calibration (one frame per lock).
#[derive(Serialize)]
pub struct PackLive {
    pub telemetry: Telemetry,
    pub status: Status,
}

#[tauri::command]
pub async fn pack_live(state: State<'_, AppState>, address: u8) -> Result<PackLive, String> {
    pack_bus_only(&state)?;
    let telemetry = with_conn(&state.conn, &state.flashing, |c| c.telemetry(address))?;
    let status = with_conn(&state.conn, &state.flashing, |c| c.status(address))?;
    Ok(PackLive { telemetry, status })
}

/// One calibration step (0xA0). The UI reads the live values again afterwards.
#[tauri::command]
pub async fn bms_calibrate(state: State<'_, AppState>, address: u8, calibration: Calibration) -> Result<(), String> {
    pack_bus_only(&state)?;
    with_conn(&state.conn, &state.flashing, |c| refusable(c.calibrate(address, calibration)))?.map_err(refusal_error)
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
pub async fn set_can_protocol(state: State<'_, AppState>, address: u8, protocol: u8) -> Result<CanProtocolSet, String> {
    pack_bus_only(&state)?;
    with_conn(&state.conn, &state.flashing, |c| refusable(c.set_can_protocol(address, protocol)))?.map_err(refusal_error)?;
    Ok(CanProtocolSet::from_read(with_conn(&state.conn, &state.flashing, |c| c.device_info(address))))
}

/// Keep a parameter block in the history database. A failure there must not fail the exchange with the pack.
pub fn snapshot(state: &AppState, address: u8, kind: SnapshotKind, block: &[u8]) {
    if let Ok(mut h) = state.history.lock() {
        if let Err(e) = h.snapshot(now_ms(), address, kind, block) {
            eprintln!("parameter snapshot: {e}");
        }
    }
}

/// A 0x47 read through the connection, kept as a snapshot.
fn read_parameters(state: &AppState, address: u8) -> Result<Parameters, String> {
    let p = with_conn(&state.conn, &state.flashing, |c| c.parameters(address))?;
    snapshot(state, address, SnapshotKind::Read, &p.block);
    Ok(p)
}

/// Outcome of a parameter write.
#[derive(Serialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParamWrite {
    /// The pack changed a value the user did not touch since the edit's read: nothing was written; `fresh` is the
    /// pack now.
    Stale { fresh: Parameters },
    /// Written; the block read back, or why that read failed.
    Written { read_back: Option<Parameters>, read_error: Option<String> },
    /// The BMS did not take the block (RTN 06 plausibility, RTN 04 markers). `stored` is the read right before the
    /// write: after RTN 06 the switches sent are live, these are the stored ones.
    Refused { refusal: Refusal, stored: Parameters },
}

/// Write one pack's parameters (0xA1): read 0x47 again, put the user's changes (`edited` against `base`, the read
/// the edit started from) on top of it, send, read back. `restore` sends `edited` as it is on top of the fresh read
/// (the stored block after a refusal); the rebase would take the live switches for a change of the pack.
#[tauri::command]
pub async fn write_parameters(state: State<'_, AppState>, address: u8, base: Values, edited: Values, restore: bool) -> Result<ParamWrite, String> {
    pack_bus_only(&state)?;
    history_ready(&state)?;
    let fresh = read_parameters(&state, address)?;
    // byte 0 is the pack's own address: a block from another pack must never be written here
    if fresh.address != address {
        return Err(format!("the pack at address {address} answered with address {}", fresh.address));
    }
    let values = if restore { edited } else {
        match Values::rebase(&base, &edited, &fresh.values()) {
            Some(v) => v,
            None => return Ok(ParamWrite::Stale { fresh }),
        }
    };
    let block = values.apply(&fresh.block).map_err(|e| e.to_string())?;
    if let Err(refusal) = with_conn(&state.conn, &state.flashing, |c| refusable(c.write_parameters(address, &block)))? {
        snapshot(&state, address, SnapshotKind::Refused, &block);
        return Ok(ParamWrite::Refused { refusal, stored: fresh });
    }
    snapshot(&state, address, SnapshotKind::Written, &block);
    Ok(match read_parameters(&state, address) {
        Ok(p) => ParamWrite::Written { read_back: Some(p), read_error: None },
        Err(e) => ParamWrite::Written { read_back: None, read_error: Some(e) },
    })
}

/// The last write of a pack at this installation and the parameters it replaced, for „Vorherigen Stand“.
#[derive(Serialize, Debug)]
pub struct ParamHistory {
    pub last_written: Option<i64>,
    pub previous: Option<Parameters>,
}

#[tauri::command]
pub fn param_history(state: State<AppState>, address: u8) -> Result<ParamHistory, String> {
    let w = state.history.lock().map_err(|e| e.to_string())?.last_write(address)?;
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
#[derive(Serialize, Debug)]
pub struct SerialSet {
    pub value: Option<SerialValue>,
    pub read_error: Option<String>,
    pub master_updated: Vec<String>,
    pub master_unchecked: bool,
}

/// Write a serial number (0xA3 battery, 0xA5 BMS) and read it back. String 2 of the master (address 0): an
/// installation of this endpoint that stored the old value gets the new one. Device and database are not updated
/// atomically; a crash in between shows as a bank mismatch, which „Seriennummer übernehmen“ resolves.
#[tauri::command]
pub async fn set_serial(state: State<'_, AppState>, address: u8, which: Which, text: String) -> Result<SerialSet, String> {
    pack_bus_only(&state)?;
    history_ready(&state)?;
    let master = which == Which::Bms && address == 0;
    // String 2 before the write: whether the board keeps its identity under the new serial depends on it
    let old = if which == Which::Bms { with_conn(&state.conn, &state.flashing, |c| c.serial(address, which)).ok() } else { None };
    with_conn(&state.conn, &state.flashing, |c| refusable(c.set_serial(address, which, &text)))?.map_err(refusal_error)?;
    let read = with_conn(&state.conn, &state.flashing, |c| c.serial(address, which));
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
    Ok(match read {
        Ok(v) => SerialSet { value: Some(v), read_error: None, master_updated, master_unchecked },
        Err(e) => SerialSet { value: None, read_error: Some(e), master_updated, master_unchecked },
    })
}

/// Read the whole history of one pack; progress arrives as "bms-history-progress" events. One read at a time.
#[tauri::command]
pub async fn bms_history(app: AppHandle, state: State<'_, AppState>, address: u8) -> Result<HistoryRead, String> {
    pack_bus_only(&state)?;
    let busy = crate::reserve_port(&state.memory_busy).ok_or("a history read is already running")?;
    state.memory_cancel.store(false, Ordering::SeqCst);
    let (conn, flashing, cancel) = (state.conn.clone(), state.flashing.clone(), state.memory_cancel.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _busy: PortSlot = busy;
        read_all(
            address,
            &cancel,
            |next| with_conn(&conn, &flashing, |c| c.history_record(address, next)),
            |n| {
                let _ = app.emit("bms-history-progress", Progress { address, n });
            },
        )
    })
    .await
    .map_err(|e| e.to_string())
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
    fn refusals_become_json_other_errors_stay() {
        use openbms_transport::Error as E;
        let refused = refusable::<()>(Err(E::Proto(openbms_proto::Error::Rtn(0x06)))).unwrap();
        assert_eq!(refused.map_err(refusal_error), Err(r#"{"kind":"refused"}"#.to_string()));
        let invalid = refusable::<()>(Err(E::Proto(openbms_proto::Error::Rtn(0x04)))).unwrap();
        assert_eq!(invalid.map_err(refusal_error), Err(r#"{"kind":"invalid"}"#.to_string()));
        assert!(refusable::<()>(Err(E::Proto(openbms_proto::Error::Rtn(0x02)))).is_err());
        assert!(refusable::<()>(Err(E::Closed)).is_err());
        assert_eq!(refusable(Ok(5)).unwrap(), Ok(5));
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
        let inst = |id: &str, serial: Option<&str>| Installation { id: id.into(), name: id.into(), endpoints: vec![], control: None, master_serial: serial.map(Into::into), archived: false };
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
        assert_eq!((r.end, r.records.len(), r.error.as_deref()), (End::Error, 2, Some("timeout")));

        let cancel = AtomicBool::new(false);
        let r = read_all(1, &cancel, |_| Ok(Some(record(false))), |n| if n == 4 { cancel.store(true, Ordering::SeqCst) });
        assert_eq!((r.end, r.records.len()), (End::Cancelled, 4));

        let r = read_all(1, &AtomicBool::new(false), |_| Ok(Some(record(false))), |_| {});
        assert_eq!((r.end, r.records.len()), (End::Cap, MAX_RECORDS));
    }
}
