//! Pack control (RS485-1/2 only): restart, on/off, heater, clearing history and counters (0x45) and the remaining
//! capacity (0x49), always one pack. And the reservations every action of several steps takes (`Ops`).
//!
//! `with_conn` holds the bus for one frame only. So that nothing falls between the steps of an action, one mutex holds:
//! - the reserved packs: every write to a pack, the user's parameter and serial number reads and the history read hold
//!   their pack until they end (a restart cannot fall between the 0x47 and the 0xA1 of a write, clearing cannot cut a
//!   history read short). Actions on different packs still interleave frame by frame.
//! - the bus: connect and the operations that close or take the pack bus (maintenance panel, firmware) start only
//!   while no pack is reserved (`reserve_bus`), and no pack is reserved while one of them runs.
//! - the installations under a structural operation (connect, merge, delete, export, import).
//!
//! Besides, each step of a pack action runs through `with_link`: once the connection changed since the action began, no
//! further frame is sent.

use crate::bms_memory::{bound, pack_bus_only, refusable, refusal_error};
use crate::maint::{refused, MaintError};
use crate::{current_action, reserve_port, with_link, AppState, Link, PortSlot};
use openbms_proto::Comeback;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::State;

#[derive(Default)]
pub(crate) struct OpsState {
    /// reserved packs: address → the action that holds it
    packs: HashMap<u8, &'static str>,
    /// installations under a structural operation: id → the operation
    installations: HashMap<String, &'static str>,
    /// a disconnect waits for the running pack actions: no new one starts
    disconnecting: bool,
}

pub(crate) type Ops = Arc<Mutex<OpsState>>;

/// Frees the pack when dropped.
pub(crate) struct PackGuard {
    ops: Ops,
    address: u8,
}

impl Drop for PackGuard {
    fn drop(&mut self) {
        if let Ok(mut m) = self.ops.lock() {
            m.packs.remove(&self.address);
        }
    }
}

/// The refusal while another action holds the pack, as JSON for the UI (`packBusy` in control.ts):
/// `{"kind":"pack-busy","address":…,"action":…}`.
fn pack_busy_error(address: u8, action: &str) -> String {
    serde_json::json!({ "kind": "pack-busy", "address": address, "action": action }).to_string()
}

/// Reserve `address` for `action`. Refused while another action holds it, or while an operation holds the bus (the
/// action then names that operation: "live", "scan", "halt", "release", "flash", or "port").
pub(crate) fn reserve_pack(state: &AppState, address: u8, action: &'static str) -> Result<PackGuard, String> {
    let mut m = state.ops.lock().map_err(|e| e.to_string())?;
    if let Some(running) = m.packs.get(&address) {
        return Err(pack_busy_error(address, running));
    }
    if m.disconnecting {
        return Err(pack_busy_error(address, "disconnect"));
    }
    if state.port_busy.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(pack_busy_error(address, current_action(state).unwrap_or("port")));
    }
    m.packs.insert(address, action);
    Ok(PackGuard { ops: state.ops.clone(), address })
}

/// The pack action of `address`, if one runs (polling skips a pack while it restarts).
pub(crate) fn pack_action(ops: &Ops, address: u8) -> Option<&'static str> {
    ops.lock().ok().and_then(|m| m.packs.get(&address).copied())
}

/// Any pack reserved: its action, for the "port busy" text of connect.
pub(crate) fn any_pack_action(ops: &Ops) -> Option<&'static str> {
    ops.lock().ok().and_then(|m| m.packs.values().next().copied())
}

/// The port for connect or an operation that closes or takes the pack bus: only while no pack is reserved, checked
/// and taken under the same mutex as the pack reservations.
pub(crate) fn reserve_bus(state: &AppState) -> Option<PortSlot> {
    let m = state.ops.lock().ok()?;
    if !m.packs.is_empty() {
        return None;
    }
    reserve_port(&state.port_busy)
}

