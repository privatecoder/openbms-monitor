use openbms_proto::{DeviceInfo, SerialNumbers, SerialValue, Status, SystemValues, Telemetry};
use openbms_transport::{list_serial_ports, Bus, Connection, Endpoint};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

mod balancer;
mod bank;
mod bms_memory;
mod bundle;
mod capture;
mod cell_analysis;
mod control;
mod installation_bundle;
mod firmware;
mod health;
mod history;
mod installations;
mod maint;
mod maint_store;
mod migrate;
mod notifications;
mod notify;
mod power;
mod profile;
mod uart1_id;
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
    /// Writes over the CAN socket's parameter service now (`PollPause`): host polling pauses meanwhile.
    params_pause: Arc<AtomicUsize>,
    /// Asks the running history read to stop after the current frame.
    memory_cancel: Arc<AtomicBool>,
    /// Asks the running pack search on the pack bus to stop before the next address (`maint_scan_cancel`).
    scan_cancel: Arc<AtomicBool>,
    /// „Abbrechen“ while connecting: the check, the pack search after it and the other-bus probe stop before the next
    /// address (`connect_cancel`); cleared by the next connect and by disconnect.
    connect_cancel: Arc<AtomicBool>,
    /// Counted up by every disconnect: a connect that started before it publishes nothing (or takes it down again).
    connect_epoch: std::sync::atomic::AtomicU64,
    /// The installation of the monitor connection (None: „Nur ansehen“ or not connected).
    link: Arc<Mutex<Link>>,
    /// Reserved packs and installations under a structural operation; see `control.rs`.
    ops: control::Ops,
    /// The unconfirmed packs of the connection (right bank before writing); see `bank.rs`.
    bank: Mutex<bank::BankState>,
    /// Notifications from the poll answers (`notify.rs`) and their wording from the UI.
    notify: Mutex<notify::Notifier>,
    notify_texts: Mutex<notify::Texts>,
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

/// Host polling holds while a parameter write runs over the CAN socket (fresh read, write, acknowledgement and read-back
/// with no poll frame between them; at most four frames of 3 s). A count, since writes of two packs may overlap; dropped
/// also when the write ends with an error.
pub(crate) struct PollPause(Arc<AtomicUsize>);

impl PollPause {
    pub fn hold(count: &Arc<AtomicUsize>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        PollPause(count.clone())
    }
}

