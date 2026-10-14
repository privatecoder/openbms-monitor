//! The BMS clock and the pack's internal history (RS485-1/2 only, like the parameters).
//!
//! A history read runs on its own thread and takes the connection lock per frame, through `with_conn` like every
//! other exchange; meanwhile `memory_busy` pauses polling, so the bus serves the read alone. Between two frames a
//! disconnect, a reconnect or a flash can still take the connection; the read then stops with that error.

use crate::{with_conn, AppState, PortSlot};
use openbms_proto::{BmsTime, HistoryRecord};
use openbms_transport::Bus;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, State};

/// Guard against a device that never sets the last flag: the ring holds at most 500 records.
pub const MAX_RECORDS: usize = 520;

const PACK_BUS_ONLY: &str = "the BMS clock and history can only be read on RS485-1/2";

fn pack_bus_only(state: &AppState) -> Result<(), String> {
    if *state.bus.lock().map_err(|e| e.to_string())? == Some(Bus::CanSocketBus) {
        return Err(PACK_BUS_ONLY.into());
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

/// Read the whole history of one pack; progress arrives as "bms-history-progress" events, the result also as
/// "bms-history-done". One read at a time.
#[tauri::command]
pub async fn bms_history(app: AppHandle, state: State<'_, AppState>, address: u8) -> Result<HistoryRead, String> {
    pack_bus_only(&state)?;
    let busy = crate::reserve_port(&state.memory_busy).ok_or("a history read is already running")?;
    state.memory_cancel.store(false, Ordering::SeqCst);
    let (conn, flashing, cancel) = (state.conn.clone(), state.flashing.clone(), state.memory_cancel.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _busy: PortSlot = busy;
        let read = read_all(
            address,
            &cancel,
            |next| with_conn(&conn, &flashing, |c| c.history_record(address, next)),
            |n| {
                let _ = app.emit("bms-history-progress", Progress { address, n });
            },
        );
        // for a UI that lost track of this read (e.g. reloaded) and waits for its end
        let _ = app.emit("bms-history-done", read.clone());
        read
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
