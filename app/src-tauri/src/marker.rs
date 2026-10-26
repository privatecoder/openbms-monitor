//! The maintenance marker under „Steuern“: read, set and clear it over the monitor's own connection, also without an
//! installation (a board on the bench). CAN socket: the master (address 0) of every supported MAINT build, set and
//! clear. RS485-1/2: a build with image flag bit 4 (MAINT 1.5.1/1.6.1) at the pack's own address, read and clear; the
//! start exists on the CAN socket only. An older build or the original firmware does not know 0x64 on RS485-1/2.

use crate::bms_memory::bus_of;
use crate::control::reserve_pack_wait;
use crate::maint::{event, keep_master_read, marker_name, master_board, with_store, MaintError};
use crate::{link_now, with_link, AppState, Link};
use openbms_flash::maint::dialog::{self, Action, Budgets, DialogError, PackAnswer, Probe};
use openbms_flash::maint::policy::{Master, Patch};
use openbms_flash::maint::SystemClock;
use openbms_proto::{MaintInfo, MaintState, MarkerWord};
use openbms_transport::Bus;
use serde::Serialize;
use tauri::State;

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct MarkerView {
    pub gen: u64,
    pub address: u8,
    /// "can" | "pack"
    pub bus: &'static str,
    /// "patched": a supported build that shows its marker here; "no-uart0": RS485-1/2, a supported build without image
    /// flag bit 4 (use the CAN socket); "unknown-patch"; "no-0x64": the CAN socket, RTN 04 (original firmware);
    /// "unknown": no valid answer (on RS485-1/2 also RTN 04 of an older build or the original firmware)
    pub kind: &'static str,
    /// the identity as reported, e.g. "16.06.13 MAINT 1.6.1"
    pub version: Option<String>,
    /// "off" | "on" | "foreign"
    pub marker: Option<&'static str>,
    /// the slave count the board works with (0 while the marker is set); after a clear None: not read back (a master
    /// polling again does not answer on RS485-1/2)
    pub slaves: Option<u8>,
    pub may_set: bool,
    pub may_clear: bool,
}

/// What one read found: the identity, and the state where the build shows it on this bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Found {
    pub kind: &'static str,
    pub info: Option<MaintInfo>,
    pub state: Option<MaintState>,
    /// set and clear are known to be offered by this build on this bus (`status_split` on the CAN socket, flag bit 4 on
    /// RS485-1/2)
    pub trusted: bool,
    /// the read as the maintenance view keeps it (the banner of an installation's master)
    pub master: Master,
}

impl Found {
    fn marker(&self) -> Option<MarkerWord> {
        self.state.map(|s| s.marker())
    }
    /// Set: the CAN socket only, marker known off.
    pub fn may_set(&self, can: bool) -> bool {
        can && self.trusted && self.marker() == Some(MarkerWord::Off)
    }
    /// Clear: marker known on (a foreign word is never touched).
    pub fn may_clear(&self) -> bool {
        self.trusted && self.marker() == Some(MarkerWord::On)
    }
}

pub(crate) fn of_master(m: &Master) -> Found {
    let info = match m {
        Master::Patched { patch, .. } => Some(patch.info),
        Master::UnknownPatch(i) => *i,
        _ => None,
    };
    let kind = match m {
        Master::Patched { .. } => "patched",
        Master::UnknownPatch(_) => "unknown-patch",
        Master::No0x64 => "no-0x64",
        Master::Unknown => "unknown",
    };
    Found { kind, info, state: m.state(), trusted: m.trusted(), master: *m }
}

/// The RS485-1/2 identity: whether this build shows its marker there (supported patch with flag bit 4).
pub(crate) fn rs485_kind(answer: PackAnswer) -> (&'static str, Option<MaintInfo>) {
    match answer {
        PackAnswer::Info(i) if Patch::recognise(i).is_some_and(|p| p.caps.status_split) && i.uart0 => ("patched", Some(i)),
        PackAnswer::Info(i) if Patch::recognise(i).is_some() => ("no-uart0", Some(i)),
        PackAnswer::Info(i) => ("unknown-patch", Some(i)),
        PackAnswer::OtherMnt => ("unknown-patch", None),
        PackAnswer::Undecided => ("unknown", None),
    }
}

