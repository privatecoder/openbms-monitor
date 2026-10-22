//! The maintenance marker under „Steuern“: read, set and clear it over the monitor's own connection, also without an
//! installation (a board on the bench). CAN socket: the master (address 0) of every supported MAINT build, set and
//! clear. RS485-1/2: a build with image flag bit 4 (MAINT 1.5.1/1.6.1) at the pack's own address, read and clear; the
//! start exists on the CAN socket only. An older build or the original firmware does not know 0x64 on RS485-1/2.

use crate::bms_memory::bus_of;
use crate::control::reserve_pack_wait;
use crate::maint::{event, marker_name};
use crate::{link_now, with_link, AppState, Link};
use openbms_flash::maint::dialog::{self, Action, Budgets, DialogError, PackAnswer};
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
    /// the slave count the board works with (0 while the marker is set)
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
    Found { kind, info, state: m.state(), trusted: m.trusted() }
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
        let io = |e: openbms_flash::Error| openbms_transport::Error::Io(std::io::Error::other(e.to_string()));
        if can {
            return dialog::read(c, &clock, Budgets::default()).map(|m| of_master(&m)).map_err(io);
        }
        let (kind, info) = rs485_kind(dialog::probe_pack(c, &clock, address, crate::profile::identity_budget(c)).map_err(io)?);
        if kind != "patched" {
            return Ok(Found { kind, info, state: None, trusted: false });
        }
        let state = dialog::dialog_at(c, &clock, address, Action::Status, Budgets::default()).ok();
        Ok(Found { kind, info, state, trusted: true })
    })
}

/// Read the board's identity and marker (changes nothing).
#[tauri::command]
pub async fn marker_read(state: State<'_, AppState>, address: u8, installation: Option<String>) -> Result<MarkerView, String> {
    let _guard = reserve_pack_wait(&state, address, "marker")?;
    let (link, can) = bound(&state, installation.as_deref(), address)?;
    let f = read(&state, &link, address, can)?;
    Ok(view(link.gen, address, can, &f))
}

/// Set (`on`, the CAN socket only) or clear the marker, after a fresh read that offers it; the view after the change.
/// Clearing waits for the marker off and one more DIP cycle: `slaves` then shows whether the master polls again.
#[tauri::command]
pub async fn marker_set(state: State<'_, AppState>, address: u8, installation: Option<String>, on: bool) -> Result<MarkerView, String> {
    let _guard = reserve_pack_wait(&state, address, "marker")?;
    let (link, can) = bound(&state, installation.as_deref(), address)?;
    let before = read(&state, &link, address, can)?;
    let offered = if on { before.may_set(can) } else { before.may_clear() };
    if !offered {
        return Err(if on && !can { "marker-set-can-only" } else if on { "halt-not-offered" } else { "release-not-offered" }.into());
    }
    let inst = link.installation.as_deref();
    let name = if on { "marker set" } else { "marker clear" };
    event(&state, inst, &format!("{name} pressed address={address} bus={}", if can { "can" } else { "pack" }));
    let clock = SystemClock::new();
    let action = if on { Action::Start } else { Action::Clear };
    let r = with_link(&state.conn, &state.flashing, &state.link, &link, |c| Ok(dialog::dialog_at(c, &clock, address, action, Budgets::default())))?;
    if let Err(DialogError::Rejected(5)) = &r {
        if on {
            // RTN 05 alone names no cause: the master's fresh block explains it
            let block = with_link(&state.conn, &state.flashing, &state.link, &link, |c| c.uart1_parameters(Bus::CanSocketBus, 0)).ok().map(|p| p.block);
            let caps = before.info.and_then(Patch::recognise).map(|p| p.caps);
            let reason = crate::profile::start_refusal(caps, block.as_deref());
            event(&state, inst, &format!("{name} refused {reason}"));
            return Err(format!("start-refused:{reason}"));
        }
    }
    let after = read(&state, &link, address, can);
    match &r {
        Ok(s) => event(&state, inst, &format!("{name} done marker={} slaves={}", marker_name(s.marker()), s.effective_slaves)),
        Err(e) => event(&state, inst, &format!("{name} failed {e}")),
    }
    let s = r.map_err(|e| e.to_string())?;
    // the dialog's state is the one after the change (for a clear one DIP cycle later); a lost read after it (a master
    // polling again on RS485-1/2) keeps the identity read before
    let f = Found { state: Some(s), ..after.unwrap_or(before) };
    Ok(view(link.gen, address, can, &f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(format: u8, patch: [u8; 2], uart0: bool) -> MaintInfo {
        MaintInfo { format, base: [16, 6, 13], patch, patch_rev: u8::from(format == 4), build_id: [1; 8], hard_stop: (format > 2).then_some(false), power_rtc: true, pack_cutoff: true, uart0, reserved_flags: 0 }
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
        let off = Found { kind: "patched", info: Some(info(4, [1, 6], true)), state: st(u32::MAX), trusted: true };
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
