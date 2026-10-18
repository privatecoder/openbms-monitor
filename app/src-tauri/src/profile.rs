//! The firmware profile of the monitor connection and what a pack's patch version is proven by.
//!
//! On the CAN socket the profile is read on the connected endpoint itself, right after `connect` found an answer and
//! before the connection is published (and again on „Erneut prüfen“, through `with_link`): `0x64/03`, and only for a
//! supported patch the 5-byte `02` state and the master's Group0 (binary 0x47 at address 0). It belongs to that endpoint
//! and link generation (`bank.rs`) and is gone with them. The stored control adapter's maintenance read (`maint.rs`)
//! stays apart from it. RS485-1/2 knows no 0x64: the profile there says only „RS485-1/2“.
//!
//! Patch functions follow capabilities (`known::Caps`), never a version number. A supported master proves itself at
//! address 0 of this link; a slave only by a proof bound to its String 2 (`patch_proof` in the store): after the app
//! flashed a listed patched image there, or by the user's confirmation. Without proof the release bits of Group0 are not
//! named, and a binary write keeps them as the pack has them.

use crate::bms_memory::uart1_refusal;
use crate::maint::{master_view, MasterView};
use crate::{bank, link_now, with_link, AppState, Link};
use openbms_flash::known::{self, Caps};
use openbms_flash::maint::dialog::{self, Budgets};
use openbms_flash::maint::policy::Master;
use openbms_flash::maint::SystemClock;
use openbms_proto::uart1;
use openbms_transport::{Bus, Connection};
use serde::Serialize;
use tauri::Emitter;

/// Offset of Group0 (function switch byte 0) in a parameter block (the 169-byte 0x47 INFO).
pub const GROUP0: usize = 151;
/// Offset of Group7: bit 5 is the automatic address/slave detection, which refuses a maintenance start.
pub const GROUP7: usize = 158;
const WRITE_BIT: u8 = 0x01;
const START_BIT: u8 = 0x04;
const AUTO_ADDRESS_BIT: u8 = 0x20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// endpoint key of the connection it was read on
    pub endpoint: String,
    pub bus: Bus,
    pub master: Master,
    /// the master's Group0 from a fresh binary 0x47 (None: no such capability, or not readable)
    pub group0: Option<u8>,
    /// a write to the master since then left Group0 open: the shown release bits may be old
    pub gates_stale: bool,
    pub at: i64,
}

impl Profile {
    /// What the master on this link offers (none on RS485-1/2).
    pub fn caps(&self) -> Option<Caps> {
        (self.bus == Bus::CanSocketBus).then(|| self.master.caps()).flatten()
    }
}

/// Read the profile on an open CAN socket connection. A link error leaves the master unknown.
pub fn detect(conn: &mut Connection) -> (Master, Option<u8>) {
    let m = dialog::read(conn, &SystemClock::new(), Budgets::default()).unwrap_or(Master::Unknown);
    let gates = m.caps().is_some_and(|c| c.uart1_params && (c.write_gate_bit0 || c.start_gate_bit2));
    let group0 = if gates { conn.uart1_parameters(Bus::CanSocketBus, 0).ok().map(|p| p.function_switches[0]) } else { None };
    (m, group0)
}

pub fn of(endpoint: &str, bus: Bus, read: Option<(Master, Option<u8>)>) -> Profile {
    let (master, group0) = read.unwrap_or((Master::Unknown, None));
    Profile { endpoint: endpoint.into(), bus, master, group0, gates_stale: false, at: crate::history::now_ms() }
}

/// The two release bits of a Group0 value, each only where the capability has it.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gates {
    pub write: Option<bool>,
    pub start: Option<bool>,
}

pub fn gates_of(caps: Caps, group0: u8) -> Gates {
    Gates { write: caps.write_gate_bit0.then_some(group0 & WRITE_BIT != 0), start: caps.start_gate_bit2.then_some(group0 & START_BIT != 0) }
}

/// The capabilities as the UI uses them.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsView {
    pub status_split: bool,
    pub uart1_params: bool,
    pub uart1_identity: bool,
    pub write_gate: bool,
    pub start_gate: bool,
}

