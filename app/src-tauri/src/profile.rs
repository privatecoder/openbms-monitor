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
use openbms_flash::maint::policy::{Master, PackFinding};
use openbms_flash::maint::SystemClock;
use openbms_proto::uart1;
use openbms_transport::timing::{self, Class};
use openbms_transport::{Bus, Connection};
use serde::Serialize;
use tauri::Emitter;

/// Offset of Group0 (function switch byte 0) in a parameter block (the 169-byte 0x47 INFO).
pub const GROUP0: usize = 151;
/// Offset of Group7: bit 5 is the automatic address/slave detection, which refuses a maintenance start.
pub const GROUP7: usize = 158;
use openbms_proto::access::{self, Action, Channel};
// the bit numbers live in the access matrix (`openbms_proto::access`)
const WRITE_BIT: u8 = 1 << access::G0_WRITE;
const START_BIT: u8 = 1 << access::G0_START;
/// MAINT 1.5 with image flag bit 1: 1 locks ein/aus, restart and setting the clock over the CAN socket; a binary 0xA1
/// must not change it (E3)
const POWER_BIT: u8 = 1 << access::G0_POWER_LOCK;
/// MAINT 1.6 with image flag bit 2: 0 switches the extended off on (an addressed off stores the pack cutoff), 1 keeps the
/// OEM soft off; a normal setting, also over the CAN socket while writing is released there
const CUTOFF_BIT: u8 = 1 << access::G0_EXTENDED_OFF;
/// MAINT 1.6: 0 shows the stored individual pack cutoff. Status only: set and cleared by an addressed ein/aus, never by a
/// write (a binary 0xA1 that changes it is refused with E3)
pub const LOCK_BIT: u8 = 1 << access::G0_LOCK_STATE;
const AUTO_ADDRESS_BIT: u8 = 1 << access::G7_AUTO_ADDRESS;

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

    /// The master's image flag bit 1: power and clock over the CAN socket, with the lock Group0 bit 3.
    pub fn power_rtc(&self) -> bool {
        self.caps().is_some() && matches!(&self.master, Master::Patched { patch, .. } if patch.info.power_rtc)
    }

    /// Group0 bit 3 at 1 allows power and clock in the master's build (`access::positive_build`).
    pub fn positive(&self) -> bool {
        matches!(&self.master, Master::Patched { patch, .. } if access::positive_build(access::Patch::Version(patch.info.patch), Some(patch.info.image_flags())))
    }
}

/// Read the profile on an open CAN socket connection. A link error leaves the master unknown.
pub fn detect(conn: &mut Connection) -> (Master, Option<u8>) {
    let answer = conn.latency().probe_budget(Class::Ascii, timing::PACK_IDENTITY);
    let m = dialog::read(conn, &SystemClock::new(), Budgets { answer, ..Budgets::default() }).unwrap_or(Master::Unknown);
    let gates = m.caps().is_some_and(|c| c.uart1_params && (c.write_gate_bit0 || c.start_gate_bit2));
    let group0 = if gates { conn.uart1_parameters(Bus::CanSocketBus, 0).ok().map(|p| p.function_switches[0]) } else { None };
    (m, group0)
}

pub fn of(endpoint: &str, bus: Bus, read: Option<(Master, Option<u8>)>) -> Profile {
    let (master, group0) = read.unwrap_or((Master::Unknown, None));
    Profile { endpoint: endpoint.into(), bus, master, group0, gates_stale: false, at: crate::history::now_ms() }
}

/// The release bits of a Group0 value, each only where the capability has it (`power`: power and clock over the CAN
/// socket allowed, only with the image flag: bit 3 at 0, or at 1 in the positive build).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gates {
    pub write: Option<bool>,
    pub start: Option<bool>,
    pub power: Option<bool>,
}

pub fn gates_of(caps: Caps, power_rtc: bool, positive: bool, group0: u8) -> Gates {
    Gates {
        write: caps.write_gate_bit0.then_some(group0 & WRITE_BIT != 0),
        start: caps.start_gate_bit2.then_some(group0 & START_BIT != 0),
        power: power_rtc.then_some((group0 & POWER_BIT == 0) != positive),
    }
}

/// The capabilities as the UI uses them.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsView {
    pub status_split: bool,
    pub uart1_params: bool,
    pub uart1_identity: bool,
    pub write_gate: bool,
    pub start_gate: bool,
    /// image flag bit 1 of the pack's own 64/03 (power and clock over the CAN socket, lock Group0 bit 3); None: not known
    /// (a stored proof carries no image flags)
    pub power_rtc: Option<bool>,
    /// image flag bit 2 with patch 1.6 or later (`known::pack_cutoff_of`): the stored pack cutoff, Group0 bit 4 extended
    /// off and bit 5 the lock state; None: not known (a stored proof), false below 1.6 even with the bit set
    pub pack_cutoff: Option<bool>,
    /// the patch version (major, minor) the identity reports; a stored proof: the one its release names, at least the
    /// lowest supported
    pub version: [u8; 2],
    /// the image flag byte of the pack's own 64/03 (the facts of `access`); None: not known (a stored proof)
    pub flags: Option<u8>,
}

pub fn caps_view(c: Caps, power_rtc: Option<bool>, pack_cutoff: Option<bool>, version: [u8; 2], flags: Option<u8>) -> CapsView {
    CapsView { status_split: c.status_split, uart1_params: c.uart1_params, uart1_identity: c.uart1_identity, write_gate: c.write_gate_bit0, start_gate: c.start_gate_bit2, power_rtc, pack_cutoff, version, flags }
}

/// The capabilities of a pack whose own 64/03 (or, for the master, the profile's) this connection read.
pub fn caps_of_info(c: Caps, info: &openbms_proto::MaintInfo) -> CapsView {
    caps_view(c, Some(info.power_rtc), Some(known::pack_cutoff_of(info.patch, info.pack_cutoff)), info.patch, Some(info.image_flags()))
}

