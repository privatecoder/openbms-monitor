//! Maintenance of a bank (design: docs/superpowers/specs/2026-10-13-simple-flash-flow.md): the live status of
//! the pack bus and the master, the pack list, the master's marker (`start`/`stop` over the CAN socket bus), the
//! right-bank check and the packs that may hang in their bootloader. All bus operations hold the one operation
//! lock (`port_busy`, through `control::reserve_bus`: only while no pack action runs); a control exchange on the endpoint of the main connection runs through that connection,
//! so no endpoint is opened twice.

use crate::maint_store::{endpoint_key, ControlEndpoint, Installation, MarkerOn, Pending, Store};
use crate::{lock, AppState, EndpointArg};
use openbms_flash::maint::dialog::{self, Action, Budgets, DialogError};
use openbms_flash::maint::policy::Master;
use openbms_flash::maint::{bus, SystemClock};
use openbms_flash::session::Progress;
use openbms_proto::{DeviceInfo, MaintStatus, MarkerWord, SerialValue};
use openbms_transport::{Bus, Connection, Endpoint};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

/// The maintenance event log (`firmware-logs/maint-events.log`): what the live status saw whenever it changes,
/// and every halt, release and flash with its result. For bench work and for finding out afterwards who
/// changed the marker.
pub fn event(state: &AppState, installation: Option<&str>, line: &str) {
    use std::io::Write;
    let Some(app) = state.app.get() else { return };
    let Ok(dir) = tauri::Manager::path(app).app_data_dir().map(|d| d.join("firmware-logs")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let stamp = format!("{}.{:03}", crate::firmware::utc_stamp(now.as_secs()), now.subsec_millis());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("maint-events.log")) {
        let _ = writeln!(f, "{}", event_line(&stamp, installation, line));
    }
}

/// One line of the event log: time, installation id („-“ without one), what happened.
pub fn event_line(stamp: &str, installation: Option<&str>, line: &str) -> String {
    format!("{stamp} inst={} {line}", installation.unwrap_or("-"))
}

/// One line for a master read: kind, version, marker word, effective slaves.
fn master_text(m: &MasterView) -> String {
    format!("master={} version={} marker={} word={} slaves={}", m.kind, m.version.as_deref().unwrap_or("-"),
        m.marker.unwrap_or("-"), m.marker_word.as_deref().unwrap_or("-"), m.slaves.map_or("-".into(), |n| n.to_string()))
}

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
    Store { message: String },
    /// the master at this pack bus answers with another BMS serial number than the installation stored
    OtherBank { serial: String },
    /// a pack on this pack bus endpoint may hang in its bootloader: no other address is flashed there
    BootloaderPending { pack: u8 },
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

pub fn refused(reason: &str) -> MaintError {
    MaintError::Refused { reason: reason.into() }
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

/// A store refusal (name) reaches the UI as a refusal key, anything else as a store error.
pub fn store_refusal(e: MaintError) -> MaintError {
    match e {
        MaintError::Store { message } if ["name-required", "name-taken", "unknown-installation", "installation-busy", "merge-self", "installation-connected"].contains(&message.as_str()) => refused(&message),
        other => other,
    }
}

pub fn installation(state: &AppState, id: &str) -> Result<Installation, MaintError> {
    with_store(state, |s| s.installation(id)).map_err(store_refusal)
}

/// The operation that holds the port now, for the sidebar's "port busy" text; cleared on drop.
pub struct ActionScope<'a>(&'a AppState);

impl Drop for ActionScope<'_> {
    fn drop(&mut self) {
        if let Ok(mut a) = self.0.maint_action.lock() {
            *a = None;
        }
    }
}

pub fn begin_action<'a>(state: &'a AppState, action: &'static str) -> ActionScope<'a> {
    if let Ok(mut a) = state.maint_action.lock() {
        *a = Some(action);
    }
    ActionScope(state)
}

// --- control channel (CAN socket bus) ---

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

fn control_endpoint(c: &ControlEndpoint) -> Endpoint {
    match c {
        ControlEndpoint::Serial { path } => Endpoint::Serial { path: path.clone(), baud: Bus::CanSocketBus.baud() },
        ControlEndpoint::Tcp { addr } => Endpoint::Tcp { addr: addr.clone() },
    }
}