pub fn caps_view(c: Caps) -> CapsView {
    CapsView { status_split: c.status_split, uart1_params: c.uart1_params, uart1_identity: c.uart1_identity, write_gate: c.write_gate_bit0, start_gate: c.start_gate_bit2 }
}

#[derive(Serialize, Clone, Debug)]
pub struct ProfileView {
    pub gen: u64,
    /// "can" | "pack"
    pub bus: &'static str,
    pub endpoint: String,
    pub master: MasterView,
    /// the master's release bits (fresh 0x47 when the profile was read, or the read-back of a later write)
    pub gates: Option<Gates>,
    /// a write to the master left them open: shown as possibly old, „Erneut prüfen“ reads them again
    pub gates_stale: bool,
    pub at: i64,
}

pub fn view(gen: u64, p: &Profile) -> ProfileView {
    ProfileView {
        gen,
        bus: if p.bus == Bus::CanSocketBus { "can" } else { "pack" },
        endpoint: p.endpoint.clone(),
        master: master_view(&p.master),
        gates: p.caps().zip(p.group0).map(|(c, g)| gates_of(c, g)),
        gates_stale: p.gates_stale,
        at: p.at,
    }
}

/// The profile of the current connection (None: not connected).
#[tauri::command]
pub async fn firmware_profile(state: tauri::State<'_, AppState>) -> Result<Option<ProfileView>, String> {
    let link = link_now(&state);
    Ok(bank::profile(&state, link.gen).map(|p| view(link.gen, &p)))
}

/// „Erneut prüfen“: read the profile again on the connection `gen` (CAN socket only), under its port lock.
#[tauri::command]
pub async fn firmware_profile_check(state: tauri::State<'_, AppState>, gen: u64) -> Result<ProfileView, String> {
    let link = link_now(&state);
    if link.gen != gen {
        return Err(crate::LINK_CHANGED.into());
    }
    let old = bank::profile(&state, gen).ok_or("not connected")?;
    if old.bus != Bus::CanSocketBus {
        return Ok(view(gen, &old));
    }
    let _op = crate::control::reserve_bus_wait(&state).ok_or("the port is busy")?;
    let read = with_link(&state.conn, &state.flashing, &state.link, &link, |c| Ok(detect(c)))?;
    let p = of(&old.endpoint, old.bus, Some(read));
    bank::set_profile(&state, gen, p.clone());
    let v = view(gen, &p);
    if let Some(app) = state.app.get() {
        let _ = app.emit("firmware-profile", &v);
    }
    Ok(v)
}

/// Group0 of the master after a parameter write to it, as far as the result shows it (a read-back, the fresh read of a
/// stale edit, the stored block of a refusal); None: open (lost answer without read-back, E3, an error).
pub fn group0_after(r: Option<&crate::bms_memory::ParamWrite>) -> Option<u8> {
    use crate::bms_memory::ParamWrite as W;
    match r? {
        W::Written { read_back: Some(p), .. } | W::UnclearRead { read_back: Some(p), .. } | W::Stale { fresh: p } => Some(p.function_switches[0]),
        W::Refused { refusal, stored, .. } if *refusal != openbms_proto::Refusal::Locked => Some(stored.function_switches[0]),
        _ => None,
    }
}

/// After a parameter write to the master (address 0) on connection `link`: the profile's release bits follow the
/// result, or are marked as possibly old; the UI is told.
pub fn after_master_write(state: &AppState, link: &Link, address: u8, r: Option<&crate::bms_memory::ParamWrite>) {
    if address != 0 {
        return;
    }
    let Some(mut p) = bank::profile(state, link.gen) else { return };
    if p.caps().is_none() {
        return;
    }
    match group0_after(r) {
        Some(g) => (p.group0, p.gates_stale) = (Some(g), false),
        None => p.gates_stale = true,
    }
    bank::set_profile(state, link.gen, p.clone());
    if let Some(app) = state.app.get() {
        let _ = app.emit("firmware-profile", &view(link.gen, &p));
    }
}

