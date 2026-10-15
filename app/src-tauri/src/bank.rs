//! Right bank before writing (design `docs/superpowers/specs/2026-10-15-bank-identity.md`, r2).
//!
//! A board is known by its BMS serial (String 2, 0xA4). `identity` tells for a pack at an address of the connection's
//! installation whether its board is known there, not confirmed yet (new, moved, another installation's), unchecked
//! (no serial to compare) or whether the check failed. Nothing is recorded for an unconfirmed board until the user
//! confirms it (`bank_confirm`); every write reads 0xA4 of its target first (`check_target`) and refuses an unconfirmed
//! board or a failed check.
//!
//! Per connection (link generation) `BankState` keeps the unconfirmed packs. While one of them belongs to another
//! installation (foreign), the connection stores nothing under its installation: `for_link` refuses, the recorder is
//! held, `maint::read_master` keeps no marker state.
//!
//! Data read before the identity is known: on RS485-1/2 the recorder starts held and the endpoint is not linked to the
//! installation yet. `BankState::verdict` decides: a foreign pack holds; a known pack records; only unconfirmed packs
//! (a bank never seen, a new installation) hold until „Übernehmen“; packs without a serial record; a failed read decides
//! nothing, but once the UI's reads of the connection ended (`bank_reads_done`, also with no pack) it records. The first
//! `Record` links the endpoint. Rows polled while held are dropped, not kept pending (simpler, and a few seconds of
//! history are worth less than a wrong installation's); the rows recorded before a later foreign find stay. A parameter
//! read checks 0xA4 first and keeps a snapshot only of a known or unchecked board. On the CAN socket the BMS serial is
//! not visible: recording and the link run from the start, a master battery serial of another installation is only a
//! hint (`can_hint`).

use crate::maint_store::{real, SerialPlace, Store};
use crate::{link_now, with_link, AppState, Link, FLASHING, LINK_CHANGED};
use openbms_proto::SerialValue;
use serde::Serialize;
use std::collections::BTreeMap;
use tauri::{Emitter, State};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Why {
    /// the serial is unknown to every installation
    New,
    /// a board of this installation at another address (DIP changed)
    Moved,
    /// a board or the master serial of another installation, archived ones included
    Foreign,
}

/// The other installation of a foreign board.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Other {
    pub id: String,
    pub name: String,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identity {
    Known,
    Unconfirmed { why: Why, other: Option<Other> },
    /// a valid answer without a serial to compare (empty, the placeholder, not offered): writes go ahead with a note
    Unchecked,
    /// no answer or an invalid one: writes are refused
    Failed,
}

/// The identity of a real serial at `address` of installation `inst`, from every place it is known at.
pub fn classify(inst: &str, address: u8, places: &[SerialPlace]) -> Identity {
    if places.iter().any(|p| p.installation == inst && p.address == address) {
        // also when it is known elsewhere too (a copy of the installation imported): copies must not block
        return Identity::Known;
    }
    if places.iter().any(|p| p.installation == inst) {
        return Identity::Unconfirmed { why: Why::Moved, other: None };
    }
    match places.first() {
        Some(p) => Identity::Unconfirmed { why: Why::Foreign, other: Some(Other { id: p.installation.clone(), name: p.name.clone(), archived: p.archived }) },
        None => Identity::Unconfirmed { why: Why::New, other: None },
    }
}

/// The identity of a pack read with BMS serial `read` at `address`, against installation `inst`.
pub fn identity(store: &Store, inst: &str, address: u8, read: &SerialValue) -> Result<Identity, String> {
    match read {
        SerialValue::NoAnswer => Ok(Identity::Failed),
        v => match real(v) {
            Some(s) => Ok(classify(inst, address, &store.serial_places(s)?)),
            None => Ok(Identity::Unchecked),
        },
    }
}

/// CAN socket bus: the master's battery serial (Modbus, maybe only its first 22 characters) names another installation
/// only when exactly one installation carries it, that one is not `inst`, and it carries it at address 0. A serial in
/// several installations, or one of `inst`'s own boards, gives no hint. Only a hint, nothing is held.
pub fn can_hint_of(inst: &str, places: &[SerialPlace]) -> Option<Other> {
    let first = places.first()?;
    if places.iter().any(|p| p.installation != first.installation) || first.installation == inst {
        return None;
    }
    places.iter().find(|p| p.address == 0).map(|p| Other { id: p.installation.clone(), name: p.name.clone(), archived: p.archived })
}

