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
//! Data read before the identity is known: on RS485-1/2 the recorder starts held and is released by the first serial
//! read that finds no foreign pack (rows polled before are dropped, not kept pending: simpler, and a few seconds of
//! history are worth less than a wrong installation's). A foreign pack found later holds it again; the rows recorded
//! between the first read and that one stay (seconds, while the bank was not yet in doubt). Parameter snapshots of a
//! pack that is not confirmed are not stored either. On the CAN socket the BMS serial is not visible: recording runs
//! from the start, a known master battery serial of another installation is only a hint (`can_hint`).

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

/// CAN socket bus: the master's battery serial (Modbus) is a board at address 0 of another installation and of none
/// of `inst`'s. Only a hint, nothing is held.
pub fn can_hint(store: &Store, inst: &str, battery: &SerialValue) -> Result<Option<Other>, String> {
    let SerialValue::Set { value, truncated } = battery else { return Ok(None) };
    let places = store.master_battery_places(value, *truncated)?;
    if places.iter().any(|p| p.installation == inst) {
        return Ok(None);
    }
    Ok(places.first().map(|p| Other { id: p.installation.clone(), name: p.name.clone(), archived: p.archived }))
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

/// The unconfirmed packs of one connection; another generation starts empty.
#[derive(Default)]
pub(crate) struct BankState {
    gen: u64,
    packs: BTreeMap<u8, Held>,
}

impl BankState {
    fn of(&mut self, gen: u64) -> &mut BTreeMap<u8, Held> {
        if self.gen != gen {
            self.gen = gen;
            self.packs.clear();
        }
        &mut self.packs
    }
}

/// A pack of connection `gen` is unconfirmed (Some) or not (None). Returns whether the connection is foreign now.
fn note(state: &AppState, gen: u64, address: u8, held: Option<Held>) -> bool {
    let Ok(mut b) = state.bank.lock() else { return false };
    let packs = b.of(gen);
    match held {
        Some(h) => packs.insert(address, h),
        None => packs.remove(&address),
    };
    packs.values().any(|h| h.why == Why::Foreign)
}

/// A pack of another installation is unconfirmed on connection `gen`: nothing is stored for it.
pub(crate) fn foreign(state: &AppState, gen: u64) -> bool {
    state.bank.lock().map(|mut b| b.of(gen).values().any(|h| h.why == Why::Foreign)).unwrap_or(true)
}

/// The pack at `address` is unconfirmed on connection `gen`.
pub(crate) fn unconfirmed(state: &AppState, gen: u64, address: u8) -> bool {
    state.bank.lock().map(|mut b| b.of(gen).contains_key(&address)).unwrap_or(true)
}

/// Hold the recorder while the connection is foreign, else let it run; only while `link` is the current connection.
fn recorder_follow(state: &AppState, link: &Link, foreign: bool) {
    if let Ok(mut h) = state.history.lock() {
        if link_now(state).gen == link.gen {
            h.hold(foreign);
        }
    }
}

/// A serial read of the monitor (`serial_numbers`) classified: an unconfirmed pack is kept for the confirmation, the
/// others are dropped from it; the recorder follows (RS485-1/2: the first decided read releases it).
pub(crate) fn after_read(state: &AppState, link: &Link, address: u8, id: &Identity, bms: &SerialValue, battery: &SerialValue) {
    let held = match id {
        Identity::Unconfirmed { why, .. } => Some(Held { bms: bms.clone(), battery: battery.clone(), why: *why }),
        _ => None,
    };
    let foreign = note(state, link.gen, address, held);
    recorder_follow(state, link, foreign);
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

fn failed(address: u8, error: &str) -> String {
    refusal(&IdentityView::of(&Identity::Failed, address, &SerialValue::NoAnswer), Some(error))
}

/// What the check before a write found: `unchecked` (no serial to compare, the UI says so) and the String 2 read.
pub(crate) struct Checked {
    pub unchecked: bool,
    pub read: SerialValue,
}

/// The first step of every write, inside its pack reservation and on its connection: read 0xA4 of the target and
/// classify it. Known and unchecked go on; unconfirmed and a failed check refuse, nothing is written.
pub(crate) fn check_target(state: &AppState, link: &Link, address: u8) -> Result<Checked, String> {
    let read = match with_link(&state.conn, &state.flashing, &state.link, link, |c| c.bms_serial(address)) {
        Ok(v) => v,
        // the connection changed or a flash took it: that error, not a bank refusal
        Err(e) if e == LINK_CHANGED || e == FLASHING => return Err(e),
        Err(e) => return Err(failed(address, &e)),
    };
    let inst = link.installation.as_deref().ok_or_else(|| failed(address, "no installation"))?;
    let id = crate::maint::with_store(state, |s| identity(s, inst, address, &read)).map_err(|e| failed(address, &format!("{e:?}")))?;
    let view = IdentityView::of(&id, address, &read);
    match id {
        Identity::Known => Ok(Checked { unchecked: false, read }),
        Identity::Unchecked => Ok(Checked { unchecked: true, read }),
        Identity::Failed => Err(failed(address, "no-answer")),
        Identity::Unconfirmed { why, .. } => {
            // the sidebar lists it from now on, also when its serial read had not run yet
            let foreign = note(state, link.gen, address, Some(Held { bms: read.clone(), battery: SerialValue::NoAnswer, why }));
            recorder_follow(state, link, foreign);
            if let Some(app) = state.app.get() {
                let _ = app.emit("bank-unconfirmed", &view);
            }
            Err(refusal(&view, None))
        }
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
        let mut b = state.bank.lock().map_err(|e| e.to_string())?;
        let packs = b.of(link.gen);
        addresses.iter().filter_map(|a| packs.get(a).map(|h| (*a, h.clone()))).collect()
    };
    let now = crate::history::now_ms();
    // one transaction per pack (`board_seen`): a failure leaves the packs not recorded yet unconfirmed
    crate::maint::with_store(state, |s| {
        for (a, h) in &held {
            s.board_seen(installation, *a, &h.bms, &h.battery, now)?;
        }
        Ok(())
    })
    .map_err(|e| format!("{e:?}"))?;
    let mut foreign = false;
    for (a, _) in &held {
        foreign = note(state, link.gen, *a, None);
    }
    if held.is_empty() {
        foreign = self::foreign(state, link.gen);
    }
    recorder_follow(state, &link, foreign);
    Ok(held.into_iter().map(|(a, _)| a).collect())
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