/// A link error that no retry on the same connection heals (the polling thread's rule): the gateway closed
/// the socket, a broken pipe, any other I/O error.
pub fn link_dead(message: &str) -> bool {
    message.contains("I/O") || message.contains("connection closed") || message.ends_with("not connected")
}

/// After an exchange through the main connection failed with a dead link, the exchange runs once more on a
/// freshly opened control endpoint. An exchange on an own connection, or any other failure, is not repeated.
pub fn retry_fresh(through_main: bool, error: Option<&str>) -> bool {
    through_main && error.is_some_and(link_dead)
}

/// Drop a dead main connection like the polling thread does on link loss.
fn drop_dead_main(state: &AppState, error: &str) {
    if let Ok(p) = state.polling.lock() {
        p.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    if let Ok(mut c) = state.conn.lock() {
        *c = None;
    }
    if let Ok(mut m) = state.main.lock() {
        *m = None;
    }
    crate::new_link(state, None);
    if let Ok(mut h) = state.history.lock() {
        h.disconnected();
    }
    if let Some(app) = state.app.get() {
        let _ = app.emit("link-lost", error.to_string());
    }
}

/// Run `f` on the control channel: through the main connection when it is that endpoint on the CAN socket
/// bus, otherwise on a connection opened for this exchange. A dead main connection is dropped and the
/// exchange repeated once on its own channel (`retry_fresh`).
pub fn with_control<T, E: std::fmt::Display>(state: &AppState, inst: &Installation, mut f: impl FnMut(&mut Connection) -> Result<T, E>) -> Result<Result<T, E>, MaintError> {
    let control = inst.control.as_ref().ok_or_else(|| refused("no-control-channel"))?;
    let main = state.main.lock().map_err(|e| store_err(e.to_string()))?.clone();
    let key = control.key();
    let mut guard = lock(&state.conn).map_err(|m| MaintError::Connection { message: m })?;
    let open = |f: &mut dyn FnMut(&mut Connection) -> Result<T, E>| -> Result<Result<T, E>, MaintError> {
        let mut c = Connection::open(&control_endpoint(control)).map_err(|e| MaintError::Connection { message: e.to_string() })?;
        Ok(f(&mut c))
    };
    match route_with(main.as_ref(), &key, guard.is_some()) {
        Route::Main => {
            let c = guard.as_mut().ok_or_else(|| MaintError::Connection { message: "not connected".into() })?;
            let r = f(c);
            let error = r.as_ref().err().map(|e| e.to_string());
            if !retry_fresh(true, error.as_deref()) {
                return Ok(r);
            }
            drop(guard);
            drop_dead_main(state, error.as_deref().unwrap_or_default());
            open(&mut f)
        }
        Route::Refused => Err(refused("control-on-pack-bus")),
        Route::Open => {
            drop(guard);
            open(&mut f)
        }
    }
}

/// The state a `start` (1/0) or `stop` (marker off) waits for is already there.
pub fn target_reached(action: Action, s: MaintStatus) -> bool {
    match action {
        Action::Status => true,
        Action::Start => s.marker && s.effective_slaves == 0,
        Action::Stop => !s.marker,
    }
}

/// `start`/`stop` over the control channel. A repeat on a fresh channel (`with_control`) reads the status
/// first: a target already reached is not sent again.
fn control_dialog(state: &AppState, inst: &Installation, action: Action) -> Result<Result<MaintStatus, DialogError>, MaintError> {
    let mut retry = false;
    with_control(state, inst, |c| {
        if std::mem::replace(&mut retry, true) {
            if let Ok(s) = dialog::dialog(c, &SystemClock::new(), Action::Status, None, Budgets::default()) {
                if target_reached(action, s) {
                    return Ok(s);
                }
            }
        }
        dialog::dialog(c, &SystemClock::new(), action, None, Budgets::default())
    })
}

/// The master as the UI shows it (status line, panel buttons).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MasterView {
    /// "maint" (known build) | "unknown-build" | "legacy" (MAINT 1.0 compatible) | "no-0x64" | "unknown"
    pub kind: &'static str,
    /// what the master reports, e.g. "16.06.13 MAINT 1.1"
    pub version: Option<String>,
    pub build_id: Option<String>,
    /// "off" | "on" | "foreign"
    pub marker: Option<&'static str>,
    /// the raw marker word (MAINT 1.1), 8 hex digits
    pub marker_word: Option<String>,
    pub slaves: Option<u8>,
    pub may_halt: bool,
    pub may_release: bool,
}