pub fn can_hint(store: &Store, inst: &str, battery: &SerialValue) -> Result<Option<Other>, String> {
    let SerialValue::Set { value, truncated } = battery else { return Ok(None) };
    Ok(can_hint_of(inst, &store.battery_places(value, *truncated)?))
}

/// What the UI gets about a pack's identity (serial read, write refusal, "bank-unconfirmed" event).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct IdentityView {
    /// "known" | "unconfirmed" | "unchecked" | "failed"
    pub kind: &'static str,
    pub address: u8,
    pub serial: Option<String>,
    pub why: Option<Why>,
    pub installation: Option<Other>,
    /// CAN socket bus: a hint only, nothing to confirm
    pub hint: bool,
}

impl IdentityView {
    pub fn of(id: &Identity, address: u8, read: &SerialValue) -> Self {
        let serial = match read {
            SerialValue::Set { value, .. } => Some(value.clone()),
            _ => None,
        };
        let (kind, why, installation) = match id {
            Identity::Known => ("known", None, None),
            Identity::Unconfirmed { why, other } => ("unconfirmed", Some(*why), other.clone()),
            Identity::Unchecked => ("unchecked", None, None),
            Identity::Failed => ("failed", None, None),
        };
        Self { kind, address, serial, why, installation, hint: false }
    }
}

/// An unconfirmed pack of the current connection, with what `board_seen` records once it is confirmed.
#[derive(Clone, Debug)]
struct Held {
    bms: SerialValue,
    battery: SerialValue,
    why: Why,
}

/// What the connection has learned about a pack.
#[derive(Clone, Debug)]
enum Seen {
    Known,
    Unchecked,
    Failed,
    Unconfirmed(Held),
}

impl Seen {
    fn of(id: &Identity, bms: &SerialValue, battery: &SerialValue) -> Self {
        match id {
            Identity::Known => Seen::Known,
            Identity::Unchecked => Seen::Unchecked,
            Identity::Failed => Seen::Failed,
            Identity::Unconfirmed { why, .. } => Seen::Unconfirmed(Held { bms: bms.clone(), battery: battery.clone(), why: *why }),
        }
    }
}

/// The endpoint of a connection on RS485-1/2, linked to its installation only once the bank is decided.
pub(crate) struct PendingLink {
    pub key: String,
    pub control: crate::maint_store::ControlEndpoint,
}

/// What the connection may do with the installation: record (and link its endpoint), or wait because a pack of another
/// installation is open (`Foreign`), because every pack decided so far is unconfirmed (`Confirm`), or because no read
/// has decided anything yet (`Pending`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Record,
    Foreign,
    Confirm,
    Pending,
}

/// What one connection (link generation) has learned about its packs. Only `note` and `begin` start a new generation;
/// queries about another one find nothing.
#[derive(Default)]
pub(crate) struct BankState {
    gen: u64,
    packs: BTreeMap<u8, Seen>,
    /// the UI's serial reads of this connection have ended (`bank_reads_done`)
    reads_done: bool,
    /// RS485-1/2: the recorder and the endpoint link follow the verdict; the CAN socket records from the start
    pack_bus: bool,
    link: Option<PendingLink>,
}

impl BankState {
    fn reset_to(&mut self, gen: u64) {
        if self.gen != gen {
            *self = BankState { gen, ..Default::default() };
        }
    }

    fn current(&self, gen: u64) -> Option<&Self> {
        (self.gen == gen).then_some(self)
    }

    fn foreign(&self) -> bool {
        self.packs.values().any(|s| matches!(s, Seen::Unconfirmed(h) if h.why == Why::Foreign))
    }

    fn verdict(&self) -> Verdict {
        if self.foreign() {
            return Verdict::Foreign;
        }
        let has = |f: fn(&Seen) -> bool| self.packs.values().any(f);
        if has(|s| matches!(s, Seen::Known)) {
            Verdict::Record
        } else if has(|s| matches!(s, Seen::Unconfirmed(_))) {
            // no known pack at all (an adapter moved to a bank never seen, a new installation): wait for „Übernehmen“
            Verdict::Confirm
        } else if has(|s| matches!(s, Seen::Unchecked)) || self.reads_done {
            // nothing to compare, or every read failed: as before this check existed
            Verdict::Record
        } else {
            Verdict::Pending
        }
    }

