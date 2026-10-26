//! Maintenance of a bank (design: docs/superpowers/specs/2026-10-13-simple-flash-flow.md): the live status of
//! the pack bus and the master, the pack list, the master's marker (`start`/`stop` over the CAN socket bus), the
//! right-bank check and the packs that may hang in their bootloader. All bus operations hold the one operation
//! lock (`port_busy`, through `control::reserve_bus`: only while no pack action runs); a control exchange on the endpoint of the main connection runs through that connection,
//! so no endpoint is opened twice.

use crate::maint_store::{normalize_key, real, Board, ControlEndpoint, Installation, MarkerOn, Pending, Store};
use crate::{lock, AppState, EndpointArg};
use openbms_flash::maint::dialog::{self, Action, Budgets, DialogError};
use openbms_flash::maint::policy::Master;
use openbms_flash::maint::{bus, SystemClock};
use openbms_flash::session::Progress;
use openbms_proto::{DeviceInfo, MaintState, MarkerWord, SerialValue};
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

/// One line for a master read: kind, version, build, marker word, effective slaves.
fn master_text(m: &MasterView) -> String {
    format!("master={} version={} build={} marker={} word={} slaves={}", m.kind, m.version.as_deref().unwrap_or("-"), m.build_id.as_deref().unwrap_or("-"),
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
    /// the master refused a maintenance start (RTN 05); `reason` from its fresh block (`profile::start_refusal`)
    StartRefused { reason: String },
    /// "off" | "on" | "foreign": the marker the last state read showed
    Timeout { marker: &'static str, slaves: u8 },
    /// not allowed in the current state; `reason` is a stable key for the UI text
    Refused { reason: String },
    Busy,
    Store { message: String },
    /// the master at this pack bus answers with another BMS serial number than the installation stored
    OtherBank { serial: String },
    /// a pack may hang in its bootloader (`endpoint`: the key of its record): no other pack is flashed anywhere
    BootloaderPending { pack: u8, endpoint: String },
}

impl From<DialogError> for MaintError {
    fn from(e: DialogError) -> Self {
        match e {
            DialogError::Link(e) => MaintError::Connection { message: e.to_string() },
            DialogError::Unsupported => MaintError::Unsupported,
            DialogError::NoAnswer => MaintError::NoAnswer,
            DialogError::Rejected(rtn) => MaintError::Rejected { rtn },
            DialogError::Timeout { last, .. } => MaintError::Timeout { marker: marker_name(last.marker()), slaves: last.effective_slaves },
        }
    }
}

pub(crate) fn marker_name(w: MarkerWord) -> &'static str {
    match w {
        MarkerWord::Off => "off",
        MarkerWord::On => "on",
        MarkerWord::Foreign(_) => "foreign",
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
        MaintError::Store { message } if ["name-required", "name-taken", "unknown-installation", "installation-busy", "merge-self", "merge-devices-differ", "installation-connected", "maintenance-open", "pending-on-port"].contains(&message.as_str()) => refused(&message),
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
        ControlEndpoint::Serial { path } => crate::port::serial(path.clone(), Bus::CanSocketBus),
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
        let ep = control_endpoint(control);
        let mut c = crate::port::open(&ep, Bus::CanSocketBus).map_err(|e| MaintError::Connection { message: e.to_string() })?;
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

/// The state a `start` or `stop` waits for is already there (marker and slave count, `dialog::reached`).
pub fn target_reached(action: Action, s: MaintState) -> bool {
    dialog::reached(action, s)
}

/// `start`/`stop` over the control channel. A repeat on a fresh channel (`with_control`) reads the status
/// first: a target already reached is not sent again.
fn control_dialog(state: &AppState, inst: &Installation, action: Action) -> Result<Result<MaintState, DialogError>, MaintError> {
    let mut retry = false;
    with_control(state, inst, |c| {
        if std::mem::replace(&mut retry, true) {
            if let Ok(s) = dialog::dialog(c, &SystemClock::new(), Action::Status, Budgets::default()) {
                if target_reached(action, s) {
                    return Ok(s);
                }
            }
        }
        dialog::dialog(c, &SystemClock::new(), action, Budgets::default())
    })
}

/// The master as the UI shows it (status line, panel buttons, connection profile).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MasterView {
    /// "patched" (a supported patch) | "unknown-patch" | "no-0x64" | "unknown"
    pub kind: &'static str,
    /// what the master reports, e.g. "16.06.13 MAINT 1.5"
    pub version: Option<String>,
    /// patch version "1.5" and base "16.06.13" as reported
    pub patch: Option<String>,
    pub base: Option<String>,
    pub build_id: Option<String>,
    /// the listed build's name (None: not listed)
    pub listed: Option<String>,
    /// format 3: the hard-stop patch is compiled into the image (an image property, apart from the maintenance state, the
    /// release bits and the MOSFETs); None: not reported (format 2) or no identity
    pub hard_stop: Option<bool>,
    pub caps: Option<crate::profile::CapsView>,
    /// "off" | "on" | "foreign"
    pub marker: Option<&'static str>,
    /// the raw marker word, 8 hex digits
    pub marker_word: Option<String>,
    pub slaves: Option<u8>,
    pub may_halt: bool,
    pub may_release: bool,
    /// after a release: "unverified" (the marker ended, the polling of the slaves is not verified) or "no-slaves"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polling: Option<&'static str>,
}

pub fn master_view(m: &Master) -> MasterView {
    let info = match m {
        Master::Patched { patch, .. } => Some(patch.info),
        Master::UnknownPatch(i) => *i,
        _ => None,
    };
    let listed = match m {
        Master::Patched { patch, .. } => patch.listed.map(String::from),
        _ => None,
    };
    MasterView {
        kind: match m {
            Master::Patched { .. } => "patched",
            Master::UnknownPatch(_) => "unknown-patch",
            Master::No0x64 => "no-0x64",
            Master::Unknown => "unknown",
        },
        version: info.map(|i| i.version()),
        patch: info.map(|i| i.patch_text()),
        base: info.map(|i| i.base_text()),
        build_id: info.map(|i| i.build_id_hex()),
        listed,
        hard_stop: info.and_then(|i| i.hard_stop),
        caps: m.caps().zip(info).map(|(c, i)| crate::profile::caps_of_info(c, &i)),
        marker: m.marker().map(marker_name),
        marker_word: m.state().map(|s| format!("{:08X}", s.marker_word)),
        slaves: m.effective_slaves(),
        may_halt: m.may_halt(),
        may_release: m.may_release(),
        polling: None,
    }
}

/// Read the master on the control adapter (`03`, and for a supported patch `02`) and keep what it showed for the banner.
/// A read without a valid answer never replaces the last one: a set marker stays visible until a read shows it off.
pub fn read_master(state: &AppState, inst: &Installation) -> Result<Master, MaintError> {
    let m = with_control(state, inst, |c| dialog::read(c, &SystemClock::new(), Budgets::default()))?
        .map_err(|e| MaintError::Connection { message: e.to_string() })?;
    keep_master_read(state, &inst.id, &m)?;
    Ok(m)
}

/// Keep what a read of installation `inst`'s master showed, for the banner (also „Wartungs-Marker“ under Parameters).
/// A read without a valid answer never replaces the last one: a set marker stays visible until a read shows it off.
pub fn keep_master_read(state: &AppState, inst: &str, m: &Master) -> Result<(), MaintError> {
    // the monitor connection of this installation shows a pack of another installation: nothing is kept for it
    let link = crate::link_now(state);
    let foreign = link.installation.as_deref() == Some(inst) && crate::bank::foreign(state, link.gen);
    if *m != Master::Unknown && !foreign {
        let v = master_view(m);
        let data = serde_json::to_string(&v).unwrap_or_default();
        let now = crate::history::now_ms();
        match marker_update(m) {
            Some(on) => with_store(state, |s| s.set_master_read(inst, &data, on, now))?,
            // no valid state: the stored marker stays as it was (a lost 02 never clears a marker seen on)
            None => with_store(state, |s| s.set_master_read_keep_marker(inst, &data, now))?,
        }
    }
    Ok(())
}

/// What a master read says about the stored marker: only a valid `02` of a patched master sets (on) or clears (off) it.
/// A master without 0x64 (RTN 04) proves only that its current image ignores the marker word, which stays in its own
/// flash sector: a marker kept on stays, latent under that image. A missing or foreign state, or an unknown patch, leaves
/// it as it was too (None).
pub fn marker_update(m: &Master) -> Option<bool> {
    match m {
        Master::Patched { .. } => match m.marker()? {
            MarkerWord::On => Some(true),
            MarkerWord::Off => Some(false),
            MarkerWord::Foreign(_) => None,
        },
        Master::No0x64 | Master::UnknownPatch(_) | Master::Unknown => None,
    }
}

// --- pack bus ---

/// A little more than one polling cycle of the master (`timing::BUS_LISTEN`).
pub const LISTEN: Duration = openbms_transport::timing::BUS_LISTEN;

/// Before the pack bus is opened: the monitor holds that adapter, or it polls the pack bus.
pub fn must_stop_monitor(main: Option<&(String, Bus)>, pack_key: &str) -> bool {
    main.is_some_and(|(key, bus)| matches!(bus, Bus::PackBus) || key == pack_key)
}

/// The RS485-1/2 port as the UI passes it: an endpoint key (`serial:<path>` / `tcp:<host:port>`), formed once where
/// the user chose it and used unchanged for opening, pending records and comparisons. A bare path is refused.
pub fn pack_key(port: &str) -> Result<String, MaintError> {
    normalize_key(port).map_err(|_| refused("no-port"))
}

/// The transport endpoint of a pack bus key: a network gateway (its channel set to the pack bus's baud rate) or a USB port.
pub fn pack_endpoint(key: &str) -> Result<Endpoint, MaintError> {
    let key = pack_key(key)?;
    Ok(match key.split_once(':') {
        Some(("tcp", addr)) => Endpoint::Tcp { addr: addr.into() },
        Some((_, path)) => crate::port::serial(path, Bus::PackBus),
        None => return Err(refused("no-port")),
    })
}

/// Open the pack bus port (an endpoint key, `pack_key`); first stop the monitor when it holds that endpoint or polls
/// the pack bus, and tell the UI ("monitor-stopped"): a stop by the app is no lost connection.
pub fn open_pack_for(state: &AppState, app: &AppHandle, key: &str) -> Result<Connection, MaintError> {
    let ep = pack_endpoint(key)?;
    let main = state.main.lock().map_err(|e| store_err(e.to_string()))?.clone();
    if must_stop_monitor(main.as_ref(), &pack_key(key)?) {
        crate::stop_monitor_and_tell(state, app).map_err(|m| MaintError::Connection { message: m })?;
    }
    crate::port::open(&ep, Bus::PackBus).map_err(|e| MaintError::Connection { message: e.to_string() })
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
    /// "match" | "stored" | "mismatch" | "unchecked" | "absent" (address 0 did not answer the search) | "none" (no
    /// installation selected)
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

/// Before a flash with checks: the target's String 2 against the board stored for its address in the installation.
/// Only a real serial that is neither the stored board's nor one of its earlier ones refuses; no stored board or no
/// readable serial goes on as before.
pub fn target_check(current: Option<&Board>, read: &SerialValue) -> Result<(), MaintError> {
    match (current, real(read)) {
        (Some(b), Some(s)) if b.bms_serial != s && !b.aliases.iter().any(|a| a == s) => Err(refused("other-pack")),
        _ => Ok(()),
    }
}

/// Read String 2 of the flash target on the open pack bus link and check it (`target_check`). Returns the serial read,
/// if real (kept with the pending record).
pub fn check_target_board(store: &Arc<Mutex<Option<Store>>>, inst: &Installation, address: u8, link: &mut Connection) -> Result<Option<String>, MaintError> {
    let read = match link.bms_serial(address) {
        Ok(v) => v,
        Err(e) if link_dead(&e.to_string()) => return Err(MaintError::Connection { message: e.to_string() }),
        Err(_) => return Ok(None),
    };
    let current = with_store_arc(store, |s| s.current_board(&inst.id, address))?;
    target_check(current.as_ref(), &read)?;
    Ok(real(&read).map(String::from))
}

/// String 2 of a pack on the open pack bus link, if read as a real serial.
pub fn string2(link: &mut Connection, address: u8) -> Option<String> {
    link.bms_serial(address).ok().as_ref().and_then(real).map(String::from)
}

/// The packs among `found` (answered 0x51) with an open record on `key`, each with its String 2 read now, for
/// `Store::clear_answered`. Only pending packs are asked.
pub fn answered_with_serial(pending: &[Pending], found: &[u8], link: &mut Connection) -> Vec<(u8, Option<String>)> {
    pending.iter().filter(|p| found.contains(&p.pack)).map(|p| (p.pack, string2(link, p.pack))).collect()
}

/// The bootloader-pending rule on every open record: only the pending pack itself, at its own endpoint key, may be
/// flashed again. Another address there, and every flash on another key, is refused: two names of one gateway (a DNS
/// alias, its IP address) are different keys, and bootloader frames carry no pack address.
pub fn pending_refusal(pending: &[Pending], key: &str, address: u8) -> Result<(), MaintError> {
    match pending.iter().find(|p| p.endpoint != key || p.pack != address) {
        Some(p) => Err(MaintError::BootloaderPending { pack: p.pack, endpoint: p.endpoint.clone() }),
        None => Ok(()),
    }
}

/// The one flash `pending_refusal` lets through, the pending pack again at its own key, only for that same board: a
/// record with String 2 needs the target's String 2 (`read`, if real) to match. Another board refuses
/// ("pending-other-pack"), an unreadable one too ("pending-unverified": the user clears the record by hand). A record
/// without String 2 cannot tell the board apart and refuses as well ("pending-unverified"): another pack at the same
/// address must never pass a hanging bootloader; only the user's confirmation clears it.
pub fn pending_same_board(pending: &[Pending], key: &str, address: u8, read: Option<&str>) -> Result<(), MaintError> {
    for p in pending.iter().filter(|p| p.endpoint == key && p.pack == address) {
        match (p.bms_serial.as_deref(), read) {
            (None, _) => return Err(refused("pending-unverified")),
            (Some(s), Some(r)) if s == r => {}
            (Some(_), Some(_)) => return Err(refused("pending-other-pack")),
            (Some(_), None) => return Err(refused("pending-unverified")),
        }
    }
    Ok(())
}

/// The pending rule on all stored records, for a flash at endpoint key `key` (`firmware_start`).
pub fn pending_check(store: &Arc<Mutex<Option<Store>>>, key: &str, address: u8) -> Result<(), MaintError> {
    with_store_arc(store, |s| s.all_pending()).and_then(|p| pending_refusal(&p, key, address))
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

/// After a failed flash: the pack at the address answers and is the board of the record's String 2 (`stored`; `read`
/// asked only then). Without a stored serial no answer proves the same board: the record stays for the user.
pub fn answered_same(answers: bool, stored: Option<&str>, read: impl FnOnce() -> Option<String>) -> bool {
    answers && stored.is_some_and(|s| read().as_deref() == Some(s))
}

/// After the flash returned, before the pack port is closed: settle the pending record (`stored`: its String 2).
/// Returns whether it stays (a record that cannot be cleared stays too).
pub fn settle_pending(store: &Arc<Mutex<Option<Store>>>, pending: i64, address: u8, stored: Option<&str>, ok: bool, last: Option<Progress>, link: &mut Connection) -> bool {
    let answers = || {
        let a = bus::app_answers(link, &mut std::io::sink(), address, openbms_transport::timing::RECOVERY);
        answered_same(a, stored, || string2(link, address))
    };
    match after_flash(ok, last, answers) {
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

/// The ports and name from the installations view or the sidebar's „Speichern“. Takes the bus like `connect`, so no
/// maintenance step or flash runs on a port while it is replaced; the store refuses a port still needed.
#[tauri::command]
pub async fn maint_save_installation(state: State<'_, AppState>, installation: Installation) -> Result<Installation, MaintError> {
    save_installation(&state, &installation)
}

pub fn save_installation(state: &AppState, installation: &Installation) -> Result<Installation, MaintError> {
    if state.flashing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(MaintError::Busy);
    }
    let _op = crate::control::reserve_bus(state).ok_or(MaintError::Busy)?;
    with_store(state, |s| {
        s.save_installation(installation)?;
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

/// „Latenten Marker verwerfen“, after the user's confirmation: the marker kept on for a master that now runs an image
/// without 0x64 is no longer shown (only a patched image could check the real marker word). Refused for any other marker.
#[tauri::command]
pub async fn maint_discard_latent_marker(state: State<'_, AppState>, installation: String) -> Result<(), MaintError> {
    match with_store(&state, |s| s.discard_latent_marker(&installation))? {
        true => {
            event(&state, Some(&installation), "latent marker discarded by the user");
            Ok(())
        }
        false => Err(refused("not-latent")),
    }
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
    let (bus, polled, bus_error) = if port.trim().is_empty() {
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
    /// stopped by „Abbrechen“: `packs` holds what answered until then
    cancelled: bool,
    /// each pack's own 64/03 as far as known (the pack bus cannot give it: UART0 answers RTN 04): a fresh short read over
    /// the installation's control channel, only for a pack whose String 2 read fresh on the pack bus equals the one read
    /// fresh over the CAN socket; missing: not checked
    identities: Vec<crate::profile::PackProbeView>,
}

/// Addresses 0–15 until `cancel` is set: checked before each address, so a cancelled search ends after the one
/// running (at most one 0x51 timeout).
pub fn scan_addresses(cancel: &std::sync::atomic::AtomicBool) -> impl Iterator<Item = u8> + '_ {
    openbms_proto::address::DIP.take_while(move |_| !cancel.load(std::sync::atomic::Ordering::SeqCst))
}

/// One `0x51` scan of addresses 0–15 on the pack bus (when the bus became quiet, or the re-scan button); „Abbrechen“
/// (`maint_scan_cancel`) stops it before the next address and the packs found so far are listed. A pending pack that
/// answers with the String 2 stored in its record is cleared; with an installation selected the master's serial number is compared.
#[tauri::command]
pub async fn maint_scan_packs(app: AppHandle, state: State<'_, AppState>, installation: Option<String>, port: String) -> Result<PackScan, MaintError> {
    let key = pack_key(&port)?;
    if state.flashing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(MaintError::Busy);
    }
    // a cancel while the search still waits for the bus counts too
    state.scan_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
    let _op = crate::control::reserve_bus_wait(&state).ok_or(MaintError::Busy)?;
    let _a = begin_action(&state, "scan");
    if state.scan_cancel.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Ok(PackScan { packs: vec![], bank: BankView { state: "none", serial: None }, cleared: vec![], cancelled: true, identities: vec![] });
    }
    let inst = installation.as_deref().map(|id| self::installation(&state, id)).transpose()?;
    let mut link = open_pack_for(&state, &app, &key)?;
    // RS485-1/2 has no bus quiet to wait for; an error here is the link's
    let packs = link.scan(scan_addresses(&state.scan_cancel)).map_err(|e| MaintError::Connection { message: e.to_string() })?;
    let cancelled = state.scan_cancel.swap(false, std::sync::atomic::Ordering::SeqCst);
    let found: Vec<u8> = packs.iter().map(|(a, _)| *a).collect();
    // a pending pack clears only when the board that answers is the one flashed (its String 2)
    let pending = with_store(&state, |s| s.pending_on(&key))?;
    let answered = answered_with_serial(&pending, &found, &mut link);
    let cleared = with_store(&state, |s| s.clear_answered(&key, &answered, crate::history::now_ms()))?;
    let bank = match &inst {
        Some(i) if found.contains(&0) => bank_view(&check_bank(&state.maint, i, &mut link)?),
        // address 0 did not answer the search: nothing was asked, which is not a master without a serial number
        Some(_) => BankView { state: "absent", serial: None },
        None => BankView { state: "none", serial: None },
    };
    // a CAN version belongs to a pack only by its board: String 2 read fresh on the pack bus (canonical, unique among the
    // found) equals the one read fresh over the CAN socket; „Abbrechen“ is checked before every read
    let mut bus_read = vec![];
    let mut stopped = inst.is_some() && !cancelled
        && probe_until_cancel::<_, ()>(&found, &state.scan_cancel, &mut bus_read, |a| Ok(canonical_string2(&mut link, a))).unwrap_or(false);
    let bus_serials = unique_serials(bus_read);
    drop(link);
    let identities = match &inst {
        Some(i) if !cancelled && !stopped && !bus_serials.is_empty() => {
            let (v, s) = board_identities(&state, i, &bus_serials, &state.scan_cancel);
            stopped = s;
            v
        }
        _ => vec![],
    };
    let cancelled = cancelled || stopped;
    Ok(PackScan { packs, bank, cleared, cancelled, identities })
}

/// String 2 of a pack on the pack bus as a board identity (canonical raw bytes, a real value), or None.
pub fn canonical_string2(link: &mut Connection, address: u8) -> Option<String> {
    match link.bms_serial_canonical(address) {
        Ok((SerialValue::Set { value, truncated: false }, true)) => Some(value),
        _ => None,
    }
}

/// The packs whose String 2 identifies them alone: read, and not shared with another found pack (a duplicate proves
/// nothing).
pub fn unique_serials(read: Vec<(u8, Option<String>)>) -> Vec<(u8, String)> {
    let all: Vec<(u8, String)> = read.into_iter().filter_map(|(a, s)| s.map(|s| (a, s))).collect();
    all.iter().filter(|(_, s)| all.iter().filter(|(_, o)| o == s).count() == 1).cloned().collect()
}

/// A pack's CAN 64/03 belongs to the board scanned on the pack bus when its canonical String 2 on the CAN side equals
/// the canonical one read there.
pub fn same_board(pack_bus: &str, can: Option<&str>) -> bool {
    can == Some(pack_bus)
}

/// Per pack over the installation's control channel: binary String 2 (canonical) fresh, and only where it is the board read
/// on the pack bus a fresh 64/03. „Abbrechen“ is checked before every CAN read.
fn board_identities(state: &AppState, inst: &Installation, boards: &[(u8, String)], cancel: &std::sync::atomic::AtomicBool) -> (Vec<crate::profile::PackProbeView>, bool) {
    use openbms_flash::maint::policy::PackFinding;
    if inst.control.is_none() {
        return (vec![], false);
    }
    let gen = crate::link_now(state).gen;
    let mut out: Vec<(u8, PackFinding)> = vec![];
    let mut stopped = false;
    let ask: Vec<u8> = boards.iter().map(|(a, _)| *a).collect();
    let _ = with_control(state, inst, |c| -> Result<(), openbms_flash::Error> {
        stopped = probe_until_cancel(&ask, cancel, &mut out, |a| {
            let expected = boards.iter().find(|(b, _)| *b == a).map(|(_, s)| s.as_str()).unwrap_or_default();
            let can = c.uart1_serial(Bus::CanSocketBus, a, openbms_proto::uart1::CMD_STRING2).ok().and_then(|raw| openbms_proto::serial::canonical(&raw));
            let cancelled = cancel.load(std::sync::atomic::Ordering::SeqCst);
            board_finding(expected, can.as_deref(), cancelled, || dialog::probe_pack(c, &SystemClock::new(), a, crate::profile::identity_budget(c)).map(PackFinding::of))
        })?;
        Ok(())
    });
    // a cancel seen before a 64/03 ends the phase too
    stopped |= cancel.swap(false, std::sync::atomic::Ordering::SeqCst);
    // a pack whose String 2 did not match is no finding of this board: left out (shown as unchecked)
    let shown = out.iter().filter(|(_, f)| *f != PackFinding::Undecided).map(|(a, f)| crate::profile::probe_view(gen, *a, f)).collect();
    (shown, stopped)
}

/// One pack of the board-level proof: only when String 2 read over the CAN socket is the one read on the pack bus is the
/// 64/03 asked, always fresh in this search (a cached live finding may be older than a flash outside the app; it serves
/// the running pack view only), and not after a cancel.
pub fn board_finding<E>(pack_bus: &str, can: Option<&str>, cancelled: bool,
    probe: impl FnOnce() -> Result<openbms_flash::maint::policy::PackFinding, E>) -> Result<openbms_flash::maint::policy::PackFinding, E> {
    use openbms_flash::maint::policy::PackFinding;
    if !same_board(pack_bus, can) || cancelled {
        return Ok(PackFinding::Undecided);
    }
    probe()
}

/// Probe `ask` in order into `found`, checking „Abbrechen“ before each; Ok(true) when cancelled (what came stays).
pub fn probe_until_cancel<T, E>(ask: &[u8], cancel: &std::sync::atomic::AtomicBool, found: &mut Vec<(u8, T)>, mut probe: impl FnMut(u8) -> Result<T, E>) -> Result<bool, E> {
    for a in ask {
        if cancel.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Ok(true);
        }
        found.push((*a, probe(*a)?));
    }
    Ok(false)
}

/// „Abbrechen“ of the pack search: it stops before the next address.
#[tauri::command]
pub async fn maint_scan_cancel(state: State<'_, AppState>) -> Result<(), MaintError> {
    state.scan_cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// „Master anhalten“: `start` on a trusted patched master with the marker off, then a fresh read. The slave count it
/// polled right before is kept: the release waits for it again.
#[tauri::command]
pub async fn maint_halt(state: State<'_, AppState>, installation: String) -> Result<MasterView, MaintError> {
    marker_action(&state, &installation, Action::Start, None)
}

/// „Master freigeben“: `stop`, only when 0x64 shows the marker on (never automatic), until the master reports its DIP
/// slave count again: the one kept by the halt, else `expected_slaves` the user confirmed (None then: refused with
/// "expected-slaves-needed").
#[tauri::command]
pub async fn maint_release(state: State<'_, AppState>, installation: String, expected_slaves: Option<u8>) -> Result<MasterView, MaintError> {
    marker_action(&state, &installation, Action::Stop { expected: 0 }, expected_slaves)
}

/// The DIP slave count kept for the installation's master by its halt (None: none kept for this master board).
#[tauri::command]
pub async fn maint_expected_slaves(state: State<'_, AppState>, installation: String) -> Result<Option<u8>, MaintError> {
    let board = master_board(&state, &installation)?;
    with_store(&state, |s| s.expected_slaves(&installation, board))
}

pub(crate) fn master_board(state: &AppState, inst: &str) -> Result<Option<i64>, MaintError> {
    with_store(state, |s| Ok(s.current_board(inst, 0)?.map(|b| b.id)))
}

/// The count a release waits for: the one kept by the halt, else the user's (None: ask the user).
pub fn release_target(kept: Option<u8>, confirmed: Option<u8>) -> Option<u8> {
    kept.or(confirmed)
}

/// What a finished release can say about the polling: N = 0 has no slaves to poll; for N > 0 only a pack bus capture
/// with polls and slave answers would verify it, which this step does not take (CAN socket telemetry of the slaves is
/// no proof: they answer there on their own).
pub fn polling_after_release(expected: u8) -> &'static str {
    if expected == 0 { "no-slaves" } else { "unverified" }
}

static LAST_LIVE: Mutex<String> = Mutex::new(String::new());

fn marker_action(state: &AppState, installation: &str, action: Action, confirmed: Option<u8>) -> Result<MasterView, MaintError> {
    let name = if action == Action::Start { "halt" } else { "release" };
    event(state, Some(installation), &format!("{name} pressed"));
    let r = marker_action_inner(state, installation, action, confirmed);
    match &r {
        Ok(m) => event(state, Some(installation), &format!("{name} done {}", master_text(m))),
        Err(e) => event(state, Some(installation), &format!("{name} failed {}", serde_json::to_string(e).unwrap_or_default())),
    }
    if let Ok(mut last) = LAST_LIVE.lock() {
        last.clear();
    }
    r
}

/// The master behind the CAN socket must be the installation's own, if its battery serial is stored: the CAN socket
/// address may change while the marker is set (a gateway that moved), and a new address must never halt or release
/// another bank. With a stored serial only a set value that matches passes: another serial is refused, and so is a
/// read that shows none (empty, the placeholder, not offered, a read error). Without a stored serial nothing is checked.
fn same_master(state: &AppState, inst: &Installation) -> Result<(), MaintError> {
    let stored = with_store(state, |s| Ok(s.current_board(&inst.id, 0)?.and_then(|b| b.battery_serial)))?;
    let Some(stored) = stored else { return Ok(()) };
    // Modbus, else the patch's binary 0xA2 (bases before 16.06.11 have no Modbus serial block)
    let read = with_control(state, inst, |c| Ok::<_, String>(crate::marker::master_serial(c)))?;
    master_check(&stored, read.ok().flatten().as_ref())
}

/// `same_master` on a stored battery serial and the UART1 read (None: the read failed).
pub fn master_check(stored: &str, read: Option<&SerialValue>) -> Result<(), MaintError> {
    match read {
        Some(read @ SerialValue::Set { .. }) if crate::notify::master_matches(Some(stored), read) => Ok(()),
        Some(SerialValue::Set { .. }) => Err(refused("other-master")),
        _ => Err(refused("master-serial-unreadable")),
    }
}

fn marker_action_inner(state: &AppState, installation: &str, action: Action, confirmed: Option<u8>) -> Result<MasterView, MaintError> {
    if state.flashing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(MaintError::Busy);
    }
    let _op = crate::control::reserve_bus_wait(state).ok_or(MaintError::Busy)?;
    let _a = begin_action(state, if action == Action::Start { "halt" } else { "release" });
    let inst = self::installation(state, installation)?;
    same_master(state, &inst)?;
    // a fresh exact identity and state on the very adapter the action goes to
    let m = read_master(state, &inst)?;
    let allowed = if action == Action::Start { m.may_halt() } else { m.may_release() };
    if !allowed {
        return Err(refused(if action == Action::Start { "halt-not-offered" } else { "release-not-offered" }));
    }
    let board = master_board(state, &inst.id)?;
    let action = match action {
        Action::Start => {
            // the slave count it polls now is the one the release must see again
            if let Some(n) = m.effective_slaves() {
                with_store(state, |s| s.set_expected_slaves(&inst.id, board, n, crate::history::now_ms()))?;
            }
            Action::Start
        }
        _ => {
            let kept = with_store(state, |s| s.expected_slaves(&inst.id, board))?;
            Action::Stop { expected: release_target(kept, confirmed).ok_or_else(|| refused("expected-slaves-needed"))? }
        }
    };
    let r = control_dialog(state, &inst, action)?;
    if let (Action::Start, Err(DialogError::Rejected(5))) = (action, &r) {
        // RTN 05 alone names no cause: the master's fresh block explains it
        let block = with_control(state, &inst, |c| c.uart1_parameters(Bus::CanSocketBus, 0)).ok().and_then(|r| r.ok()).map(|p| p.block);
        return Err(MaintError::StartRefused { reason: crate::profile::start_refusal(m.caps(), block.as_deref()).into() });
    }
    // the read after the dialog decides what is shown, also after a failed one
    let after = read_master(state, &inst);
    r?;
    let mut v = master_view(&after?);
    if let Action::Stop { expected } = action {
        with_store(state, |s| s.clear_expected_slaves(&inst.id))?;
        v.polling = Some(polling_after_release(expected));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbms_flash::maint::policy::Patch;
    use openbms_proto::MaintInfo;

    fn refused_with(r: Result<impl std::fmt::Debug, MaintError>, key: &str) -> bool {
        matches!(r, Err(MaintError::Refused { reason }) if reason == key)
    }

    fn pending(pack: u8) -> Pending {
        Pending { id: 1, endpoint: "serial:/dev/p".into(), pack, release: "16.06.13".into(), since: 0, bms_serial: None }
    }

    fn store_at(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("openbms-maint-rs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Store::open(&dir.join("maintenance.sqlite")).unwrap()
    }

    #[test]
    fn a_tcp_flash_that_failed_after_erase_blocks_other_packs_on_the_same_key() {
        let store = Arc::new(Mutex::new(Some(store_at("tcp-pending"))));
        // the key the UI formed for a gateway; it opens the network endpoint
        let key = pack_key("tcp:192.0.2.20:4196").unwrap();
        assert!(matches!(pack_endpoint(&key).unwrap(), Endpoint::Tcp { addr } if addr == "192.0.2.20:4196"));
        assert!(matches!(pack_endpoint("serial:/dev/cu.test").unwrap(), Endpoint::Serial { path, baud: 19_200 } if path == "/dev/cu.test"));
        // `firmware_start`: free, the record written before the restart, the flash breaks off after the erase
        pending_check(&store, &key, 3).unwrap();
        with_store_arc(&store, |s| s.add_pending(&key, 3, "16.06.13", None, 1)).unwrap();
        assert_eq!(after_flash(false, Some(Progress::Erase), || false), AfterFlash::Keep);
        // another pack on the same gateway is refused, also when its key arrives written in another case
        assert!(matches!(pending_check(&store, &key, 4), Err(MaintError::BootloaderPending { pack: 3, .. })));
        assert!(matches!(pending_check(&store, &pack_key(" TCP:192.0.2.20:4196 ").unwrap(), 4), Err(MaintError::BootloaderPending { pack: 3, .. })));
        pending_check(&store, &key, 3).unwrap();
        // the record carries the key the UI compares with; a bare path is no key
        assert_eq!(with_store_arc(&store, |s| s.all_pending()).unwrap()[0].endpoint, key);
        // every other key is refused too, naming the open record's endpoint: another name of the same gateway is one
        for other in ["serial:/dev/cu.test", "tcp:bms.example:4196"] {
            for address in [3, 4] {
                assert!(matches!(pending_check(&store, other, address), Err(MaintError::BootloaderPending { pack: 3, endpoint }) if endpoint == key));
            }
        }
        assert!(refused_with(pack_key("/dev/cu.test"), "no-port"));
        assert!(refused_with(pack_key(""), "no-port"));
    }

    #[test]
    fn saving_ports_takes_the_bus_and_names_the_refusal() {
        let s = store_at("save-ports");
        let a = s.create_installation("A", Some(ControlEndpoint::Tcp { addr: "192.0.2.10:4196".into() }), vec!["tcp:192.0.2.11:4196".into()]).unwrap();
        s.set_master_read(&a.id, "{}", true, 1).unwrap();
        let state = AppState::default();
        *state.maint.lock().unwrap() = Some(s);
        assert!(refused_with(save_installation(&state, &Installation { control: None, ..a.clone() }), "maintenance-open"));
        // while a pack action holds the bus, nothing is saved
        let held = crate::control::reserve_pack(&state, 1, "params").unwrap();
        assert!(matches!(save_installation(&state, &Installation { name: "B".into(), ..a.clone() }), Err(MaintError::Busy)));
        drop(held);
        assert_eq!(save_installation(&state, &Installation { name: "B".into(), ..a.clone() }).unwrap().name, "B");
        state.flashing.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(save_installation(&state, &a), Err(MaintError::Busy)));
    }

    #[test]
    fn a_cancelled_scan_stops_before_the_next_address() {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut seen = vec![];
        for a in scan_addresses(&cancel) {
            seen.push(a);
            if a == 2 {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        assert_eq!(seen, vec![0, 1, 2]);
        cancel.store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(scan_addresses(&cancel).count(), 16);
    }

    #[test]
    fn every_event_line_names_its_installation() {
        assert_eq!(event_line("20260102-030405Z.123", Some("inst-1"), "halt pressed"), "20260102-030405Z.123 inst=inst-1 halt pressed");
        assert_eq!(event_line("t", None, "live bus=quiet"), "t inst=- live bus=quiet");
    }

    #[test]
    fn a_pending_pack_blocks_every_flash_but_its_own_at_its_endpoint() {
        assert!(pending_refusal(&[], "serial:/dev/p", 3).is_ok());
        assert!(pending_refusal(&[pending(3)], "serial:/dev/p", 3).is_ok());
        assert!(matches!(pending_refusal(&[pending(3)], "serial:/dev/p", 4), Err(MaintError::BootloaderPending { pack: 3, .. })));
        assert!(matches!(pending_refusal(&[pending(3)], "serial:/dev/q", 3), Err(MaintError::BootloaderPending { pack: 3, .. })));
        assert!(matches!(pending_refusal(&[pending(3)], "serial:/dev/q", 4), Err(MaintError::BootloaderPending { pack: 3, .. })));
        let v = serde_json::to_value(MaintError::BootloaderPending { pack: 3, endpoint: "serial:/dev/p".into() }).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "bootloader-pending", "pack": 3, "endpoint": "serial:/dev/p" }));
    }

    #[test]
    fn a_marker_action_needs_the_stored_master_serial_read_back() {
        let set = |v: &str| SerialValue::Set { value: v.into(), truncated: false };
        assert!(master_check("BANK-A", Some(&set("BANK-A"))).is_ok());
        // the CAN socket may show only the first 22 characters
        assert!(master_check("BANK-A-0123456789-ABCDEFG", Some(&SerialValue::Set { value: "BANK-A-0123456789-ABCD".into(), truncated: true })).is_ok());
        assert!(refused_with(master_check("BANK-A", Some(&set("BANK-B"))), "other-master"));
        for v in [SerialValue::Empty, SerialValue::Placeholder { value: "EMU110110S-XXXX".into() }, SerialValue::Unavailable, SerialValue::NoAnswer] {
            assert!(refused_with(master_check("BANK-A", Some(&v)), "master-serial-unreadable"));
        }
        assert!(refused_with(master_check("BANK-A", None), "master-serial-unreadable"));
    }

    #[test]
    fn a_flash_target_with_another_boards_serial_is_refused() {
        let set = |v: &str| SerialValue::Set { value: v.into(), truncated: false };
        let board = Board { id: 1, bms_serial: "SYN-5".into(), battery_serial: None, first_seen: 0, last_seen: 0, aliases: vec!["SYN-5-OLD".into()] };
        assert!(target_check(Some(&board), &set("SYN-5")).is_ok());
        assert!(target_check(Some(&board), &set("SYN-5-OLD")).is_ok());
        assert!(refused_with(target_check(Some(&board), &set("SYN-OTHER")), "other-pack"));
        // unknown or unreadable identity: as before
        assert!(target_check(None, &set("SYN-OTHER")).is_ok());
        for v in [SerialValue::NoAnswer, SerialValue::Unavailable, SerialValue::Empty, SerialValue::Placeholder { value: "EMU110110S-XXXX".into() },
            SerialValue::Set { value: "SYN-CUT".into(), truncated: true }] {
            assert!(target_check(Some(&board), &v).is_ok());
        }
    }

    #[test]
    fn the_pending_pack_is_flashed_again_only_as_the_same_board() {
        let with = |serial: Option<&str>| Pending { bms_serial: serial.map(Into::into), ..pending(3) };
        let key = "serial:/dev/p";
        assert!(pending_same_board(&[with(Some("SYN-3"))], key, 3, Some("SYN-3")).is_ok());
        assert!(refused_with(pending_same_board(&[with(Some("SYN-3"))], key, 3, Some("SYN-B")), "pending-other-pack"));
        assert!(refused_with(pending_same_board(&[with(Some("SYN-3"))], key, 3, None), "pending-unverified"));
        // no stored serial: the board cannot be told apart, refused whatever the target reports; other records are
        // `pending_refusal`'s
        assert!(refused_with(pending_same_board(&[with(None)], key, 3, None), "pending-unverified"));
        assert!(refused_with(pending_same_board(&[with(None)], key, 3, Some("SYN-3")), "pending-unverified"));
        assert!(pending_same_board(&[with(Some("SYN-3"))], key, 4, None).is_ok());
    }

    #[test]
    fn a_failed_flash_clears_its_record_only_for_the_same_board() {
        let read = |v: Option<&str>| { let v = v.map(String::from); move || v };
        // without a stored serial an answer proves nothing: the record stays
        assert!(!answered_same(true, None, || unreachable!()));
        assert!(answered_same(true, Some("SYN-3"), read(Some("SYN-3"))));
        assert!(!answered_same(true, Some("SYN-3"), read(Some("SYN-B"))));
        assert!(!answered_same(true, Some("SYN-3"), read(None)));
        assert!(!answered_same(false, Some("SYN-3"), || unreachable!()));
        assert!(!answered_same(false, None, || unreachable!()));
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
        let info = |patch: [u8; 2], id: [u8; 8]| MaintInfo { format: 4, base: [16, 6, 13], patch, build_id: id, hard_stop: None, power_rtc: false, pack_cutoff: false , reserved_flags: 0, patch_rev: 3, uart0: false };
        let st = |word: u32| Some(MaintState { marker_word: word, effective_slaves: 0 });
        let listed = [0xBA, 0xA0, 0x1E, 0xCD, 0x5A, 0xF4, 0xEB, 0x28];
        let patched = |i: MaintInfo, s| Master::Patched { patch: Patch::recognise(i).unwrap(), state: s };
        let v = master_view(&patched(info([1, 6], listed), st(0)));
        assert_eq!((v.kind, v.version.as_deref(), v.listed.as_deref(), v.marker, v.marker_word.as_deref()), ("patched", Some("16.06.13 MAINT 1.6.3"), Some("16.06.13 MAINT 1.6.3"), Some("on"), Some("00000000")));
        assert!(v.may_release && !v.may_halt && v.caps.is_some_and(|c| c.start_gate));
        let v = master_view(&patched(info([1, 5], listed), st(0x0102_0304)));
        assert_eq!((v.marker, v.may_release, v.may_halt), (Some("foreign"), false, false));
        // not listed, same patch version: same configuration, no name
        let v = master_view(&patched(info([1, 5], [0; 8]), st(0)));
        assert_eq!((v.kind, v.listed, v.may_release, v.patch.as_deref(), v.base.as_deref()), ("patched", None, true, Some("1.5.3"), Some("16.06.13")));
        let v = master_view(&Master::UnknownPatch(Some(info([1, 4], [0; 8]))));
        assert_eq!((v.kind, v.may_release, v.caps), ("unknown-patch", false, None));
        let v = master_view(&Master::No0x64);
        assert_eq!((v.kind, v.marker, v.version, v.hard_stop), ("no-0x64", None, None, None));
        // format 3: the hard stop of the image; an unknown OEM revision
        let i3 = MaintInfo { format: 4, base: [16, 6, 0xFF], patch: [1, 5], build_id: [1; 8], hard_stop: Some(false), power_rtc: false, pack_cutoff: false , reserved_flags: 0, patch_rev: 3, uart0: false };
        let v = master_view(&patched(i3, st(u32::MAX)));
        assert_eq!((v.kind, v.hard_stop, v.base.as_deref(), v.version.as_deref(), v.may_halt), ("patched", Some(false), Some("16.06.?"), Some("16.06.? MAINT 1.5.3"), true));
        assert_eq!(master_view(&patched(info([1, 5], listed), st(0))).hard_stop, None);
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
        let st = |marker: bool, slaves| MaintState { marker_word: if marker { 0 } else { u32::MAX }, effective_slaves: slaves };
        assert!(target_reached(Action::Start, st(true, 0)));
        assert!(!target_reached(Action::Start, st(true, 3)));
        assert!(target_reached(Action::Stop { expected: 3 }, st(false, 3)));
        // the marker off with another slave count is not the target yet
        assert!(!target_reached(Action::Stop { expected: 3 }, st(false, 0)));
        assert!(!target_reached(Action::Stop { expected: 0 }, st(true, 0)));
    }

    #[test]
    fn a_release_waits_for_the_kept_or_the_confirmed_slave_count() {
        assert_eq!(release_target(Some(11), Some(3)), Some(11), "the count read before the halt wins");
        assert_eq!(release_target(None, Some(3)), Some(3));
        assert_eq!(release_target(None, None), None);
        assert_eq!(polling_after_release(0), "no-slaves");
        assert_eq!(polling_after_release(11), "unverified");
        let s = store_at("expected");
        let a = s.create_installation("A", None, vec![]).unwrap();
        s.set_expected_slaves(&a.id, Some(4), 11, 1).unwrap();
        assert_eq!(s.expected_slaves(&a.id, Some(4)).unwrap(), Some(11));
        // another master board: not its count
        assert_eq!(s.expected_slaves(&a.id, Some(5)).unwrap(), None);
        s.clear_expected_slaves(&a.id).unwrap();
        assert_eq!(s.expected_slaves(&a.id, Some(4)).unwrap(), None);
    }

    #[test]
    fn a_version_proof_is_per_place_replaced_cleared_and_gone_with_its_installation() {
        let s = store_at("proof");
        let a = s.create_installation("A", None, vec![]).unwrap();
        s.set_patch_proof(&a.id, 3, "SYN-3", "16.06.13 MAINT 1.5", "flash", 1).unwrap();
        assert_eq!(s.patch_proof(&a.id, 3).unwrap().map(|p| (p.bms_serial, p.source)), Some(("SYN-3".into(), "flash".into())));
        assert_eq!(s.patch_proof(&a.id, 4).unwrap(), None);
        s.set_patch_proof(&a.id, 3, "SYN-3B", "MAINT ≥ 1.5", "user", 2).unwrap();
        assert_eq!(s.patch_proof(&a.id, 3).unwrap().map(|p| p.bms_serial), Some("SYN-3B".into()));
        s.clear_patch_proof(&a.id, 3).unwrap();
        assert_eq!(s.patch_proof(&a.id, 3).unwrap(), None);
        s.set_patch_proof(&a.id, 3, "SYN-3", "16.06.13 MAINT 1.5", "flash", 3).unwrap();
        s.set_expected_slaves(&a.id, None, 2, 3).unwrap();
        s.delete_installation(&a.id).unwrap();
        assert_eq!(s.patch_proof(&a.id, 3).unwrap(), None);
        assert_eq!(s.expected_slaves(&a.id, None).unwrap(), None);
        assert!(s.set_patch_proof("missing", 1, "X", "r", "user", 4).is_err());
    }

    #[test]
    fn a_flash_start_writes_its_pending_record_and_ends_the_proof_together() {
        let s = store_at("pending-with");
        let a = s.create_installation("A", None, vec![]).unwrap();
        s.set_patch_proof(&a.id, 0, "SYN-0", "16.06.13 MAINT 1.5", "flash", 1).unwrap();
        // the second step fails: no pending record stays behind, the proof is untouched
        assert!(s.add_pending_with("serial:/dev/p", 0, "16.06.13", Some("SYN-0"), 2, |_| Err("disk I/O error".into())).is_err());
        assert!(s.all_pending().unwrap().is_empty());
        assert!(s.patch_proof(&a.id, 0).unwrap().is_some());
        s.note_hard_stop(&a.id, 0, 2).unwrap();
        // a failing step after both clears: nothing of it stays
        assert!(s.add_pending_with("serial:/dev/p", 0, "16.06.13", Some("SYN-0"), 2, |st| {
            st.clear_patch_proof(&a.id, 0)?;
            st.clear_hard_stop(&a.id, 0)?;
            Err("disk I/O error".into())
        }).is_err());
        assert!(s.all_pending().unwrap().is_empty() && s.patch_proof(&a.id, 0).unwrap().is_some() && s.hard_stops(&a.id).unwrap().len() == 1);
        // all succeed: the record is there, the proof and the hard-stop observation gone
        s.add_pending_with("serial:/dev/p", 0, "16.06.13", Some("SYN-0"), 3, |st| {
            st.clear_patch_proof(&a.id, 0)?;
            st.clear_hard_stop(&a.id, 0)
        }).unwrap();
        assert_eq!(s.all_pending().unwrap().len(), 1);
        assert!(s.patch_proof(&a.id, 0).unwrap().is_none());
        assert!(s.hard_stops(&a.id).unwrap().is_empty());
    }

    #[test]
    fn a_lost_02_never_clears_a_stored_marker_and_the_flash_stays_refused() {
        let info = MaintInfo { format: 4, base: [16, 6, 13], patch: [1, 5], build_id: [0x89, 0xD8, 0x5C, 0x61, 0xB4, 0x0E, 0x03, 0xDA], hard_stop: None, power_rtc: false, pack_cutoff: false , reserved_flags: 0, patch_rev: 3, uart0: false };
        let patched = |state| Master::Patched { patch: Patch::recognise(info).unwrap(), state };
        let on = patched(Some(MaintState { marker_word: 0, effective_slaves: 0 }));
        let off = patched(Some(MaintState { marker_word: u32::MAX, effective_slaves: 3 }));
        let lost = patched(None);
        assert_eq!((marker_update(&on), marker_update(&off), marker_update(&lost)), (Some(true), Some(false), None));
        assert_eq!(marker_update(&patched(Some(MaintState { marker_word: 7, effective_slaves: 0 }))), None);
        assert_eq!(marker_update(&Master::UnknownPatch(None)), None);
        // RTN 04: the current image does not know 0x64; the marker word stays in flash
        assert_eq!(marker_update(&Master::No0x64), None);
        let s = store_at("marker-keep");
        let a = s.create_installation("A", None, vec![]).unwrap();
        s.set_master_read(&a.id, "{}", true, 1).unwrap();
        // the read without a state keeps the marker on
        s.set_master_read_keep_marker(&a.id, "{\"state\":null}", 2).unwrap();
        assert!(s.marker_last_on(&a.id).unwrap());
        assert_eq!(crate::firmware::master_marker_check(0, openbms_flash::known::ImageKind::Original, Some(&lost), s.marker_last_on(&a.id).unwrap()), Err("release-master-first-unread"));
        // an image without 0x64 afterwards (flashed outside the app): still on, now latent
        let data = serde_json::to_string(&master_view(&Master::No0x64)).unwrap();
        s.set_master_read_keep_marker(&a.id, &data, 3).unwrap();
        assert!(s.marker_last_on(&a.id).unwrap());
        assert!(s.markers_on().unwrap()[0].latent);
        // „Latenten Marker verwerfen“ clears only a latent one: not one read on under a patched image
        let b2 = s.create_installation("B2", None, vec![]).unwrap();
        s.set_master_read(&b2.id, &serde_json::to_string(&master_view(&on)).unwrap(), true, 3).unwrap();
        assert!(!s.discard_latent_marker(&b2.id).unwrap());
        assert!(s.marker_last_on(&b2.id).unwrap());
        assert!(s.discard_latent_marker(&a.id).unwrap());
        assert!(!s.marker_last_on(&a.id).unwrap());
        assert!(s.delete_installation(&a.id).is_ok(), "no longer open work");
        let a = s.create_installation("A", None, vec![]).unwrap();
        s.set_master_read(&a.id, "{}", true, 3).unwrap();
        // only a valid off clears it
        s.set_master_read(&a.id, "{}", marker_update(&off).unwrap(), 3).unwrap();
        assert!(!s.marker_last_on(&a.id).unwrap());
        // a first read without a state stores the marker off
        let b = s.create_installation("B", None, vec![]).unwrap();
        s.set_master_read_keep_marker(&b.id, "{}", 4).unwrap();
        assert!(!s.marker_last_on(&b.id).unwrap());
    }

    #[test]
    fn cancel_stops_the_identity_probes_before_the_next_pack() {
        use openbms_flash::maint::policy::PackFinding;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut found = vec![];
        let mut asked = vec![];
        // „Abbrechen“ during the probe of pack 2: pack 3 and later are not asked, what came is kept
        let stopped = probe_until_cancel::<_, ()>(&[1, 2, 3, 4], &cancel, &mut found, |a| {
            asked.push(a);
            if a == 2 {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            Ok(PackFinding::Undecided)
        }).unwrap();
        assert!(stopped);
        assert_eq!(asked, vec![1, 2]);
        assert_eq!(found.len(), 2);
        let mut found = vec![];
        assert!(!probe_until_cancel::<_, ()>(&[1, 2], &cancel, &mut found, |_| Ok(PackFinding::Undecided)).unwrap());
        // the same rule before every pack bus String 2 read
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut serials: Vec<(u8, Option<String>)> = vec![];
        assert!(probe_until_cancel::<_, ()>(&[1, 2], &cancel, &mut serials, |_| Ok(Some("SYN".into()))).unwrap());
        assert!(serials.is_empty());
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn the_pack_list_asks_64_03_fresh_after_a_matching_string_2() {
        use openbms_flash::maint::dialog::PackAnswer;
        use openbms_flash::maint::policy::PackFinding;
        let info = MaintInfo { format: 4, base: [16, 6, 13], patch: [1, 5], build_id: [1; 8], hard_stop: Some(false), power_rtc: false, pack_cutoff: false , reserved_flags: 0, patch_rev: 3, uart0: false };
        let fresh = PackFinding::of(PackAnswer::Info(info));
        let never = || -> Result<PackFinding, ()> { panic!("no 64/03 without a matching String 2 or after a cancel") };
        // another board on the CAN side, or none readable: no 64/03
        assert_eq!(board_finding("SYN-1", Some("SYN-B"), false, never).unwrap(), PackFinding::Undecided);
        assert_eq!(board_finding("SYN-1", None, false, never).unwrap(), PackFinding::Undecided);
        // the same board: always a fresh 64/03 in this search (no cached finding is taken)
        let mut asked = 0;
        assert_eq!(board_finding::<()>("SYN-1", Some("SYN-1"), false, || { asked += 1; Ok(fresh) }).unwrap(), fresh);
        assert_eq!(asked, 1);
        assert_eq!(board_finding("SYN-1", Some("SYN-1"), true, never).unwrap(), PackFinding::Undecided);
    }

    #[test]
    fn a_single_board_is_proven_by_its_string_2_on_both_sides() {
        // matching String 2 without any master: the CAN version belongs to that board
        assert!(same_board("SYN-1", Some("SYN-1")));
        // another board on the CAN side, or none readable: not shown
        assert!(!same_board("SYN-1", Some("SYN-2")));
        assert!(!same_board("SYN-1", None));
        // a placeholder or empty serial is no canonical identity, so it never reaches the comparison
        assert_eq!(openbms_proto::serial::canonical(b"EMU110110S-XXXX               "), None);
        assert_eq!(unique_serials(vec![(1, None)]), vec![]);
        // a serial shared by two found packs proves nothing
        assert_eq!(unique_serials(vec![(1, Some("SYN-A".into())), (2, Some("SYN-A".into())), (3, Some("SYN-C".into()))]), vec![(3, "SYN-C".to_string())]);
    }

    #[test]
    fn a_timeout_names_the_marker_it_saw() {
        let last = MaintState { marker_word: 0x1234_5678, effective_slaves: 2 };
        let e = MaintError::from(DialogError::Timeout { after: Duration::from_secs(3), last });
        assert_eq!(serde_json::to_value(e).unwrap(), serde_json::json!({ "kind": "timeout", "marker": "foreign", "slaves": 2 }));
    }

    #[test]
    fn every_pack_bus_open_frees_the_port_first() {
        let pack = |k: &str| (k.to_string(), Bus::PackBus);
        let can = |k: &str| (k.to_string(), Bus::CanSocketBus);
        assert!(must_stop_monitor(Some(&pack("serial:/dev/b")), "serial:/dev/a"));
        assert!(must_stop_monitor(Some(&can("serial:/dev/a")), "serial:/dev/a"));
        assert!(!must_stop_monitor(Some(&can("tcp:gw:4196")), "serial:/dev/a"));
        assert!(!must_stop_monitor(None, "serial:/dev/a"));
        // a monitor on the same network gateway is stopped like one on the same USB port
        assert!(must_stop_monitor(Some(&can("tcp:192.0.2.5:4196")), "tcp:192.0.2.5:4196"));
        assert!(!must_stop_monitor(Some(&can("tcp:192.0.2.5:4196")), "tcp:192.0.2.6:4196"));
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
