//! Maintenance of a multi-pack bank (design: docs/superpowers/specs/2026-10-12-maintenance-mode-design.md):
//! the 0x64 dialog over the control channel, the master state and the session per installation.
//! All operations hold the one maintenance-operation lock (`port_busy`); a control exchange on the
//! endpoint of the main connection runs through that connection, so no endpoint is opened twice.

use crate::maint_store::{ControlEndpoint, FlashBlock, Installation, MaintSession, SessionKind, Store, StoredMasterState};
use crate::{lock, reserve_port, AppState, EndpointArg};
use openbms_flash::known::ImageKind;
use openbms_flash::maint::dialog::{self, Action, Budgets, DialogError, Probe};
use openbms_flash::maint::policy::{Expect, Outcome, Step};
use openbms_flash::maint::{bus, policy, SystemClock};
use openbms_flash::session::Progress;
use openbms_transport::{Bus, Connection, Endpoint};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, State};

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
    with_store_arc(&state.maint, f)
}

/// `with_store` for the flash thread, which has no `State`.
pub fn with_store_arc<T>(store: &Arc<Mutex<Option<Store>>>, f: impl FnOnce(&Store) -> Result<T, String>) -> Result<T, MaintError> {
    let guard = store.lock().map_err(|e| store_err(e.to_string()))?;
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

/// Unclear master updates (`master-unclear`) that a successful read resolves: the read shows the state the
/// master-flash plan expected. A failed read resolves nothing.
pub fn resolved_master_blocks(blocks: &[FlashBlock], probe: Option<Probe>, read_ok: bool) -> Vec<i64> {
    if !read_ok {
        return vec![];
    }
    blocks
        .iter()
        .filter(|b| b.reason == "master-unclear" && b.expect.as_deref().is_some_and(|x| clears_master_unclear(x, probe)))
        .map(|b| b.id)
        .collect()
}

/// Read 0x64 and store the state. A connection or dialog failure stores `unknown` (never `no-0x64`; a stored
/// `marker-unexplained` stays); a refusal (no control channel, control on the pack bus) is returned and stores nothing.
/// A successful read that shows the expected state clears an unclear master update.
pub fn check(state: &AppState, inst: &Installation) -> Result<StoredMasterState, MaintError> {
    let session = with_store(state, |s| s.open_session(&inst.id))?;
    let probe = match with_control(state, inst, |c| dialog::probe(c, &SystemClock::new(), Budgets::default()).ok()) {
        Ok(p) => p,
        Err(e @ MaintError::Refused { .. }) => return Err(e),
        Err(_) => None,
    };
    let read_ok = matches!(probe, Some(Probe::Status(_) | Probe::Unsupported));
    let expected = session.is_some_and(|s| s.kind == SessionKind::Marker);
    let classified: StoredMasterState = policy::classify(probe, expected).into();
    let now = crate::history::now_ms();
    with_store(state, |s| {
        let stored = s.master_state(&inst.id)?.map(|(m, _)| m);
        let m = next_state(stored.as_ref(), classified, read_ok);
        s.set_master_state(&inst.id, &m, now)?;
        for id in resolved_master_blocks(&s.blocks(&inst.id)?, probe, read_ok) {
            s.clear_block(id, "checked", now)?;
        }
        Ok(m)
    })
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
    // a marker session needs a polling master: 0/0 is a single pack, it takes the DIP way
    slaves_allowed(n, false)?;
    let now = crate::history::now_ms();
    with_store(&state, |s| s.start_session(&inst.id, SessionKind::Marker, n, now))?;
    let r = with_control(&state, &inst, |c| dialog::dialog(c, &SystemClock::new(), Action::Start, None, Budgets::default()))?;
    check(&state, &inst)?;
    r?;
    overview(&state, &installation)
}

/// The slave count `N` (packs on the bus minus the master, = the master's DIP 5–8 value). A bank has 1 to 15
/// slaves. `N` = 0 (a single pack) only when `zero_verified`: a fresh 0x64 read in the same command showed
/// "patched, marker off, 0 slaves"; otherwise End's bus step could pass on a master that still does not poll.
pub fn slaves_allowed(n: u8, zero_verified: bool) -> Result<(), MaintError> {
    match n {
        1..=15 => Ok(()),
        0 if zero_verified => Ok(()),
        _ => Err(refused("slaves-range")),
    }
}

/// The master itself reported "patched, marker off, 0 slaves" (status 0/0).
pub fn reports_zero(state: Option<&StoredMasterState>) -> bool {
    state == Some(&StoredMasterState::PatchedOff { slaves: 0 })
}

/// Proof of a single pack: the master reports 0/0 (which alone a bank master with DIP 5–8 OFF also does)
/// and no slave answers on the pack bus (`slave`: the first address 1..=15 that answered 0x51).
pub fn single_pack(state: Option<&StoredMasterState>, slave: Option<u8>) -> bool {
    reports_zero(state) && slave.is_none()
}

/// For `N` = 0 (the caller holds `port_busy`): read 0x64 now and store it; if it shows 0/0, prove on the pack
/// bus that it is the pack bus (quiet, 0x47 from address 0) and scan for a slave (`bus::first_slave`, two
/// passes, fail-closed). Returns the fresh state and the scan result; a slave found is refused `slave-present`.
/// Without a control channel nothing can be verified: `slaves-range`.
fn verify_single_pack(state: &AppState, app: &AppHandle, inst: &Installation, port: &str) -> Result<(StoredMasterState, Option<u8>), MaintError> {
    if port.is_empty() {
        return Err(refused("no-port"));
    }
    port_of(state, inst, port)?;
    let fresh = match check(state, inst) {
        Err(MaintError::Refused { reason }) if reason == "no-control-channel" => return Err(refused("slaves-range")),
        r => r?,
    };
    if !reports_zero(Some(&fresh)) {
        return Ok((fresh, None));
    }
    let mut link = open_pack_for(state, app, port)?;
    bus::measure(&mut link, &mut std::io::sink()).map_err(|e| MaintError::Measure { message: e.to_string() })?;
    let slave = bus::first_slave(&mut link, &SystemClock::new(), &mut std::io::sink(), bus::SLAVE_WINDOW)
        .map_err(|e| MaintError::Connection { message: e.to_string() })?;
    if slave.is_some() {
        return Err(refused("slave-present"));
    }
    Ok((fresh, slave))
}

/// The user confirmed DIP 5–8 OFF (and noted the positions): a DIP session with the slave count they entered
/// (`N` = 0 only for a proven single pack, see `single_pack`; `port`: the pack port, used only then).
#[tauri::command]
pub async fn maint_start_dip(app: AppHandle, state: State<'_, AppState>, installation: String, expected_slaves: u8, port: String) -> Result<Overview, MaintError> {
    if expected_slaves != 0 {
        slaves_allowed(expected_slaves, false)?;
    }
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    if expected_slaves == 0 {
        let (fresh, slave) = verify_single_pack(&state, &app, &inst, &port)?;
        slaves_allowed(0, single_pack(Some(&fresh), slave))?;
    }
    let now = crate::history::now_ms();
    with_store(&state, |s| s.start_session(&inst.id, SessionKind::Dip, expected_slaves, now).map(|_| ()))?;
    overview(&state, &installation)
}

/// The user states where an unexplained marker comes from ("I set it earlier"): a marker session with `N`.
#[tauri::command]
pub async fn maint_account_marker(state: State<'_, AppState>, installation: String, expected_slaves: u8) -> Result<Overview, MaintError> {
    // a set marker is never a verified 0/0 single pack
    slaves_allowed(expected_slaves, false)?;
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    let now = crate::history::now_ms();
    let (session, ms) = with_store(&state, |s| Ok((s.open_session(&inst.id)?, s.master_state(&inst.id)?)))?;
    account_allowed(session.as_ref(), ms.as_ref().map(|(m, _)| m))?;
    with_store(&state, |s| {
        if s.open_session(&inst.id)?.is_none() {
            s.start_session(&inst.id, SessionKind::Marker, expected_slaves, now)?;
        }
        s.set_master_state(&inst.id, &StoredMasterState::PatchedOn, now)
    })?;
    overview(&state, &installation)
}

/// Correcting `N` of an open session: never in the locked state (its only ways out are the recovery row
/// and the user's statement). `single`: for `N` = 0, a single pack proven in this command (`single_pack`).
pub fn set_slaves_allowed(session: Option<&MaintSession>, state: Option<&StoredMasterState>, n: u8, single: bool) -> Result<(), MaintError> {
    if session.is_none() {
        return Err(refused("no-session"));
    }
    stop_allowed(state)?;
    slaves_allowed(n, single)
}

/// A session with a wrong `N` could never be closed: the user corrects the slave count per DIP. End
/// starts over, its earlier steps were checked against the old count.
#[tauri::command]
pub async fn maint_set_slaves(app: AppHandle, state: State<'_, AppState>, installation: String, expected_slaves: u8, port: String) -> Result<Overview, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    let (session, ms) = with_store(&state, |s| Ok((s.open_session(&inst.id)?, s.master_state(&inst.id)?)))?;
    let mut ms = ms.map(|(m, _)| m);
    let mut single = false;
    if expected_slaves == 0 {
        // nothing stored counts for N = 0: prove a single pack now (session and lock checked first)
        set_slaves_allowed(session.as_ref(), ms.as_ref(), 1, false)?;
        let (fresh, slave) = verify_single_pack(&state, &app, &inst, &port)?;
        single = single_pack(Some(&fresh), slave);
        ms = Some(fresh);
    }
    set_slaves_allowed(session.as_ref(), ms.as_ref(), expected_slaves, single)?;
    if let Some(ses) = &session {
        with_store(&state, |s| s.set_expected_slaves(ses.id, expected_slaves))?;
    }
    state.end_progress.lock().map_err(|e| store_err(e.to_string()))?.remove(&inst.id);
    overview(&state, &installation)
}

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
/// An unassigned port is refused when the UI named an installation (the user assigns it there; a new
/// installation would block later with `installation-mismatch`) or while any installation has open work.
pub fn port_gate(port_inst: Option<&str>, ui_inst: Option<&str>, any_open: bool) -> Result<(), MaintError> {
    match (port_inst, ui_inst) {
        (Some(p), Some(u)) if p != u => Err(refused("installation-mismatch")),
        (None, Some(_)) => Err(refused("port-not-assigned")),
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
/// `stored`: the master's post-check outcome was stored in full (true when there was none). If not, the
/// record of the restart stays. `answers` asks the pack for 0x51/0x47 and is called only when that decides.
pub fn finish_block(ok: bool, stored: bool, last: Option<Progress>, answers: impl FnOnce() -> bool) -> BlockAction {
    if ok && !stored {
        return BlockAction::Set("update-interrupted");
    }
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

/// "I set the marker myself" turns an unexplained marker into a marker session; only that state is
/// accounted for this way. A DIP session (the recovery) stays what it is: its End sends no `stop` and
/// expects the polls of a quiet master.
pub fn account_allowed(session: Option<&MaintSession>, state: Option<&StoredMasterState>) -> Result<(), MaintError> {
    if state != Some(&StoredMasterState::MarkerUnexplained) {
        return Err(refused("not-marker-unexplained"));
    }
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

/// Pause between the two Modbus reads of End: one full polling round (150 ms per slave) plus 1 s, at least 2 s.
pub fn values_pause(n: u8) -> Duration {
    (Duration::from_millis(150) * u32::from(n) + Duration::from_secs(1)).max(Duration::from_secs(2))
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
const PACK_BUS_BAUD: u32 = 19_200;

fn open_pack(port: &str) -> Result<Connection, MaintError> {
    Connection::open(&Endpoint::Serial { path: port.into(), baud: PACK_BUS_BAUD }).map_err(|e| MaintError::Connection { message: e.to_string() })
}

/// The measurement of step 3. First, whenever the stored state is known and the control channel is a
/// separate adapter, a fresh 0x64 status that must match it (`policy::fresh_status`); a mismatch re-checks
/// the master and refuses. Then on the pack bus 2 s quiet and 0x47 from address 0. Stops the monitor when
/// it holds the pack port or polls the pack bus.
pub fn measure(state: &AppState, app: &AppHandle, inst: &Installation, port: &str) -> Result<i64, MaintError> {
    let ms = with_store(state, |s| s.master_state(&inst.id))?;
    let fresh = ms.as_ref().map(|(m, _)| policy::fresh_status(m.into())).unwrap_or(policy::Fresh::Unchecked);
    if fresh != policy::Fresh::Unchecked && inst.control.is_some() && !same_adapter(inst, port) {
        let probe = with_control(state, inst, |c| dialog::probe(c, &SystemClock::new(), Budgets::default()).ok())?;
        if !policy::fresh_matches(fresh, probe) {
            check(state, inst)?;
            return Err(refused("state-changed"));
        }
    }
    let mut link = open_pack_for(state, app, port)?;
    bus::measure(&mut link, &mut std::io::sink()).map_err(|e| MaintError::Measure { message: e.to_string() })?;
    Ok(crate::history::now_ms())
}

/// Open the pack bus port; first stop the monitor when it holds that adapter or polls the pack bus, and tell
/// the UI ("monitor-stopped"): a stop by the app is no lost connection.
pub fn open_pack_for(state: &AppState, app: &AppHandle, port: &str) -> Result<Connection, MaintError> {
    let main = state.main.lock().map_err(|e| store_err(e.to_string()))?.clone();
    if must_stop_monitor(main.as_ref(), &crate::maint_store::endpoint_key(false, port)) {
        crate::stop_monitor_and_tell(state, app).map_err(|m| MaintError::Connection { message: m })?;
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
    pub app: &'a AppHandle,
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
        measure(self.state, self.app, self.inst, self.port).map(|_| ())
    }
}

/// The pack port must belong to the installation (as a main endpoint or as the control adapter).
fn port_of(state: &AppState, inst: &Installation, port: &str) -> Result<(), MaintError> {
    let key = crate::maint_store::endpoint_key(false, port);
    let (owner, any_open) = with_store(state, |s| Ok((s.installation_of(&key)?, s.any_open()?)))?;
    port_gate(owner.as_ref().map(|i| i.id.as_str()), Some(&inst.id), any_open)
}

#[tauri::command]
pub async fn maint_measure(app: AppHandle, state: State<'_, AppState>, installation: String, port: String) -> Result<i64, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    port_of(&state, &inst, &port)?;
    measure(&state, &app, &inst, &port)
}

#[derive(Serialize)]
pub struct EndResult {
    ok: bool,
    missing: Vec<u8>,
    failing: Option<String>,
}

/// The three steps of End. In the sequential way (one adapter for both buses) the UI drives them one by
/// one and the user moves the cable between them: `stop` and the Modbus reads need the CAN socket bus,
/// the polling-round check needs the pack bus.
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EndStep {
    /// marker session: `stop` and wait for 0/N (a DIP session sends no `stop`; the step passes without one)
    Stop,
    /// pack bus: every slave polled and answering (`N` = 0: quiet bus, 0x47 answered by address 0, no slave
    /// answering 0x51)
    Bus,
    /// two Modbus reads one polling round apart (`values_pause`) over the control channel, for `N` = 0 after a
    /// fresh 0x64 status 0/0; closes the session
    Values,
}

/// Steps of End that passed in this app run, for one session. Only in memory: after a restart End starts
/// again from its first step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EndProgress {
    pub session: i64,
    pub stopped: bool,
    pub bus_ok: bool,
}

/// The progress that counts for `session`: progress of another session starts over.
pub fn end_progress_of(stored: Option<&EndProgress>, session: i64) -> EndProgress {
    match stored {
        Some(p) if p.session == session => *p,
        _ => EndProgress { session, ..Default::default() },
    }
}

/// Whether `step` may run now: Bus after Stop (marker sessions), Values after Bus (and Stop). A single pack
/// (`N` = 0) ends only with a control channel: there is no polling round to see, so its Bus step also scans
/// addresses 1..=15 for a slave (a bank master with DIP 5–8 still OFF reports 0/0 too) and its Values step
/// needs a fresh 0/0 from the master (`reports_zero`). Without a control channel (or with `N` > 15) every step
/// is refused until the user corrects `N`.
pub fn end_next(p: &EndProgress, kind: SessionKind, slaves: u8, has_control: bool, step: EndStep) -> Result<(), MaintError> {
    slaves_allowed(slaves, has_control)?;
    let stopped = p.stopped || kind == SessionKind::Dip;
    match step {
        EndStep::Stop => Ok(()),
        EndStep::Bus if stopped => Ok(()),
        EndStep::Values if stopped && p.bus_ok => Ok(()),
        _ => Err(refused("end-order")),
    }
}

/// The progress after `step` ran: a failure starts End over; a passed step counts, and a new `stop`
/// makes an earlier bus check stale.
pub fn end_record(p: EndProgress, step: EndStep, ok: bool) -> EndProgress {
    if !ok {
        return EndProgress { session: p.session, ..Default::default() };
    }
    match step {
        EndStep::Stop => EndProgress { stopped: true, bus_ok: false, ..p },
        EndStep::Bus => EndProgress { bus_ok: true, ..p },
        EndStep::Values => p,
    }
}

/// Run one step of End for the open session; the refusals (unclear update, locked marker, order) are
/// checked at every step.
fn end_step(state: &AppState, app: &AppHandle, inst: &Installation, port: &str, step: EndStep) -> Result<EndResult, MaintError> {
    let (session, ms, blocks) = with_store(state, |s| Ok((s.open_session(&inst.id)?, s.master_state(&inst.id)?, s.blocks(&inst.id)?)))?;
    let session = session.ok_or_else(|| refused("no-session"))?;
    stop_allowed(ms.as_ref().map(|(m, _)| m))?;
    end_allowed(&blocks)?;
    let progress = {
        let map = state.end_progress.lock().map_err(|e| store_err(e.to_string()))?;
        end_progress_of(map.get(&inst.id), session.id)
    };
    end_next(&progress, session.kind, session.expected_slaves, inst.control.is_some(), step)?;
    let r = run_end_step(state, app, inst, port, &session, step);
    let ok = matches!(r, Ok(EndResult { ok: true, .. }));
    let mut map = state.end_progress.lock().map_err(|e| store_err(e.to_string()))?;
    if ok && step == EndStep::Values {
        map.remove(&inst.id);
    } else {
        map.insert(inst.id.clone(), end_record(progress, step, ok));
    }
    r
}

fn run_end_step(state: &AppState, app: &AppHandle, inst: &Installation, port: &str, session: &MaintSession, step: EndStep) -> Result<EndResult, MaintError> {
    let n = session.expected_slaves;
    let fail = |what: &str, missing: Vec<u8>| -> Result<EndResult, MaintError> {
        with_store(state, |s| s.set_failing(session.id, Some(what)))?;
        Ok(EndResult { ok: false, missing, failing: Some(what.into()) })
    };
    match step {
        EndStep::Stop if session.kind == SessionKind::Marker => {
            let r = with_control(state, inst, |c| dialog::dialog(c, &SystemClock::new(), Action::Stop, Some(n), Budgets::default()))?;
            check(state, inst)?;
            if let Err(e) = r {
                return fail(&format!("stop: {}", serde_json::to_string(&MaintError::from(e)).unwrap_or_default()), vec![]);
            }
        }
        EndStep::Stop => {}
        EndStep::Bus if n == 0 => {
            // a single pack: nothing polls, the master answers 0x47 on a quiet bus, and no slave answers
            let mut link = open_pack_for(state, app, port)?;
            match bus::measure(&mut link, &mut std::io::sink()) {
                Ok(()) => {}
                Err(openbms_flash::Error::MasterSilent) => return fail("master-not-answering", vec![]),
                Err(e) => return Err(MaintError::Measure { message: e.to_string() }),
            }
            // a slave that answers proves a bank (its master's DIP 5–8 may still be OFF)
            match bus::first_slave(&mut link, &SystemClock::new(), &mut std::io::sink(), bus::SLAVE_WINDOW) {
                Ok(None) => {}
                Ok(Some(a)) => return fail(&format!("slave-present: {a}"), vec![]),
                Err(e) => return Err(MaintError::Connection { message: e.to_string() }),
            }
        }
        EndStep::Bus => {
            // N is 1..=15 here (`end_next`)
            let mut link = open_pack_for(state, app, port)?;
            let round = bus::watch_round(&mut link, &SystemClock::new(), &mut std::io::sink(), n, end_timeout(n))
                .map_err(|e| MaintError::Connection { message: e.to_string() })?;
            if !round.complete(n) {
                return fail("pack-communication-incomplete", round.missing(n));
            }
        }
        EndStep::Values => {
            // a single pack: the master must still report 0/0 now (it cannot show a polling round)
            if n == 0 && !reports_zero(Some(&check(state, inst)?)) {
                return fail("master-not-zero", vec![]);
            }
            for i in 0..2 {
                if i == 1 {
                    std::thread::sleep(values_pause(n));
                }
                if !with_control(state, inst, |c| c.system_values().is_ok())? {
                    return fail("system-values", vec![]);
                }
            }
            with_store(state, |s| s.end_session(session.id, crate::history::now_ms()))?;
        }
    }
    Ok(EndResult { ok: true, missing: vec![], failing: None })
}

/// End (step 5) in one call: refused while an update is unclear; marker → `stop` and wait for 0/N; then
/// the pack bus check against N; then two Modbus reads ≥ 2 s apart over the control channel (for N = 0 after
/// a fresh 0/0 from the master). The session
/// closes only when all of it passed. In the sequential way this is impossible in one call
/// (`end-sequential`): the UI runs `maint_end_step` with a cable change in between.
#[tauri::command]
pub async fn maint_end(app: AppHandle, state: State<'_, AppState>, installation: String, port: String) -> Result<EndResult, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    port_of(&state, &inst, &port)?;
    if same_adapter(&inst, &port) {
        return Err(refused("end-sequential"));
    }
    let mut last = EndResult { ok: false, missing: vec![], failing: None };
    for step in [EndStep::Stop, EndStep::Bus, EndStep::Values] {
        last = end_step(&state, &app, &inst, &port, step)?;
        if !last.ok {
            break;
        }
    }
    Ok(last)
}

/// One step of End (see `EndStep`); refused (`end-order`) before its predecessors passed in this app run.
#[tauri::command]
pub async fn maint_end_step(app: AppHandle, state: State<'_, AppState>, installation: String, port: String, step: EndStep) -> Result<EndResult, MaintError> {
    let _op = reserve_port(&state.port_busy).ok_or(MaintError::Busy)?;
    let inst = self::installation(&state, &installation)?;
    port_of(&state, &inst, &port)?;
    end_step(&state, &app, &inst, &port, step)
}

/// Clear a flash block: "answered" checks the pack on the pack bus (0x51 and 0x47), "isolated" is the
/// user's confirmation that the pack was isolated or recovered.
#[tauri::command]
pub async fn maint_clear_block(app: AppHandle, state: State<'_, AppState>, installation: String, id: i64, how: String, port: Option<String>) -> Result<Overview, MaintError> {
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
            let mut link = open_pack_for(&state, &app, port)?;
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

/// After the flash returned, before the pack port is closed: settle the `update-interrupted` block written
/// before the restart, and note a successful flash in the open session. Returns the block reason that stays.
/// If the store cannot be written the block stays as it is and is reported. `stored`: see `finish_block`.
#[allow(clippy::too_many_arguments)] // the flash thread's values, one each
pub fn record_after_flash(
    store: &Arc<Mutex<Option<Store>>>,
    inst: &Installation,
    pending: i64,
    address: u8,
    release: &str,
    ok: bool,
    stored: bool,
    last: Option<Progress>,
    link: &mut Connection,
) -> Option<String> {
    let now = crate::history::now_ms();
    let kept = match finish_block(ok, stored, last, || bus::app_answers(link, &mut std::io::sink(), address, Duration::from_secs(6))) {
        BlockAction::Clear(how) => with_store_arc(store, |s| s.clear_block(pending, how, now)).err().map(|_| "update-interrupted".to_string()),
        BlockAction::Set(reason) => {
            let _ = with_store_arc(store, |s| s.set_block_reason(pending, reason));
            Some(reason.to_string())
        }
    };
    if ok {
        let _ = with_store_arc(store, |s| match s.open_session(&inst.id)? {
            Some(ses) => s.note_done(ses.id, &format!("flash {address} {release}")),
            None => Ok(()),
        });
    }
    kept
}

/// Read 0x64 on a control channel opened for this check, every 500 ms for up to 6 s until the answer is
/// final; the last read counts. None when there is no control channel or it cannot be opened.
fn read_settled(inst: &Installation) -> Option<Probe> {
    use openbms_flash::maint::Clock;
    let control = inst.control.as_ref()?;
    let mut c = Connection::open(&control_endpoint(control)).ok()?;
    let clock = SystemClock::new();
    loop {
        let probe = dialog::probe(&mut c, &clock, Budgets::default()).ok();
        if policy::settled(probe) || clock.now() >= Duration::from_secs(6) {
            return probe;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub fn outcome_key(o: Outcome) -> &'static str {
    match o {
        Outcome::AsExpected => "as-expected",
        Outcome::Unclear => "unclear",
        Outcome::MarkerUnexplained => "marker-unexplained",
        Outcome::UnknownImage => "unknown-image",
    }
}

/// Store what the post-flash check of the master found (design, "Flashing the master itself"), in one
/// transaction: all of it or nothing. A stored `marker-unexplained` is left only by an as-expected result
/// (same rule as `next_state`).
pub fn apply_master_outcome(s: &Store, inst: &str, plan: &policy::Plan, outcome: Outcome, now: i64) -> Result<(), String> {
    s.atomic(|s| master_outcome_rows(s, inst, plan, outcome, now))
}

fn master_outcome_rows(s: &Store, inst: &str, plan: &policy::Plan, outcome: Outcome, now: i64) -> Result<(), String> {
    let locked = s.master_state(inst)?.is_some_and(|(m, _)| m == StoredMasterState::MarkerUnexplained);
    let next = if locked && outcome != Outcome::AsExpected { StoredMasterState::MarkerUnexplained } else { state_after(outcome, plan.after) };
    s.set_master_state(inst, &next, now)?;
    match outcome {
        // back on the original image: the master is quiet only through DIP 5–8 OFF from now on
        Outcome::AsExpected if plan.after == Expect::Unsupported => {
            if let Some(ses) = s.open_session(inst)? {
                s.set_session_kind(ses.id, SessionKind::Dip)?;
            }
        }
        Outcome::MarkerUnexplained => s.note_sector(inst, "marker after first flash of a maintenance build", now)?,
        Outcome::Unclear => {
            s.add_master_block(inst, expect_key(plan.after), now)?;
        }
        Outcome::UnknownImage => s.note_sector(inst, "unknown image on the master", now)?,
        Outcome::AsExpected => {}
    }
    if plan.recovery {
        s.note_sector(inst, "0x23000 unexplained, recovered to original without stop", now)?;
    }
    Ok(())
}

/// After a successful flash of the master with a plan: compare 0x64 with the plan's expectation and store
/// the result. In the sequential way (`sequential`: one adapter for both buses) the control channel is not
/// reachable now; the result is then what an unanswered read gives (an unclear update for every row with an
/// expectation), and the user's later check clears it. Returns the outcome key and whether it was stored.
pub fn post_master_check(store: &Arc<Mutex<Option<Store>>>, inst: &Installation, plan: &policy::Plan, sequential: bool) -> (&'static str, bool) {
    let probe = if sequential { None } else { read_settled(inst) };
    let outcome = policy::after_flash(plan.after, probe);
    let now = crate::history::now_ms();
    if with_store_arc(store, |s| apply_master_outcome(s, &inst.id, plan, outcome, now)).is_err() {
        // nothing was stored: never report a passed check then, and keep the update-interrupted block
        return (outcome_key(Outcome::Unclear), false);
    }
    (outcome_key(outcome), true)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // the UI named an installation, but the port belongs to none: assign it first, never a new installation
        assert!(refused_with(port_gate(None, Some("a"), false), "port-not-assigned"));
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
        assert_eq!(finish_block(true, true, Some(Progress::PostCheck), || unreachable!()), BlockAction::Clear("flashed"));
        assert_eq!(finish_block(false, true, Some(Progress::Preflight), || unreachable!()), BlockAction::Clear("preflight"));
        assert_eq!(finish_block(false, true, Some(Progress::Erase), || true), BlockAction::Clear("answered"));
        assert_eq!(finish_block(false, true, Some(Progress::Erase), || false), BlockAction::Set("update-unclear"));
        assert_eq!(finish_block(false, true, Some(Progress::Bootloader), || false), BlockAction::Set("bootloader-unclear"));
        // the master's post-check outcome could not be stored (fully): the record of the restart stays
        assert_eq!(finish_block(true, false, Some(Progress::PostCheck), || unreachable!()), BlockAction::Set("update-interrupted"));
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
        let locked = Some(&StoredMasterState::MarkerUnexplained);
        assert!(account_allowed(None, locked).is_ok());
        assert!(account_allowed(Some(&session(SessionKind::Marker)), locked).is_ok());
        assert!(refused_with(account_allowed(Some(&session(SessionKind::Dip)), locked), "dip-session-open"));
    }

    #[test]
    fn account_marker_only_for_an_unexplained_marker() {
        assert!(refused_with(account_allowed(None, None), "not-marker-unexplained"));
        assert!(refused_with(account_allowed(None, Some(&StoredMasterState::PatchedOn)), "not-marker-unexplained"));
        assert!(refused_with(account_allowed(None, Some(&StoredMasterState::PatchedOff { slaves: 3 })), "not-marker-unexplained"));
    }

    #[test]
    fn slave_count_is_one_to_fifteen_or_a_verified_zero() {
        for verified in [false, true] {
            assert!(slaves_allowed(1, verified).is_ok());
            assert!(slaves_allowed(15, verified).is_ok());
            assert!(refused_with(slaves_allowed(16, verified), "slaves-range"));
        }
        // a single pack only when a fresh read of the master showed 0/0
        assert!(refused_with(slaves_allowed(0, false), "slaves-range"));
        assert!(slaves_allowed(0, true).is_ok());
    }

    #[test]
    fn only_a_patched_master_with_marker_off_and_zero_slaves_reports_zero() {
        use StoredMasterState as S;
        assert!(reports_zero(Some(&S::PatchedOff { slaves: 0 })));
        assert!(!reports_zero(Some(&S::PatchedOff { slaves: 1 })));
        assert!(!reports_zero(Some(&S::PatchedOn)));
        assert!(!reports_zero(Some(&S::MarkerUnexplained)));
        assert!(!reports_zero(Some(&S::No0x64)));
        assert!(!reports_zero(Some(&S::Unknown)));
        assert!(!reports_zero(None));
    }

    #[test]
    fn slave_count_changes_only_in_an_open_session() {
        let on = StoredMasterState::PatchedOn;
        let ses = session(SessionKind::Dip);
        assert!(set_slaves_allowed(Some(&ses), Some(&on), 4, false).is_ok());
        assert!(set_slaves_allowed(Some(&ses), None, 4, false).is_ok());
        assert!(refused_with(set_slaves_allowed(None, Some(&on), 4, false), "no-session"));
        assert!(refused_with(set_slaves_allowed(Some(&ses), Some(&on), 0, false), "slaves-range"));
        // N = 0 only for a single pack proven in the command
        let zero = StoredMasterState::PatchedOff { slaves: 0 };
        assert!(set_slaves_allowed(Some(&ses), Some(&zero), 0, true).is_ok());
        assert!(refused_with(set_slaves_allowed(Some(&ses), Some(&zero), 0, false), "slaves-range"));
        assert!(refused_with(set_slaves_allowed(Some(&ses), Some(&StoredMasterState::MarkerUnexplained), 4, false), "marker-unexplained"));
        assert!(refused_with(set_slaves_allowed(Some(&ses), Some(&StoredMasterState::MarkerUnexplained), 0, true), "marker-unexplained"));
    }

    #[test]
    fn a_single_pack_needs_the_master_at_zero_and_no_slave_on_the_bus() {
        use StoredMasterState as S;
        assert!(single_pack(Some(&S::PatchedOff { slaves: 0 }), None));
        // a bank master with DIP 5-8 OFF also reports 0/0, but its slaves answer
        assert!(!single_pack(Some(&S::PatchedOff { slaves: 0 }), Some(7)));
        assert!(!single_pack(Some(&S::PatchedOff { slaves: 2 }), None));
        assert!(!single_pack(Some(&S::PatchedOn), None));
        assert!(!single_pack(Some(&S::Unknown), None));
        assert!(!single_pack(None, None));
    }

    #[test]
    fn values_pause_covers_a_full_polling_round() {
        assert_eq!(values_pause(0), std::time::Duration::from_secs(2));
        assert_eq!(values_pause(1), std::time::Duration::from_secs(2));
        assert_eq!(values_pause(11), std::time::Duration::from_millis(11 * 150 + 1000));
        assert_eq!(values_pause(15), std::time::Duration::from_millis(15 * 150 + 1000));
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

    #[test]
    fn a_check_clears_only_a_master_block_it_resolves() {
        let st = |marker, n| Some(Probe::Status(MaintStatus { marker, effective_slaves: n }));
        let master = |id, expect: &str| FlashBlock { id, pack: 0, reason: "master-unclear".into(), expect: Some(expect.into()), since: 0 };
        let blocks = [master(1, "marker-on"), master(2, "unsupported"), block(3)];
        assert_eq!(resolved_master_blocks(&blocks, st(true, 0), true), vec![1]);
        assert_eq!(resolved_master_blocks(&blocks, Some(Probe::Unsupported), true), vec![2]);
        // a pack block is never cleared by a master check; a failed read clears nothing
        assert!(resolved_master_blocks(&[block(3)], st(true, 0), true).is_empty());
        assert!(resolved_master_blocks(&blocks, None, false).is_empty());
    }

    fn temp_store(name: &str) -> (Store, String) {
        let dir = std::env::temp_dir().join(format!("openbms-maint8-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::open(&dir.join("maintenance.sqlite")).unwrap();
        let id = s.installation_for("tcp:gw:1").unwrap().id;
        (s, id)
    }

    #[test]
    fn post_flash_outcome_is_stored_by_the_table() {
        use policy::{Expect as E, Outcome as O, Plan};
        let plan = |after, recovery| Plan { before: vec![], after, recovery };
        // back on the original as expected: the session continues as a DIP session
        let (s, i) = temp_store("orig");
        s.start_session(&i, SessionKind::Marker, 2, 0).unwrap();
        apply_master_outcome(&s, &i, &plan(E::Unsupported, false), O::AsExpected, 5).unwrap();
        assert_eq!(s.master_state(&i).unwrap().unwrap().0, StoredMasterState::No0x64);
        assert_eq!(s.open_session(&i).unwrap().unwrap().kind, SessionKind::Dip);
        assert!(s.blocks(&i).unwrap().is_empty());
        // unclear: unknown plus a master block with the expectation, also without a session
        let (s, i) = temp_store("unclear");
        apply_master_outcome(&s, &i, &plan(E::MarkerOn, false), O::Unclear, 5).unwrap();
        assert_eq!(s.master_state(&i).unwrap().unwrap().0, StoredMasterState::Unknown);
        let b = s.blocks(&i).unwrap();
        assert_eq!((b[0].reason.as_str(), b[0].expect.as_deref()), ("master-unclear", Some("marker-on")));
        // marker after the first flash of a maintenance build: locked, with a sector note
        let (s, i) = temp_store("unexpl");
        apply_master_outcome(&s, &i, &plan(E::ZeroOrUnexplained, false), O::MarkerUnexplained, 5).unwrap();
        assert_eq!(s.master_state(&i).unwrap().unwrap().0, StoredMasterState::MarkerUnexplained);
        assert!(s.sector_note(&i).unwrap().is_some());
        // the recovery row leaves its note
        let (s, i) = temp_store("recovery");
        apply_master_outcome(&s, &i, &plan(E::Unsupported, true), O::AsExpected, 5).unwrap();
        assert!(s.sector_note(&i).unwrap().unwrap().contains("recovered to original without stop"));
        assert_eq!(s.master_state(&i).unwrap().unwrap().0, StoredMasterState::No0x64);
        // an unclear recovery keeps the lock, and still blocks
        let (s, i) = temp_store("recovery-unclear");
        s.set_master_state(&i, &StoredMasterState::MarkerUnexplained, 1).unwrap();
        apply_master_outcome(&s, &i, &plan(E::Unsupported, true), O::Unclear, 5).unwrap();
        assert_eq!(s.master_state(&i).unwrap().unwrap().0, StoredMasterState::MarkerUnexplained);
        assert_eq!(s.blocks(&i).unwrap()[0].reason, "master-unclear");
    }

    #[test]
    fn end_steps_run_in_order_and_start_over_after_a_failure() {
        use EndStep::*;
        use SessionKind::{Dip, Marker};
        let fresh = end_progress_of(None, 7);
        // Values before Bus is refused, for both kinds
        assert!(refused_with(end_next(&fresh, Dip, 11, true, Values), "end-order"));
        assert!(refused_with(end_next(&fresh, Marker, 11, true, Values), "end-order"));
        // Bus before Stop: refused for a marker session, allowed for a DIP session (no stop there)
        assert!(refused_with(end_next(&fresh, Marker, 11, true, Bus), "end-order"));
        assert!(end_next(&fresh, Dip, 11, true, Bus).is_ok());
        assert!(end_next(&fresh, Marker, 11, true, Stop).is_ok());
        let stopped = end_record(fresh, Stop, true);
        assert!(end_next(&stopped, Marker, 11, true, Bus).is_ok());
        assert!(refused_with(end_next(&stopped, Marker, 11, true, Values), "end-order"));
        let bus = end_record(stopped, Bus, true);
        assert!(end_next(&bus, Marker, 11, true, Values).is_ok());
        // a failure starts over
        let failed = end_record(bus, Values, false);
        assert_eq!(failed, EndProgress { session: 7, stopped: false, bus_ok: false });
        assert!(refused_with(end_next(&failed, Marker, 11, true, Bus), "end-order"));
        // a new stop makes the earlier bus check stale
        assert!(!end_record(bus, Stop, true).bus_ok);
        // progress of another session does not count
        assert_eq!(end_progress_of(Some(&bus), 8), EndProgress { session: 8, stopped: false, bus_ok: false });
        assert_eq!(end_progress_of(Some(&bus), 7), bus);
        // a single pack (N = 0) ends in the same order when there is a control channel (its Values step needs
        // a fresh 0/0); without one it is refused at every step until the user corrects N
        assert!(end_next(&fresh, Dip, 0, true, Bus).is_ok());
        assert!(refused_with(end_next(&fresh, Dip, 0, true, Values), "end-order"));
        assert!(end_next(&bus, Dip, 0, true, Values).is_ok());
        assert!(refused_with(end_next(&fresh, Marker, 0, true, Bus), "end-order"));
        for step in [Stop, Bus, Values] {
            assert!(refused_with(end_next(&bus, Dip, 0, false, step), "slaves-range"));
            assert!(refused_with(end_next(&bus, Marker, 0, false, step), "slaves-range"));
            assert!(refused_with(end_next(&bus, Dip, 16, true, step), "slaves-range"));
        }
        assert!(end_next(&bus, Dip, 11, false, Values).is_ok());
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