impl Drop for PollPause {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Polling pauses on purpose (a BMS history read, a parameter write over the CAN socket): its skipped rounds are no
/// missing answers and no lost connection.
pub(crate) fn polling_paused(state: &AppState) -> bool {
    state.memory_busy.load(Ordering::SeqCst) || state.params_pause.load(Ordering::SeqCst) > 0
}

/// One poll exchange; None once a parameter write over the CAN socket holds polling. Checked under the connection lock,
/// so no poll frame starts between that write's frames.
fn poll_exchange<T>(conn: &Shared, flashing: &AtomicBool, hold: &AtomicUsize, f: impl FnOnce(&mut Connection) -> openbms_transport::Result<T>) -> Option<Result<T, String>> {
    if flashing.load(Ordering::SeqCst) {
        return Some(Err(FLASHING.into()));
    }
    let mut guard = match lock(conn) {
        Ok(g) => g,
        Err(e) => return Some(Err(e)),
    };
    if hold.load(Ordering::SeqCst) > 0 {
        return None;
    }
    Some(guard.as_mut().ok_or_else(|| "not connected".to_string()).and_then(|c| f(c).map_err(|e| e.to_string())))
}

fn reserve_port(flag: &Arc<AtomicBool>) -> Option<PortSlot> {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .ok()
        .map(|_| PortSlot(flag.clone()))
}

/// Longer than one live status (listen plus master read) and one pack scan (`timing::PORT_WAIT`): a user action
/// pressed during a scan waits for it instead of failing with "busy".
pub(crate) const PORT_WAIT: Duration = openbms_transport::timing::PORT_WAIT;

/// Run one exchange on the monitor connection; refused during a flash, which owns the bus.
fn with_conn<T>(conn: &Shared, flashing: &AtomicBool, f: impl FnOnce(&mut Connection) -> openbms_transport::Result<T>) -> Result<T, String> {
    if flashing.load(Ordering::SeqCst) {
        return Err(FLASHING.into());
    }
    let mut guard = lock(conn)?;
    let c = guard.as_mut().ok_or("not connected")?;
    f(c).map_err(|e| e.to_string())
}

/// The order of one poll round: the packs that answered last time first, in address order, then the silent ones
/// starting at `turn` (`may_ask` ends a round once its silent waits used up their budget).
fn poll_order(addresses: &[u8], silent: &std::collections::BTreeSet<u8>, turn: usize) -> (Vec<u8>, Vec<u8>) {
    let (quiet, live): (Vec<u8>, Vec<u8>) = addresses.iter().partition(|a| silent.contains(a));
    let n = quiet.len().max(1);
    let quiet = (0..quiet.len()).map(|i| quiet[(turn + i) % n]).collect();
    (live, quiet)
}

/// Whether a poll round asks its next pack: until the round's silent waits reach `timing::ROUND_SILENT_BUDGET`; one
/// pack that was silent before (`quiet`) is always asked, so a pack that comes back is seen however long the answers
/// of the others take.
fn may_ask(waited_silent: Duration, quiet: bool, quiet_asked: usize) -> bool {
    waited_silent < openbms_transport::timing::ROUND_SILENT_BUDGET || (quiet && quiet_asked == 0)
}

/// The text of `openbms_transport::Error::Timeout`: the pack stayed silent (not a broken link, not a refusal).
fn silent(e: &str) -> bool {
    e.starts_with("no answer within")
}

/// The answer times learnt on the monitor connection (`timing::Latency`), for the diagnosis: None without a connection,
/// or while an exchange holds it (the UI keeps the last view).
/// `busy`: an exchange holds the connection now (the UI keeps its last view of the same `gen`); None: not connected.
#[derive(Serialize)]
struct LinkTiming {
    gen: u64,
    busy: bool,
    adaptive: bool,
    classes: Vec<openbms_transport::timing::ClassView>,
}

#[tauri::command]
fn link_timing(state: State<'_, AppState>) -> Option<LinkTiming> {
    let gen = link_now(&state).gen;
    let guard = match state.conn.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::WouldBlock) => return Some(LinkTiming { gen, busy: true, adaptive: false, classes: Vec::new() }),
        Err(_) => return None,
    };
    let l = guard.as_ref()?.latency();
    Some(LinkTiming { gen, busy: false, adaptive: l.adaptive(), classes: l.view(openbms_transport::timing::normal_floor) })
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

/// macOS ports that are never an RS485 adapter (debug console, Bluetooth, Wi-Fi debug); left out of every port list.
fn rs485_candidate(port: &str) -> bool {
    let name = port.rsplit('/').next().unwrap_or(port);
    let name = name.strip_prefix("cu.").or_else(|| name.strip_prefix("tty.")).unwrap_or(name);
    !["debug-console", "Bluetooth-Incoming-Port", "wlan-debug"].contains(&name)
}

#[tauri::command]
fn list_ports() -> Vec<String> {
    list_serial_ports().into_iter().filter(|p| rs485_candidate(p)).collect()
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
    /// the firmware profile read on this endpoint (`profile.rs`); the UI configures itself from it
    profile: profile::ProfileView,
}

/// Ask once whether anything answers: on the CAN socket the master's Modbus values first, then 0x51 of 0..=15
/// (the addresses to be polled first), stop at the first answer. Read-only.
fn check_link(conn: &mut Connection, bus: Bus, first: &[u8], cancel: &AtomicBool) -> Result<u8, openbms_transport::Probe> {
    if bus == Bus::CanSocketBus && conn.system_values().is_ok() {
        return Ok(0);
    }
    let p = conn.probe(openbms_transport::probe_order(first).into_iter().take_while(|_| !cancel.load(Ordering::SeqCst)));
    p.answered.ok_or(p)
}