    /// What a pack showed. A failed read does not replace what an earlier read decided; a write's check (battery not
    /// read) keeps String 1 of the serial read.
    fn note(&mut self, address: u8, seen: Seen) {
        let seen = match (seen, self.packs.get(&address)) {
            (Seen::Failed, Some(old)) => old.clone(),
            (Seen::Unconfirmed(mut h), Some(Seen::Unconfirmed(old))) if h.battery == SerialValue::NoAnswer => {
                h.battery = old.battery.clone();
                Seen::Unconfirmed(h)
            }
            (s, _) => s,
        };
        self.packs.insert(address, seen);
    }
}

/// A new connection: on RS485-1/2 the recorder waits and `link` is linked once the bank is decided.
pub(crate) fn begin(state: &AppState, gen: u64, pack_bus: bool, link: Option<PendingLink>) {
    if let Ok(mut b) = state.bank.lock() {
        *b = BankState { gen, pack_bus, link, ..Default::default() };
    }
}

fn note(state: &AppState, gen: u64, address: u8, seen: Seen) {
    if let Ok(mut b) = state.bank.lock() {
        b.reset_to(gen);
        b.note(address, seen);
    }
}

/// A pack of another installation is unconfirmed on connection `gen`: nothing is stored for it.
pub(crate) fn foreign(state: &AppState, gen: u64) -> bool {
    state.bank.lock().map(|b| b.current(gen).is_some_and(|b| b.foreign())).unwrap_or(true)
}

/// The pack at `address` is unconfirmed on connection `gen`.
pub(crate) fn unconfirmed(state: &AppState, gen: u64, address: u8) -> bool {
    state.bank.lock().map(|b| b.current(gen).is_some_and(|b| matches!(b.packs.get(&address), Some(Seen::Unconfirmed(_))))).unwrap_or(true)
}