pub fn master_view(m: &Master) -> MasterView {
    let info = match m {
        Master::Maint { info, .. } | Master::UnknownBuild(info) => Some(info),
        _ => None,
    };
    MasterView {
        kind: match m {
            Master::Maint { .. } => "maint",
            Master::UnknownBuild(_) => "unknown-build",
            Master::Legacy(_) => "legacy",
            Master::No0x64 => "no-0x64",
            Master::Unknown => "unknown",
        },
        version: info.map(|i| i.version()),
        build_id: info.map(|i| i.build_id_hex()),
        marker: m.marker().map(|w| match w {
            MarkerWord::Off => "off",
            MarkerWord::On => "on",
            MarkerWord::Foreign(_) => "foreign",
        }),
        marker_word: info.map(|i| format!("{:08X}", i.marker_word)),
        slaves: m.effective_slaves(),
        may_halt: m.may_halt(),
        may_release: m.may_release(),
    }
}

/// Read the master (`03` → RTN 05 → `02`) and keep what it showed for the banner. A read without a valid answer
/// never replaces the last one: a set marker stays visible until a read shows it off.
pub fn read_master(state: &AppState, inst: &Installation) -> Result<Master, MaintError> {
    let probe = with_control(state, inst, |c| dialog::probe(c, &SystemClock::new(), Budgets::default()))?
        .map_err(|e| MaintError::Connection { message: e.to_string() })?;
    let m = Master::of(Some(probe));
    if m != Master::Unknown {
        let v = master_view(&m);
        let data = serde_json::to_string(&v).unwrap_or_default();
        with_store(state, |s| s.set_master_read(&inst.id, &data, v.marker == Some("on"), crate::history::now_ms()))?;
    }
    Ok(m)
}

// --- pack bus ---

const PACK_BUS_BAUD: u32 = 19_200;
/// A little more than one polling cycle of the master (every slave once a second).
pub const LISTEN: Duration = Duration::from_millis(1200);

/// Before the pack bus is opened: the monitor holds that adapter, or it polls the pack bus.
pub fn must_stop_monitor(main: Option<&(String, Bus)>, pack_key: &str) -> bool {
    main.is_some_and(|(key, bus)| matches!(bus, Bus::PackBus) || key == pack_key)
}

/// Open the pack bus port; first stop the monitor when it holds that adapter or polls the pack bus, and tell
/// the UI ("monitor-stopped"): a stop by the app is no lost connection.
pub fn open_pack_for(state: &AppState, app: &AppHandle, port: &str) -> Result<Connection, MaintError> {
    let main = state.main.lock().map_err(|e| store_err(e.to_string()))?.clone();
    if must_stop_monitor(main.as_ref(), &endpoint_key(false, port)) {
        crate::stop_monitor_and_tell(state, app).map_err(|m| MaintError::Connection { message: m })?;
    }
    Connection::open(&Endpoint::Serial { path: port.into(), baud: PACK_BUS_BAUD }).map_err(|e| MaintError::Connection { message: e.to_string() })
}

/// "quiet" (no byte), "polling" (the master polls slaves) or "traffic" (bytes, but no poll of the master).
pub fn bus_state(t: &bus::Traffic) -> &'static str {
    if t.quiet() {
        "quiet"
    } else if t.polled.is_empty() {
        "traffic"
    } else {
        "polling"
    }
}

/// The right-bank check against the master's BMS serial number (0xA4 at address 0 on the pack bus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bank {
    Match,
    /// first read for this installation: stored now
    Stored(String),
    /// another bank hangs on this pack bus
    Mismatch(String),
    /// no serial number to compare (no answer, not offered, empty, the factory placeholder)
    Unchecked,
}

