//! Maintenance of a multi-pack bank (design: docs/superpowers/specs/2026-10-12-maintenance-mode-design.md):
//! the 0x64 dialog over the control channel, the master state and the session per installation.
//! All operations hold the one maintenance-operation lock (`port_busy`); a control exchange on the
//! endpoint of the main connection runs through that connection, so no endpoint is opened twice.

use crate::maint_store::{ControlEndpoint, FlashBlock, Installation, MaintSession, SessionKind, Store, StoredMasterState};
use crate::{lock, reserve_port, AppState, EndpointArg};
use openbms_flash::maint::dialog::{self, Action, Budgets, DialogError};
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
    #[allow(dead_code)] // used from Task 8 on
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

/// `main_conn_present`: the main connection object still exists; if it was dropped meanwhile (link loss),
/// the control exchange opens its own channel.
pub fn route_with(main: Option<&(String, Bus)>, control_key: &str, main_conn_present: bool) -> Route {
    match main {
        _ if uses_main_connection(main, control_key) && main_conn_present => Route::Main,
        _ if uses_main_connection(main, control_key) => Route::Open,
        Some((key, _)) if key == control_key => Route::Refused,
        _ => Route::Open,
    }
}

#[cfg(test)]
fn route(main: Option<&(String, Bus)>, control_key: &str) -> Route {
    route_with(main, control_key, true)
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
    let key = control.key();
    let mut guard = lock(&state.conn).map_err(|m| MaintError::Connection { message: m })?;
    match route_with(main.as_ref(), &key, guard.is_some()) {
        Route::Main => match guard.as_mut() {
            Some(c) => Ok(f(c)),
            None => Err(MaintError::Connection { message: "not connected".into() }),
        },
        Route::Refused => Err(refused("control-on-pack-bus")),
        Route::Open => {
            drop(guard);
            let mut c = Connection::open(&control_endpoint(control)).map_err(|e| MaintError::Connection { message: e.to_string() })?;
            Ok(f(&mut c))
        }
    }
}

#[allow(dead_code)] // used from Task 8 on
pub fn stop_allowed(state: Option<&StoredMasterState>) -> Result<(), MaintError> {
    if state == Some(&StoredMasterState::MarkerUnexplained) {
        return Err(refused("marker-unexplained"));
    }
    Ok(())
}

fn installation(state: &AppState, id: &str) -> Result<Installation, MaintError> {
    with_store(state, |s| s.installations()?.into_iter().find(|i| i.id == id).ok_or_else(|| "unknown installation".into()))
}

/// The state to store after a read. A failed read (no usable answer) never clears `marker-unexplained`:
/// only a successful read may change it.
pub fn next_state(stored: Option<&StoredMasterState>, classified: StoredMasterState, read_ok: bool) -> StoredMasterState {
    if !read_ok && stored == Some(&StoredMasterState::MarkerUnexplained) {
        return StoredMasterState::MarkerUnexplained;
    }
    classified
}

/// Read 0x64 and store the state. A connection or dialog failure stores `unknown` (never `no-0x64`; a stored
/// `marker-unexplained` stays); a refusal (no control channel, control on the pack bus) is returned and stores nothing.
pub fn check(state: &AppState, inst: &Installation) -> Result<StoredMasterState, MaintError> {
    let session = with_store(state, |s| s.open_session(&inst.id))?;
    let probe = match with_control(state, inst, |c| dialog::probe(c, &SystemClock::new(), Budgets::default()).ok()) {
        Ok(p) => p,
        Err(e @ MaintError::Refused { .. }) => return Err(e),
        Err(_) => None,
    };
    let read_ok = matches!(probe, Some(dialog::Probe::Status(_) | dialog::Probe::Unsupported));
    let expected = session.is_some_and(|s| s.kind == SessionKind::Marker);
    let classified: StoredMasterState = policy::classify(probe, expected).into();
    let stored = with_store(state, |s| s.master_state(&inst.id))?.map(|(m, _)| m);
    let m = next_state(stored.as_ref(), classified, read_ok);
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
    .map_err(|e| match e {
        MaintError::Store { message } if message == "endpoint-in-use" => refused("endpoint-in-use"),
        other => other,
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

    #[test]
    fn control_on_the_main_endpoint_opens_its_own_channel_when_the_main_connection_is_gone() {
        let can = ("serial:/dev/a".to_string(), Bus::CanSocketBus);
        assert!(matches!(route_with(Some(&can), "serial:/dev/a", true), Route::Main));
        assert!(matches!(route_with(Some(&can), "serial:/dev/a", false), Route::Open));
    }

    #[test]
    fn a_failed_read_never_clears_marker_unexplained() {
        use StoredMasterState as S;
        assert_eq!(next_state(Some(&S::MarkerUnexplained), S::Unknown, false), S::MarkerUnexplained);
        assert_eq!(next_state(Some(&S::MarkerUnexplained), S::PatchedOff { slaves: 3 }, true), S::PatchedOff { slaves: 3 });
        assert_eq!(next_state(Some(&S::PatchedOn), S::Unknown, false), S::Unknown);
    }
}
