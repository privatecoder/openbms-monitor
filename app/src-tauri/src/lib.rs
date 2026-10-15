use openbms_proto::{DeviceInfo, MaintStatus, Parameters, SerialNumbers, Status, SystemValues, Telemetry};
use openbms_transport::{list_serial_ports, Bus, Connection, Endpoint};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

mod bank;
mod bms_memory;
mod bundle;
mod control;
mod installation_bundle;
mod firmware;
mod history;
mod installations;
mod maint;
mod maint_store;
mod migrate;
use history::{Format, Settings, SharedHistory};

#[derive(Default)]
struct AppState {
    conn: Arc<Mutex<Option<Connection>>>,
    bus: Mutex<Option<Bus>>,
    /// Run flag of the current polling thread; each start gets a fresh one, so an old thread cannot stop a new one.
    polling: Mutex<Arc<AtomicBool>>,
    history: SharedHistory,
    /// Set while a firmware update runs; the monitor must not touch the bus then.
    flashing: Arc<AtomicBool>,
    /// Exclusive reservation while opening or using the pack bus for firmware operations.
    port_busy: Arc<AtomicBool>,
    /// Endpoint key and bus of the main connection; a control exchange on that endpoint runs through it.
    main: Mutex<Option<(String, Bus)>>,
    /// Maintenance database (installations, master reads, pending bootloader records); None if it could not be opened.
    maint: Arc<Mutex<Option<maint_store::Store>>>,
    /// The maintenance operation that holds the port now ("live", "scan", "halt", "release", "flash").
    maint_action: Mutex<Option<&'static str>>,
    /// For events from code that has no `AppHandle` (progress, a dead main connection); set once at start-up.
    app: std::sync::OnceLock<AppHandle>,
    /// Set while a BMS history read runs; polling pauses meanwhile.
    memory_busy: Arc<AtomicBool>,
    /// Asks the running history read to stop after the current frame.
    memory_cancel: Arc<AtomicBool>,
    /// The installation of the monitor connection (None: „Nur ansehen“ or not connected).
    link: Arc<Mutex<Link>>,
    /// Reserved packs and installations under a structural operation; see `control.rs`.
    ops: control::Ops,
    /// The unconfirmed packs of the connection (right bank before writing); see `bank.rs`.
    bank: Mutex<bank::BankState>,
}

/// The installation a connection belongs to. `gen` counts connects and disconnects: an answer that arrives after one is
/// never stored under another connection's installation.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Link {
    pub gen: u64,
    pub installation: Option<String>,
}

pub(crate) fn link_now(state: &AppState) -> Link {
    state.link.lock().map(|l| l.clone()).unwrap_or_default()
}

/// A new connection or none: what comes after belongs to it.
pub(crate) fn new_link(state: &AppState, installation: Option<String>) {
    if let Ok(mut l) = state.link.lock() {
        l.gen += 1;
        l.installation = installation;
    }
}

/// Store something for the installation of the connection an answer came on (`before`, taken before the exchange):
/// only while that connection is still the current one, and not while a pack of another installation is unconfirmed
/// on it (`bank.rs`). A failure is logged; it must not fail the exchange.
pub(crate) fn for_link<T>(state: &AppState, before: &Link, what: &str, f: impl FnOnce(&maint_store::Store, &str) -> Result<T, String>) -> Option<T> {
    if bank::foreign(state, before.gen) {
        return None;
    }
    for_link_checked(state, before, what, f)
}

/// `for_link` also on a connection with a foreign pack: for a pack whose identity the caller has just checked (the
/// app's own serial write to a known board).
pub(crate) fn for_link_checked<T>(state: &AppState, before: &Link, what: &str, f: impl FnOnce(&maint_store::Store, &str) -> Result<T, String>) -> Option<T> {
    let inst = before.installation.as_deref()?;
    if link_now(state).gen != before.gen {
        return None;
    }
    match maint::with_store(state, |s| f(s, inst)) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("{what}: {e:?}");
            None
        }
    }
}

#[derive(Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum EndpointArg {
    Serial { path: String, bus: BusArg },
    // The baud rate of a TCP gateway is configured on the gateway itself.
    Tcp { addr: String, bus: BusArg },
}