/// The error of a connect whose check found no pack: JSON, so the UI can explain it.
const CONNECT_CANCELLED: &str = r#"{"code":"cancelled"}"#;

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
    state.connect_cancel.store(false, Ordering::SeqCst);
    let epoch = state.connect_epoch.load(Ordering::SeqCst);
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
    conn.set_label(capture::label(&ep, bus));
    // nothing answers (wrong port, bus or baud rate, pack off or in the bootloader): the port is closed again,
    // nothing is linked or recorded
    let checked = check_link(&mut conn, bus, &addresses.unwrap_or_default(), &state.connect_cancel);
    // „Abbrechen“: the port is closed again, nothing is linked or recorded
    if state.connect_cancel.swap(false, Ordering::SeqCst) {
        return Err(CONNECT_CANCELLED.into());
    }
    let answered = checked.map_err(|p| no_answer_error(&p))?;
    // the firmware profile on exactly this endpoint, while the port is ours alone (CAN socket: 0x64; RS485-1/2 has none)
    let read = (bus == Bus::CanSocketBus).then(|| profile::detect(&mut conn));
    let found = profile::of(&key, bus, read);
    {
        // opening can take seconds; a flash started meanwhile has already dropped the monitor and owns the bus
        let mut slot = lock(&state.conn)?;
        if state.flashing.load(Ordering::SeqCst) {
            return Err(FLASHING.into());
        }
        // „Trennen“ meanwhile: this connection is not set up
        if state.connect_epoch.load(Ordering::SeqCst) != epoch {
            return Err(CONNECT_CANCELLED.into());
        }
        *slot = Some(conn);
    }
    *state.bus.lock().map_err(|e| e.to_string())? = Some(bus);
    *state.main.lock().map_err(|e| e.to_string())? = Some((key.clone(), bus));
    let can = bus == Bus::CanSocketBus;
    // the CAN socket shows no String 2: linked now, and only into an empty CAN socket. RS485-1/2: only once a serial
    // read decided the bank (`bank.rs`), so a foreign bank never gets this endpoint linked to the installation, and
    // only as its first RS485-1/2 port (`link_plan`); replacing a stored port is the user's „Speichern“
    let linked = match &installation {
        Some(id) if can => maint::with_store(&state, |s| s.link_connection(id, &key, control.clone(), can, history::now_ms())).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?,
        _ => false,
    };
    new_link(&state, installation.clone());
    let gen = link_now(&state).gen;
    let pending = (!can && installation.is_some()).then(|| bank::PendingLink { key: key.clone(), control });
    bank::begin(&state, gen, !can, pending);
    bank::set_profile(&state, gen, found.clone());
    notifications::begin(&state, gen, installation.as_deref());
    if let (Some(id), Ok(mut h)) = (&installation, state.history.lock()) {
        h.connected(id, if can { "can" } else { "pack" });
        // RS485-1/2: nothing is recorded until a serial read shows no pack of another installation (`bank.rs`)
        h.hold_for((!can).then_some("pending"));
    }
    // „Trennen“ came after the connection was set: it ends again
    if state.connect_epoch.load(Ordering::SeqCst) != epoch {
        stop_monitor(&state)?;
        return Err(CONNECT_CANCELLED.into());
    }
    Ok(Connected { linked, answered, gen, profile: profile::view(gen, &found) })
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
    let ep = Endpoint::Serial { path, baud: other.baud() };
    let mut conn = Connection::open(&ep).map_err(|e| e.to_string())?;
    conn.set_label(capture::label(&ep, other));
    let checked = check_link(&mut conn, other, &addresses.unwrap_or_default(), &state.connect_cancel);
    if state.connect_cancel.swap(false, Ordering::SeqCst) {
        return Err(CONNECT_CANCELLED.into());
    }
    Ok(match checked {
        Ok(a) => OtherBus { answered: Some(a), noise: false, intra: false, pack_at_9600: other == Bus::CanSocketBus && conn.answers_parameters(a) },
        Err(p) => OtherBus { answered: None, noise: p.noise, intra: p.intra, pack_at_9600: false },
    })
}