pub fn bank_check(stored: Option<&str>, read: &SerialValue) -> Bank {
    match (read, stored) {
        (SerialValue::Set { value, .. }, None) => Bank::Stored(value.clone()),
        (SerialValue::Set { value, .. }, Some(s)) if s == value => Bank::Match,
        (SerialValue::Set { value, .. }, Some(_)) => Bank::Mismatch(value.clone()),
        _ => Bank::Unchecked,
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct BankView {
    /// "match" | "stored" | "mismatch" | "unchecked" | "none" (no installation selected)
    pub state: &'static str,
    pub serial: Option<String>,
}

pub fn bank_view(b: &Bank) -> BankView {
    match b {
        Bank::Match => BankView { state: "match", serial: None },
        Bank::Stored(s) => BankView { state: "stored", serial: Some(s.clone()) },
        Bank::Mismatch(s) => BankView { state: "mismatch", serial: Some(s.clone()) },
        Bank::Unchecked => BankView { state: "unchecked", serial: None },
    }
}

/// Read the master's BMS serial number on the open pack bus link and compare it with the installation; the first
/// value is stored without asking.
pub fn check_bank(store: &Arc<Mutex<Option<Store>>>, inst: &Installation, link: &mut Connection) -> Result<Bank, MaintError> {
    let read = match link.bms_serial(0) {
        Ok(v) => v,
        Err(e) if link_dead(&e.to_string()) => return Err(MaintError::Connection { message: e.to_string() }),
        Err(_) => SerialValue::NoAnswer,
    };
    let b = bank_check(inst.master_serial.as_deref(), &read);
    if let Bank::Stored(s) = &b {
        with_store_arc(store, |st| st.set_master_serial(&inst.id, s, crate::history::now_ms()))?;
    }
    Ok(b)
}

/// The bootloader-pending rule: flashing another address than a pending pack on the same endpoint is refused.
pub fn pending_refusal(pending: &[Pending], address: u8) -> Result<(), MaintError> {
    match pending.iter().find(|p| p.pack != address) {
        Some(p) => Err(MaintError::BootloaderPending { pack: p.pack }),
        None => Ok(()),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AfterFlash {
    /// clear the pending record, with this `how`
    Clear(&'static str),
    /// the pack may hang in its bootloader: the record stays
    Keep,
}

/// What happens to the pending record written before the restart, once the flash returned. A failure in the
/// preflight changed nothing; after the restart the pack counts as answering only when it does (`answers` asks
/// for 0x51/0x47 and runs only when that decides).
pub fn after_flash(ok: bool, last: Option<Progress>, answers: impl FnOnce() -> bool) -> AfterFlash {
    if ok {
        return AfterFlash::Clear("flashed");
    }
    match last {
        None | Some(Progress::Preflight) => AfterFlash::Clear("preflight"),
        _ if answers() => AfterFlash::Clear("answered"),
        _ => AfterFlash::Keep,
    }
}

/// After the flash returned, before the pack port is closed: settle the pending record. Returns whether it stays
/// (a record that cannot be cleared stays too).
pub fn settle_pending(store: &Arc<Mutex<Option<Store>>>, pending: i64, address: u8, ok: bool, last: Option<Progress>, link: &mut Connection) -> bool {
    match after_flash(ok, last, || bus::app_answers(link, &mut std::io::sink(), address, Duration::from_secs(6))) {
        AfterFlash::Clear(how) => with_store_arc(store, |s| s.clear_pending(pending, how, crate::history::now_ms())).is_err(),
        AfterFlash::Keep => true,
    }
}

// --- commands ---

/// All installations by creation, for the picker.
#[tauri::command]
pub async fn maint_installations(state: State<'_, AppState>) -> Result<Vec<Installation>, MaintError> {
    with_store(&state, |s| s.installations())
}

/// The installations that list this endpoint or have it as CAN socket (lookup only). `active_only`: without archived
/// installations (for preselection).
#[tauri::command]
pub async fn maint_installations_of(state: State<'_, AppState>, endpoint: EndpointArg, active_only: Option<bool>) -> Result<Vec<Installation>, MaintError> {
    let key = endpoint.key();
    with_store(&state, |s| s.installations_of(&key, active_only.unwrap_or(false)))
}

#[tauri::command]
pub async fn maint_save_installation(state: State<'_, AppState>, installation: Installation) -> Result<Installation, MaintError> {
    with_store(&state, |s| {
        s.save_installation(&installation)?;
        s.installation(&installation.id)
    })
    .map_err(store_refusal)
}

#[tauri::command]
pub async fn maint_create_installation(state: State<'_, AppState>, name: String, control: Option<ControlEndpoint>, endpoints: Vec<String>) -> Result<Installation, MaintError> {
    with_store(&state, |s| s.create_installation(&name, control, endpoints)).map_err(store_refusal)
}

/// „Diese Anlage hat einen neuen Master“: the serial number the master now reports becomes the stored one.
#[tauri::command]
pub async fn maint_adopt_master_serial(state: State<'_, AppState>, installation: String, serial: String) -> Result<Installation, MaintError> {
    if serial.trim().is_empty() {
        return Err(refused("no-serial"));
    }
    with_store(&state, |s| {
        s.set_master_serial(&installation, serial.trim(), crate::history::now_ms())?;
        s.installation(&installation)
    })
    .map_err(store_refusal)
}

/// What stays visible until it is resolved: a set marker and packs that may hang in their bootloader.
#[derive(Serialize)]
pub struct Alerts {
    markers: Vec<MarkerOn>,
    pending: Vec<Pending>,
}

#[tauri::command]
pub async fn maint_alerts(state: State<'_, AppState>) -> Result<Alerts, MaintError> {
    with_store(&state, |s| Ok(Alerts { markers: s.markers_on()?, pending: s.all_pending()? }))
}

/// The user's confirmation „Pack X ist abgeklemmt oder repariert“.
#[tauri::command]
pub async fn maint_clear_pending(state: State<'_, AppState>, id: i64) -> Result<(), MaintError> {
    with_store(&state, |s| s.clear_pending(id, "confirmed", crate::history::now_ms()))
}

#[derive(Serialize)]
pub struct LiveStatus {
    /// "quiet" | "polling" | "traffic" | "no-port" (not set or cannot be opened) | "error"
    bus: &'static str,
    polled: Vec<u8>,
    bus_error: Option<String>,
    /// with a CAN socket endpoint: what the master reported (kind "unknown" after a failed read)
    master: Option<MasterView>,
    master_error: Option<MaintError>,
}

/// The panel's live status (every 2 s while it is open): a short listen on the pack bus (sends nothing) and,
/// with a CAN socket endpoint, the master read. Refused (busy) while a flash or another operation runs.
#[tauri::command]
pub async fn maint_live(app: AppHandle, state: State<'_, AppState>, installation: Option<String>, port: String) -> Result<LiveStatus, MaintError> {
    if state.flashing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(MaintError::Busy);
    }
    let _op = crate::control::reserve_bus(&state).ok_or(MaintError::Busy)?;
    let _a = begin_action(&state, "live");
    let inst = installation.as_deref().map(|id| self::installation(&state, id)).transpose()?;
    let (bus, polled, bus_error) = if port.is_empty() {
        ("no-port", vec![], None)
    } else {
        match open_pack_for(&state, &app, &port) {
            Err(e) => ("no-port", vec![], Some(serde_json::to_string(&e).unwrap_or_default())),
            Ok(mut link) => match bus::listen(&mut link, &SystemClock::new(), LISTEN) {
                Ok(t) => (bus_state(&t), t.polled.into_iter().collect(), None),
                Err(e) => ("error", vec![], Some(e.to_string())),
            },
        }
    };
    let (master, master_error) = match &inst {
        Some(i) if i.control.is_some() => match read_master(&state, i) {
            Ok(m) => (Some(master_view(&m)), None),
            Err(e) => (Some(master_view(&Master::Unknown)), Some(e)),
        },
        _ => (None, None),
    };
    // only changes, else one line every 2 s
    let line = format!("live bus={bus} polled={polled:?}{} {}{}", bus_error.as_deref().map(|e| format!(" bus_error={e}")).unwrap_or_default(),
        master.as_ref().map(master_text).unwrap_or_else(|| "master=-".into()),
        master_error.as_ref().map(|e| format!(" master_error={}", serde_json::to_string(e).unwrap_or_default())).unwrap_or_default());
    if let Ok(mut last) = LAST_LIVE.lock() {
        let key = format!("{installation:?} {line}");
        if *last != key {
            event(&state, installation.as_deref(), &line);
            *last = key;
        }
    }
    Ok(LiveStatus { bus, polled, bus_error, master, master_error })
}

#[derive(Serialize)]
pub struct PackScan {
    packs: Vec<(u8, DeviceInfo)>,
    bank: BankView,
    /// pending bootloader records cleared because their pack answered
    cleared: Vec<u8>,
}

/// One `0x51` scan of addresses 0–15 on the pack bus (when the bus became quiet, or the re-scan button). A pending
/// pack that answers is cleared; with an installation selected the master's serial number is compared.
#[tauri::command]
pub async fn maint_scan_packs(app: AppHandle, state: State<'_, AppState>, installation: Option<String>, port: String) -> Result<PackScan, MaintError> {
    if port.is_empty() {
        return Err(refused("no-port"));
    }
    if state.flashing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(MaintError::Busy);
    }
    let _op = crate::control::reserve_bus_wait(&state).ok_or(MaintError::Busy)?;
    let _a = begin_action(&state, "scan");
    let inst = installation.as_deref().map(|id| self::installation(&state, id)).transpose()?;
    let mut link = open_pack_for(&state, &app, &port)?;
    let packs = link.scan(0..=15);
    let found: Vec<u8> = packs.iter().map(|(a, _)| *a).collect();
    let cleared = with_store(&state, |s| s.clear_answered(&endpoint_key(false, &port), &found, crate::history::now_ms()))?;
    let bank = match &inst {
        Some(i) if found.contains(&0) => bank_view(&check_bank(&state.maint, i, &mut link)?),
        Some(_) => bank_view(&Bank::Unchecked),
        None => BankView { state: "none", serial: None },
    };
    Ok(PackScan { packs, bank, cleared })
}

/// „Master anhalten“: `start` on a trusted maintenance build with the marker off, then a fresh read.
#[tauri::command]
pub async fn maint_halt(state: State<'_, AppState>, installation: String) -> Result<MasterView, MaintError> {
    marker_action(&state, &installation, Action::Start)
}

/// „Master freigeben“: `stop`, only when 0x64 shows the marker on (never automatic), then a fresh read.
#[tauri::command]
pub async fn maint_release(state: State<'_, AppState>, installation: String) -> Result<MasterView, MaintError> {
    marker_action(&state, &installation, Action::Stop)
}

static LAST_LIVE: Mutex<String> = Mutex::new(String::new());

fn marker_action(state: &AppState, installation: &str, action: Action) -> Result<MasterView, MaintError> {
    let name = if action == Action::Start { "halt" } else { "release" };
    event(state, Some(installation), &format!("{name} pressed"));
    let r = marker_action_inner(state, installation, action);
    match &r {
        Ok(m) => event(state, Some(installation), &format!("{name} done {}", master_text(m))),
        Err(e) => event(state, Some(installation), &format!("{name} failed {}", serde_json::to_string(e).unwrap_or_default())),
    }
    if let Ok(mut last) = LAST_LIVE.lock() {
        last.clear();
    }
    r
}

fn marker_action_inner(state: &AppState, installation: &str, action: Action) -> Result<MasterView, MaintError> {
    if state.flashing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(MaintError::Busy);
    }
    let _op = crate::control::reserve_bus_wait(state).ok_or(MaintError::Busy)?;
    let _a = begin_action(state, if action == Action::Start { "halt" } else { "release" });
    let inst = self::installation(state, installation)?;
    let m = read_master(state, &inst)?;
    let allowed = if action == Action::Start { m.may_halt() } else { m.may_release() };
    if !allowed {
        return Err(refused(if action == Action::Start { "halt-not-offered" } else { "release-not-offered" }));
    }
    let r = control_dialog(state, &inst, action)?;
    // the read after the dialog decides what is shown, also after a failed one
    let after = read_master(state, &inst);
    r?;
    Ok(master_view(&after?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbms_proto::MaintInfo;

    fn refused_with(r: Result<impl std::fmt::Debug, MaintError>, key: &str) -> bool {
        matches!(r, Err(MaintError::Refused { reason }) if reason == key)
    }

    fn pending(pack: u8) -> Pending {
        Pending { id: 1, endpoint: "serial:/dev/p".into(), pack, release: "16.06.13".into(), since: 0 }
    }

    #[test]
    fn every_event_line_names_its_installation() {
        assert_eq!(event_line("20260102-030405Z.123", Some("inst-1"), "halt pressed"), "20260102-030405Z.123 inst=inst-1 halt pressed");
        assert_eq!(event_line("t", None, "live bus=quiet"), "t inst=- live bus=quiet");
    }

    #[test]
    fn a_pending_pack_blocks_only_other_addresses_on_its_endpoint() {
        assert!(pending_refusal(&[], 3).is_ok());
        assert!(pending_refusal(&[pending(3)], 3).is_ok());
        assert!(matches!(pending_refusal(&[pending(3)], 4), Err(MaintError::BootloaderPending { pack: 3 })));
        let v = serde_json::to_value(MaintError::BootloaderPending { pack: 3 }).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "bootloader-pending", "pack": 3 }));
    }

    #[test]
    fn after_flash_by_stage() {
        assert_eq!(after_flash(true, Some(Progress::PostCheck), || unreachable!()), AfterFlash::Clear("flashed"));
        assert_eq!(after_flash(false, None, || unreachable!()), AfterFlash::Clear("preflight"));
        assert_eq!(after_flash(false, Some(Progress::Preflight), || unreachable!()), AfterFlash::Clear("preflight"));
        assert_eq!(after_flash(false, Some(Progress::Bootloader), || false), AfterFlash::Keep);
        assert_eq!(after_flash(false, Some(Progress::Block { index: 3, total: 200 }), || false), AfterFlash::Keep);
        assert_eq!(after_flash(false, Some(Progress::Erase), || true), AfterFlash::Clear("answered"));
    }

    #[test]
    fn the_right_bank_by_the_masters_serial_number() {
        let set = |v: &str| SerialValue::Set { value: v.into(), truncated: false };
        assert_eq!(bank_check(None, &set("SN-A")), Bank::Stored("SN-A".into()));
        assert_eq!(bank_check(Some("SN-A"), &set("SN-A")), Bank::Match);
        assert_eq!(bank_check(Some("SN-A"), &set("SN-B")), Bank::Mismatch("SN-B".into()));
        for v in [SerialValue::NoAnswer, SerialValue::Unavailable, SerialValue::Empty, SerialValue::Placeholder { value: "EMU110110S-XXXX".into() }] {
            assert_eq!(bank_check(Some("SN-A"), &v), Bank::Unchecked);
            assert_eq!(bank_check(None, &v), Bank::Unchecked);
        }
        assert_eq!(bank_view(&Bank::Mismatch("SN-B".into())), BankView { state: "mismatch", serial: Some("SN-B".into()) });
        let v = serde_json::to_value(MaintError::OtherBank { serial: "SN-B".into() }).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "other-bank", "serial": "SN-B" }));
    }

    #[test]
    fn the_bus_state_of_a_listen() {
        let mut t = bus::Traffic::default();
        assert_eq!(bus_state(&t), "quiet");
        t.bytes = 12;
        assert_eq!(bus_state(&t), "traffic");
        t.polled.insert(1);
        assert_eq!(bus_state(&t), "polling");
    }

    #[test]
    fn the_master_view_names_version_marker_and_actions() {
        let info = |word: u32, id: [u8; 8]| MaintInfo { base: [16, 6, 13], patch: [1, 1], marker_word: word, effective_slaves: 0, build_id: id };
        let known = [0xFE, 0x38, 0x19, 0x85, 0xBF, 0xFF, 0x8A, 0xD0];
        let v = master_view(&Master::of(Some(dialog::Probe::Info(info(0, known)))));
        assert_eq!((v.kind, v.version.as_deref(), v.marker, v.marker_word.as_deref()), ("maint", Some("16.06.13 MAINT 1.1"), Some("on"), Some("00000000")));
        assert!(v.may_release && !v.may_halt);
        let v = master_view(&Master::of(Some(dialog::Probe::Info(info(0x0102_0304, known)))));
        assert_eq!((v.marker, v.may_release, v.may_halt), (Some("foreign"), false, false));
        let v = master_view(&Master::of(Some(dialog::Probe::Info(info(0, [0; 8])))));
        assert_eq!((v.kind, v.may_release), ("unknown-build", false));
        let v = master_view(&Master::No0x64);
        assert_eq!((v.kind, v.marker, v.version), ("no-0x64", None, None));
    }

    #[test]
    fn control_route_uses_main_connection_only_for_same_endpoint_on_can_bus() {
        let can = ("serial:/dev/a".to_string(), Bus::CanSocketBus);
        let pack = ("serial:/dev/a".to_string(), Bus::PackBus);
        assert!(uses_main_connection(Some(&can), "serial:/dev/a"));
        assert!(!uses_main_connection(Some(&can), "serial:/dev/b"));
        assert!(!uses_main_connection(Some(&pack), "serial:/dev/a"));
        assert!(matches!(route_with(Some(&pack), "serial:/dev/a", true), Route::Refused));
        assert!(matches!(route_with(Some(&pack), "serial:/dev/b", true), Route::Open));
        assert!(matches!(route_with(Some(&can), "serial:/dev/a", true), Route::Main));
        assert!(matches!(route_with(Some(&can), "serial:/dev/a", false), Route::Open));
    }

    #[test]
    fn a_dead_main_connection_is_retried_once_on_a_fresh_channel() {
        assert!(retry_fresh(true, Some("connection: transport: I/O: Broken pipe (os error 32)")));
        assert!(retry_fresh(true, Some("connection closed by the gateway")));
        assert!(!retry_fresh(true, Some("the master at address 0 does not answer")));
        assert!(!retry_fresh(true, None));
        assert!(!retry_fresh(false, Some("connection: transport: I/O: Broken pipe")));
    }

    #[test]
    fn a_retried_start_or_stop_continues_from_the_state_found() {
        let st = |marker, slaves| MaintStatus { marker, effective_slaves: slaves };
        assert!(target_reached(Action::Start, st(true, 0)));
        assert!(!target_reached(Action::Start, st(true, 3)));
        assert!(target_reached(Action::Stop, st(false, 3)));
        assert!(!target_reached(Action::Stop, st(true, 0)));
    }

    #[test]
    fn every_pack_bus_open_frees_the_port_first() {
        let pack = |k: &str| (k.to_string(), Bus::PackBus);
        let can = |k: &str| (k.to_string(), Bus::CanSocketBus);
        assert!(must_stop_monitor(Some(&pack("serial:/dev/b")), "serial:/dev/a"));
        assert!(must_stop_monitor(Some(&can("serial:/dev/a")), "serial:/dev/a"));
        assert!(!must_stop_monitor(Some(&can("tcp:gw:4196")), "serial:/dev/a"));
        assert!(!must_stop_monitor(None, "serial:/dev/a"));
    }

    #[test]
    fn dialog_errors_and_refusals_reach_the_ui_typed() {
        use openbms_flash::maint::dialog::DialogError as D;
        assert!(matches!(MaintError::from(D::Unsupported), MaintError::Unsupported));
        assert!(matches!(MaintError::from(D::Link(openbms_flash::Error::Transport("x".into()))), MaintError::Connection { .. }));
        assert_eq!(serde_json::to_value(MaintError::Rejected { rtn: 5 }).unwrap(), serde_json::json!({ "kind": "rejected", "rtn": 5 }));
        assert!(refused_with(Err::<(), _>(store_refusal(store_err("name-taken".into()))), "name-taken"));
        assert!(matches!(store_refusal(store_err("disk full".into())), MaintError::Store { .. }));
    }
}
