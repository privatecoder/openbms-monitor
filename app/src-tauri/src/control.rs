//! Pack control (RS485-1/2 only): restart, on/off, heater, clearing history and counters (0x45) and the remaining
//! capacity (0x49), always one pack.
//!
//! `with_conn` holds the bus for one frame only. An action of several frames on one pack (a parameter write, a history
//! read, a restart and its watch) reserves the pack for its whole length, so a restart cannot fall between the 0x47 and
//! the 0xA1 of a write, and clearing cannot cut a running history read short. Actions on different packs still
//! interleave frame by frame.

use crate::bms_memory::{bound, pack_bus_only, refusable, refusal_error};
use crate::{with_conn, AppState};
use openbms_proto::Comeback;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::State;

/// Reserved packs: address → the action that holds it.
pub(crate) type PackBusy = Arc<Mutex<HashMap<u8, &'static str>>>;

/// Frees the pack when dropped.
pub(crate) struct PackGuard {
    busy: PackBusy,
    address: u8,
}

impl Drop for PackGuard {
    fn drop(&mut self) {
        if let Ok(mut m) = self.busy.lock() {
            m.remove(&self.address);
        }
    }
}

/// The refusal while another action holds the pack, as JSON for the UI (`packBusy` in control.ts):
/// `{"kind":"pack-busy","address":…,"action":…}`.
fn pack_busy_error(address: u8, action: &str) -> String {
    serde_json::json!({ "kind": "pack-busy", "address": address, "action": action }).to_string()
}

/// Reserve `address` for `action` ("restart", "on_off", "heater", "clear", "remaining", "params", "history").
pub(crate) fn reserve_pack(busy: &PackBusy, address: u8, action: &'static str) -> Result<PackGuard, String> {
    let mut m = busy.lock().map_err(|e| e.to_string())?;
    if let Some(running) = m.get(&address) {
        return Err(pack_busy_error(address, running));
    }
    m.insert(address, action);
    Ok(PackGuard { busy: busy.clone(), address })
}

/// Longest wait for the pack to answer again after a restart, counted from sending it.
const COMEBACK_LIMIT: Duration = Duration::from_secs(15);

/// Outcome of a restart. `not_reachable`: the pack did not answer 0x51 before, nothing was sent.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Restart {
    NotReachable,
    Back { after_ms: u64 },
    NoInterruption,
    NoAnswer,
}

impl From<Comeback> for Restart {
    fn from(c: Comeback) -> Self {
        match c {
            Comeback::Back { after_ms } => Restart::Back { after_ms },
            Comeback::NoInterruption => Restart::NoInterruption,
            Comeback::NoAnswer => Restart::NoAnswer,
        }
    }
}

/// Restart one pack (0x45 type 6): 0x51 first (a polling master never answers on RS485-1/2), the restart, then 0x51
/// back to back for up to 15 s. Runs off the async runtime; the watch holds the connection, so polling waits.
#[tauri::command]
pub async fn pack_restart(state: State<'_, AppState>, address: u8, installation: Option<String>) -> Result<Restart, String> {
    pack_bus_only(&state)?;
    bound(&state, installation.as_deref())?;
    let guard = reserve_pack(&state.pack_busy, address, "restart")?;
    let (conn, flashing) = (state.conn.clone(), state.flashing.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        if !with_conn(&conn, &flashing, |c| c.answers(address))? {
            return Ok(Restart::NotReachable);
        }
        let sent = with_conn(&conn, &flashing, |c| refusable(c.restart(address)))?.map_err(refusal_error)?;
        with_conn(&conn, &flashing, |c| c.await_comeback(address, sent, COMEBACK_LIMIT)).map(Restart::from)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Switch one pack on or off (0x45 type 4). The UI reads the status again 2 s later.
#[tauri::command]
pub async fn pack_on_off(state: State<'_, AppState>, address: u8, installation: Option<String>, on: bool) -> Result<(), String> {
    pack_bus_only(&state)?;
    bound(&state, installation.as_deref())?;
    let _guard = reserve_pack(&state.pack_busy, address, "on_off")?;
    with_conn(&state.conn, &state.flashing, |c| refusable(c.on_off(address, on)))?.map_err(refusal_error)
}

/// Force the heater for one hour, or stop it (0x45 type 3).
#[tauri::command]
pub async fn pack_heater(state: State<'_, AppState>, address: u8, installation: Option<String>, on: bool) -> Result<(), String> {
    pack_bus_only(&state)?;
    bound(&state, installation.as_deref())?;
    let _guard = reserve_pack(&state.pack_busy, address, "heater")?;
    with_conn(&state.conn, &state.flashing, |c| refusable(c.heater(address, on)))?.map_err(refusal_error)
}

/// Clear the pack's history, energy counters and cycle count (0x45 type 5).
#[tauri::command]
pub async fn pack_clear_history(state: State<'_, AppState>, address: u8, installation: Option<String>) -> Result<(), String> {
    pack_bus_only(&state)?;
    bound(&state, installation.as_deref())?;
    let _guard = reserve_pack(&state.pack_busy, address, "clear")?;
    with_conn(&state.conn, &state.flashing, |c| refusable(c.clear(address)))?.map_err(refusal_error)
}

/// Set the remaining capacity, `raw` in 10 mAh (0x49 index 59); RTN 06 when it exceeds the full capacity.
#[tauri::command]
pub async fn pack_set_remaining(state: State<'_, AppState>, address: u8, installation: Option<String>, raw: u16) -> Result<(), String> {
    pack_bus_only(&state)?;
    bound(&state, installation.as_deref())?;
    let _guard = reserve_pack(&state.pack_busy, address, "remaining")?;
    with_conn(&state.conn, &state.flashing, |c| refusable(c.set_remaining(address, raw)))?.map_err(refusal_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_action_per_pack_until_its_guard_drops() {
        let busy = PackBusy::default();
        let g = reserve_pack(&busy, 3, "history").unwrap();
        let e = reserve_pack(&busy, 3, "clear").err().unwrap();
        let v: serde_json::Value = serde_json::from_str(&e).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "pack-busy", "address": 3, "action": "history" }));
        // another pack is free
        let other = reserve_pack(&busy, 4, "restart").unwrap();
        drop(g);
        assert!(reserve_pack(&busy, 3, "clear").is_ok());
        drop(other);
        assert!(busy.lock().unwrap().is_empty());
    }

    #[test]
    fn restart_outcome_shape() {
        assert_eq!(serde_json::to_string(&Restart::NotReachable).unwrap(), r#"{"kind":"not_reachable"}"#);
        assert_eq!(Restart::from(Comeback::Back { after_ms: 3100 }), Restart::Back { after_ms: 3100 });
        assert_eq!(serde_json::to_string(&Restart::from(Comeback::Back { after_ms: 3100 })).unwrap(), r#"{"kind":"back","after_ms":3100}"#);
        assert_eq!(Restart::from(Comeback::NoAnswer), Restart::NoAnswer);
    }
}