impl EndpointArg {
    pub fn key(&self) -> String {
        match self {
            EndpointArg::Serial { path, .. } => maint_store::endpoint_key(false, path),
            EndpointArg::Tcp { addr, .. } => maint_store::endpoint_key(true, addr),
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum BusArg {
    Pack,
    Can,
}

impl From<BusArg> for Bus {
    fn from(b: BusArg) -> Self {
        match b {
            BusArg::Pack => Bus::PackBus,
            BusArg::Can => Bus::CanSocketBus,
        }
    }
}

/// Snapshot of one pack, sent to the UI as event "pack".
#[derive(Serialize, Clone)]
struct PackUpdate {
    address: u8,
    telemetry: Option<Telemetry>,
    status: Option<Status>,
    error: Option<String>,
}

/// System values of the master, sent as event "system".
#[derive(Serialize, Clone)]
struct SystemUpdate {
    values: Option<SystemValues>,
    error: Option<String>,
}

type Shared = Arc<Mutex<Option<Connection>>>;

pub(crate) fn lock(m: &Mutex<Option<Connection>>) -> Result<std::sync::MutexGuard<'_, Option<Connection>>, String> {
    m.lock().map_err(|e| e.to_string())
}

pub(crate) const FLASHING: &str = "a firmware update is running";

/// The refusal of `connect` while another operation holds the port, as JSON for the UI (`portBusy` in maint.ts):
/// `{"kind":"busy","action":…}` names the running maintenance action, "flash" for a firmware update, or null
/// (e.g. "check pack"); the UI says so and connects once the port is free.
fn port_busy_error(action: Option<&str>) -> String {
    serde_json::json!({ "kind": "busy", "action": action }).to_string()
}

/// What holds the port now, for `port_busy_error`.
pub(crate) fn current_action(state: &AppState) -> Option<&'static str> {
    let maint = state.maint_action.lock().ok().and_then(|a| *a);
    maint.or_else(|| state.flashing.load(Ordering::SeqCst).then_some("flash"))
}

pub(crate) struct PortSlot(Arc<AtomicBool>);

impl Drop for PortSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn reserve_port(flag: &Arc<AtomicBool>) -> Option<PortSlot> {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .ok()
        .map(|_| PortSlot(flag.clone()))
}

/// Longer than one live status (listen plus master read) and one pack scan (15 silent addresses at 600 ms plus the
/// installation check): a user action pressed during a scan waits for it instead of failing with "busy".
pub(crate) const PORT_WAIT: Duration = Duration::from_secs(15);

/// Run one exchange on the monitor connection; refused during a flash, which owns the bus.
fn with_conn<T>(conn: &Shared, flashing: &AtomicBool, f: impl FnOnce(&mut Connection) -> openbms_transport::Result<T>) -> Result<T, String> {
    if flashing.load(Ordering::SeqCst) {
        return Err(FLASHING.into());
    }
    let mut guard = lock(conn)?;
    let c = guard.as_mut().ok_or("not connected")?;
    f(c).map_err(|e| e.to_string())
}

/// The refusal of a step whose connection changed since its action began, as JSON for the UI (`errorText`).
pub(crate) const LINK_CHANGED: &str = r#"{"kind":"link-changed"}"#;

/// One exchange of an action of several steps: refused once the connection changed since the action began (`at`). The
/// check runs under the connection lock, and a reconnect counts up the generation before it puts in a new connection,
/// so no step can reach another bank's pack.
pub(crate) fn with_link<T>(conn: &Shared, flashing: &AtomicBool, links: &Mutex<Link>, at: &Link, f: impl FnOnce(&mut Connection) -> openbms_transport::Result<T>) -> Result<T, String> {
    if flashing.load(Ordering::SeqCst) {
        return Err(FLASHING.into());
    }
    let mut guard = lock(conn)?;
    if links.lock().map(|l| l.gen).ok() != Some(at.gen) {
        return Err(LINK_CHANGED.into());
    }
    let c = guard.as_mut().ok_or("not connected")?;
    f(c).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_ports() -> Vec<String> {
    list_serial_ports()
}

/// What `connect` did with the installation: its endpoint was linked to it just now.
#[derive(Serialize)]
struct Connected {
    /// RS485-1/2: always false here, the endpoint is linked once the bank is decided (event "bank-linked")
    linked: bool,
    /// the connection's generation, for `bank_reads_done`
    gen: u64,
    /// the first pack address that answered the check (Modbus answer of the master on the CAN socket: 0)
    answered: u8,
}

/// Ask once whether anything answers: on the CAN socket the master's Modbus values first, then 0x51 of 0..=15
/// (the addresses to be polled first), stop at the first answer. Read-only.
fn check_link(conn: &mut Connection, bus: Bus, first: &[u8]) -> Result<u8, openbms_transport::Probe> {
    if bus == Bus::CanSocketBus && conn.system_values().is_ok() {
        return Ok(0);
    }
    let p = conn.probe(openbms_transport::probe_order(first));
    p.answered.ok_or(p)
}

/// The error of a connect whose check found no pack: JSON, so the UI can explain it.
fn no_answer_error(p: &openbms_transport::Probe) -> String {
    serde_json::json!({ "code": "no-answer", "noise": p.noise, "intra": p.intra }).to_string()
}

/// Open the monitor connection for an installation (None: „Nur ansehen“, nothing is stored). The endpoint is linked to
/// the installation after the port opened and before the first recorded row; recorder and stores use the installation
/// for this whole connection.
#[tauri::command]
async fn connect(state: State<'_, AppState>, endpoint: EndpointArg, installation: Option<String>, addresses: Option<Vec<u8>>) -> Result<Connected, String> {
    // not while an action runs on a pack: it would continue on the new connection ("pack": the UI waits and retries)
    let _port_slot = control::reserve_bus(&state)
        .ok_or_else(|| port_busy_error(current_action(&state).or(control::any_pack_action(&state.ops).map(|_| "pack"))))?;
    if state.flashing.load(Ordering::SeqCst) {
        return Err(FLASHING.into());
    }
    // a merge, delete, export or import of this installation runs to its end first
    let ids: Vec<&str> = installation.as_deref().into_iter().collect();
    let _inst = control::reserve_installations(&state.ops, &ids, "connect").map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    if let Some(id) = &installation {
        maint::with_store(&state, |s| s.installation(id)).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    }
    if let Ok(p) = state.polling.lock() {
        p.store(false, Ordering::SeqCst);
    }
    let key = endpoint.key();
    let (ep, bus, control) = match endpoint {
        EndpointArg::Serial { path, bus } => (Endpoint::Serial { path: path.clone(), baud: Bus::from(bus).baud() }, Bus::from(bus), maint_store::ControlEndpoint::Serial { path }),
        EndpointArg::Tcp { addr, bus } => (Endpoint::Tcp { addr: addr.clone() }, Bus::from(bus), maint_store::ControlEndpoint::Tcp { addr }),
    };
    // close the previous link first: a serial port can be open only once, so reconnecting to the same port would fail with "busy"
    lock(&state.conn)?.take();
    // the old connection is gone; control exchanges must not route through it, nothing more is stored for it
    *state.main.lock().map_err(|e| e.to_string())? = None;
    new_link(&state, None);
    if let Ok(mut h) = state.history.lock() {
        h.disconnected();
    }
    let mut conn = Connection::open(&ep).map_err(|e| e.to_string())?;
    // nothing answers (wrong port, bus or baud rate, pack off or in the bootloader): the port is closed again,
    // nothing is linked or recorded
    let answered = check_link(&mut conn, bus, &addresses.unwrap_or_default()).map_err(|p| no_answer_error(&p))?;
    {
        // opening can take seconds; a flash started meanwhile has already dropped the monitor and owns the bus
        let mut slot = lock(&state.conn)?;
        if state.flashing.load(Ordering::SeqCst) {
            return Err(FLASHING.into());
        }
        *slot = Some(conn);
    }
    *state.bus.lock().map_err(|e| e.to_string())? = Some(bus);
    *state.main.lock().map_err(|e| e.to_string())? = Some((key.clone(), bus));
    let can = bus == Bus::CanSocketBus;
    // the CAN socket shows no String 2: linked now. RS485-1/2: only once a serial read decided the bank (`bank.rs`), so
    // a foreign bank never gets this endpoint linked to the installation
    let linked = match &installation {
        Some(id) if can => maint::with_store(&state, |s| s.link_connection(id, &key, control.clone(), can, history::now_ms())).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?,
        _ => false,
    };
    new_link(&state, installation.clone());
    let gen = link_now(&state).gen;
    let pending = (!can && installation.is_some()).then(|| bank::PendingLink { key: key.clone(), control });
    bank::begin(&state, gen, !can, pending);
    if let (Some(id), Ok(mut h)) = (&installation, state.history.lock()) {
        h.connected(id, if can { "can" } else { "pack" });
        // RS485-1/2: nothing is recorded until a serial read shows no pack of another installation (`bank.rs`)
        h.hold(!can);
    }
    Ok(Connected { linked, answered, gen })
}

/// What answered at the other bus's baud rate on a USB port whose connect check found nothing.
#[derive(Serialize)]
struct OtherBus {
    answered: Option<u8>,
    noise: bool,
    intra: bool,
    /// the answer at 9600 baud came from RS485-1/2 (it knows 0x47): old firmware 16.04.06 `_9600`, not the CAN socket
    pack_at_9600: bool,
}

/// Open the USB port at the other bus's baud rate, ask like `connect` does, close it again. Read-only.
#[tauri::command]
async fn probe_other_bus(state: State<'_, AppState>, path: String, bus: BusArg, addresses: Option<Vec<u8>>) -> Result<OtherBus, String> {
    let _port_slot = control::reserve_bus(&state).ok_or_else(|| port_busy_error(current_action(&state).or(control::any_pack_action(&state.ops).map(|_| "pack"))))?;
    if state.flashing.load(Ordering::SeqCst) {
        return Err(FLASHING.into());
    }
    let other = match Bus::from(bus) {
        Bus::PackBus => Bus::CanSocketBus,
        Bus::CanSocketBus => Bus::PackBus,
    };
    let mut conn = Connection::open(&Endpoint::Serial { path, baud: other.baud() }).map_err(|e| e.to_string())?;
    Ok(match check_link(&mut conn, other, &addresses.unwrap_or_default()) {
        Ok(a) => OtherBus { answered: Some(a), noise: false, intra: false, pack_at_9600: other == Bus::CanSocketBus && conn.answers_parameters(a) },
        Err(p) => OtherBus { answered: None, noise: p.noise, intra: p.intra, pack_at_9600: false },
    })
}

/// Stop polling and drop the monitor's connection (also before a firmware update takes the port).
fn stop_monitor(state: &AppState) -> Result<(), String> {
    if let Ok(p) = state.polling.lock() {
        p.store(false, Ordering::SeqCst);
    }
    *lock(&state.conn)? = None;
    if let Ok(mut m) = state.main.lock() {
        *m = None;
    }
    new_link(state, None);
    if let Ok(mut h) = state.history.lock() {
        h.disconnected();
    }
    Ok(())
}

/// `stop_monitor` for a maintenance step that needs the pack port: the UI is told ("monitor-stopped"), so it
/// shows "not connected" instead of waiting for its data timeout and counting a lost connection.
pub(crate) fn stop_monitor_and_tell(state: &AppState, app: &AppHandle) -> Result<(), String> {
    stop_monitor(state)?;
    let _ = app.emit("monitor-stopped", ());
    Ok(())
}

/// Async, so the window never waits: a running pack action (a restart watch up to 15 s) ends first, a history read is
/// asked to stop. After 20 s the monitor stops anyway; the action's next step then finds the connection changed.
#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let _pending = control::begin_disconnect(&state.ops);
    state.memory_cancel.store(true, Ordering::SeqCst);
    let ops = state.ops.clone();
    // waiting blocks: off the async runtime
    tauri::async_runtime::spawn_blocking(move || control::wait_packs_idle(&ops, Duration::from_secs(20))).await.map_err(|e| e.to_string())?;
    stop_monitor(&state)
}

#[tauri::command]
async fn scan(state: State<'_, AppState>, addresses: Option<Vec<u8>>) -> Result<Vec<(u8, DeviceInfo)>, String> {
    // without addresses every DIP address 0..=15 is asked; with a fixed pack count only those
    let addresses = addresses.unwrap_or_else(|| (0..=15).collect());
    with_conn(&state.conn, &state.flashing, |c| Ok(c.scan(addresses)))
}

/// Read the master's system values once and send them like a polling round would (CAN socket bus only).
/// Used right after connecting, before the slower pack search.
#[tauri::command]
async fn system_once(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if *state.bus.lock().map_err(|e| e.to_string())? != Some(Bus::CanSocketBus) {
        return Err("system values exist only on the CAN socket bus".into());
    }
    let before = link_now(&state);
    let r = with_conn(&state.conn, &state.flashing, |c| c.system_values());
    let ok = r.is_ok();
    // an answer of a connection replaced meanwhile is not shown
    if link_now(&state).gen != before.gen {
        return Err("the connection changed".into());
    }
    let _ = app.emit("system", SystemUpdate { values: r.as_ref().ok().cloned(), error: r.err() });
    if ok { Ok(()) } else { Err("no system values".into()) }
}

/// Maintenance state of a patched master (CID2 0x64, CAN socket bus). Original firmware: error.
#[tauri::command]
async fn maint_status(state: State<'_, AppState>) -> Result<MaintStatus, String> {
    if *state.bus.lock().map_err(|e| e.to_string())? != Some(Bus::CanSocketBus) {
        return Err("the maintenance command exists only on the CAN socket bus".into());
    }
    with_conn(&state.conn, &state.flashing, |c| c.maint_status())
}

#[tauri::command]
async fn telemetry(state: State<'_, AppState>, address: u8) -> Result<Telemetry, String> {
    with_conn(&state.conn, &state.flashing, |c| c.telemetry(address))
}

/// A serial number read and what it says about the board at that place of the connection's installation (None:
/// „Nur ansehen“, or not stored), and its identity against the installation (None: „Nur ansehen“).
#[derive(Serialize)]
struct SerialRead {
    #[serde(flatten)]
    serials: SerialNumbers,
    board: Option<maint_store::BoardSeen>,
    identity: Option<bank::IdentityView>,
}

/// Battery and BMS serial number of one pack, as far as the connected bus shows them (RS485-1/2: both; CAN socket
/// bus: the master's battery serial number only, slaves are not asked). The read is classified first (`bank.rs`): only
/// a board known at this place is recorded as seen there; an unconfirmed one waits for `bank_confirm`.
#[tauri::command]
async fn serial_numbers(state: State<'_, AppState>, address: u8) -> Result<SerialRead, String> {
    let bus = state.bus.lock().map_err(|e| e.to_string())?.ok_or("not connected")?;
    // read and `board_seen` together: between the 0xA5 of a serial write and its `board_written` a read would take
    // the new String 2 for another board
    let _guard = control::reserve_pack_wait(&state, address, "serial_read")?;
    let before = link_now(&state);
    let serials = with_conn(&state.conn, &state.flashing, |c| c.serial_numbers(bus, address))?;
    let Some(inst) = before.installation.clone() else { return Ok(SerialRead { serials, board: None, identity: None }) };
    if link_now(&state).gen != before.gen {
        return Err(LINK_CHANGED.into());
    }
    if bus == Bus::CanSocketBus {
        // no String 2 here: the master's battery serial of another installation is a hint only, nothing is held
        let other = if address == 0 { maint::with_store(&state, |s| bank::can_hint(s, &inst, &serials.battery)).ok().flatten() } else { None };
        let identity = other.map(|o| bank::IdentityView {
            kind: "unconfirmed", address, serial: None, why: Some(bank::Why::Foreign), installation: Some(o), hint: true,
        });
        return Ok(SerialRead { serials, board: None, identity });
    }
    let id = maint::with_store(&state, |s| bank::identity(s, &inst, address, &serials.bms)).unwrap_or(bank::Identity::Failed);
    bank::after_read(&state, &before, address, &id, &serials.bms, &serials.battery);
    let board = match id {
        bank::Identity::Known | bank::Identity::Unchecked => {
            for_link(&state, &before, "board", |s, inst| s.board_seen(inst, address, &serials.bms, &serials.battery, history::now_ms()))
        }
        _ => None,
    };
    let view = bank::IdentityView::of(&id, address, &serials.bms);
    // a read outside the sidebar (Parameters → Serial numbers) puts the pack on the sidebar line as well
    if view.kind == "unconfirmed" {
        if let Some(app) = state.app.get() {
            let _ = app.emit("bank-unconfirmed", &view);
        }
    }
    Ok(SerialRead { serials, board, identity: Some(view) })
}

/// All parameters and function switches of one pack (0x47). Only RS485-1/2 knows this command; there the
/// master of a multi-pack bank does not answer.
#[tauri::command]
async fn parameters(state: State<'_, AppState>, address: u8) -> Result<Parameters, String> {
    if *state.bus.lock().map_err(|e| e.to_string())? == Some(Bus::CanSocketBus) {
        return Err("parameters can only be read on RS485-1/2".into());
    }
    read_parameters_reserved(&state, address)
}

/// 0x47 and its snapshot under the pack's reservation: a write of the same pack keeps it until its own `written`
/// snapshot and read-back, so a read in between cannot store the new block as `read` before it (the write's previous
/// state would then be the written block).
fn read_parameters_reserved(state: &AppState, address: u8) -> Result<Parameters, String> {
    let _guard = control::reserve_pack_wait(state, address, "read")?;
    let before = link_now(state);
    // the board first (one 0xA4): the values always go to the UI, a snapshot only for a known board or one without a
    // serial; „Nur ansehen“ stores nothing anyway
    let checked = if before.installation.is_some() { bank::check(state, &before, address) } else { Err(String::new()) };
    let p = with_conn(&state.conn, &state.flashing, |c| c.parameters(address))?;
    if bank::snapshot_allowed(&checked) {
        bms_memory::snapshot(state, &before, address, history::SnapshotKind::Read, &p.block);
    }
    Ok(p)
}

/// Poll the given packs in a loop and emit "pack" (and on the CAN socket bus "system") events.
#[tauri::command]
fn start_polling(app: AppHandle, state: State<AppState>, addresses: Vec<u8>, interval_ms: u64) -> Result<(), String> {
    if state.flashing.load(Ordering::SeqCst) {
        return Err(FLASHING.into());
    }
    let bus = state.bus.lock().map_err(|e| e.to_string())?.ok_or("not connected")?;
    let running = Arc::new(AtomicBool::new(true));
    {
        // stop the previous thread and install the new flag
        let mut current = state.polling.lock().map_err(|e| e.to_string())?;
        current.store(false, Ordering::SeqCst);
        *current = running.clone();
    }
    std::thread::sleep(Duration::from_millis(50));
    let conn = state.conn.clone();
    let flashing = state.flashing.clone();
    let rec = state.history.clone();
    let paused = state.memory_busy.clone();
    let (ops, links, gen) = (state.ops.clone(), state.link.clone(), link_now(&state).gen);
    // answers of a connection replaced meanwhile are neither recorded nor shown
    let current = move || links.lock().map(|l| l.gen == gen).unwrap_or(false);
    std::thread::spawn(move || {
        while running.load(Ordering::SeqCst) {
            // a BMS history read has the bus to itself
            if paused.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let started = std::time::Instant::now();
            if bus == Bus::CanSocketBus {
                let r = with_conn(&conn, &flashing, |c| c.system_values());
                if !current() {
                    break;
                }
                let update = SystemUpdate { values: r.as_ref().ok().cloned(), error: r.err() };
                if let (Ok(mut rec), Ok(v)) = (rec.lock(), serde_json::to_value(&update)) {
                    rec.system(history::now_ms(), &v);
                }
                let _ = app.emit("system", update);
            }
            for &address in &addresses {
                if !running.load(Ordering::SeqCst) || paused.load(Ordering::SeqCst) {
                    break;
                }
                // a restarting pack is watched by its own action; asking it now would only wait for timeouts
                if control::pack_action(&ops, address) == Some("restart") {
                    continue;
                }
                let telemetry = with_conn(&conn, &flashing, |c| c.telemetry(address));
                let status = with_conn(&conn, &flashing, |c| c.status(address));
                let error = telemetry.as_ref().err().or(status.as_ref().err()).cloned();
                // A dead link (gateway closed the socket, broken pipe) does not heal by retrying: drop it and
                // stop polling. The UI reconnects on the "link-lost" event (or after its data timeout).
                if error.as_deref().is_some_and(|e| e == "not connected" || e.starts_with("I/O") || e.starts_with("connection closed")) {
                    if running.load(Ordering::SeqCst) {
                        if let Ok(mut c) = conn.lock() {
                            *c = None;
                        }
                        // tell the UI right away instead of letting it wait for its data timeout
                        let _ = app.emit("link-lost", error.clone().unwrap_or_default());
                    }
                    running.store(false, Ordering::SeqCst);
                    break;
                }
                if !current() {
                    running.store(false, Ordering::SeqCst);
                    break;
                }
                let update = PackUpdate { address, telemetry: telemetry.ok(), status: status.ok(), error };
                if let (Ok(mut rec), Ok(v)) = (rec.lock(), serde_json::to_value(&update)) {
                    rec.pack(history::now_ms(), address, &v);
                }
                let _ = app.emit("pack", update);
            }
            let wait = Duration::from_millis(interval_ms).saturating_sub(started.elapsed());
            std::thread::sleep(wait.max(Duration::from_millis(100)));
        }
    });
    Ok(())
}

#[tauri::command]
fn stop_polling(state: State<AppState>) {
    if let Ok(p) = state.polling.lock() {
        p.store(false, Ordering::SeqCst);
    }
}

/// User cell entries (own and overridden models) as one JSON array in the app data directory.
fn user_cells_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("cells.user.json"))
}