/// `reserve_bus` for a user action: waits until running pack actions and background exchanges end.
pub(crate) fn reserve_bus_wait(state: &AppState) -> Option<PortSlot> {
    let until = Instant::now() + crate::PORT_WAIT;
    loop {
        if let Some(slot) = reserve_bus(state) {
            return Some(slot);
        }
        if Instant::now() >= until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A disconnect in progress: no new pack action starts until it is dropped.
pub(crate) struct Disconnecting(Ops);

impl Drop for Disconnecting {
    fn drop(&mut self) {
        if let Ok(mut m) = self.0.lock() {
            m.disconnecting = false;
        }
    }
}

pub(crate) fn begin_disconnect(ops: &Ops) -> Disconnecting {
    if let Ok(mut m) = ops.lock() {
        m.disconnecting = true;
    }
    Disconnecting(ops.clone())
}

/// Wait until no pack is reserved, at most `max` (disconnect: a running action ends first). Blocking.
pub(crate) fn wait_packs_idle(ops: &Ops, max: Duration) {
    let until = Instant::now() + max;
    while any_pack_action(ops).is_some() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Frees the installation when dropped.
pub(crate) struct InstallationGuard {
    ops: Ops,
    ids: Vec<String>,
}

impl Drop for InstallationGuard {
    fn drop(&mut self) {
        if let Ok(mut m) = self.ops.lock() {
            for id in &self.ids {
                m.installations.remove(id);
            }
        }
    }
}

/// All installations: an import, whose target is decided inside it.
pub(crate) const ALL: &str = "*";

/// Hold installations for a structural operation (connect, merge, delete, export, import) over its whole length.
/// Refused ("installation-in-use") while another one holds any of them; `ALL` conflicts with every other.
pub(crate) fn reserve_installations(ops: &Ops, ids: &[&str], action: &'static str) -> Result<InstallationGuard, MaintError> {
    let mut m = ops.lock().map_err(|e| MaintError::Store { message: e.to_string() })?;
    let all_held = m.installations.contains_key(ALL) || (ids.contains(&ALL) && !m.installations.is_empty());
    if all_held || ids.iter().any(|id| m.installations.contains_key(*id)) {
        return Err(refused("installation-in-use"));
    }
    for id in ids {
        m.installations.insert((*id).to_string(), action);
    }
    Ok(InstallationGuard { ops: ops.clone(), ids: ids.iter().map(|s| s.to_string()).collect() })
}

/// The pre-check before a restart takes the normal timeout; each try of the watch this one.
const WATCH_TRY: Duration = Duration::from_millis(600);
/// Longest wait for the pack to answer again after a restart, counted from sending it.
const COMEBACK_LIMIT: Duration = Duration::from_secs(15);

/// Outcome of a restart. `not_reachable`: the pack did not answer 0x51 before, nothing was sent. `sent_link_lost`:
/// the restart was sent, then the connection failed or changed, so the comeback is unknown.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Restart {
    NotReachable,
    Back { after_ms: u64 },
    NoInterruption,
    NoAnswer,
    SentLinkLost { error: String },
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

/// A command the pack did not answer, as JSON for the UI (`noAnswer` in control.ts): typically a master that polls its
/// slaves and ignores RS485-1/2.
fn no_answer_error() -> String {
    serde_json::json!({ "kind": "no-answer" }).to_string()
}

/// One control frame: silence and a refusal by the BMS come back as JSON for the UI, every other error as text.
fn command<T>(r: openbms_transport::Result<T>) -> openbms_transport::Result<Result<T, String>> {
    match r {
        Err(openbms_transport::Error::Timeout(_)) => Ok(Err(no_answer_error())),
        r => refusable(r).map(|x| x.map_err(refusal_error)),
    }
}

/// Run one control frame on the pack the action reserved, on the connection it began on.
fn control_frame(state: &AppState, link: &Link, address: u8, action: &'static str, f: impl FnOnce(&mut openbms_transport::Connection) -> openbms_transport::Result<()>) -> Result<(), String> {
    let _guard = reserve_pack(state, address, action)?;
    with_link(&state.conn, &state.flashing, &state.link, link, |c| command(f(c)))?
}

/// Restart one pack (0x45 type 6): 0x51 first (a polling master never answers on RS485-1/2), the restart, then 0x51
/// back to back for up to 15 s. One frame per lock: the bus stays usable, polling skips this pack meanwhile.
#[tauri::command]
pub async fn pack_restart(state: State<'_, AppState>, address: u8, installation: Option<String>) -> Result<Restart, String> {
    pack_bus_only(&state)?;
    let link = bound(&state, installation.as_deref())?;
    let guard = reserve_pack(&state, address, "restart")?;
    let (conn, flashing, links) = (state.conn.clone(), state.flashing.clone(), state.link.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        if !with_link(&conn, &flashing, &links, &link, |c| c.answers(address))? {
            return Ok(Restart::NotReachable);
        }
        let sent = with_link(&conn, &flashing, &links, &link, |c| refusable(c.restart(address)))?.map_err(refusal_error)?;
        // from here the restart is out: a failure only means its comeback is unknown
        let watch = openbms_transport::watch_comeback(sent, COMEBACK_LIMIT, || with_link(&conn, &flashing, &links, &link, |c| c.answers_within(address, WATCH_TRY)));
        Ok(watch.map(Restart::from).unwrap_or_else(|error| Restart::SentLinkLost { error }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Switch one pack on or off (0x45 type 4). The UI reads the status again 2 s later.
#[tauri::command]
pub async fn pack_on_off(state: State<'_, AppState>, address: u8, installation: Option<String>, on: bool) -> Result<(), String> {
    pack_bus_only(&state)?;
    let link = bound(&state, installation.as_deref())?;
    control_frame(&state, &link, address, "on_off", |c| c.on_off(address, on))
}

/// Force the heater for one hour, or stop it (0x45 type 3).
#[tauri::command]
pub async fn pack_heater(state: State<'_, AppState>, address: u8, installation: Option<String>, on: bool) -> Result<(), String> {
    pack_bus_only(&state)?;
    let link = bound(&state, installation.as_deref())?;
    control_frame(&state, &link, address, "heater", |c| c.heater(address, on))
}

/// Clear the pack's history, energy counters and cycle count (0x45 type 5).
#[tauri::command]
pub async fn pack_clear_history(state: State<'_, AppState>, address: u8, installation: Option<String>) -> Result<(), String> {
    pack_bus_only(&state)?;
    let link = bound(&state, installation.as_deref())?;
    control_frame(&state, &link, address, "clear", |c| c.clear(address))
}

/// Set the remaining capacity, `raw` in 10 mAh (0x49 index 59); RTN 06 when it exceeds the full capacity.
#[tauri::command]
pub async fn pack_set_remaining(state: State<'_, AppState>, address: u8, installation: Option<String>, raw: u16) -> Result<(), String> {
    pack_bus_only(&state)?;
    let link = bound(&state, installation.as_deref())?;
    control_frame(&state, &link, address, "remaining", |c| c.set_remaining(address, raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn json(e: &str) -> serde_json::Value {
        serde_json::from_str(e).unwrap()
    }

    #[test]
    fn one_action_per_pack_until_its_guard_drops() {
        let state = AppState::default();
        let g = reserve_pack(&state, 3, "history").unwrap();
        let e = reserve_pack(&state, 3, "clear").err().unwrap();
        assert_eq!(json(&e), serde_json::json!({ "kind": "pack-busy", "address": 3, "action": "history" }));
        // another pack is free
        let other = reserve_pack(&state, 4, "restart").unwrap();
        assert_eq!(pack_action(&state.ops, 4), Some("restart"));
        drop(g);
        assert!(reserve_pack(&state, 3, "clear").is_ok());
        drop(other);
        assert_eq!(any_pack_action(&state.ops), None);
    }

    #[test]
    fn the_bus_and_the_packs_exclude_each_other() {
        let state = AppState::default();
        let pack = reserve_pack(&state, 1, "params").unwrap();
        assert!(reserve_bus(&state).is_none(), "no bus operation while a pack action runs");
        drop(pack);
        let bus = reserve_bus(&state).expect("free now");
        *state.maint_action.lock().unwrap() = Some("scan");
        assert_eq!(json(&reserve_pack(&state, 1, "params").err().unwrap())["action"], "scan");
        *state.maint_action.lock().unwrap() = None;
        assert_eq!(json(&reserve_pack(&state, 1, "params").err().unwrap())["action"], "port");
        drop(bus);
        assert!(!state.port_busy.load(Ordering::SeqCst));
        assert!(reserve_pack(&state, 1, "params").is_ok());
    }

    #[test]
    fn no_new_pack_action_while_a_disconnect_waits() {
        let state = AppState::default();
        let running = reserve_pack(&state, 1, "restart").unwrap();
        let d = begin_disconnect(&state.ops);
        assert_eq!(json(&reserve_pack(&state, 2, "params").err().unwrap())["action"], "disconnect");
        drop(running);
        wait_packs_idle(&state.ops, Duration::from_secs(1));
        drop(d);
        assert!(reserve_pack(&state, 2, "params").is_ok());
    }

    #[test]
    fn installations_are_held_for_one_structural_operation() {
        let ops = Ops::default();
        let merge = reserve_installations(&ops, &["a", "b"], "merge").unwrap();
        assert!(matches!(reserve_installations(&ops, &["b"], "connect"), Err(MaintError::Refused { reason }) if reason == "installation-in-use"));
        assert!(reserve_installations(&ops, &["c"], "connect").is_ok());
        drop(merge);
        assert!(reserve_installations(&ops, &["a"], "delete").is_ok());
        // an import holds them all
        let import = reserve_installations(&ops, &[ALL], "import").unwrap();
        assert!(reserve_installations(&ops, &["z"], "connect").is_err());
        drop(import);
        let connect = reserve_installations(&ops, &["z"], "connect").unwrap();
        assert!(reserve_installations(&ops, &[ALL], "import").is_err());
        drop(connect);
    }

    #[test]
    fn silence_is_its_own_error() {
        let silent = command::<()>(Err(openbms_transport::Error::Timeout(Duration::from_millis(1500)))).unwrap();
        assert_eq!(json(&silent.unwrap_err()), serde_json::json!({ "kind": "no-answer" }));
        let refused = command::<()>(Err(openbms_transport::Error::Proto(openbms_proto::Error::Rtn(0x06)))).unwrap();
        assert_eq!(refused.unwrap_err(), r#"{"kind":"refused"}"#);
        assert!(command::<()>(Err(openbms_transport::Error::Closed)).is_err());
    }

    #[test]
    fn restart_outcome_shape() {
        assert_eq!(serde_json::to_string(&Restart::NotReachable).unwrap(), r#"{"kind":"not_reachable"}"#);
        assert_eq!(Restart::from(Comeback::Back { after_ms: 3100 }), Restart::Back { after_ms: 3100 });
        assert_eq!(serde_json::to_string(&Restart::from(Comeback::Back { after_ms: 3100 })).unwrap(), r#"{"kind":"back","after_ms":3100}"#);
        assert_eq!(serde_json::to_string(&Restart::SentLinkLost { error: "not connected".into() }).unwrap(), r#"{"kind":"sent_link_lost","error":"not connected"}"#);
    }
}