fn view(gen: u64, address: u8, can: bool, f: &Found) -> MarkerView {
    MarkerView {
        gen,
        address,
        bus: if can { "can" } else { "pack" },
        kind: f.kind,
        version: f.info.map(|i| i.version()),
        marker: f.marker().map(marker_name),
        slaves: f.state.map(|s| s.effective_slaves),
        may_set: f.may_set(can),
        may_clear: f.may_clear(),
    }
}

/// The connection to act on: the current one, of the installation the UI shows (None: „Nur ansehen“ or no
/// installation); the CAN socket only at the master.
fn bound(state: &AppState, installation: Option<&str>, address: u8) -> Result<(Link, bool), String> {
    let link = link_now(state);
    if link.installation.as_deref() != installation {
        return Err("the connection belongs to another installation than the one chosen; connect again".into());
    }
    let can = bus_of(state, &link)? == Bus::CanSocketBus;
    if can && address != 0 {
        return Err("marker-master-only".into());
    }
    Ok((link, can))
}

fn read(state: &AppState, link: &Link, address: u8, can: bool) -> Result<Found, String> {
    let clock = SystemClock::new();
    with_link(&state.conn, &state.flashing, &state.link, link, |c| {
        let io = |e: openbms_flash::Error| match e {
            // nothing sent: the bus rule's own error, never a silent board
            openbms_flash::Error::Refused(k) => openbms_transport::Error::Refused(k),
            e => openbms_transport::Error::Io(std::io::Error::other(e.to_string())),
        };
        if can {
            return dialog::read(c, &clock, Budgets::default()).map(|m| of_master(&m)).map_err(io);
        }
        rs485_read(c, &clock, address).map_err(io)
    })
}

/// The identity and marker of the pack at `address` over RS485-1/2: a build with image flag bit 4 shows its state,
/// anything else none (`trusted` false). Also „Platine flashen“.
pub(crate) fn rs485_found(c: &mut openbms_transport::Connection, clock: &SystemClock, address: u8) -> Found {
    rs485_read(c, clock, address).unwrap_or_else(|_| found_of(c, clock, address, PackAnswer::Undecided))
}

/// `rs485_found`, but a refusal of the bus rule (nothing sent) stays an error instead of an undecided board.
pub(crate) fn rs485_read(c: &mut openbms_transport::Connection, clock: &SystemClock, address: u8) -> Result<Found, openbms_flash::Error> {
    let budget = crate::profile::identity_budget(c);
    let answer = match dialog::probe_pack(c, clock, address, budget) {
        Ok(a) => a,
        Err(e @ openbms_flash::Error::Refused(_)) => return Err(e),
        Err(_) => PackAnswer::Undecided,
    };
    Ok(found_of(c, clock, address, answer))
}

fn found_of(c: &mut openbms_transport::Connection, clock: &SystemClock, address: u8, answer: PackAnswer) -> Found {
    let (kind, info) = rs485_kind(answer);
    let state = if kind == "patched" { dialog::dialog_at(c, clock, address, Action::Status, Budgets::default()).ok() } else { None };
    // as the maintenance view keeps a master read: the identity, and the state where this bus shows it
    let master = Master::of(info.map(Probe::Info)).with_state(state);
    Found { kind, info, state, trusted: kind == "patched", master }
}

/// Read the board's identity and marker (changes nothing).
#[tauri::command]
pub async fn marker_read(state: State<'_, AppState>, address: u8, installation: Option<String>) -> Result<MarkerView, String> {
    let _guard = reserve_pack_wait(&state, address, "marker")?;
    let (link, can) = bound(&state, installation.as_deref(), address)?;
    let f = read(&state, &link, address, can)?;
    // the installation's master: the banner follows what this read showed
    // (only for its own master: a read of another bank is shown, never kept under this installation)
    if let Some(i) = link.installation.as_deref().filter(|_| address == 0) {
        if right_bank(&state, &link, i, address, can).is_ok() {
            keep_master_read(&state, i, &f.master).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
        }
    }
    Ok(view(link.gen, address, can, &f))
}