/// One fresh 0x44 status shows a hard stop: waiting for intermittent recharge (event 6 bit 1) with the charge MOSFET off.
/// Waiting with it on proves nothing against it (the firmware closes it again after 10 s of its own discharge over 10 A).
pub fn hard_stop_observation(status: &openbms_proto::Status) -> bool {
    status.events[5] & 0x02 != 0 && !status.charge_mosfet_on()
}

/// The image itself reports the hard stop for this pack on connection `gen` (a format-3 master at address 0): then no
/// observation is kept or shown for it.
pub fn hard_stop_reported(profile: Option<&Profile>, address: u8) -> bool {
    address == 0 && profile.is_some_and(|p| p.caps().is_some() && matches!(&p.master, Master::Patched { patch, .. } if patch.info.hard_stop.is_some()))
}

/// The poll thread's fresh status of `address` on connection `link`: a hard-stop observation is kept for the board current
/// there (not for an unconfirmed or foreign pack, not where the image reports the flag) and the UI is told of a change.
pub fn observe_hard_stop(state: &AppState, link: &Link, address: u8, status: &openbms_proto::Status) {
    if !hard_stop_observation(status) {
        return;
    }
    if bank::unconfirmed(state, link.gen, address) || hard_stop_reported(bank::profile(state, link.gen).as_ref(), address) {
        return;
    }
    let now = crate::history::now_ms();
    if crate::for_link(state, link, "hard stop", |s, inst| s.note_hard_stop(inst, address, now)) == Some(true) {
        if let Some(app) = state.app.get() {
            let _ = app.emit("hard-stop", crate::maint_store::HardStopSeen { address, at: now });
        }
    }
}

/// The hard-stop observations of the connection's installation for the boards now current (none with „Nur ansehen“);
/// the place whose image reports the flag is left out.
#[tauri::command]
pub async fn hard_stops(state: tauri::State<'_, AppState>) -> Result<Vec<crate::maint_store::HardStopSeen>, String> {
    let link = link_now(&state);
    let Some(inst) = link.installation.as_deref() else { return Ok(vec![]) };
    let profile = bank::profile(&state, link.gen);
    let all = crate::maint::with_store(&state, |s| s.hard_stops(inst)).map_err(|e| format!("{e:?}"))?;
    Ok(all.into_iter().filter(|h| !hard_stop_reported(profile.as_ref(), h.address)).collect())
}

/// What proves the patch version of one pack: the master's identity on this link ("master"), or a stored proof for the
/// board with this String 2 ("flash", "user").
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PackPatch {
    pub source: String,
    /// the listed name, or the version the master reports, or the proof's release
    pub release: String,
    pub caps: CapsView,
    pub at: Option<i64>,
}

/// The pure decision of `pack_patch`: the master's profile for address 0 on the CAN socket, else a proof of the same
/// board (String 2 `serial`, canonical as read now).
pub fn pack_patch_of(address: u8, profile: Option<&Profile>, proof: Option<&crate::maint_store::PatchProof>, serial: Option<&str>) -> Option<PackPatch> {
    if address == 0 {
        if let (Some(p), Some(caps)) = (profile, profile.and_then(|p| p.caps())) {
            if let Master::Patched { patch, .. } = &p.master {
                let release = patch.listed.map(String::from).unwrap_or_else(|| patch.info.version());
                return Some(PackPatch { source: "master".into(), release, caps: caps_view(caps), at: Some(p.at) });
            }
        }
    }
    let proof = proof.filter(|p| serial == Some(p.bms_serial.as_str()))?;
    // a proof means „at least the lowest supported patch“: its capabilities
    let caps = known::caps_of(2, known::MIN_PATCH)?;
    Some(PackPatch { source: proof.source.clone(), release: proof.release.clone(), caps: caps_view(caps), at: Some(proof.at) })
}

/// `pack_patch_of` with the connection's profile and the installation's stored proof.
pub fn pack_patch(state: &AppState, link: &Link, address: u8, serial: Option<&str>) -> Option<PackPatch> {
    let profile = bank::profile(state, link.gen);
    let proof = link.installation.as_deref().and_then(|inst| crate::maint::with_store(state, |s| s.patch_proof(inst, address)).ok().flatten());
    pack_patch_of(address, profile.as_ref(), proof.as_ref(), serial)
}