/// „Abbrechen“ while connecting (`connect_cancel` in `AppState`).
#[tauri::command]
async fn connect_cancel(state: State<'_, AppState>) -> Result<(), String> {
    state.connect_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// Stop polling and drop the monitor's connection (also before a firmware update takes the port).
fn stop_monitor(state: &AppState) -> Result<(), String> {
    stop_monitor_as(state, false)
}

/// `lost`: the UI drops a dead link and reconnects; notifications keep watching for the lost connection.
fn stop_monitor_as(state: &AppState, lost: bool) -> Result<(), String> {
    notifications::end(state, lost);
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
/// `lost`: the UI drops a dead link to reconnect, not closed by the user.
#[tauri::command]
async fn disconnect(state: State<'_, AppState>, lost: Option<bool>) -> Result<(), String> {
    let _pending = control::begin_disconnect(&state.ops);
    state.memory_cancel.store(true, Ordering::SeqCst);
    state.connect_cancel.store(false, Ordering::SeqCst);
    state.connect_epoch.fetch_add(1, Ordering::SeqCst);
    let ops = state.ops.clone();
    // waiting blocks: off the async runtime
    tauri::async_runtime::spawn_blocking(move || control::wait_packs_idle(&ops, Duration::from_secs(20))).await.map_err(|e| e.to_string())?;
    stop_monitor_as(&state, lost.unwrap_or(false))
}

#[tauri::command]
async fn scan(state: State<'_, AppState>, addresses: Option<Vec<u8>>, connecting: Option<bool>) -> Result<Vec<(u8, DeviceInfo)>, String> {
    // without addresses every DIP address 0..=15 is asked; with a fixed pack count only those
    let addresses = addresses.unwrap_or_else(|| (0..=15).collect());
    // only the search of a connect stops on its „Abbrechen“ (not e.g. the calibration's device query)
    let cancel = state.connect_cancel.clone();
    let connecting = connecting.unwrap_or(false);
    // one address per hold of the connection: the polling started right after connecting goes on between them instead of
    // waiting up to some 15 s for the whole query (twelve silent addresses at up to 1.2 s each)
    let mut found = Vec::new();
    for a in addresses {
        if connecting && cancel.load(Ordering::SeqCst) {
            break;
        }
        found.extend(with_conn(&state.conn, &state.flashing, |c| Ok(c.scan([a])))?);
    }
    Ok(found)
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
    /// CAN socket: String 2 came over the binary service (true), refused with 0x81 (false); None: not asked or no answer
    #[serde(skip_serializing_if = "Option::is_none")]
    uart1: Option<bool>,
}

/// Battery and BMS serial number of one pack, as far as the connected bus shows them (RS485-1/2: both; CAN socket
/// bus: String 1/2 of every pack over the binary service when the connection's master offers it, or for this pack when
/// the user asks (`binary`); else the master's battery serial number only). The read is classified first (`bank.rs`):
/// only a board known at this place is recorded as seen there; an unconfirmed one waits for `bank_confirm`.
#[tauri::command]
async fn serial_numbers(state: State<'_, AppState>, address: u8, binary: Option<bool>) -> Result<SerialRead, String> {
    let bus = state.bus.lock().map_err(|e| e.to_string())?.ok_or("not connected")?;
    // read and `board_seen` together: between the 0xA5 of a serial write and its `board_written` a read would take
    // the new String 2 for another board
    let _guard = control::reserve_pack_wait(&state, address, "serial_read")?;
    let before = link_now(&state);
    // CAN socket: String 1/2 over the binary service when the connection's profile offers it, or asked for this pack
    if bus == Bus::CanSocketBus && (binary == Some(true) || uart1_serials_offered(&state, &before, address)) {
        let (serials, identity) = uart1_id::serial_numbers(&state, &before, bus, address)?;
        if let (0, Some(inst)) = (address, before.installation.as_deref()) {
            can_master_check(&state, before.gen, inst, &serials.battery);
        }
        return Ok(SerialRead { serials, board: None, identity, uart1: bank::uart1_of(&state, before.gen, address) });
    }
    let (serials, canonical) = with_conn(&state.conn, &state.flashing, |c| match bus {
        Bus::PackBus => c.serial_numbers_pack_bus_canonical(address),
        Bus::CanSocketBus => c.serial_numbers(bus, address).map(|s| (s, false)),
    })?;
    let Some(inst) = before.installation.clone() else { return Ok(SerialRead { serials, board: None, identity: None, uart1: None }) };
    if link_now(&state).gen != before.gen {
        return Err(LINK_CHANGED.into());
    }
    if bus == Bus::CanSocketBus {
        // no String 2 here: the master's battery serial of another installation is a hint only, nothing is held
        let other = if address == 0 { maint::with_store(&state, |s| bank::can_hint(s, &inst, &serials.battery)).ok().flatten() } else { None };
        if address == 0 {
            can_master_check(&state, before.gen, &inst, &serials.battery);
        }
        let identity = other.map(|o| bank::IdentityView {
            kind: "unconfirmed", address, serial: None, why: Some(bank::Why::Foreign), installation: Some(o), hint: true,
        });
        return Ok(SerialRead { serials, board: None, identity, uart1: None });
    }
    let id = maint::with_store(&state, |s| bank::identity(s, &inst, address, &serials.bms)).unwrap_or(bank::Identity::Failed);
    bank::after_read_canonical(&state, &before, address, &id, &serials.bms, &serials.battery, canonical);
    let board = match id {
        bank::Identity::Known | bank::Identity::Unchecked => {
            for_link(&state, &before, "board", |s, inst| s.board_seen_canonical(inst, address, &serials.bms, &serials.battery, canonical, history::now_ms()))
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
    Ok(SerialRead { serials, board, identity: Some(view), uart1: None })
}

/// All parameters and function switches of one pack (0x47). RS485-1/2 knows this command (there the master of a
/// multi-pack bank does not answer); on the CAN socket only a patched pack, over its binary service. `patch`: what proves
/// the pack's patch version (the release bits of Group0 are named only then).
#[tauri::command]
async fn parameters(state: State<'_, AppState>, address: u8) -> Result<uart1_id::ParamRead, String> {
    read_parameters(&state, address)
}

/// The read under the pack's reservation: a write of the same pack keeps it until its own `written` snapshot and
/// read-back, so a read in between cannot store the new block as `read` before it (the write's previous state would
/// then be the written block). The connection and its bus are taken under the reservation, and every frame goes to
/// exactly that link: over the CAN socket the binary service with String 2/1 in the same port session
/// (`uart1_id::read`; only a valid binary answer shows the pack has the service), else 0x47 over RS485-1/2.
fn read_parameters(state: &AppState, address: u8) -> Result<uart1_id::ParamRead, String> {
    let _guard = control::reserve_pack_wait(state, address, "read")?;
    let link = link_now(state);
    let bus = bms_memory::bus_of(state, &link)?;
    match bms_memory::on_bus(openbms_proto::access::Action::ReadParameters, bus)? {
        openbms_proto::access::Channel::Uart1 => uart1_id::read(state, &link, bus, address),
        _ => read_parameters_ascii(state, &link, address),
    }
}

fn read_parameters_ascii(state: &AppState, before: &Link, address: u8) -> Result<uart1_id::ParamRead, String> {
    // the board first (one 0xA4): the values always go to the UI, a snapshot only for a known board or one without a
    // serial; „Nur ansehen“ stores nothing anyway
    let checked = if before.installation.is_some() { bank::check(state, before, address) } else { Err(String::new()) };
    let p = with_link(&state.conn, &state.flashing, &state.link, before, |c| c.parameters(address))?;
    if bank::snapshot_allowed(&checked) {
        bms_memory::snapshot(state, before, address, history::SnapshotKind::Read, &p.block);
    }
    // a version proof counts for the board whose String 2 the check read just now
    let serial = checked.as_ref().ok().and_then(|(_, read)| maint_store::real(read).map(String::from));
    let patch = profile::pack_patch(state, before, address, serial.as_deref());
    Ok(uart1_id::ParamRead { parameters: p, bms_serial: None, patch })
}

/// Whether String 1/2 of `address` are read over the binary service on the CAN socket connection `link` without being
/// asked, decided per target pack (`profile::binary_serials`): the master by its profile; another pack by its own 64/03
/// (asked once per connection here, in the background reads after connecting), a stored proof for the place, or an A4
/// that already came.
fn uart1_serials_offered(state: &AppState, link: &Link, address: u8) -> bool {
    let master = bank::profile(state, link.gen).and_then(|p| p.caps()).is_some_and(|c| c.uart1_identity);
    let live = profile::live_or_probe(state, link, address);
    let stored = link.installation.as_deref().is_some_and(|inst| maint::with_store(state, |s| s.patch_proof(inst, address)).ok().flatten().is_some());
    profile::binary_serials(master, address, live.as_ref(), stored, bank::uart1_of(state, link.gen, address) == Some(true))
}

/// Notifications on the CAN socket run only with the installation's own master: compare its battery serial with the one
/// stored for address 0. No answer decides nothing (the poll thread asks again).
fn can_master_check(state: &AppState, gen: u64, inst: &str, battery: &SerialValue) {
    if *battery == SerialValue::NoAnswer {
        return;
    }
    let stored = maint::with_store(state, |s| Ok(s.current_board(inst, 0)?.and_then(|b| b.battery_serial))).ok().flatten();
    bank::can_master_read(state, gen, notify::master_matches(stored.as_deref(), battery));
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
    let hold = state.params_pause.clone();
    let (ops, links, gen) = (state.ops.clone(), state.link.clone(), link_now(&state).gen);
    // answers of a connection replaced meanwhile are neither recorded nor shown
    let current = move || links.lock().map(|l| l.gen == gen).unwrap_or(false);
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        // CAN socket: the master's battery serial until it answers (the UI reads it once after connecting), every
        // 1, 2, 4 … 32 rounds
        let (mut round, mut master_at, mut master_every) = (0u32, 0u32, 1u32);
        // packs whose last 0x42 stayed silent, and where the next round goes on with them
        let (mut silent_packs, mut turn) = (std::collections::BTreeSet::<u8>::new(), 0usize);
        while running.load(Ordering::SeqCst) {
            // a BMS history read or a parameter write over the CAN socket has the bus to itself
            if paused.load(Ordering::SeqCst) || hold.load(Ordering::SeqCst) > 0 {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let started = std::time::Instant::now();
            if bus == Bus::CanSocketBus {
                let Some(r) = poll_exchange(&conn, &flashing, &hold, |c| c.system_values()) else { continue };
                if !current() {
                    break;
                }
                let update = SystemUpdate { values: r.as_ref().ok().cloned(), error: r.err() };
                if let (Ok(mut rec), Ok(v)) = (rec.lock(), serde_json::to_value(&update)) {
                    rec.system(history::now_ms(), &v);
                }
                let _ = app.emit("system", update);
                round += 1;
                let inst = link_now(&state).installation.filter(|_| round >= master_at && control::pack_action(&ops, 0).is_none() && bank::can_master_unread(&state, gen));
                if let Some(inst) = inst {
                    let r = poll_exchange(&conn, &flashing, &hold, |c| c.serial_numbers(bus, 0));
                    if !current() {
                        break;
                    }
                    match r {
                        // held by a parameter write: asked again in a later round, no back-off
                        None => {}
                        Some(Ok(s)) if s.battery != SerialValue::NoAnswer => can_master_check(&state, gen, &inst, &s.battery),
                        _ => {
                            master_at = round + master_every;
                            master_every = (master_every * 2).min(32);
                        }
                    }
                }
            }
            let (live_first, quiet) = poll_order(&addresses, &silent_packs, turn);
            let (mut quiet_asked, mut waited_silent) = (0usize, Duration::ZERO);
            for (i, &address) in live_first.iter().chain(quiet.iter()).enumerate() {
                if !running.load(Ordering::SeqCst) || paused.load(Ordering::SeqCst) || hold.load(Ordering::SeqCst) > 0 {
                    break;
                }
                // the round's silent waits are used up: the rest waits for the next round, counted as nothing (packs
                // that answered last time come first then, the silent ones go on in turns)
                let is_quiet = i >= live_first.len();
                if !may_ask(waited_silent, is_quiet, quiet_asked) {
                    break;
                }
                if is_quiet {
                    quiet_asked += 1;
                }
                // a restarting pack is watched by its own action; asking it now would only wait for timeouts
                if control::pack_action(&ops, address) == Some("restart") {
                    continue;
                }
                // the time of the exchange itself, measured under the connection lock: waiting for another holder of
                // the port (a device query, a serial read) is no silence of this pack
                let mut took = Duration::ZERO;
                // held by a parameter write meanwhile: the rest of the round is skipped, no pack counts as silent
                let Some(telemetry) = poll_exchange(&conn, &flashing, &hold, |c| {
                    let asked = std::time::Instant::now();
                    let r = c.telemetry(address);
                    took = asked.elapsed();
                    r
                }) else { break };
                // a pack silent to 0x42 (through its longer wait over a gateway) is not asked 0x44 in this round, and
                // counts as silent for the order of the next round (`poll_order`)
                let mute = telemetry.as_ref().is_err_and(|e| silent(e));
                if mute { silent_packs.insert(address); waited_silent += took; } else { silent_packs.remove(&address); }
                let status = if mute { Err(telemetry.as_ref().unwrap_err().clone()) }
                    else { let Some(s) = poll_exchange(&conn, &flashing, &hold, |c| c.status(address)) else { break }; s };
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
                // notifications from this fresh answer, never from the UI's merged state
                notifications::answer(&state, gen, address, telemetry.as_ref().ok(), status.as_ref().ok());
                // the hard stop as the pack's own behaviour shows it (waiting for recharge, charge MOSFET off)
                if let Ok(st) = status.as_ref() {
                    profile::observe_hard_stop(&state, &Link { gen, installation: link_now(&state).installation }, address, st);
                }
                let update = PackUpdate { address, telemetry: telemetry.ok(), status: status.ok(), error };
                if let (Ok(mut rec), Ok(v)) = (rec.lock(), serde_json::to_value(&update)) {
                    rec.pack(history::now_ms(), address, &v);
                }
                let _ = app.emit("pack", update);
            }
            turn = turn.wrapping_add(quiet_asked);
            if running.load(Ordering::SeqCst) && current() {
                notifications::round_end(&app, &state);
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

/// The health report of an installation over a period (local days), with the pairs as stored for it. On a reader
/// connection: the recorder's lock is held only to look up the file.
#[tauri::command]
async fn health_report(state: State<'_, AppState>, site: String, from: i64, to: i64, pairs: Vec<health::Pair>) -> Result<health::Report, String> {
    let path = history_path(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let db = history::reader(&path)?;
        let Some((from, to)) = health::site_span(&db, &site, from, to)? else { return Ok(health::Report { from, to, ..Default::default() }) };
        health::report(&db, &site, &health::local_days(from, to), &pairs)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Both balancing lanes of a pack in `slices` equal parts of the span: when its external balancer worked (computed from
/// the cells with the settings valid at the time, `balancer.rs`) and when its BMS balanced (reported bits). `devices`:
/// the installation's device setting as the UI holds it. On a reader connection, like the health report.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn balancer_timeline(state: State<'_, AppState>, site: String, pack: u8, from: i64, to: i64, devices: serde_json::Value, slices: usize) -> Result<balancer::Timeline, String> {
    let path = history_path(&state)?;
    tauri::async_runtime::spawn_blocking(move || balancer::timeline(&history::reader(&path)?, &site, pack, from, to, &balancer::Devices::parse(&devices), slices))
        .await
        .map_err(|e| e.to_string())?
}

/// The cell analysis of a pack over [from, to] from its raw rows (`cell_analysis.rs`): deviations per situation,
/// resistance from load steps, a class per cell, and `slices` heatmap columns. `devices`: the installation's device
/// setting as the UI holds it (rows where an external balancer is computed active are left out). On a reader
/// connection, like the health report.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn cell_analysis(state: State<'_, AppState>, site: String, pack: u8, from: i64, to: i64, devices: serde_json::Value, slices: usize) -> Result<cell_analysis::Analysis, String> {
    let path = history_path(&state)?;
    tauri::async_runtime::spawn_blocking(move || cell_analysis::analyse(&history::reader(&path)?, &site, pack, from, to, &balancer::Devices::parse(&devices), slices))
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
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .setup(|app| {
            let path = app.path().app_data_dir()?.join("history.sqlite");
            let state = app.state::<AppState>();
            let _ = state.app.set(app.handle().clone());
            notifications::start(app.handle());
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
        .invoke_handler(tauri::generate_handler![list_ports, connect, connect_cancel, probe_other_bus, disconnect, scan, system_once, telemetry, serial_numbers, start_polling, stop_polling, load_user_cells, save_user_cells, history_status, history_settings, history_sites, history_series, health_report, balancer_timeline, cell_analysis, history_export, history_import, history_clear, history_dirs, reveal_history, parameters, save_parameters, reveal_parameters, bms_memory::bms_clock, bms_memory::set_bms_clock, bms_memory::pack_live, bms_memory::pack_full_capacity, bms_memory::bms_calibrate, bms_memory::set_can_protocol, bms_memory::write_parameters, bms_memory::param_history, bms_memory::set_serial, bms_memory::write_name, uart1_id::bank_mark_gone, bms_memory::bms_history, bms_memory::bms_history_cancel, bms_memory::save_bms_history, bank::bank_confirm, bank::bank_reads_done, control::pack_restart, control::pack_on_off, control::pack_heater, control::pack_clear_history, control::pack_set_remaining, control::pack_set_learned_full, firmware::firmware_check_image, firmware::firmware_inspect, firmware::firmware_start, firmware::firmware_group0, firmware::reveal_firmware_logs, maint::maint_installations, maint::maint_installations_of, maint::maint_save_installation, maint::maint_create_installation, installations::installations_migrate, installations::installation_settings, installations::set_installation_setting, installations::set_installation_cell_types, installations::installation_overview, installations::installation_merge, installations::installation_archive, installations::installation_rename, installations::installation_delete, installations::installation_snapshots, installations::logs_unassigned, installations::installation_export, installations::installation_import_inspect, installations::installation_import_apply, installations::installation_imports_open, installations::installation_import_discard, installations::installation_import_abandon, maint::maint_adopt_master_serial, maint::maint_alerts, maint::maint_clear_pending, maint::maint_discard_latent_marker, maint::maint_live, maint::maint_scan_packs, maint::maint_scan_cancel, maint::maint_halt, maint::maint_release, maint::maint_expected_slaves, profile::firmware_profile, profile::firmware_profile_check, profile::hard_stops, profile::pack_probe, profile::pack_probes, profile::access_view, profile::patch_proof_confirm, profile::patch_proof_revoke, capture::capture_start, capture::capture_stop, capture::capture_clear, capture::capture_status, link_timing, capture::capture_export, notifications::notify_texts, notifications::notify_test, notifications::notify_status])
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
    fn a_poll_round_asks_the_answering_packs_first_and_the_silent_ones_in_turns() {
        let silent: std::collections::BTreeSet<u8> = [2, 3, 5].into();
        assert_eq!(poll_order(&[0, 1, 2, 3, 4, 5], &silent, 0), (vec![0, 1, 4], vec![2, 3, 5]));
        assert_eq!(poll_order(&[0, 1, 2, 3, 4, 5], &silent, 2), (vec![0, 1, 4], vec![5, 2, 3]));
        assert_eq!(poll_order(&[0, 1], &Default::default(), 7), (vec![0, 1], vec![]));
    }

    #[test]
    fn a_round_ends_after_its_silent_waits_but_always_tries_one_silent_pack() {
        let budget = openbms_transport::timing::ROUND_SILENT_BUDGET;
        // packs that fall silent during the round: their waits end it
        assert!(may_ask(budget - Duration::from_millis(1), false, 0));
        assert!(!may_ask(budget, false, 0));
        // long answers of the others cost no budget, and one silent pack is tried anyway
        assert!(may_ask(budget, true, 0));
        assert!(!may_ask(budget, true, 1));
        assert!(may_ask(Duration::ZERO, true, 5));
    }

    #[test]
    fn only_a_timeout_counts_as_a_silent_pack() {
        assert!(silent(&openbms_transport::Error::Timeout(Duration::from_millis(1500)).to_string()));
        assert!(!silent(&openbms_transport::Error::Closed.to_string()));
        assert!(!silent(&openbms_transport::Error::Proto(openbms_proto::Error::Rtn(2)).to_string()));
    }

    #[test]
    fn system_ports_are_not_offered() {
        for p in ["/dev/cu.debug-console", "/dev/tty.debug-console", "/dev/cu.Bluetooth-Incoming-Port", "/dev/tty.Bluetooth-Incoming-Port", "/dev/cu.wlan-debug", "/dev/tty.wlan-debug"] {
            assert!(!rs485_candidate(p), "{p}");
        }
        for p in ["/dev/cu.test", "/dev/tty.usbserial-test", "COM3", "/dev/ttyUSB0", "/dev/cu.debug-console-2"] {
            assert!(rs485_candidate(p), "{p}");
        }
    }

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
        let e = read_parameters(&state, 2).unwrap_err();
        assert!(e.contains("pack-busy") && e.contains("params"), "{e}");
        // another pack is not held up (it fails here only for the missing connection)
        assert_eq!(read_parameters(&state, 3).unwrap_err(), "not connected");
        drop(write);
        assert_eq!(read_parameters(&state, 2).unwrap_err(), "not connected");
        // the bus is the link's own, never the global one: without a profile for the link nothing is read
        *state.bus.lock().unwrap() = Some(Bus::CanSocketBus);
        assert_eq!(read_parameters(&state, 2).unwrap_err(), "not connected");
    }

    #[test]
    fn the_poll_pause_holds_until_every_write_ends_also_on_an_error() {
        let state = AppState::default();
        let failing = |state: &AppState| -> Result<(), String> {
            let _pause = PollPause::hold(&state.params_pause);
            assert!(polling_paused(state));
            Err("timeout".into())
        };
        assert!(failing(&state).is_err());
        assert!(!polling_paused(&state));
        let a = PollPause::hold(&state.params_pause);
        let b = PollPause::hold(&state.params_pause);
        drop(a);
        assert!(polling_paused(&state), "the other write still runs");
        // a poll exchange is skipped while held, not failed
        assert!(poll_exchange(&state.conn, &state.flashing, &state.params_pause, |_| Ok(())).is_none());
        drop(b);
        assert!(!polling_paused(&state));
        assert_eq!(poll_exchange(&state.conn, &state.flashing, &state.params_pause, |_| Ok(())), Some(Err("not connected".to_string())));
        // a history read pauses as before
        state.memory_busy.store(true, Ordering::SeqCst);
        assert!(polling_paused(&state));
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