/// The target belongs to installation `inst`: over the CAN socket (the master) its stored battery serial must be the one
/// the master reports there (`master_serial`; none stored: not checked); over
/// RS485-1/2 the right-bank check of every write (ASCII 0xA4).
fn right_bank(state: &AppState, link: &Link, inst: &str, address: u8, can: bool) -> Result<(), String> {
    if !can {
        return crate::bank::check_target(state, link, address).map(|_| ());
    }
    let text = |e: MaintError| serde_json::to_string(&e).unwrap_or_default();
    let stored = with_store(state, |s| Ok(s.current_board(inst, 0)?.and_then(|b| b.battery_serial))).map_err(text)?;
    let Some(stored) = stored else { return Ok(()) };
    let read = with_link(&state.conn, &state.flashing, &state.link, link, |c| Ok(master_serial(c)))?;
    crate::maint::master_check(&stored, read.as_ref()).map_err(text)
}

/// The master's battery serial (String 1) over the CAN socket: Modbus, else the patch's binary 0xA2 (bases before
/// 16.06.11 have no Modbus serial block). None: neither readable.
pub(crate) fn master_serial(c: &mut openbms_transport::Connection) -> Option<openbms_proto::SerialValue> {
    use openbms_proto::{serial, uart1, SerialValue};
    let modbus = c.master_battery_serial().ok();
    if matches!(modbus, Some(SerialValue::Set { .. })) {
        return modbus;
    }
    let binary = c.uart1_serial(Bus::CanSocketBus, 0, uart1::CMD_STRING1).ok().and_then(|raw| serial::parse_ascii_serial(&raw).ok());
    binary.or(modbus)
}