#[tauri::command]
fn load_user_cells(app: AppHandle) -> Result<serde_json::Value, String> {
    let path = user_cells_path(&app)?;
    if !path.exists() {
        return Ok(serde_json::Value::Array(vec![]));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[tauri::command]
fn save_user_cells(app: AppHandle, cells: serde_json::Value) -> Result<(), String> {
    if !cells.is_array() {
        return Err("expected an array of cells".into());
    }
    // one write at a time, each through its own temporary file, renamed into place, so neither a
    // crash nor two overlapping saves can leave a half-written or mixed database
    static WRITE: Mutex<()> = Mutex::new(());
    let _guard = WRITE.lock().map_err(|e| e.to_string())?;
    let path = user_cells_path(&app)?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = path.with_extension(format!("json.{}.{nanos}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(&cells).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

fn recordings_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("recordings"))
}

fn history_path(state: &State<AppState>) -> Result<std::path::PathBuf, String> {
    state.history.lock().map_err(|e| e.to_string())?.path().ok_or_else(|| "history database not open".into())
}

#[tauri::command]
fn history_status(state: State<AppState>) -> Result<history::Status, String> {
    Ok(state.history.lock().map_err(|e| e.to_string())?.status())
}

#[tauri::command]
fn history_settings(state: State<AppState>, settings: Settings) -> Result<history::Status, String> {
    state.history.lock().map_err(|e| e.to_string())?.set_settings(settings)
}

/// Installations in the database with their time span and packs.
#[tauri::command]
async fn history_sites(state: State<'_, AppState>) -> Result<Vec<history::SiteSpan>, String> {
    let path = history_path(&state)?;
    tauri::async_runtime::spawn_blocking(move || history::sites(&history::reader(&path)?)).await.map_err(|e| e.to_string())?
}

/// Chart data for a span: packs and master on a common grid, the cells of one pack, the alarms.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn history_series(
    state: State<'_, AppState>,
    site: String,
    from: i64,
    to: i64,
    packs: Vec<u8>,
    cells_pack: Option<u8>,
    max_points: usize,
) -> Result<history::Series, String> {
    let path = history_path(&state)?;
    tauri::async_runtime::spawn_blocking(move || history::series(&history::reader(&path)?, &site, from, to, &packs, cells_pack, max_points))
        .await
        .map_err(|e| e.to_string())?
}

/// Export a span to a file the user picked (JSON Lines or CSV).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn history_export(
    state: State<'_, AppState>,
    site: String,
    from: i64,
    to: i64,
    packs: Vec<u8>,
    format: Format,
    path: String,
) -> Result<history::ExportStats, String> {
    let db = history_path(&state)?;
    tauri::async_runtime::spawn_blocking(move || history::export(&history::reader(&db)?, &site, from, to, &packs, format, std::path::Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

/// Import a JSON Lines recording (earlier versions, exports) for an installation.
#[tauri::command]
async fn history_import(state: State<'_, AppState>, path: String, site: String) -> Result<history::ImportStats, String> {
    let h = state.history.clone();
    tauri::async_runtime::spawn_blocking(move || h.lock().map_err(|e| e.to_string())?.import(std::path::Path::new(&path), &site))
        .await
        .map_err(|e| e.to_string())?
}

/// Delete the data of one installation, or all of it.
#[tauri::command]
fn history_clear(state: State<AppState>, site: Option<String>) -> Result<(), String> {
    state.history.lock().map_err(|e| e.to_string())?.clear(site.as_deref())
}

/// Folders the file dialogs start in: old recordings (to import) and exports.
#[tauri::command]
fn history_dirs(app: AppHandle) -> Result<(String, String), String> {
    let exports = app.path().app_data_dir().map_err(|e| e.to_string())?.join("exports");
    std::fs::create_dir_all(&exports).map_err(|e| e.to_string())?;
    Ok((recordings_dir(&app)?.display().to_string(), exports.display().to_string()))
}

fn parameters_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("parameters"))
}

/// Save a parameter export (BatteryMonitor XML, built by the UI) and return its path.
#[tauri::command]
fn save_parameters(app: AppHandle, name: String, content: String) -> Result<String, String> {
    // the name comes from the UI: keep it a plain file name
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return Err(format!("invalid file name: {name}"));
    }
    let dir = parameters_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

/// Show a file (or the folder itself) in the file manager. Only paths inside `dir`, so the UI cannot open
/// arbitrary files.
fn reveal(dir: &std::path::Path, path: Option<String>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let target = match path.map(std::path::PathBuf::from) {
        // inside `dir` by its components, not only by its text (`dir/../x` starts with `dir` too)
        Some(p) if p.starts_with(dir) && !p.components().any(|c| c == std::path::Component::ParentDir) && p.exists() => p,
        _ => dir.to_path_buf(),
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(&target).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(format!("/select,{}", target.display())).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(if target.is_dir() { &target } else { dir }).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

/// Show the history database (or the app data folder) in the file manager.
#[tauri::command]
fn reveal_history(app: AppHandle, path: Option<String>) -> Result<(), String> {
    reveal(&app.path().app_data_dir().map_err(|e| e.to_string())?, path)
}

/// Show a saved parameter export (or the parameters folder) in the file manager.
#[tauri::command]
fn reveal_parameters(app: AppHandle, path: Option<String>) -> Result<(), String> {
    reveal(&parameters_dir(&app)?, path)
}

/// The standard macOS menu, except Quit: the predefined item sends `terminate:`, which ends the app without
/// an ExitRequested event, so a flash could be cut off. Our own item goes through `app.exit`, which can be refused.
#[cfg(target_os = "macos")]
fn macos_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem as P, Submenu};
    let info = app.package_info();
    let about = AboutMetadata { name: Some(info.name.clone()), version: Some(info.version.to_string()), ..Default::default() };
    let quit = MenuItem::with_id(app, QUIT_ID, format!("Quit {}", info.name), true, Some("CmdOrCtrl+Q"))?;
    let app_menu = Submenu::with_items(app, info.name.clone(), true, &[
        &P::about(app, None, Some(about))?, &P::separator(app)?, &P::services(app, None)?, &P::separator(app)?,
        &P::hide(app, None)?, &P::hide_others(app, None)?, &P::show_all(app, None)?, &P::separator(app)?, &quit,
    ])?;
    let file = Submenu::with_items(app, "File", true, &[&P::close_window(app, None)?])?;
    let edit = Submenu::with_items(app, "Edit", true, &[
        &P::undo(app, None)?, &P::redo(app, None)?, &P::separator(app)?,
        &P::cut(app, None)?, &P::copy(app, None)?, &P::paste(app, None)?, &P::select_all(app, None)?,
    ])?;
    let view = Submenu::with_items(app, "View", true, &[&P::fullscreen(app, None)?])?;
    let window = Submenu::with_items(app, "Window", true, &[
        &P::minimize(app, None)?, &P::maximize(app, None)?, &P::separator(app)?, &P::close_window(app, None)?,
    ])?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window])
}