/// What a write may do with the release bits of Group0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group0 {
    /// RS485-1/2: the bits go as edited (that is the way to change them)
    Free,
    /// CAN socket without proof: both bits always as the pack has them now
    Keep,
    /// CAN socket with proof: no write while writing is locked there; the start release bit never changes
    Gated { write: bool, start: bool },
}

pub fn group0_rule(uart1: bool, patch: Option<&PackPatch>) -> Group0 {
    match (uart1, patch) {
        (false, _) => Group0::Free,
        (true, Some(p)) => Group0::Gated { write: p.caps.write_gate, start: p.caps.start_gate },
        (true, None) => Group0::Keep,
    }
}

/// Apply `rule` to `block` (built on the fresh read `fresh`) before it is sent; Err: refused here, nothing sent.
pub fn apply_group0(rule: Group0, fresh: &[u8], block: &mut [u8]) -> Result<(), String> {
    let keep = |block: &mut [u8], mask: u8| block[GROUP0] = (block[GROUP0] & !mask) | (fresh[GROUP0] & mask);
    match rule {
        Group0::Free => {}
        Group0::Keep => keep(block, WRITE_BIT | START_BIT),
        Group0::Gated { write, start } => {
            if write && fresh[GROUP0] & WRITE_BIT == 0 {
                return Err(uart1_refusal("write-locked"));
            }
            if start && (block[GROUP0] ^ fresh[GROUP0]) & START_BIT != 0 {
                return Err(uart1_refusal("start-bit"));
            }
            if !write {
                keep(block, WRITE_BIT);
            }
            if !start {
                keep(block, START_BIT);
            }
        }
    }
    Ok(())
}

/// Why a maintenance start was refused (RTN 05), from the master's fresh block: the start release off, the automatic
/// address detection on, else the token or the marker.
pub fn start_refusal(caps: Option<Caps>, block: Option<&[u8]>) -> &'static str {
    match block {
        Some(b) if caps.is_some_and(|c| c.start_gate_bit2) && b[GROUP0] & START_BIT == 0 => "start-not-allowed",
        Some(b) if b[GROUP7] & AUTO_ADDRESS_BIT != 0 => "auto-address",
        Some(_) => "token-or-marker",
        None => "start-refused-unread",
    }
}

/// The proof text of a confirmation: at least the lowest supported patch version.
pub fn confirmed_release() -> String {
    format!("MAINT ≥ {}.{}", known::MIN_PATCH[0], known::MIN_PATCH[1])
}

/// „Dieses Pack hat die Patch-Funktionen“: bound to String 2 read now at `address` on this connection, equal to `serial`
/// (the one the column was read with). On the CAN socket over the binary service, on RS485-1/2 with 0xA4.
#[tauri::command]
pub async fn patch_proof_confirm(state: tauri::State<'_, AppState>, installation: String, address: u8, serial: String, gen: u64) -> Result<PackPatch, String> {
    let link = crate::bms_memory::bound(&state, Some(&installation))?;
    if link.gen != gen {
        return Err(crate::LINK_CHANGED.into());
    }
    let bus = state.bus.lock().map_err(|e| e.to_string())?.ok_or("not connected")?;
    let _guard = crate::control::reserve_pack_wait(&state, address, "proof")?;
    let read = with_link(&state.conn, &state.flashing, &state.link, &link, |c| {
        Ok(match bus {
            Bus::CanSocketBus => c.uart1_serial(bus, address, uart1::CMD_STRING2).ok().and_then(|raw| openbms_proto::serial::canonical(&raw)),
            Bus::PackBus => match c.bms_serial_canonical(address) {
                Ok((openbms_proto::SerialValue::Set { value, truncated: false }, true)) => Some(value),
                _ => None,
            },
        })
    })?;
    if read.as_deref() != Some(serial.as_str()) {
        return Err(uart1_refusal("changed"));
    }
    crate::maint::with_store(&state, |s| s.set_patch_proof(&installation, address, &serial, &confirmed_release(), "user", crate::history::now_ms()))
        .map_err(|e| format!("{e:?}"))?;
    pack_patch(&state, &link, address, Some(&serial)).ok_or_else(|| "no proof".into())
}