/// Set (`on`, the CAN socket only) or clear the marker, after a fresh read that offers it; the view after the change.
/// With an installation the target must be its board (`right_bank`), and at the master the
/// maintenance view's records follow: the slave count kept before a set, dropped with a clear, and the marker for the
/// banner. Clearing waits for the marker off and reads the slave count one DIP cycle later (None: not read back).
#[tauri::command]
pub async fn marker_set(state: State<'_, AppState>, address: u8, installation: Option<String>, on: bool) -> Result<MarkerView, String> {
    let _guard = reserve_pack_wait(&state, address, "marker")?;
    let (link, can) = bound(&state, installation.as_deref(), address)?;
    let inst = link.installation.clone();
    if let Some(i) = inst.as_deref() {
        right_bank(&state, &link, i, address, can)?;
    }
    let before = read(&state, &link, address, can)?;
    let offered = if on { before.may_set(can) } else { before.may_clear() };
    if !offered {
        return Err(if on && !can { "marker-set-can-only" } else if on { "halt-not-offered" } else { "release-not-offered" }.into());
    }
    // the installation's master: the maintenance view's records
    let master_of = inst.as_deref().filter(|_| address == 0);
    let text = |e: MaintError| serde_json::to_string(&e).unwrap_or_default();
    let name = if on { "marker set" } else { "marker clear" };
    event(&state, inst.as_deref(), &format!("{name} pressed address={address} bus={}", if can { "can" } else { "pack" }));
    if let (Some(i), true, Some(n)) = (master_of, on, before.state.map(|s| s.effective_slaves)) {
        // the slave count it polls now is the one a release must see again
        let board = master_board(&state, i).map_err(text)?;
        with_store(&state, |s| s.set_expected_slaves(i, board, n, crate::history::now_ms())).map_err(text)?;
    }
    let clock = SystemClock::new();
    let r: Result<Option<u8>, DialogError> = with_link(&state.conn, &state.flashing, &state.link, &link, |c| {
        Ok(if on { dialog::dialog_at(c, &clock, address, Action::Start, Budgets::default()).map(|s| Some(s.effective_slaves)) } else { dialog::clear_at(c, &clock, address, Budgets::default()) })
    })?;
    if let (true, Err(DialogError::Rejected(5))) = (on, &r) {
        // RTN 05 alone names no cause: the master's fresh block explains it
        let block = with_link(&state.conn, &state.flashing, &state.link, &link, |c| c.uart1_parameters(Bus::CanSocketBus, 0)).ok().map(|p| p.block);
        let caps = before.info.and_then(Patch::recognise).map(|p| p.caps);
        let reason = crate::profile::start_refusal(caps, block.as_deref());
        event(&state, inst.as_deref(), &format!("{name} refused {reason}"));
        return Err(format!("start-refused:{reason}"));
    }
    match &r {
        Ok(n) => event(&state, inst.as_deref(), &format!("{name} done slaves={}", n.map_or("unread".into(), |n| n.to_string()))),
        Err(e) => event(&state, inst.as_deref(), &format!("{name} failed {e}")),
    }
    // the state now: a fresh read (also after a failed change); a cleared marker confirmed but the board silent then
    // (a master polling again on RS485-1/2) keeps the confirmation
    let after = read(&state, &link, address, can).ok().filter(|f| f.state.is_some());
    // released over RS485-1/2: unless the bus stayed quiet (no slaves), the master polls it again, and the monitor on
    // this connection must not ask any more; it stops and the UI says so
    if !can && !on && !matches!(r, Ok(Some(0))) {
        if let Some(app) = state.app.get() {
            let _ = crate::stop_monitor_and_tell(&state, app);
        }
    }
    let f = match (&r, after) {
        (_, Some(f)) => f,
        (Ok(n), None) => {
            let state = Some(MaintState { marker_word: if on { 0 } else { u32::MAX }, effective_slaves: n.unwrap_or(0) });
            Found { state, master: before.master.with_state(state), ..before }
        }
        (Err(_), None) => before,
    };
    if let Some(i) = master_of {
        keep_master_read(&state, i, &f.master).map_err(text)?;
        if f.marker() == Some(MarkerWord::Off) {
            with_store(&state, |s| s.clear_expected_slaves(i)).map_err(text)?;
        }
    }
    let n = r.map_err(|e| e.to_string())?;
    let mut v = view(link.gen, address, can, &f);
    if !on {
        v.slaves = n;
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(format: u8, patch: [u8; 2], uart0: bool) -> MaintInfo {
        MaintInfo { format, base: [16, 6, 13], patch, patch_rev: if format == 4 { 3 } else { 0 },
            // TEMP(maint-1.6): format 3 as the first 1.6 (V1_6_CUTOFF, by its build ID)
            build_id: if format == 3 { openbms_proto::maint::temp16::BUILD_ID } else { [1; 8] },
            hard_stop: (format > 2).then_some(false), power_rtc: true, pack_cutoff: true, uart0, reserved_flags: 0 }
    }

    #[test]
    fn rs485_shows_the_marker_only_with_flag_bit_4() {
        assert_eq!(rs485_kind(PackAnswer::Info(info(4, [1, 6], true))).0, "patched");
        assert_eq!(rs485_kind(PackAnswer::Info(info(4, [1, 6], false))).0, "no-uart0");
        assert_eq!(rs485_kind(PackAnswer::Info(info(3, [1, 6], false))).0, "no-uart0");
        assert_eq!(rs485_kind(PackAnswer::Info(info(4, [1, 4], true))).0, "unknown-patch");
        assert_eq!(rs485_kind(PackAnswer::OtherMnt).0, "unknown-patch");
        // RTN 04 of an older build or the original, or silence
        assert_eq!(rs485_kind(PackAnswer::Undecided).0, "unknown");
    }

    #[test]
    fn set_only_over_the_can_socket_clear_on_both_never_a_foreign_word() {
        let st = |w: u32| Some(MaintState { marker_word: w, effective_slaves: 0 });
        let off = Found { kind: "patched", info: Some(info(4, [1, 6], true)), state: st(u32::MAX), trusted: true, master: Master::Unknown };
        assert!(off.may_set(true) && !off.may_set(false) && !off.may_clear());
        let on = Found { state: st(0), ..off };
        assert!(on.may_clear() && !on.may_set(true));
        let foreign = Found { state: st(0x1234_5678), ..off };
        assert!(!foreign.may_clear() && !foreign.may_set(true));
        // the state not read, or a build not trusted: nothing offered
        assert!(!Found { state: None, ..on }.may_clear());
        assert!(!Found { trusted: false, ..on }.may_clear());
    }
}