/// Apply the verdict of `link`'s connection: the recorder waits unless it may record; the first `Record` links the
/// endpoint to the installation (event "bank-linked" with its name). Never while another connection is current.
fn settle(state: &AppState, link: &Link) {
    let (v, pending) = {
        let Ok(mut b) = state.bank.lock() else { return };
        if b.gen != link.gen || !b.pack_bus {
            return;
        }
        let v = b.verdict();
        (v, if v == Verdict::Record { b.link.take() } else { None })
    };
    if link_now(state).gen != link.gen {
        return;
    }
    if let Ok(mut h) = state.history.lock() {
        h.hold(v != Verdict::Record);
    }
    if let (Some(p), Some(inst)) = (pending, link.installation.as_deref()) {
        match crate::maint::with_store(state, |s| {
            let linked = s.link_connection(inst, &p.key, p.control.clone(), false, crate::history::now_ms())?;
            Ok((linked, s.installation(inst)?.name))
        }) {
            Ok((true, name)) => {
                if let Some(app) = state.app.get() {
                    let _ = app.emit("bank-linked", name);
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("link: {e:?}"),
        }
    }
}

/// A serial read of the monitor (`serial_numbers`) classified; the recorder and the endpoint link follow.
pub(crate) fn after_read(state: &AppState, link: &Link, address: u8, id: &Identity, bms: &SerialValue, battery: &SerialValue) {
    note(state, link.gen, address, Seen::of(id, bms, battery));
    settle(state, link);
}

/// The UI's serial reads of connection `gen` have ended (also when it found no pack or every read failed): a bank
/// where nothing could be decided records as before.
#[tauri::command]
pub async fn bank_reads_done(state: State<'_, AppState>, gen: u64) -> Result<(), String> {
    reads_done(&state, gen);
    Ok(())
}

fn reads_done(state: &AppState, gen: u64) {
    let link = link_now(state);
    if link.gen != gen {
        return;
    }
    if let Ok(mut b) = state.bank.lock() {
        if b.gen != gen {
            return;
        }
        b.reads_done = true;
    }
    settle(state, &link);
}

/// The refusal of a write to a pack that is not confirmed, or whose check failed, as JSON for the UI (`bankRefusal`
/// in control.ts): `{"kind":"bank","state":"unconfirmed"|"failed","address":…,…}`.
pub fn refusal(v: &IdentityView, error: Option<&str>) -> String {
    let mut j = serde_json::json!({ "kind": "bank", "state": v.kind, "address": v.address });
    if v.kind == "unconfirmed" {
        j["why"] = serde_json::json!(v.why);
        j["serial"] = serde_json::json!(v.serial);
        j["installation"] = serde_json::json!(v.installation);
    }
    if let Some(e) = error {
        j["error"] = serde_json::json!(e);
    }
    j.to_string()
}

/// `code`: "no-answer" | "connection" | "store" | "no-installation" (the UI words it).
fn failed(address: u8, code: &str) -> String {
    refusal(&IdentityView::of(&Identity::Failed, address, &SerialValue::NoAnswer), Some(code))
}

/// What the check before a write found: `unchecked` (no serial to compare, the UI says so) and the String 2 read.
pub(crate) struct Checked {
    pub unchecked: bool,
    pub read: SerialValue,
}

/// Read 0xA4 of `address` on `link`'s connection and classify it; what it decides is noted for the connection (an
/// unconfirmed pack also reaches the sidebar). Errors: a changed connection or a flash as they are, else a bank
/// refusal of state "failed".
pub(crate) fn check(state: &AppState, link: &Link, address: u8) -> Result<(Identity, SerialValue), String> {
    let inst = link.installation.as_deref().ok_or_else(|| failed(address, "no-installation"))?;
    let read = match with_link(&state.conn, &state.flashing, &state.link, link, |c| c.bms_serial(address)) {
        Ok(v) => v,
        // the connection changed or a flash took it: that error, not a bank refusal
        Err(e) if e == LINK_CHANGED || e == FLASHING => return Err(e),
        Err(e) => {
            eprintln!("bank check {address}: {e}");
            return Err(failed(address, "connection"));
        }
    };
    let id = crate::maint::with_store(state, |s| identity(s, inst, address, &read)).map_err(|e| {
        eprintln!("bank check {address}: {e:?}");
        failed(address, "store")
    })?;
    if id != Identity::Failed {
        note(state, link.gen, address, Seen::of(&id, &read, &SerialValue::NoAnswer));
        settle(state, link);
    }
    if matches!(id, Identity::Unconfirmed { .. }) {
        if let Some(app) = state.app.get() {
            let _ = app.emit("bank-unconfirmed", &IdentityView::of(&id, address, &read));
        }
    }
    Ok((id, read))
}

/// A parameter read keeps its snapshot only for a pack whose board is known or has no serial to compare.
pub(crate) fn snapshot_allowed(checked: &Result<(Identity, SerialValue), String>) -> bool {
    matches!(checked, Ok((Identity::Known | Identity::Unchecked, _)))
}

/// The first step of every write, inside its pack reservation and on its connection: read 0xA4 of the target and
/// classify it. Known and unchecked go on; unconfirmed and a failed check refuse, nothing is written.
pub(crate) fn check_target(state: &AppState, link: &Link, address: u8) -> Result<Checked, String> {
    let (id, read) = check(state, link, address)?;
    match id {
        Identity::Known => Ok(Checked { unchecked: false, read }),
        Identity::Unchecked => Ok(Checked { unchecked: true, read }),
        Identity::Failed => Err(failed(address, "no-answer")),
        Identity::Unconfirmed { .. } => Err(refusal(&IdentityView::of(&id, address, &read), None)),
    }
}

/// A write's result and whether its target could not be checked (no serial), for the note in the UI.
#[derive(Serialize, Debug)]
pub struct Written<T> {
    pub value: T,
    pub unchecked: bool,
}

/// „Übernehmen“: the listed unconfirmed packs of the current connection belong to `installation`. Each is recorded as
/// seen there (`board_seen`: a new board, a moved one at its new place; another installation keeps its history), and
/// leaves the unconfirmed set. Returns the addresses recorded.
#[tauri::command]
pub async fn bank_confirm(state: State<'_, AppState>, installation: String, addresses: Vec<u8>) -> Result<Vec<u8>, String> {
    confirm(&state, &installation, &addresses)
}

fn confirm(state: &AppState, installation: &str, addresses: &[u8]) -> Result<Vec<u8>, String> {
    let link = link_now(state);
    if link.installation.as_deref() != Some(installation) {
        return Err("the connection belongs to another installation than the one chosen; connect again".into());
    }
    let held: Vec<(u8, Held)> = {
        let b = state.bank.lock().map_err(|e| e.to_string())?;
        let Some(b) = b.current(link.gen) else { return Ok(vec![]) };
        addresses.iter().filter_map(|a| match b.packs.get(a) { Some(Seen::Unconfirmed(h)) => Some((*a, h.clone())), _ => None }).collect()
    };
    let now = crate::history::now_ms();
    // one transaction per pack (`board_seen`): a failure leaves the packs not recorded yet unconfirmed
    let mut done = vec![];
    let r = crate::maint::with_store(state, |s| {
        for (a, h) in &held {
            s.board_seen(installation, *a, &h.bms, &h.battery, now)?;
            done.push(*a);
        }
        Ok(())
    });
    for a in &done {
        note(state, link.gen, *a, Seen::Known);
    }
    settle(state, &link);
    r.map_err(|e| format!("{e:?}"))?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("openbms-bank-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("maintenance.sqlite")
    }

    fn set(v: &str) -> SerialValue {
        SerialValue::Set { value: v.into(), truncated: false }
    }

    const NONE: SerialValue = SerialValue::NoAnswer;

    fn unconf(why: Why, other: Option<&str>) -> Identity {
        Identity::Unconfirmed { why, other: other.map(|n| Other { id: String::new(), name: n.into(), archived: false }) }
    }

    /// The identity with the other installation's id blanked, so tests compare names.
    fn id(s: &Store, inst: &str, address: u8, read: &SerialValue) -> Identity {
        match identity(s, inst, address, read).unwrap() {
            Identity::Unconfirmed { why, other } => Identity::Unconfirmed { why, other: other.map(|o| Other { id: String::new(), ..o }) },
            i => i,
        }
    }

    #[test]
    fn every_case_of_a_pack_against_the_installation() {
        let s = Store::open(&temp("cases")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap().id;
        let b = s.create_installation("B", None, vec![]).unwrap().id;
        s.board_seen(&a, 2, &set("SYN-A2"), &NONE, 1).unwrap();
        s.board_seen(&b, 4, &set("SYN-B4"), &NONE, 1).unwrap();
        // known at its place, also an earlier board there and a serial the app wrote over
        assert_eq!(id(&s, &a, 2, &set("SYN-A2")), Identity::Known);
        s.board_seen(&a, 2, &set("SYN-A2X"), &NONE, 2).unwrap();
        assert_eq!(id(&s, &a, 2, &set("SYN-A2")), Identity::Known, "an earlier board at the place");
        s.board_written(&a, 2, &set("SYN-A2X"), &set("SYN-A2Y"), 3).unwrap();
        assert_eq!(id(&s, &a, 2, &set("SYN-A2X")), Identity::Known, "an alias");
        // new: unknown to every installation
        assert_eq!(id(&s, &a, 3, &set("SYN-NEW")), unconf(Why::New, None));
        // moved: a board of A at another address
        assert_eq!(id(&s, &a, 5, &set("SYN-A2Y")), unconf(Why::Moved, None));
        // foreign: a board of B
        assert_eq!(id(&s, &a, 4, &set("SYN-B4")), unconf(Why::Foreign, Some("B")));
        // placeholder, empty, not offered, cut: unchecked; silence: failed
        for v in [SerialValue::Placeholder { value: "EMU110110S-XXXX".into() }, SerialValue::Empty, SerialValue::Unavailable, SerialValue::Set { value: "SYN-A2Y".into(), truncated: true }] {
            assert_eq!(id(&s, &a, 2, &v), Identity::Unchecked, "{v:?}");
        }
        assert_eq!(id(&s, &a, 2, &NONE), Identity::Failed);
    }

    #[test]
    fn the_master_serial_identifies_address_0() {
        let s = Store::open(&temp("master")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap().id;
        let b = s.create_installation("B", None, vec![]).unwrap().id;
        s.set_master_serial(&a, "SYN-MA", 1).unwrap();
        s.set_master_serial(&b, "SYN-MB", 1).unwrap();
        assert_eq!(id(&s, &a, 0, &set("SYN-MA")), Identity::Known);
        // another installation's master, without any board row there: foreign, never „new“
        assert_eq!(id(&s, &a, 0, &set("SYN-MB")), unconf(Why::Foreign, Some("B")));
        // A's master at another address: moved
        assert_eq!(id(&s, &a, 3, &set("SYN-MA")), unconf(Why::Moved, None));
    }

    #[test]
    fn archived_installations_count_and_copies_do_not_block() {
        let s = Store::open(&temp("archived")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap().id;
        let old = s.create_installation("Old", None, vec![]).unwrap().id;
        s.board_seen(&old, 1, &set("SYN-O1"), &NONE, 1).unwrap();
        s.set_archived(&old, true).unwrap();
        let o = identity(&s, &a, 1, &set("SYN-O1")).unwrap();
        assert_eq!(o, Identity::Unconfirmed { why: Why::Foreign, other: Some(Other { id: old.clone(), name: "Old".into(), archived: true }) });
        // a board known at the place in A and also elsewhere (an imported copy): known
        s.board_seen(&a, 1, &set("SYN-O1"), &NONE, 2).unwrap();
        assert_eq!(id(&s, &a, 1, &set("SYN-O1")), Identity::Known);
        // a board of A at another address and of another installation: moved (this installation first)
        assert_eq!(id(&s, &a, 6, &set("SYN-O1")), unconf(Why::Moved, None));
    }

    #[test]
    fn active_installations_are_named_before_archived_ones() {
        let s = Store::open(&temp("order")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap().id;
        let x = s.create_installation("X", None, vec![]).unwrap().id;
        let y = s.create_installation("Y", None, vec![]).unwrap().id;
        s.board_seen(&x, 1, &set("SYN-1"), &NONE, 1).unwrap();
        s.board_seen(&y, 1, &set("SYN-1"), &NONE, 1).unwrap();
        s.set_archived(&x, true).unwrap();
        assert_eq!(id(&s, &a, 1, &set("SYN-1")), unconf(Why::Foreign, Some("Y")));
    }

    #[test]
    fn the_can_hint_compares_the_masters_battery_serial() {
        let s = Store::open(&temp("can")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap().id;
        let b = s.create_installation("B", None, vec![]).unwrap().id;
        s.board_seen(&b, 0, &set("SYN-B0"), &set("BAT-B0-0123456789-ABCDEFGH"), 1).unwrap();
        let cut = SerialValue::Set { value: "BAT-B0-0123456789-ABCD".into(), truncated: true };
        assert_eq!(can_hint(&s, &a, &cut).unwrap().map(|o| o.name), Some("B".into()));
        assert_eq!(can_hint(&s, &b, &cut).unwrap(), None, "its own master");
        assert_eq!(can_hint(&s, &a, &SerialValue::Set { value: "BAT-OTHER".into(), truncated: true }).unwrap(), None);
        assert_eq!(can_hint(&s, &a, &SerialValue::Unavailable).unwrap(), None);
        // A has the same battery serial at its master too: no hint
        s.board_seen(&a, 0, &set("SYN-A0"), &set("BAT-B0-0123456789-ABCDEFGH"), 1).unwrap();
        assert_eq!(can_hint(&s, &a, &cut).unwrap(), None);
    }

    fn place(inst: &str, address: u8) -> SerialPlace {
        SerialPlace { installation: inst.into(), address, name: inst.to_uppercase(), archived: false }
    }

    #[test]
    fn the_can_hint_names_only_one_other_installation_with_it_at_its_master() {
        assert_eq!(can_hint_of("a", &[place("b", 0)]).map(|o| o.name), Some("B".into()));
        assert_eq!(can_hint_of("a", &[]), None);
        // in more than one installation (a copy, a reused text): no hint
        assert_eq!(can_hint_of("a", &[place("b", 0), place("c", 0)]), None);
        assert_eq!(can_hint_of("a", &[place("a", 5), place("b", 0)]), None, "one of A's own boards carries it");
        // the other installation has it only at a slave: not its master, no hint
        assert_eq!(can_hint_of("a", &[place("b", 3)]), None);
        assert_eq!(can_hint_of("a", &[place("a", 0)]), None);
    }

    fn held(why: Why, battery: SerialValue) -> Seen {
        Seen::Unconfirmed(Held { bms: set("SYN-X"), battery, why })
    }

    #[test]
    fn the_verdict_records_only_once_the_bank_is_decided() {
        let mut b = BankState { pack_bus: true, ..Default::default() };
        assert_eq!(b.verdict(), Verdict::Pending);
        // a silent master decides nothing, and a later failed read does not undo a decision
        b.note(0, Seen::Failed);
        assert_eq!(b.verdict(), Verdict::Pending);
        // only unconfirmed packs (a bank never seen, a new installation): wait for the confirmation, also after the reads
        b.note(1, held(Why::New, SerialValue::NoAnswer));
        b.reads_done = true;
        assert_eq!(b.verdict(), Verdict::Confirm);
        b.note(2, Seen::Known);
        assert_eq!(b.verdict(), Verdict::Record);
        b.note(2, Seen::Failed);
        assert_eq!(b.verdict(), Verdict::Record);
        b.note(3, held(Why::Foreign, SerialValue::NoAnswer));
        assert_eq!(b.verdict(), Verdict::Foreign);
        // packs without a serial: nothing to compare, recorded as before
        let mut c = BankState::default();
        c.note(4, Seen::Unchecked);
        assert_eq!(c.verdict(), Verdict::Record);
        // every read failed, or no pack found at all: recorded once the reads ended
        let d = BankState { reads_done: true, ..Default::default() };
        assert_eq!(d.verdict(), Verdict::Record);
    }

    #[test]
    fn a_writes_check_keeps_string_1_of_the_serial_read() {
        let mut b = BankState::default();
        b.note(0, held(Why::Foreign, set("BAT-0")));
        b.note(0, held(Why::Foreign, SerialValue::NoAnswer));
        assert!(matches!(b.packs.get(&0), Some(Seen::Unconfirmed(h)) if h.battery == set("BAT-0")));
        // a fresh read of String 1 replaces it
        b.note(0, held(Why::Foreign, SerialValue::Empty));
        assert!(matches!(b.packs.get(&0), Some(Seen::Unconfirmed(h)) if h.battery == SerialValue::Empty));
    }

    #[test]
    fn a_parameter_read_keeps_a_snapshot_only_of_a_known_or_unchecked_board() {
        assert!(snapshot_allowed(&Ok((Identity::Known, NONE))));
        assert!(snapshot_allowed(&Ok((Identity::Unchecked, SerialValue::Empty))));
        assert!(!snapshot_allowed(&Ok((Identity::Unconfirmed { why: Why::New, other: None }, set("SYN-N")))));
        assert!(!snapshot_allowed(&Ok((Identity::Failed, NONE))));
        assert!(!snapshot_allowed(&Err(failed(1, "connection"))));
    }

    #[test]
    fn queries_about_another_connection_change_nothing() {
        let state = AppState::default();
        crate::new_link(&state, Some("a".into()));
        let gen = link_now(&state).gen;
        note(&state, gen, 2, held(Why::Foreign, NONE));
        assert!(!foreign(&state, gen + 1) && !unconfirmed(&state, gen - 1, 2));
        assert!(foreign(&state, gen) && unconfirmed(&state, gen, 2), "still there");
    }

    #[test]
    fn the_recorder_and_the_endpoint_link_wait_for_the_bank() {
        let s = Store::open(&temp("settle")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap().id;
        let b = s.create_installation("B", None, vec![]).unwrap().id;
        s.board_seen(&a, 1, &set("SYN-A1"), &NONE, 1).unwrap();
        s.board_seen(&b, 0, &set("SYN-B0"), &NONE, 1).unwrap();
        let state = state_with(s, &a);
        let pending = || Some(PendingLink { key: "tcp:gw-a:4196".into(), control: crate::maint_store::ControlEndpoint::Tcp { addr: "gw-a:4196".into() } });
        let start = |state: &AppState| {
            crate::new_link(state, Some(a.clone()));
            let link = link_now(state);
            begin(state, link.gen, true, pending());
            state.history.lock().unwrap().connected(&a, "pack");
            state.history.lock().unwrap().hold(true);
            link
        };
        let endpoints = |state: &AppState| crate::maint::with_store(state, |s| s.installation(&a)).unwrap().endpoints;
        let held = |state: &AppState| state.history.lock().unwrap().is_held();
        // a silent master at 00 releases nothing; the foreign pack after it holds and never links
        let link = start(&state);
        after_read(&state, &link, 0, &Identity::Failed, &NONE, &NONE);
        assert!(held(&state));
        let id = crate::maint::with_store(&state, |s| identity(s, &a, 0, &set("SYN-B0"))).unwrap();
        after_read(&state, &link, 0, &id, &set("SYN-B0"), &NONE);
        reads_done(&state, link.gen);
        assert!(held(&state) && endpoints(&state).is_empty());
        // reconnected: a known pack records and links
        let link = start(&state);
        after_read(&state, &link, 1, &Identity::Known, &set("SYN-A1"), &NONE);
        assert!(!held(&state));
        assert_eq!(endpoints(&state), vec!["tcp:gw-a:4196".to_string()]);
        // no pack decided anything (none found, every read failed): recorded once the reads ended
        let link = start(&state);
        after_read(&state, &link, 0, &Identity::Failed, &NONE, &NONE);
        assert!(held(&state));
        reads_done(&state, link.gen - 1);
        assert!(held(&state), "the end of another connection's reads");
        reads_done(&state, link.gen);
        assert!(!held(&state));
        // only new boards: held until „Übernehmen“
        let link = start(&state);
        after_read(&state, &link, 5, &Identity::Unconfirmed { why: Why::New, other: None }, &set("SYN-N5"), &NONE);
        reads_done(&state, link.gen);
        assert!(held(&state));
        confirm(&state, &a, &[5]).unwrap();
        assert!(!held(&state));
    }

    #[test]
    fn refusals_are_json_for_the_ui() {
        let v = IdentityView::of(&Identity::Unconfirmed { why: Why::Foreign, other: Some(Other { id: "b".into(), name: "B".into(), archived: false }) }, 2, &set("SYN-2"));
        let j: serde_json::Value = serde_json::from_str(&refusal(&v, None)).unwrap();
        assert_eq!(j, serde_json::json!({ "kind": "bank", "state": "unconfirmed", "address": 2, "why": "foreign", "serial": "SYN-2",
            "installation": { "id": "b", "name": "B", "archived": false } }));
        let j: serde_json::Value = serde_json::from_str(&failed(3, "no-answer")).unwrap();
        assert_eq!(j, serde_json::json!({ "kind": "bank", "state": "failed", "address": 3, "error": "no-answer" }));
    }

    #[test]
    fn a_write_whose_check_cannot_read_is_refused() {
        let state = AppState::default();
        crate::new_link(&state, Some("a".into()));
        let e = check_target(&state, &link_now(&state), 2).err().unwrap();
        let j: serde_json::Value = serde_json::from_str(&e).unwrap();
        assert_eq!((j["kind"].as_str(), j["state"].as_str(), j["address"].as_u64()), (Some("bank"), Some("failed"), Some(2)));
        // a connection changed since the action began keeps its own error
        let old = link_now(&state);
        crate::new_link(&state, Some("a".into()));
        assert_eq!(check_target(&state, &old, 2).err().unwrap(), LINK_CHANGED);
    }

    fn state_with(store: Store, inst: &str) -> AppState {
        let state = AppState::default();
        *state.maint.lock().unwrap() = Some(store);
        crate::new_link(&state, Some(inst.into()));
        state
    }

    #[test]
    fn a_foreign_pack_holds_the_connection_until_confirmed_or_reconnected() {
        let s = Store::open(&temp("held")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap().id;
        let b = s.create_installation("B", None, vec![]).unwrap().id;
        s.board_seen(&b, 2, &set("SYN-B2"), &NONE, 1).unwrap();
        let state = state_with(s, &a);
        let link = link_now(&state);
        let foreign_id = crate::maint::with_store(&state, |s| identity(s, &a, 2, &set("SYN-B2"))).unwrap();
        after_read(&state, &link, 2, &foreign_id, &set("SYN-B2"), &set("BAT-2"));
        after_read(&state, &link, 3, &Identity::Unconfirmed { why: Why::New, other: None }, &set("SYN-N3"), &NONE);
        assert!(foreign(&state, link.gen) && unconfirmed(&state, link.gen, 3));
        // nothing is stored under A while foreign
        assert_eq!(crate::for_link(&state, &link, "test", |_, _| Ok(1)), None);
        // a reconnect starts clean
        crate::new_link(&state, Some(a.clone()));
        assert!(!foreign(&state, link_now(&state).gen));
        // confirming on the old connection is refused
        assert!(confirm(&state, &b, &[2]).is_err());
        let link = link_now(&state);
        after_read(&state, &link, 2, &foreign_id, &set("SYN-B2"), &set("BAT-2"));
        after_read(&state, &link, 3, &Identity::Unconfirmed { why: Why::New, other: None }, &set("SYN-N3"), &NONE);
        // confirming one pack keeps the other unconfirmed, the foreign state ends with the foreign pack
        assert_eq!(confirm(&state, &a, &[2, 9]).unwrap(), vec![2]);
        assert!(!foreign(&state, link.gen) && unconfirmed(&state, link.gen, 3));
        assert_eq!(crate::for_link(&state, &link, "test", |_, _| Ok(1)), Some(1));
        crate::maint::with_store(&state, |s| {
            assert_eq!(identity(s, &a, 2, &set("SYN-B2"))?, Identity::Known);
            // B keeps its history
            assert_eq!(s.current_board(&b, 2)?.map(|x| x.bms_serial), Some("SYN-B2".into()));
            assert_eq!(s.current_board(&a, 2)?.and_then(|x| x.battery_serial), Some("BAT-2".into()));
            Ok(())
        })
        .unwrap();
        assert_eq!(confirm(&state, &a, &[3]).unwrap(), vec![3]);
        assert!(!unconfirmed(&state, link.gen, 3));
    }
}