#[cfg(target_os = "macos")]
const QUIT_ID: &str = "openbms-quit";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(macos_menu).on_menu_event(|app, event| {
        if event.id() == QUIT_ID {
            if app.state::<AppState>().flashing.load(Ordering::SeqCst) {
                let _ = app.emit("firmware-busy", ());
            } else {
                app.exit(0);
            }
        }
    });
    builder
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("history.sqlite");
            let state = app.state::<AppState>();
            let _ = state.app.set(app.handle().clone());
            // migrating a big file takes a while: off the setup thread, so the window opens meanwhile; the
            // recorder starts once the file is ready, the status says so until then
            if let Ok(mut h) = state.history.lock() {
                h.set_preparing(true);
            }
            let history = state.history.clone();
            std::thread::spawn(move || {
                let db = history::History::prepare(&path);
                let Ok(mut h) = history.lock() else { return };
                if let Err(e) = db.and_then(|db| h.attach(db, &path)) {
                    eprintln!("history: {e}");
                    h.failed(e);
                }
            });
            match maint_store::Store::open(&app.path().app_data_dir()?.join("maintenance.sqlite")) {
                Ok(s) => if let Ok(mut m) = state.maint.lock() { *m = Some(s); },
                Err(e) => eprintln!("maintenance: {e}"),
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // closing during a flash would cut it off and can leave the board without an application
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.app_handle().state::<AppState>().flashing.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.emit("firmware-busy", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![list_ports, connect, probe_other_bus, disconnect, scan, system_once, maint_status, telemetry, serial_numbers, start_polling, stop_polling, load_user_cells, save_user_cells, history_status, history_settings, history_sites, history_series, history_export, history_import, history_clear, history_dirs, reveal_history, parameters, save_parameters, reveal_parameters, bms_memory::bms_clock, bms_memory::set_bms_clock, bms_memory::pack_live, bms_memory::bms_calibrate, bms_memory::set_can_protocol, bms_memory::write_parameters, bms_memory::param_history, bms_memory::set_serial, bms_memory::bms_history, bms_memory::bms_history_cancel, bms_memory::save_bms_history, bank::bank_confirm, bank::bank_reads_done, control::pack_restart, control::pack_on_off, control::pack_heater, control::pack_clear_history, control::pack_set_remaining, firmware::firmware_check_image, firmware::firmware_inspect, firmware::firmware_start, firmware::reveal_firmware_logs, maint::maint_installations, maint::maint_installations_of, maint::maint_save_installation, maint::maint_create_installation, installations::installations_migrate, installations::installation_settings, installations::set_installation_setting, installations::set_installation_cell_types, installations::installation_overview, installations::installation_merge, installations::installation_archive, installations::installation_rename, installations::installation_delete, installations::installation_snapshots, installations::logs_unassigned, installations::installation_export, installations::installation_import_inspect, installations::installation_import_apply, installations::installation_imports_open, installations::installation_import_discard, installations::installation_import_abandon, maint::maint_adopt_master_serial, maint::maint_alerts, maint::maint_clear_pending, maint::maint_live, maint::maint_scan_packs, maint::maint_halt, maint::maint_release])
        .build(tauri::generate_context!())
        .expect("error while running OpenBMS Monitor")
        .run(|app, event| {
            // app.exit (our Quit menu item) and closing the last window end up here. macOS Dock -> Quit and
            // logout send `terminate:` straight to NSApplication; tao has no hook for that, so it cannot be refused.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if app.state::<AppState>().flashing.load(Ordering::SeqCst) {
                    api.prevent_exit();
                    let _ = app.emit("firmware-busy", ());
                }
            }
        });
}