/// „Beleg zurücknehmen“.
#[tauri::command]
pub async fn patch_proof_revoke(state: tauri::State<'_, AppState>, installation: String, address: u8) -> Result<(), String> {
    crate::maint::with_store(&state, |s| s.clear_patch_proof(&installation, address)).map_err(|e| format!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maint_store::PatchProof;
    use openbms_flash::maint::policy::Patch;
    use openbms_proto::{MaintInfo, MaintState};

    fn patched(patch: [u8; 2], build: [u8; 8]) -> Master {
        let info = MaintInfo { format: 2, base: [16, 6, 13], patch, build_id: build, hard_stop: None };
        Master::Patched { patch: Patch::recognise(info).unwrap(), state: Some(MaintState { marker_word: u32::MAX, effective_slaves: 3 }) }
    }
    const LISTED: [u8; 8] = [0x90, 0x9A, 0x60, 0x4E, 0x60, 0xE9, 0x22, 0x5B];

    fn profile(master: Master, bus: Bus) -> Profile {
        Profile { endpoint: "tcp:gw:4196".into(), bus, master, group0: Some(0xFF), gates_stale: false, at: 1 }
    }

    fn block(group0: u8, group7: u8) -> Vec<u8> {
        let mut b = vec![0u8; 169];
        b[GROUP0] = group0;
        b[GROUP7] = group7;
        b
    }

    #[test]
    fn the_master_proves_itself_only_at_address_0_on_the_can_socket() {
        let p = profile(patched([1, 5], LISTED), Bus::CanSocketBus);
        let m = pack_patch_of(0, Some(&p), None, None).unwrap();
        assert_eq!((m.source.as_str(), m.release.as_str(), m.caps.write_gate, m.caps.start_gate), ("master", "16.06.13 MAINT 1.5", true, true));
        // a slave is not proven by its master
        assert_eq!(pack_patch_of(3, Some(&p), None, Some("SYN-3")), None);
        // RS485-1/2 knows no 0x64
        assert_eq!(pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::PackBus)), None, None), None);
        for m in [Master::No0x64, Master::Unknown, Master::UnknownPatch(None)] {
            assert_eq!(pack_patch_of(0, Some(&profile(m, Bus::CanSocketBus)), None, None), None);
        }
    }

    #[test]
    fn an_unlisted_master_with_a_supported_patch_is_configured_the_same() {
        let listed = pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None, None).unwrap();
        let later = pack_patch_of(0, Some(&profile(patched([1, 6], [9; 8]), Bus::CanSocketBus)), None, None).unwrap();
        assert_eq!(later.caps, listed.caps);
        assert_eq!(later.release, "16.06.13 MAINT 1.6");
        let v = view(1, &profile(patched([1, 6], [9; 8]), Bus::CanSocketBus));
        assert_eq!((v.master.kind, v.master.listed.as_deref(), v.gates), ("patched", None, Some(Gates { write: Some(true), start: Some(true) })));
    }

    #[test]
    fn formats_2_and_3_configure_the_master_alike() {
        let f2 = pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None, None).unwrap();
        let info3 = MaintInfo { format: 3, base: [16, 6, 13], patch: [1, 5], build_id: [7; 8], hard_stop: Some(true) };
        let m3 = Master::Patched { patch: Patch::recognise(info3).unwrap(), state: Some(MaintState { marker_word: u32::MAX, effective_slaves: 3 }) };
        let f3 = pack_patch_of(0, Some(&profile(m3, Bus::CanSocketBus)), None, None).unwrap();
        assert_eq!(f3.caps, f2.caps);
        let v = view(1, &profile(m3, Bus::CanSocketBus));
        assert_eq!((v.master.hard_stop, v.gates), (Some(true), Some(Gates { write: Some(true), start: Some(true) })));
    }

    #[test]
    fn a_slave_proof_counts_only_for_the_same_string_2() {
        let proof = PatchProof { address: 3, bms_serial: "SYN-3".into(), release: "16.06.13 MAINT 1.5".into(), source: "flash".into(), at: 7 };
        let p = pack_patch_of(3, None, Some(&proof), Some("SYN-3")).unwrap();
        assert_eq!((p.source.as_str(), p.caps.write_gate), ("flash", true));
        assert_eq!(pack_patch_of(3, None, Some(&proof), Some("SYN-OTHER")), None);
        assert_eq!(pack_patch_of(3, None, Some(&proof), None), None);
    }

    #[test]
    fn without_proof_a_binary_write_keeps_both_release_bits() {
        let fresh = block(0xFA, 0);
        let mut b = block(0xFF, 0);
        apply_group0(group0_rule(true, None), &fresh, &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xFA);
        let mut b = block(0x00, 0);
        apply_group0(Group0::Keep, &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0x05, "other bits as edited, the release bits as the pack has them");
        // RS485-1/2: as edited
        let mut b = block(0xFA, 0);
        apply_group0(group0_rule(false, None), &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xFA);
    }

    #[test]
    fn with_proof_a_locked_pack_or_a_changed_start_bit_is_refused_before_sending() {
        let p = pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None, None);
        let rule = group0_rule(true, p.as_ref());
        assert_eq!(rule, Group0::Gated { write: true, start: true });
        let locked = apply_group0(rule, &block(0xFE, 0), &mut block(0xFF, 0)).unwrap_err();
        assert!(locked.contains("write-locked"), "{locked}");
        let start = apply_group0(rule, &block(0xFF, 0), &mut block(0xFB, 0)).unwrap_err();
        assert!(start.contains("start-bit"), "{start}");
        // the self-lock (bit 0 from 1 to 0) goes out; bit 1 and the rest as edited
        let mut b = block(0xFC, 0);
        apply_group0(rule, &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xFC);
    }

    #[test]
    fn the_masters_release_bits_follow_a_write_or_become_stale() {
        use crate::bms_memory::ParamWrite as W;
        let params = |g0: u8| {
            let mut b = vec![0u8, 0x3C];
            b.extend(std::iter::repeat_n(0u8, 120));
            b.push(0x1B);
            b.extend(std::iter::repeat_n(0u8, 27));
            b.push(0x08);
            b.push(g0);
            b.extend([0u8; 7]);
            b.extend(*b"SYN       ");
            openbms_proto::Parameters::parse(&b).unwrap()
        };
        let written = Some(W::Written { read_back: Some(params(0xFE)), read_error: None, unanswered: false, before: None });
        assert_eq!(group0_after(written.as_ref()), Some(0xFE));
        assert_eq!(group0_after(Some(&W::Written { read_back: None, read_error: Some("x".into()), unanswered: false, before: None })), None);
        assert_eq!(group0_after(Some(&W::Stale { fresh: params(0xFB) })), Some(0xFB));
        assert_eq!(group0_after(Some(&W::Refused { refusal: openbms_proto::Refusal::Refused, stored: params(0xFF), switches_live: false })), Some(0xFF));
        // E3: the pack changed since the read
        assert_eq!(group0_after(Some(&W::Refused { refusal: openbms_proto::Refusal::Locked, stored: params(0xFF), switches_live: false })), None);
        assert_eq!(group0_after(Some(&W::Unclear { error: "lost".into() })), None);
        assert_eq!(group0_after(None), None);
    }

    #[test]
    fn a_write_to_the_master_updates_or_stales_the_profile_of_its_connection() {
        let state = AppState::default();
        crate::new_link(&state, Some("inst".into()));
        let link = crate::link_now(&state);
        bank::begin(&state, link.gen, false, None);
        bank::set_profile(&state, link.gen, profile(patched([1, 5], LISTED), Bus::CanSocketBus));
        // a slave: nothing changes
        after_master_write(&state, &link, 3, None);
        assert!(!bank::profile(&state, link.gen).unwrap().gates_stale);
        // the master without a read-back: stale, „Erneut prüfen“ reads it again
        after_master_write(&state, &link, 0, None);
        let p = bank::profile(&state, link.gen).unwrap();
        assert!(p.gates_stale && view(link.gen, &p).gates_stale);
    }

    fn status(event6: u8, switch_state: u8) -> openbms_proto::Status {
        let mut events = [0u8; 8];
        events[5] = event6;
        openbms_proto::Status {
            address: 0, cell_limits: vec![], cell_temperature_limits: vec![], ambient_temperature_limit: openbms_proto::status::LimitState::Normal, power_temperature_limit: openbms_proto::status::LimitState::Normal,
            current_limit: openbms_proto::status::LimitState::Normal, pack_voltage_limit: openbms_proto::status::LimitState::Normal, events, switch_state, balancing: 0, system_status: 0, alarms: vec![],
        }
    }

    #[test]
    fn a_hard_stop_is_seen_only_while_waiting_for_recharge() {
        assert!(hard_stop_observation(&status(0x02, 0x01)), "waiting, charge MOSFET off");
        assert!(!hard_stop_observation(&status(0x02, 0x03)), "waiting, charge MOSFET on: proves nothing");
        assert!(!hard_stop_observation(&status(0x00, 0x01)), "not waiting");
        assert!(!hard_stop_observation(&status(0x7D, 0x00)), "other event 6 bits");
    }

    #[test]
    fn a_format_3_master_keeps_no_observation_its_flag_shows() {
        let info3 = MaintInfo { format: 3, base: [16, 6, 13], patch: [1, 5], build_id: [7; 8], hard_stop: Some(false) };
        let m3 = Master::Patched { patch: Patch::recognise(info3).unwrap(), state: None };
        let p3 = profile(m3, Bus::CanSocketBus);
        assert!(hard_stop_reported(Some(&p3), 0));
        assert!(!hard_stop_reported(Some(&p3), 1), "a slave keeps the observation path");
        assert!(!hard_stop_reported(Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), 0), "format 2: flag unknown");
        assert!(!hard_stop_reported(None, 0));
        // the poll thread's status with waiting and charge MOSFET off: nothing recorded for the format-3 master
        let state = AppState::default();
        let dir = std::env::temp_dir().join(format!("openbms-hardstop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::maint_store::Store::open(&dir.join("maintenance.sqlite")).unwrap();
        let inst = store.create_installation("A", None, vec![]).unwrap();
        *state.maint.lock().unwrap() = Some(store);
        crate::new_link(&state, Some(inst.id.clone()));
        let link = crate::link_now(&state);
        bank::begin(&state, link.gen, false, None);
        bank::set_profile(&state, link.gen, p3);
        observe_hard_stop(&state, &link, 0, &status(0x02, 0x01));
        observe_hard_stop(&state, &link, 2, &status(0x02, 0x01));
        let seen = crate::maint::with_store(&state, |s| s.hard_stops(&inst.id)).unwrap();
        assert_eq!(seen.iter().map(|h| h.address).collect::<Vec<_>>(), vec![2]);
        // the re-enable phase (still waiting, charge MOSFET on) never overwrites the sighting
        let at = seen[0].at;
        observe_hard_stop(&state, &link, 2, &status(0x02, 0x03));
        assert_eq!(crate::maint::with_store(&state, |s| s.hard_stops(&inst.id)).unwrap(), vec![crate::maint_store::HardStopSeen { address: 2, at }]);
    }

    #[test]
    fn a_refused_start_is_explained_from_the_fresh_block() {
        let caps = known::caps_of(2, [1, 5]);
        assert_eq!(start_refusal(caps, Some(&block(0xFB, 0))), "start-not-allowed");
        assert_eq!(start_refusal(caps, Some(&block(0xFF, 0x20))), "auto-address");
        assert_eq!(start_refusal(caps, Some(&block(0xFF, 0))), "token-or-marker");
        assert_eq!(start_refusal(caps, None), "start-refused-unread");
        assert_eq!(confirmed_release(), "MAINT ≥ 1.5");
    }
}