/// The patch version (major, minor) a stored proof's release names ("16.06.13 MAINT 1.6.3" → 1.6); None where it names
/// no supported build (1.5.1, 1.6.2 …; a release without a version, e.g. a user's proof, counts as the lowest supported).
pub fn release_patch(release: &str) -> Option<[u8; 2]> {
    let Some((_, v)) = release.split_once("MAINT ") else { return Some(known::MIN_PATCH) };
    let mut parts = v.split('.').map(|p| p.trim_start_matches(|c: char| !c.is_ascii_digit()).chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u8>().ok());
    let (major, minor) = (parts.next().flatten()?, parts.next().flatten()?);
    let v = [major, minor, parts.next().flatten().unwrap_or(0)];
    // TEMP(maint-1.6): „MAINT 1.6“, the first 1.6 (V1_6_CUTOFF)
    let temp = v == [1, 6, 0];
    (temp || v >= openbms_proto::maint::SUPPORTED_16 || (openbms_proto::maint::SUPPORTED_15 <= v && v < [1, 6, 0])).then_some([major, minor])
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
        gates: p.caps().zip(p.group0).map(|(c, g)| gates_of(c, p.power_rtc(), p.positive(), g)),
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
        W::Written { read_back: Some(p), .. } | W::UnclearRead { read_back: Some(p), .. } | W::Stale { fresh: p, .. } => Some(p.function_switches[0]),
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

/// Every other reason for an open charge MOSFET that 0x44 shows, as (event index, mask): the „Laden verboten“ states of
/// the firmware (cell/pack OVP, charge over/under temperature, ambient and power stage protection, charge over-current,
/// charger over-voltage, cell too low to charge, reverse polarity, output connection fault) and a charge MOSFET fault.
/// The latched severe fault (event 4 bit 7) is no charge block. Not shown by 0x44 and so not excludable: the standby
/// shutdown's „Laden verboten“ 60 s before off (event 8 bit 6 is not sent), the raw cell temperature charge protection
/// while discharging (16.06.11 on; the sent event 3 follows the current direction), a modified image's own lock.
pub const CHARGE_BLOCKS: [(usize, u8); 9] = [
    (0, 0x20),        // event 1 bit 5: charge MOSFET fault
    (1, 0x02 | 0x20), // event 2 bit 1 cell OVP, bit 5 pack OVP
    (2, 0x02 | 0x08), // event 3 bit 1 charge over-temperature, bit 3 charge under-temperature protection
    (3, 0x02 | 0x08), // event 4 bit 1 ambient over-temperature, bit 3 ambient under-temperature protection
    (3, 0x10),        // event 4 bit 4 power stage over-temperature protection
    (4, 0x02),        // event 5 bit 1 charge over-current protection
    (5, 0x01),        // event 6 bit 0 charger over-voltage
    (5, 0x10 | 0x20), // event 6 bit 4 cell too low to charge, bit 5 reverse polarity
    (5, 0x40),        // event 6 bit 6 output connection fault
];

/// One fresh 0x44 status shows the hard-stop behaviour: waiting for intermittent recharge (event 6 bit 1) with the charge
/// MOSFET off, no other visible charge-blocking cause (`CHARGE_BLOCKS`), the current limiter and the heater off and the
/// pack not switched off. Shown as „an (beobachtet am …)“; invisible charge blocks remain a residual uncertainty
/// (`CHARGE_BLOCKS`). Waiting with the MOSFET on proves nothing
/// against it (the firmware closes it again after 10 s of its own discharge over 10 A).
pub fn hard_stop_observation(status: &openbms_proto::Status) -> bool {
    let other = CHARGE_BLOCKS.iter().any(|&(i, mask)| status.events[i] & mask != 0);
    status.events[5] & 0x02 != 0 && !status.charge_mosfet_on() && !other && !status.current_limiter_on() && !status.heater_on() && status.system_status & 0x20 == 0
}

/// The image itself reports the hard stop for this pack on connection `gen` (a format-3 master at address 0, or a pack's
/// own format-3 64/03 with a supported patch): then no observation is kept or shown for it.
pub fn hard_stop_reported(profile: Option<&Profile>, live: Option<&PackFinding>, address: u8) -> bool {
    if address == 0 {
        return profile.is_some_and(|p| p.caps().is_some() && matches!(&p.master, Master::Patched { patch, .. } if patch.info.hard_stop.is_some()));
    }
    matches!(live, Some(PackFinding::Proven { info, .. }) if info.hard_stop.is_some())
}

/// The poll thread's fresh status of `address` on connection `link`: a hard-stop sighting is kept for the board current
/// there only when that board's identity is settled for this installation (`bank::board_storable`: on the CAN socket the
/// master matched, the pack not unconfirmed, none foreign), never where the image reports the flag; the UI is told.
pub fn observe_hard_stop(state: &AppState, link: &Link, address: u8, status: &openbms_proto::Status) {
    if !hard_stop_observation(status) {
        return;
    }
    if !bank::board_storable(state, link.gen, address) || hard_stop_reported(bank::profile(state, link.gen).as_ref(), bank::live(state, link.gen, address).as_ref(), address) {
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
    Ok(all.into_iter().filter(|h| !hard_stop_reported(profile.as_ref(), bank::live(&state, link.gen, h.address).as_ref(), h.address)).collect())
}

/// What proves the patch version of one pack: the master's identity on this link ("master"), the pack's own 64/03 on
/// this link ("live", not retractable), or a stored proof for the board with this String 2 ("flash", "user").
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PackPatch {
    pub source: String,
    /// "master" (address 0: Group0 bit 2 is its start release) or "pack" (bit 2 stored, acts only at the master)
    pub role: &'static str,
    /// the listed name, or the version the master reports, or the proof's release
    pub release: String,
    pub caps: CapsView,
    pub at: Option<i64>,
}

/// The pure decision of `pack_patch`: the master's profile for address 0 on the CAN socket; for another pack its own live
/// 64/03 on this link (proven: "live"; negative: no patch functions, also over a stored proof); else a stored proof of
/// the same board (String 2 `serial`, canonical as read now).
pub fn pack_patch_of(address: u8, profile: Option<&Profile>, live: Option<&PackFinding>, proof: Option<&crate::maint_store::PatchProof>, serial: Option<&str>) -> Option<PackPatch> {
    let role = if address == 0 { "master" } else { "pack" };
    if address == 0 {
        if let (Some(p), Some(caps)) = (profile, profile.and_then(|p| p.caps())) {
            if let Master::Patched { patch, .. } = &p.master {
                let release = patch.listed.map(String::from).unwrap_or_else(|| patch.info.version());
                return Some(PackPatch { source: "master".into(), role, release, caps: caps_of_info(caps, &patch.info), at: Some(p.at) });
            }
        }
    } else {
        match live {
            Some(PackFinding::Proven { info, listed, .. }) => {
                let caps = known::caps_of(info)?;
                return Some(PackPatch { source: "live".into(), role, release: listed.map(String::from).unwrap_or_else(|| info.version()), caps: caps_of_info(caps, info), at: None });
            }
            // another MNT or a patch below the supported one: a stored proof no longer holds on this connection
            Some(PackFinding::Negative(_)) => return None,
            Some(PackFinding::Undecided) | None => {}
        }
    }
    let proof = proof.filter(|p| serial == Some(p.bms_serial.as_str()))?;
    // a proof means „a supported patch“: its capabilities; one naming an older build no longer holds
    let version = release_patch(&proof.release)?;
    Some(PackPatch { source: proof.source.clone(), role, release: proof.release.clone(), caps: caps_view(known::PROOF_CAPS, None, None, version, None), at: Some(proof.at) })
}

/// The pack's own identity on this link names a build the app does not support (an older patch, another MNT format):
/// the master by its profile, another pack by its live finding. Unknown (no answer yet) is not unsupported.
pub fn unsupported_of(address: u8, profile: Option<&Profile>, live: Option<&PackFinding>) -> bool {
    if address == 0 {
        return profile.is_some_and(|p| matches!(p.master, Master::UnknownPatch(_)));
    }
    matches!(live, Some(PackFinding::Negative(_)))
}

pub fn unsupported_here(state: &AppState, link: &Link, address: u8) -> bool {
    unsupported_of(address, bank::profile(state, link.gen).as_ref(), bank::live(state, link.gen, address).as_ref())
}

/// `pack_patch_of` with the connection's profile, the pack's live finding and the installation's stored proof.
pub fn pack_patch(state: &AppState, link: &Link, address: u8, serial: Option<&str>) -> Option<PackPatch> {
    let profile = bank::profile(state, link.gen);
    let live = bank::live(state, link.gen, address);
    let proof = link.installation.as_deref().and_then(|inst| crate::maint::with_store(state, |s| s.patch_proof(inst, address)).ok().flatten());
    pack_patch_of(address, profile.as_ref(), live.as_ref(), proof.as_ref(), serial)
}

/// Ein/aus, restart and the clock over the CAN socket for `address`: only where this connection proved image flag bit 1
/// for exactly this pack (the master's profile, a slave's own 64/03); a stored proof carries no image flags.
pub fn power_rtc(state: &AppState, link: &Link, address: u8) -> bool {
    // the clock read over the CAN socket needs exactly the image flag (`access::Action::ReadClock`)
    access::ready(Action::ReadClock, Channel::Uart1, &access_facts(pack_patch(state, link, address, None).as_ref(), None, None, address))
}

/// The firmware base a valid 64/03 identity reports (16.06.13 → [16, 6, 13]); None with revision FF (not known). 0x51
/// shows only 16.06, so only the identity gives the base.
pub fn base_of(info: &openbms_proto::MaintInfo) -> Option<[u8; 3]> {
    (info.base[2] != 0xFF).then_some(info.base)
}

/// The identity this connection read of `address`: the master's profile, a slave's own 64/03 (also one the app does
/// not support: its base still holds).
fn identity_of(state: &AppState, link: &Link, address: u8) -> Option<openbms_proto::MaintInfo> {
    if address == 0 {
        return match bank::profile(state, link.gen)?.master {
            Master::Patched { patch, .. } => Some(patch.info),
            Master::UnknownPatch(info) => info,
            _ => None,
        };
    }
    match bank::live(state, link.gen, address)? {
        PackFinding::Proven { info, .. } => Some(info),
        PackFinding::Negative(info) => info,
        PackFinding::Undecided => None,
    }
}

/// The access matrix's facts of one pack from what proves its patch (`pack_patch`) and its last Group0/Group7: a
/// stored proof gives the patch, never the image flags; no proof says nothing (unknown, not original).
pub fn access_facts(patch: Option<&PackPatch>, group0: Option<u8>, group7: Option<u8>, address: u8) -> access::Facts {
    let (patch, flags) = match patch {
        Some(p) => (access::Patch::Version(p.caps.version), p.caps.flags),
        None => (access::Patch::Unknown, None),
    };
    // the app conditions (installation, marker, base) are set by the caller that knows them
    access::Facts { patch, flags, group0, group7, address, installation: true, marker: None, base: None, slaves: None }
}

/// The lock state (Group0 bit 5) differs between the read an edit started from and the pack now: a write is not sent
/// over it, the user sees the pack's state first.
pub fn lock_changed(base: u8, fresh: u8) -> bool {
    (base ^ fresh) & LOCK_BIT != 0
}

/// Group0 bit 4 of the pack now differs from the one an off was confirmed with (`expect`: bit 4 was at 0; None: nothing
/// confirmed about it, never a difference). The raw bit, whatever it means in the build (`access::polar`).
pub fn extended_changed(expect: Option<bool>, group0: u8) -> bool {
    expect.is_some_and(|e| e != (group0 & CUTOFF_BIT == 0))
}

/// The answer time of a pack's own 64/03 on `c` (`timing::PACK_IDENTITY`, learning over a gateway up to twice that).
pub fn identity_budget(c: &Connection) -> std::time::Duration {
    c.latency().probe_budget(Class::Ascii, timing::PACK_IDENTITY)
}

/// A pack's live finding as the UI shows it in its pack view (never in the master profile).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PackProbeView {
    /// the connection the finding belongs to: the UI takes it only for its current one
    pub gen: u64,
    pub address: u8,
    /// "proven" | "negative" | "undecided"
    pub state: &'static str,
    pub version: Option<String>,
    pub listed: Option<String>,
    pub build_id: Option<String>,
    /// format 3: the image's hard stop; None with format 2 or no identity
    pub hard_stop: Option<bool>,
    /// image flag bit 1 of a proven pack: ein/aus, restart and the clock over the CAN socket
    pub power_rtc: Option<bool>,
    /// image flag bit 2 of a proven pack with patch 1.6 or later: the stored pack cutoff
    pub pack_cutoff: Option<bool>,
    /// the patch version of its 64/03 (also a negative finding's, e.g. 1.4) and, when proven, its image flag byte: the
    /// facts the catalog decides on (`access::Facts`)
    pub patch_version: Option<[u8; 2]>,
    pub flags: Option<u8>,
}

pub fn probe_view(gen: u64, address: u8, f: &PackFinding) -> PackProbeView {
    let (state, info, listed) = match f {
        PackFinding::Proven { info, listed, .. } => ("proven", Some(*info), listed.map(String::from)),
        PackFinding::Negative(info) => ("negative", *info, None),
        PackFinding::Undecided => ("undecided", None, None),
    };
    let proven = info.filter(|_| state == "proven");
    PackProbeView { gen, address, state, version: info.map(|i| i.version()), listed, build_id: info.map(|i| i.build_id_hex()), hard_stop: proven.and_then(|i| i.hard_stop), power_rtc: proven.map(|i| i.power_rtc),
        pack_cutoff: proven.map(|i| known::pack_cutoff_of(i.patch, i.pack_cutoff)), patch_version: info.map(|i| i.patch), flags: proven.map(|i| i.image_flags()) }
}

/// Ask the pack at `address` (not the master: its profile holds) for its own 64/03 on connection `link`, under the port
/// lock with the generation check; the finding is kept for this connection and the UI is told. A link failure is an error;
/// silence is a finding („undecided“), never a lost pack or original firmware.
pub fn probe_pack_on(state: &AppState, link: &Link, address: u8) -> Result<PackFinding, String> {
    let answer = with_link(&state.conn, &state.flashing, &state.link, link, |c| {
        dialog::probe_pack(c, &SystemClock::new(), address, identity_budget(c)).map_err(|e| openbms_transport::Error::Io(std::io::Error::other(e.to_string())))
    })?;
    let f = PackFinding::of(answer);
    // told only when kept for the current connection: a late answer after a reconnect reaches no other bank's view
    if bank::set_live(state, link.gen, address, f) {
        if let Some(app) = state.app.get() {
            let _ = app.emit("pack-probe", probe_view(link.gen, address, &f));
        }
    }
    Ok(f)
}

/// Whether a parameter read asks the firmware identity again: the master when its profile got no valid answer, a slave
/// not yet asked or silent before (a valid other MNT, RTN 04/05 stay as they are). CAN socket only.
pub fn ask_again(address: u8, can: bool, master_unknown: bool, live: Option<&PackFinding>) -> bool {
    can && if address == 0 { master_unknown } else { matches!(live, None | Some(PackFinding::Undecided)) }
}

/// `ask_again` on `link`, run by a parameter read under the pack's reservation: a silent 64/03 (a network gateway, the
/// bank's own traffic) does not leave the column unproven for the whole connection. Errors say nothing (still unproven).
pub fn reask_identity(state: &AppState, link: &Link, address: u8) {
    let Some(p) = bank::profile(state, link.gen) else { return };
    let live = bank::live(state, link.gen, address);
    if !ask_again(address, p.bus == Bus::CanSocketBus, matches!(p.master, Master::Unknown), live.as_ref()) {
        return;
    }
    if address != 0 {
        let _ = probe_pack_on(state, link, address);
        return;
    }
    let Ok(read) = with_link(&state.conn, &state.flashing, &state.link, link, |c| Ok(detect(c))) else { return };
    let fresh = of(&p.endpoint, p.bus, Some(read));
    if bank::profile(state, link.gen).is_some_and(|now| now.at == p.at) {
        bank::set_profile(state, link.gen, fresh.clone());
        if let Some(app) = state.app.get() {
            let _ = app.emit("firmware-profile", &view(link.gen, &fresh));
        }
    }
}

/// The live finding of a slave on `link`, asking it once per connection (CAN socket only; the master has its profile).
pub fn live_or_probe(state: &AppState, link: &Link, address: u8) -> Option<PackFinding> {
    if address == 0 || bank::profile(state, link.gen).is_none_or(|p| p.bus != Bus::CanSocketBus) {
        return None;
    }
    bank::live(state, link.gen, address).or_else(|| probe_pack_on(state, link, address).ok())
}

/// Whether the serial numbers of `address` are read over the binary service without being asked: the master by its
/// profile; another pack with a live proof, a stored proof for the place, or an A4 that already came on this link
/// (a negative live finding overrides the stored proof).
pub fn binary_serials(master_identity: bool, address: u8, live: Option<&PackFinding>, stored_proof: bool, a4_seen: bool) -> bool {
    if address == 0 {
        return master_identity;
    }
    match live {
        Some(PackFinding::Proven { caps, .. }) => caps.uart1_identity,
        Some(PackFinding::Negative(_)) => a4_seen,
        _ => stored_proof || a4_seen,
    }
}

/// „Erneut prüfen“ of one pack (and the first check where the serial read did not run it): its own 64/03 now.
#[tauri::command]
pub async fn pack_probe(state: tauri::State<'_, AppState>, address: u8, gen: u64) -> Result<PackProbeView, String> {
    let link = link_now(&state);
    if link.gen != gen {
        return Err(crate::LINK_CHANGED.into());
    }
    if address == 0 || bank::profile(&state, gen).is_none_or(|p| p.bus != Bus::CanSocketBus) {
        return Err("64/03 of a pack is asked on the CAN socket, for packs other than the master".into());
    }
    let _guard = crate::control::reserve_pack_wait(&state, address, "probe")?;
    probe_pack_on(&state, &link, address).map(|f| probe_view(gen, address, &f))
}

/// The access matrix for one pack, as a preview: every way over the three UARTs with its state on this connection.
/// `group0`/`group7` are the UI's last read (a hint only: a way that writes reads them fresh before it sends). The
/// proof is the connection's own (master profile, the pack's 64/03), else the stored one of the board recorded there.
#[derive(Serialize)]
pub struct AccessView {
    pub gen: u64,
    /// the UART of this connection: "uart0" (RS485-1/2) or "uart1" (CAN socket); None without a connection
    pub channel: Option<access::Channel>,
    pub decisions: Vec<access::Decision>,
}

#[tauri::command]
pub async fn access_view(state: tauri::State<'_, AppState>, address: u8, group0: Option<u8>, group7: Option<u8>, stored_proof: bool) -> Result<AccessView, String> {
    let link = link_now(&state);
    let channel = state.bus.lock().map_err(|e| e.to_string())?.map(|b| if b == Bus::CanSocketBus { access::Channel::Uart1 } else { access::Channel::Uart0 });
    // a stored proof only where this link's read of the pack matched it (`stored_proof`: the UI's column of this link
    // carries it, bound to the String 2 read then); else only the connection's own proof
    let serial = stored_proof
        .then(|| link.installation.as_deref().and_then(|inst| crate::maint::with_store(&state, |s| s.current_board(inst, address)).ok().flatten()).map(|b| b.bms_serial))
        .flatten();
    let patch = pack_patch(&state, &link, address, serial.as_deref());
    let profile = bank::profile(&state, link.gen);
    let master = profile.as_ref().filter(|_| address == 0).map(|p| &p.master);
    let facts = access::Facts {
        installation: link.installation.is_some(),
        marker: master.and_then(|m| m.marker()),
        slaves: master.and_then(|m| m.effective_slaves()),
        base: identity_of(&state, &link, address).as_ref().and_then(base_of),
        ..access_facts(patch.as_ref(), group0, group7, address)
    };
    Ok(AccessView { gen: link.gen, channel, decisions: access::view(&facts) })
}

/// The live findings of the current connection.
#[tauri::command]
pub async fn pack_probes(state: tauri::State<'_, AppState>) -> Result<Vec<PackProbeView>, String> {
    let link = link_now(&state);
    Ok(bank::lives(&state, link.gen).iter().map(|(a, f)| probe_view(link.gen, *a, f)).collect())
}

/// What a parameter write does with Group0 (`access::group0_plan`).
pub type Group0 = access::Group0Plan;

/// The patch as `access` needs it for a parameter write.
pub fn proof_of(p: &PackPatch) -> access::Proof {
    access::Proof { write_gate: p.caps.write_gate, start_gate: p.caps.start_gate, power: p.caps.power_rtc.into(), cutoff: p.caps.pack_cutoff.into() }
}

/// The Group0 plan of a parameter write the backend sends: over the CAN socket by the pack's proof on this connection;
/// over RS485-1/2 without one (the bits go as edited there; the UI keeps the bits without function by the proof).
pub fn group0_rule(uart1: bool, patch: Option<&PackPatch>) -> Group0 {
    if uart1 {
        access::group0_plan(Channel::Uart1, patch.map(proof_of))
    } else {
        access::group0_plan(Channel::Uart0, None)
    }
}

/// Apply `rule` to `block` (built on the fresh read `fresh`) before it is sent; Err: refused here, nothing sent.
pub fn apply_group0(rule: Group0, fresh: &[u8], block: &mut [u8]) -> Result<(), String> {
    access::apply_group0(rule, fresh[GROUP0], &mut block[GROUP0]).map_err(|r| {
        uart1_refusal(match r {
            access::Group0Refusal::WriteLocked => "write-locked",
            access::Group0Refusal::StartBit => "start-bit",
        })
    })
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
    let [a, b, c] = openbms_proto::maint::SUPPORTED_15;
    format!("MAINT ≥ {a}.{b}.{c}")
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
    use openbms_proto::SerialValue;
    use openbms_flash::maint::policy::Patch;
    use openbms_proto::{MaintInfo, MaintState};

    fn patched(patch: [u8; 2], build: [u8; 8]) -> Master {
        let info = MaintInfo { format: 4, base: [16, 6, 13], patch, build_id: build, hard_stop: None, power_rtc: false, pack_cutoff: false , reserved_flags: 0, patch_rev: if patch[1] == 5 { 2 } else { 3 }, uart0: false };
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
        let m = pack_patch_of(0, Some(&p), None, None, None).unwrap();
        assert_eq!((m.source.as_str(), m.release.as_str(), m.caps.write_gate, m.caps.start_gate), ("master", "16.06.13 MAINT 1.5.2", true, true));
        // a slave is not proven by its master
        assert_eq!(pack_patch_of(3, Some(&p), None, None, Some("SYN-3")), None);
        // RS485-1/2 knows no 0x64
        assert_eq!(pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::PackBus)), None, None, None), None);
        for m in [Master::No0x64, Master::Unknown, Master::UnknownPatch(None)] {
            assert_eq!(pack_patch_of(0, Some(&profile(m, Bus::CanSocketBus)), None, None, None), None);
        }
    }

    #[test]
    fn an_unlisted_master_with_a_supported_patch_is_configured_the_same() {
        let listed = pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None, None, None).unwrap();
        let later = pack_patch_of(0, Some(&profile(patched([1, 6], [9; 8]), Bus::CanSocketBus)), None, None, None).unwrap();
        // the same capabilities; only the reported version differs
        assert_eq!(CapsView { version: listed.caps.version, ..later.caps }, listed.caps);
        assert_eq!((later.caps.version, listed.caps.version), ([1, 6], [1, 5]));
        assert_eq!(later.release, "16.06.13 MAINT 1.6.3");
        let v = view(1, &profile(patched([1, 6], [9; 8]), Bus::CanSocketBus));
        assert_eq!((v.master.kind, v.master.listed.as_deref(), v.gates), ("patched", None, Some(Gates { write: Some(true), start: Some(true), power: None })));
    }

    #[test]
    fn formats_2_and_3_configure_the_master_alike() {
        let f2 = pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None, None, None).unwrap();
        let info3 = MaintInfo { format: 4, base: [16, 6, 13], patch: [1, 5], build_id: [7; 8], hard_stop: Some(true), power_rtc: false, pack_cutoff: false , reserved_flags: 0, patch_rev: 2, uart0: false };
        let m3 = Master::Patched { patch: Patch::recognise(info3).unwrap(), state: Some(MaintState { marker_word: u32::MAX, effective_slaves: 3 }) };
        let f3 = pack_patch_of(0, Some(&profile(m3, Bus::CanSocketBus)), None, None, None).unwrap();
        // the same capabilities; format 3 reports its image flags (here the hard stop), format 2 none
        assert_eq!(CapsView { flags: f2.caps.flags, ..f3.caps }, f2.caps);
        assert_eq!((f3.caps.flags, f2.caps.flags), (Some(1), Some(0)));
        let v = view(1, &profile(m3, Bus::CanSocketBus));
        assert_eq!((v.master.hard_stop, v.gates), (Some(true), Some(Gates { write: Some(true), start: Some(true), power: None })));
    }

    #[test]
    fn a_stored_proof_names_its_patch_version_and_holds_only_for_a_supported_build() {
        assert_eq!(release_patch("16.06.13 MAINT 1.6.3"), Some([1, 6]));
        assert_eq!(release_patch("16.06.13 MAINT 1.5.2"), Some([1, 5]));
        assert_eq!(release_patch("16.06.13 MAINT 1.12 (Test)"), Some([1, 12]));
        assert_eq!(release_patch(&confirmed_release()), Some([1, 5]));
        // TEMP(maint-1.6): the first 1.6 (V1_6_CUTOFF)
        assert_eq!(release_patch("16.06.13 MAINT 1.6"), Some([1, 6]));
        for old in ["16.06.13 MAINT 1.4", "16.06.13 MAINT 1.5", "16.06.13 MAINT 1.5.1", "16.06.13 MAINT 1.6.1", "16.06.13 MAINT 1.6.2"] {
            assert_eq!(release_patch(old), None, "{old}");
        }
        assert_eq!(release_patch("irgendwas"), Some(known::MIN_PATCH));
    }

    #[test]
    fn a_slave_proof_counts_only_for_the_same_string_2() {
        let proof = PatchProof { address: 3, bms_serial: "SYN-3".into(), release: "16.06.13 MAINT 1.5.2".into(), source: "flash".into(), at: 7 };
        let p = pack_patch_of(3, None, None, Some(&proof), Some("SYN-3")).unwrap();
        assert_eq!((p.source.as_str(), p.caps.write_gate), ("flash", true));
        assert_eq!(pack_patch_of(3, None, None, Some(&proof), Some("SYN-OTHER")), None);
        assert_eq!(pack_patch_of(3, None, None, Some(&proof), None), None);
    }

    #[test]
    fn without_proof_a_binary_write_keeps_the_release_bits() {
        let fresh = block(0xFA, 0);
        let mut b = block(0xFF, 0);
        apply_group0(group0_rule(true, None), &fresh, &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xFA);
        let mut b = block(0x00, 0);
        apply_group0(group0_rule(true, None), &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0x3D, "other bits as edited, the patch bits (0, 2, 3, 4, 5) as the pack has them");
        // RS485-1/2: as edited
        let mut b = block(0xFA, 0);
        apply_group0(group0_rule(false, None), &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xFA);
    }

    #[test]
    fn with_proof_a_locked_pack_or_a_changed_start_bit_is_refused_before_sending() {
        let p = pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None, None, None);
        let rule = group0_rule(true, p.as_ref());
        assert_eq!(rule, access::group0_plan(Channel::Uart1, Some(access::Proof { write_gate: true, start_gate: true, power: access::Tri::Unknown, cutoff: access::Tri::Unknown })));
        let locked = apply_group0(rule, &block(0xFE, 0), &mut block(0xFF, 0)).unwrap_err();
        assert!(locked.contains("write-locked"), "{locked}");
        let start = apply_group0(rule, &block(0xFF, 0), &mut block(0xFB, 0)).unwrap_err();
        assert!(start.contains("start-bit"), "{start}");
        // the self-lock (bit 0 from 1 to 0) goes out; bit 1 and the rest as edited (bits 4 and 5 as in the pack)
        let mut b = block(0xCC, 0);
        apply_group0(rule, &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xFC);
        // bit 3 (power and clock lock) stays as the pack has it, in both directions
        let mut b = block(0xF7, 0);
        apply_group0(rule, &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xFF);
        let mut b = block(0xFF, 0);
        apply_group0(rule, &block(0xF7, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xF7);
        // RS485-1/2 changes it
        let mut b = block(0xF7, 0);
        apply_group0(group0_rule(false, p.as_ref()), &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xF7);
    }

    /// `apply_group0` from the catalog against the rule as it stood before (`Free`/`Keep`/`Gated`), for every rule, fresh
    /// and edited Group0: the byte sent and the refusal.
    #[test]
    fn the_catalog_plan_applies_like_the_rule_before_it() {
        #[derive(Clone, Copy, Debug)]
        enum Old {
            Free,
            Keep,
            Gated { write: bool, start: bool, cutoff: bool },
        }
        fn old_apply(rule: Old, fresh: u8, block: &mut u8) -> Result<(), &'static str> {
            let keep = |b: &mut u8, m: u8| *b = (*b & !m) | (fresh & m);
            keep(block, LOCK_BIT);
            match rule {
                Old::Free => {}
                Old::Keep => keep(block, WRITE_BIT | START_BIT | POWER_BIT | CUTOFF_BIT),
                Old::Gated { write, start, cutoff } => {
                    keep(block, POWER_BIT);
                    if !cutoff {
                        keep(block, CUTOFF_BIT);
                    }
                    if write && fresh & WRITE_BIT == 0 {
                        return Err("write-locked");
                    }
                    if start && (*block ^ fresh) & START_BIT != 0 {
                        return Err("start-bit");
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
        let mut cases: Vec<(Old, access::Group0Plan)> = vec![(Old::Free, group0_rule(false, None)), (Old::Keep, group0_rule(true, None))];
        for write in [false, true] {
            for start in [false, true] {
                for cutoff in [access::Tri::Yes, access::Tri::No, access::Tri::Unknown] {
                    for power in [access::Tri::Yes, access::Tri::No, access::Tri::Unknown] {
                        let plan = access::group0_plan(Channel::Uart1, Some(access::Proof { write_gate: write, start_gate: start, power, cutoff }));
                        cases.push((Old::Gated { write, start, cutoff: cutoff == access::Tri::Yes }, plan));
                    }
                }
            }
        }
        for (rule, plan) in cases {
            for fresh in 0..=255u8 {
                for edited in 0..=255u8 {
                    let (mut a, mut b) = (edited, edited);
                    let want = old_apply(rule, fresh, &mut a);
                    let got = access::apply_group0(plan, fresh, &mut b).map_err(|r| match r {
                        access::Group0Refusal::WriteLocked => "write-locked",
                        access::Group0Refusal::StartBit => "start-bit",
                    });
                    assert_eq!(got, want, "{rule:?} fresh {fresh:02X} edited {edited:02X}");
                    if want.is_ok() {
                        assert_eq!(b, a, "{rule:?} fresh {fresh:02X} edited {edited:02X}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_lock_state_is_never_written_on_any_way() {
        let gated = |cutoff| access::group0_plan(Channel::Uart1, Some(access::Proof { write_gate: true, start_gate: true, power: access::Tri::Unknown, cutoff }));
        let rules = [group0_rule(false, None), group0_rule(true, None), gated(access::Tri::No), gated(access::Tri::Yes)];
        for rule in rules {
            // an old backup (bit 5 at 1) never clears a stored cutoff, and an edit never sets one
            let mut b = block(0xFF, 0);
            apply_group0(rule, &block(0xDF, 0), &mut b).unwrap();
            assert_eq!(b[GROUP0] & LOCK_BIT, 0, "{rule:?}");
            let mut b = block(0xDF, 0);
            apply_group0(rule, &block(0xFF, 0), &mut b).unwrap();
            assert_eq!(b[GROUP0] & LOCK_BIT, LOCK_BIT, "{rule:?}");
        }
        // RS485-1/2 changes bit 4 as edited
        let mut b = block(0xEF, 0);
        apply_group0(group0_rule(false, None), &block(0xFF, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xEF);
    }

    #[test]
    fn the_extended_off_goes_over_the_can_socket_only_with_the_proven_cutoff() {
        let finding = |patch: [u8; 2], flag: bool| PackFinding::of(openbms_flash::maint::dialog::PackAnswer::Info(MaintInfo { format: 4, base: [16, 6, 13], patch, build_id: [7; 8], hard_stop: Some(false), power_rtc: true, pack_cutoff: flag , reserved_flags: 0, patch_rev: if patch[1] == 5 { 2 } else { 3 }, uart0: false }));
        let with = pack_patch_of(3, None, Some(&finding([1, 6], true)), None, None);
        let rule = group0_rule(true, with.as_ref());
        assert_eq!(rule.keep & CUTOFF_BIT, 0, "bit 4 free with the proven cutoff");
        // on (bit 4 to 0) and back
        let mut b = block(0xE7, 0);
        apply_group0(rule, &block(0xF7, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xE7);
        let mut b = block(0xF7, 0);
        apply_group0(rule, &block(0xE7, 0), &mut b).unwrap();
        assert_eq!(b[GROUP0], 0xF7);
        // a 1.5 pack, a stored proof (flags not known) or no proof: bit 4 as the pack has it
        let proof = PatchProof { address: 3, bms_serial: "SYN-3".into(), release: "16.06.13 MAINT 1.6".into(), source: "flash".into(), at: 7 };
        for patch in [pack_patch_of(3, None, Some(&finding([1, 5], true)), None, None), pack_patch_of(3, None, None, Some(&proof), Some("SYN-3")), None] {
            let mut b = block(0xE7, 0);
            apply_group0(group0_rule(true, patch.as_ref()), &block(0xF7, 0), &mut b).unwrap();
            assert_eq!(b[GROUP0], 0xF7, "{patch:?}");
        }
    }

    #[test]
    fn the_power_and_clock_release_needs_the_image_flag() {
        let caps = known::PROOF_CAPS;
        assert_eq!(gates_of(caps, false, false, 0xFF).power, None);
        assert_eq!(gates_of(caps, true, false, 0xFF).power, Some(false), "bit 3 set: locked");
        assert_eq!(gates_of(caps, true, false, 0xF7).power, Some(true));
        // the positive build: bit 3 at 1 allows
        assert_eq!(gates_of(caps, true, true, 0xFF).power, Some(true));
        assert_eq!(gates_of(caps, true, true, 0xF7).power, Some(false));
        // the pack's own 64/03 says it; a stored proof does not know
        let flagged = PackFinding::of(openbms_flash::maint::dialog::PackAnswer::Info(MaintInfo { format: 4, base: [16, 6, 13], patch: [1, 5], build_id: [7; 8], hard_stop: Some(false), power_rtc: true, pack_cutoff: false , reserved_flags: 0, patch_rev: 2, uart0: false }));
        assert_eq!(pack_patch_of(3, None, Some(&flagged), None, None).unwrap().caps.power_rtc, Some(true));
        let proof = PatchProof { address: 3, bms_serial: "SYN-3".into(), release: "16.06.13 MAINT 1.5.2".into(), source: "flash".into(), at: 7 };
        assert_eq!(pack_patch_of(3, None, None, Some(&proof), Some("SYN-3")).unwrap().caps.power_rtc, None);
    }

    #[test]
    fn the_pack_cutoff_needs_patch_1_6_and_the_image_flag() {
        let info = |patch: [u8; 2], flag: bool| MaintInfo { format: 4, base: [16, 6, 13], patch, build_id: [0x18, 0xDA, 0xF6, 0x1D, 0x8A, 0x93, 0x67, 0x1D], hard_stop: Some(false), power_rtc: true, pack_cutoff: flag , reserved_flags: 0, patch_rev: if patch[1] == 5 { 2 } else { 3 }, uart0: false };
        let finding = |i: MaintInfo| PackFinding::of(openbms_flash::maint::dialog::PackAnswer::Info(i));
        // a slave's own 64/03: 1.6 with the flag; the same flag under 1.5 is not read that way; 1.6 without it neither
        assert_eq!(pack_patch_of(3, None, Some(&finding(info([1, 6], true))), None, None).unwrap().caps.pack_cutoff, Some(true));
        assert_eq!(pack_patch_of(3, None, Some(&finding(info([1, 5], true))), None, None).unwrap().caps.pack_cutoff, Some(false));
        assert_eq!(pack_patch_of(3, None, Some(&finding(info([1, 6], false))), None, None).unwrap().caps.pack_cutoff, Some(false));
        assert_eq!(probe_view(1, 3, &finding(info([1, 6], true))).pack_cutoff, Some(true));
        assert_eq!(probe_view(1, 3, &PackFinding::Undecided).pack_cutoff, None);
        // the master through its profile; the measured build is listed
        let m = Master::Patched { patch: Patch::recognise(info([1, 6], true)).unwrap(), state: Some(MaintState { marker_word: u32::MAX, effective_slaves: 3 }) };
        let p = pack_patch_of(0, Some(&profile(m, Bus::CanSocketBus)), None, None, None).unwrap();
        assert_eq!((p.release.as_str(), p.caps.power_rtc, p.caps.pack_cutoff), ("16.06.13 MAINT 1.6.3", Some(true), Some(true)));
        assert_eq!(view(1, &profile(m, Bus::CanSocketBus)).master.caps.unwrap().pack_cutoff, Some(true));
        // a stored proof knows no image flags
        let proof = PatchProof { address: 3, bms_serial: "SYN-3".into(), release: "16.06.13 MAINT 1.6".into(), source: "flash".into(), at: 7 };
        assert_eq!(pack_patch_of(3, None, None, Some(&proof), Some("SYN-3")).unwrap().caps.pack_cutoff, None);
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
        assert_eq!(group0_after(Some(&W::Stale { fresh: params(0xFB), lock_changed: false })), Some(0xFB));
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
        // another visible cause opens the charge MOSFET: no sighting
        let with = |i: usize, mask: u8| {
            let mut st = status(0x02, 0x01);
            st.events[i] |= mask;
            st
        };
        for (i, mask) in [(1, 0x02), (1, 0x20), (2, 0x02), (2, 0x08), (3, 0x02), (3, 0x10), (4, 0x02), (5, 0x01), (5, 0x10), (5, 0x40), (0, 0x20)] {
            assert!(!hard_stop_observation(&with(i, mask)), "event {} mask {mask:02X}", i + 1);
        }
        // warnings that do not block charging still allow it
        assert!(hard_stop_observation(&with(1, 0x01)), "cell voltage high warning");
        assert!(hard_stop_observation(&with(2, 0x01)), "charge temperature high warning");
        // the latched severe fault is no charge block: it does not suppress a later hint
        assert!(hard_stop_observation(&with(3, 0x80)), "severe fault (latched until restart)");
        // current limiter or heater on, or the pack off: no sighting
        assert!(!hard_stop_observation(&status(0x02, 0x05)));
        assert!(!hard_stop_observation(&status(0x02, 0x09)));
        let mut off = status(0x02, 0x01);
        off.system_status = 0x20;
        assert!(!hard_stop_observation(&off));
    }

    #[test]
    fn a_format_3_master_keeps_no_observation_its_flag_shows() {
        let info3 = MaintInfo { format: 4, base: [16, 6, 13], patch: [1, 5], build_id: [7; 8], hard_stop: Some(false), power_rtc: false, pack_cutoff: false , reserved_flags: 0, patch_rev: 2, uart0: false };
        let m3 = Master::Patched { patch: Patch::recognise(info3).unwrap(), state: None };
        let p3 = profile(m3, Bus::CanSocketBus);
        assert!(hard_stop_reported(Some(&p3), None, 0));
        assert!(!hard_stop_reported(Some(&p3), None, 1), "a slave keeps the observation path");
        assert!(!hard_stop_reported(Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None, 0), "format 2: flag unknown");
        assert!(!hard_stop_reported(None, None, 0));
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
        bank::can_master_read(&state, link.gen, true);
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
    fn on_the_can_socket_a_sighting_is_kept_only_for_a_confirmed_bank() {
        let state = AppState::default();
        let dir = std::env::temp_dir().join(format!("openbms-hardstop-bank-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::maint_store::Store::open(&dir.join("maintenance.sqlite")).unwrap();
        let inst = store.create_installation("A", None, vec![]).unwrap();
        *state.maint.lock().unwrap() = Some(store);
        let seen = |state: &AppState| crate::maint::with_store(state, |s| s.hard_stops(&inst.id)).unwrap().len();
        let fresh = |state: &AppState| {
            crate::new_link(state, Some(inst.id.clone()));
            let link = crate::link_now(state);
            bank::begin(state, link.gen, false, None);
            link
        };
        // master serial not read yet (polling starts before), or another bank's: nothing, also for a known pack
        let link = fresh(&state);
        bank::uart1_known(&state, &link, 3);
        observe_hard_stop(&state, &link, 3, &status(0x02, 0x01));
        bank::can_master_read(&state, link.gen, false);
        observe_hard_stop(&state, &link, 3, &status(0x02, 0x01));
        assert_eq!(seen(&state), 0);
        // the master matches: original firmware shows no per-pack serial, the address is enough
        let link = fresh(&state);
        bank::can_master_read(&state, link.gen, true);
        observe_hard_stop(&state, &link, 3, &status(0x02, 0x01));
        assert_eq!(seen(&state), 1);
        // an unconfirmed pack at the address, or a foreign one on the connection: nothing
        bank::uart1_unconfirmed(&state, &link, 5, "SYN-B5", &SerialValue::NoAnswer, bank::Why::New, None);
        observe_hard_stop(&state, &link, 5, &status(0x02, 0x01));
        assert_eq!(seen(&state), 1);
        bank::uart1_unconfirmed(&state, &link, 4, "SYN-B4", &SerialValue::NoAnswer, bank::Why::Foreign, None);
        observe_hard_stop(&state, &link, 6, &status(0x02, 0x01));
        assert_eq!(seen(&state), 1);
    }

    #[test]
    fn the_firmware_base_comes_from_the_identity_with_a_known_revision() {
        let info = |base| MaintInfo { format: 4, base, patch: [1, 6], build_id: [0; 8], hard_stop: None, power_rtc: true, pack_cutoff: true , reserved_flags: 0, patch_rev: 3, uart0: false };
        assert_eq!(base_of(&info([16, 6, 13])), Some([16, 6, 13]));
        assert_eq!(base_of(&info([16, 6, 0xFF])), None);
    }

    #[test]
    fn a_parameter_read_asks_again_only_after_silence_and_only_on_the_can_socket() {
        let proven = live([1, 6], Some(false));
        assert!(ask_again(4, true, false, None));
        assert!(ask_again(4, true, false, Some(&PackFinding::Undecided)));
        assert!(!ask_again(4, true, true, Some(&proven)));
        assert!(!ask_again(4, true, false, Some(&PackFinding::Negative(None))));
        assert!(!ask_again(4, false, true, None));
        assert!(ask_again(0, true, true, None));
        assert!(!ask_again(0, true, false, Some(&PackFinding::Undecided)));
    }

    /// A format-4 identity (1.5 → 1.5.2, 1.6 → 1.6.3); `hard_stop` None: not reported
    fn live(patch: [u8; 2], hard_stop: Option<bool>) -> PackFinding {
        use openbms_flash::maint::dialog::PackAnswer;
        let patch_rev = if patch == [1, 6] { 3 } else { 2 };
        PackFinding::of(PackAnswer::Info(MaintInfo { format: 4, base: [16, 6, 13], patch, build_id: [0x29, 0x7F, 0x9F, 0x2A, 0xA8, 0x40, 0x2E, 0x9F], hard_stop, power_rtc: false, pack_cutoff: false , reserved_flags: 0, patch_rev, uart0: false }))
    }

    #[test]
    fn a_slaves_own_03_proves_it_live_and_a_negative_one_overrides_a_stored_proof() {
        let proof = PatchProof { address: 3, bms_serial: "SYN-3".into(), release: "MAINT ≥ 1.5.2".into(), source: "user".into(), at: 7 };
        // proven live: source live, role pack, the version it reports
        let p = pack_patch_of(3, None, Some(&live([1, 5], Some(false))), None, None).unwrap();
        assert_eq!((p.source.as_str(), p.role, p.caps.write_gate), ("live", "pack", true));
        // a live proof also without a stored one, and before it
        assert_eq!(pack_patch_of(3, None, Some(&live([1, 5], None)), Some(&proof), Some("SYN-3")).unwrap().source, "live");
        // negative (patch 1.4, another MNT): no patch functions on this connection, over the stored proof
        assert_eq!(pack_patch_of(3, None, Some(&live([1, 4], None)), Some(&proof), Some("SYN-3")), None);
        assert_eq!(pack_patch_of(3, None, Some(&PackFinding::Negative(None)), Some(&proof), Some("SYN-3")), None);
        // undecided (silence, an error): the stored proof holds
        assert_eq!(pack_patch_of(3, None, Some(&PackFinding::Undecided), Some(&proof), Some("SYN-3")).unwrap().source, "user");
        // the master never takes a slave's answer: address 0 keeps its profile path
        assert_eq!(pack_patch_of(0, None, Some(&live([1, 5], Some(true))), None, None), None);
        assert_eq!(pack_patch_of(0, Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None, None, None).unwrap().role, "master");
    }

    #[test]
    fn an_older_patch_on_this_link_is_unsupported_unknown_is_not() {
        let old = PackFinding::Negative(Some(MaintInfo { patch_rev: 1, ..match live([1, 5], None) { PackFinding::Proven { info, .. } => info, _ => unreachable!() } }));
        assert!(unsupported_of(3, None, Some(&old)));
        assert!(unsupported_of(3, None, Some(&PackFinding::Negative(None))));
        assert!(!unsupported_of(3, None, Some(&live([1, 5], None))));
        assert!(!unsupported_of(3, None, Some(&PackFinding::Undecided)));
        assert!(!unsupported_of(3, None, None));
        assert!(unsupported_of(0, Some(&profile(Master::UnknownPatch(None), Bus::CanSocketBus)), None));
        assert!(!unsupported_of(0, Some(&profile(patched([1, 5], LISTED), Bus::CanSocketBus)), None));
    }

    #[test]
    fn serial_numbers_go_binary_per_target_pack() {
        let proven = live([1, 5], None);
        assert!(binary_serials(true, 0, None, false, false));
        assert!(!binary_serials(false, 0, Some(&proven), true, true), "the master by its profile only");
        assert!(binary_serials(false, 4, Some(&proven), false, false), "a patched slave behind an original master");
        assert!(binary_serials(false, 4, Some(&PackFinding::Undecided), true, false), "stored proof");
        assert!(binary_serials(false, 4, None, false, true), "an A4 that came");
        assert!(!binary_serials(false, 4, Some(&PackFinding::Negative(None)), true, false), "negative overrides the stored proof");
        assert!(!binary_serials(true, 4, None, false, false), "not from the master's profile");
    }

    #[test]
    fn a_slaves_fresh_flag_wins_over_observations() {
        assert!(hard_stop_reported(None, Some(&live([1, 5], Some(false))), 4));
        assert!(!hard_stop_reported(None, Some(&live([1, 5], None)), 4), "MNT02: unknown, observations go on");
        assert!(!hard_stop_reported(None, Some(&live([1, 4], Some(true))), 4), "not a supported patch");
        assert!(!hard_stop_reported(None, Some(&PackFinding::Undecided), 4));
        let v = probe_view(1, 4, &live([1, 5], Some(false)));
        // an unlisted build ID: proven, without a name
        assert_eq!((v.state, v.hard_stop, v.build_id.as_deref(), v.listed.as_deref()), ("proven", Some(false), Some("297f9f2aa8402e9f"), None));
        assert_eq!(probe_view(1, 4, &PackFinding::Negative(None)).state, "negative");
        assert_eq!(probe_view(1, 4, &PackFinding::Undecided).hard_stop, None);
        // no sighting is kept for a slave with a fresh MNT03 flag
        let state = AppState::default();
        let dir = std::env::temp_dir().join(format!("openbms-slaveflag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::maint_store::Store::open(&dir.join("maintenance.sqlite")).unwrap();
        let inst = store.create_installation("A", None, vec![]).unwrap();
        *state.maint.lock().unwrap() = Some(store);
        crate::new_link(&state, Some(inst.id.clone()));
        let link = crate::link_now(&state);
        bank::begin(&state, link.gen, false, None);
        bank::can_master_read(&state, link.gen, true);
        bank::set_live(&state, link.gen, 4, live([1, 5], Some(false)));
        observe_hard_stop(&state, &link, 4, &status(0x02, 0x01));
        observe_hard_stop(&state, &link, 5, &status(0x02, 0x01));
        let seen = crate::maint::with_store(&state, |s| s.hard_stops(&inst.id)).unwrap();
        assert_eq!(seen.iter().map(|h| h.address).collect::<Vec<_>>(), vec![5]);
        // a finding of a connection that is no longer current is not kept (and so not told)
        assert!(!bank::set_live(&state, link.gen + 1, 6, live([1, 5], Some(true))));
        assert_eq!(bank::live(&state, link.gen, 6), None);
        // a flash of the pack: its live finding ends
        bank::clear_live(&state, link.gen, 4);
        assert_eq!(bank::live(&state, link.gen, 4), None);
    }

    #[test]
    fn a_refused_start_is_explained_from_the_fresh_block() {
        let caps = Some(known::PROOF_CAPS);
        assert_eq!(start_refusal(caps, Some(&block(0xFB, 0))), "start-not-allowed");
        assert_eq!(start_refusal(caps, Some(&block(0xFF, 0x20))), "auto-address");
        assert_eq!(start_refusal(caps, Some(&block(0xFF, 0))), "token-or-marker");
        assert_eq!(start_refusal(caps, None), "start-refused-unread");
        assert_eq!(confirmed_release(), "MAINT ≥ 1.5.2");
    }
}