#[cfg(test)]
mod port_reservation_tests {
    use super::*;

    #[test]
    fn only_one_operation_can_reserve_the_port_until_its_guard_drops() {
        let busy = Arc::new(AtomicBool::new(false));
        let slot = reserve_port(&busy).expect("first operation owns the port");
        assert!(reserve_port(&busy).is_none());
        drop(slot);
        assert!(reserve_port(&busy).is_some());
    }

    #[test]
    fn a_parameter_read_waits_for_a_write_of_the_same_pack() {
        let state = AppState::default();
        let write = control::reserve_pack(&state, 2, "params").unwrap();
        let e = read_parameters_reserved(&state, 2).unwrap_err();
        assert!(e.contains("pack-busy") && e.contains("params"), "{e}");
        // another pack is not held up (it fails here only for the missing connection)
        assert_eq!(read_parameters_reserved(&state, 3).unwrap_err(), "not connected");
        drop(write);
        assert_eq!(read_parameters_reserved(&state, 2).unwrap_err(), "not connected");
    }

    #[test]
    fn no_step_runs_on_a_connection_changed_since_the_action_began() {
        let state = AppState::default();
        let at = link_now(&state);
        let step = |at: &Link| with_link(&state.conn, &state.flashing, &state.link, at, |_| Ok(()));
        assert_eq!(step(&at).unwrap_err(), "not connected");
        new_link(&state, Some("other".into()));
        assert_eq!(step(&at).unwrap_err(), LINK_CHANGED);
        assert_eq!(step(&link_now(&state)).unwrap_err(), "not connected");
    }

    #[test]
    fn a_busy_port_names_the_running_action() {
        let state = AppState::default();
        assert_eq!(current_action(&state), None);
        let v: serde_json::Value = serde_json::from_str(&port_busy_error(current_action(&state))).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "busy", "action": null }));
        *state.maint_action.lock().unwrap() = Some("scan");
        assert_eq!(current_action(&state), Some("scan"));
        let v: serde_json::Value = serde_json::from_str(&port_busy_error(current_action(&state))).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "busy", "action": "scan" }));
        *state.maint_action.lock().unwrap() = None;
        state.flashing.store(true, Ordering::SeqCst);
        assert_eq!(current_action(&state), Some("flash"));
    }
}
