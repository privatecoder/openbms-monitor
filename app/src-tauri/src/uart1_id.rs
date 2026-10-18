//! The board identity of a pack on the CAN socket (MAINT 1.3: String 1 and String 2 over the binary service) and what
//! depends on it: reading parameters with a snapshot, the serial numbers of the sidebar, and writing parameters.
//!
//! A write goes out only to a strictly known target (design r5/r7): its fresh String 2 is canonical (`serial::canonical`),
//! equal to the String 2 read with the edited column, the current board of this address recorded from canonical raw
//! bytes, and the current board of no other address. Identity check, fresh read, write, acknowledgement and read-back run
//! in one port session (one `with_link`) under the pack's reservation with host polling held; what the store must know
//! is read before, what it keeps is written after. MAINT 1.2 (0x81 to String 2) reads but never writes or stores here.

use crate::bank::{self, Here, Other, Why};
use crate::bms_memory::{uart1_error, uart1_pending, uart1_refusal, uart1_sent, write_checked, ParamIo, ParamWrite, Sent};
use crate::control::reserve_pack_wait;
use crate::history::{now_ms, SnapshotKind};
use crate::maint_store::{BoardRef, SerialPlace};
use crate::{for_link, for_link_checked, link_now, with_conn, with_link, AppState, Link};
use openbms_proto::{serial, uart1, Parameters, SerialValue, Values};
use openbms_transport::{Bus, Connection};
use serde::Serialize;

/// The exchanges with one pack over the binary service; the connection, or a script in tests.
pub(crate) trait Dev {
    fn serial(&mut self, address: u8, cmd: u8) -> openbms_transport::Result<Vec<u8>>;
    fn params(&mut self, address: u8) -> openbms_transport::Result<Parameters>;
    fn write(&mut self, address: u8, block: &[u8]) -> openbms_transport::Result<()>;
    /// the master's battery serial over Modbus
    fn master(&mut self) -> openbms_transport::Result<SerialValue>;
}

pub(crate) struct Conn<'c> {
    pub c: &'c mut Connection,
    pub bus: Bus,
}

impl Dev for Conn<'_> {
    fn serial(&mut self, address: u8, cmd: u8) -> openbms_transport::Result<Vec<u8>> {
        self.c.uart1_serial(self.bus, address, cmd)
    }
    fn params(&mut self, address: u8) -> openbms_transport::Result<Parameters> {
        self.c.uart1_parameters(self.bus, address)
    }
    fn write(&mut self, address: u8, block: &[u8]) -> openbms_transport::Result<()> {
        self.c.uart1_write(self.bus, address, block)
    }
    fn master(&mut self) -> openbms_transport::Result<SerialValue> {
        self.c.master_battery_serial()
    }
}

/// A broken link, as opposed to an answer that did not come or came as an error code.
fn broken(e: &openbms_transport::Error) -> bool {
    matches!(e, openbms_transport::Error::Io(_) | openbms_transport::Error::Serial(_) | openbms_transport::Error::Closed)
}

/// String 2, then String 1 of one pack as far as it offers them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Serials {
    pub bms: SerialValue,
    pub battery: SerialValue,
    /// String 2 as a board identity (`serial::canonical`)
    pub canonical: Option<String>,
}

/// String 2 first: 0x81 (1.2, not offered) gives Unavailable and String 1 is not asked; a timeout gives NoAnswer for both
/// (no second wait). A failed String 1 leaves a valid String 2 as it is. Err only for a broken link.
pub(crate) fn read_serials(dev: &mut impl Dev, address: u8) -> Result<Serials, String> {
    let raw = match dev.serial(address, uart1::CMD_STRING2) {
        Ok(raw) => raw,
        Err(e) if broken(&e) => return Err(e.to_string()),
        Err(openbms_transport::Error::Proto(openbms_proto::Error::Uart1(0x81))) => {
            return Ok(Serials { bms: SerialValue::Unavailable, battery: SerialValue::Unavailable, canonical: None });
        }
        Err(_) => return Ok(Serials { bms: SerialValue::NoAnswer, battery: SerialValue::NoAnswer, canonical: None }),
    };
    let bms = serial::parse_ascii_serial(&raw).unwrap_or(SerialValue::NoAnswer);
    let battery = match dev.serial(address, uart1::CMD_STRING1) {
        Ok(b) => serial::parse_ascii_serial(&b).unwrap_or(SerialValue::NoAnswer),
        Err(e) if broken(&e) => return Err(e.to_string()),
        Err(_) => SerialValue::NoAnswer,
    };
    Ok(Serials { bms, battery, canonical: serial::canonical(&raw) })
}

/// A parameter read for the UI: the block and, over the CAN socket, the String 2 read with it when it is a board
/// identity (the column carries it; a write must find the same).
#[derive(Serialize, Debug)]
pub struct ParamRead {
    #[serde(flatten)]
    pub parameters: Parameters,
    pub bms_serial: Option<String>,
}

/// 0x47 over the CAN socket under the pack's reservation, then String 2 and 1 in the same port session (not after a
/// failed 0x47: an older image would only wait twice more). Kept as a snapshot only for a strictly known board, with
/// exactly that String 2; a board that is not yet one the CAN socket may write to is noted for „Übernehmen“.
pub(crate) fn read(state: &AppState, bus: Bus, address: u8) -> Result<ParamRead, String> {
    let _guard = reserve_pack_wait(state, address, "read")?;
    let link = link_now(state);
    let (parameters, serials) = with_conn(&state.conn, &state.flashing, |c| {
        let mut d = Conn { c, bus };
        let p = match d.params(address) {
            Ok(p) => p,
            Err(e) => return Ok(Err(uart1_error(e))),
        };
        Ok(Ok((p, read_serials(&mut d, address).ok())))
    })??;
    let bms_serial = serials.as_ref().and_then(|s| s.canonical.clone());
    if let (Some(s), Some(serial)) = (&serials, &bms_serial) {
        let facts = link.installation.as_deref().and_then(|inst| crate::maint::with_store(state, |st| Ok((st.current_refs(inst)?, st.serial_places(serial)?))).ok());
        if let (Some(inst), Some((refs, places))) = (link.installation.as_deref(), facts) {
            match verdict(inst, address, serial, &refs, &places) {
                Verdict::Known => {
                    // this board is checked just now: kept also while another pack of the connection is foreign
                    let now = now_ms();
                    for_link_checked(state, &link, "parameter snapshot", |st, inst| st.add_param_snapshot_serial(inst, address, serial, SnapshotKind::Read, &parameters.block, now));
                    for_link_checked(state, &link, "board", |st, inst| st.board_seen_canonical(inst, address, &s.bms, &s.battery, true, now));
                }
                Verdict::Unconfirmed(why, other) => {
                    bank::uart1_unconfirmed(state, &link, address, serial, &s.battery, why, other);
                }
                Verdict::Elsewhere => {}
            }
        }
    }
    Ok(ParamRead { parameters, bms_serial })
}

/// What a canonical String 2 means for reading and showing (no device left to ask): known (snapshots), not yet one
/// to write to (with its reason for „Übernehmen“), or also current at another address (a write sorts that out).
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Known,
    Unconfirmed(Why, Option<Other>),
    Elsewhere,
}

fn verdict(inst: &str, address: u8, serial: &str, refs: &[BoardRef], places: &[SerialPlace]) -> Verdict {
    let s = bank::strict(serial, address, refs);
    match s.here {
        Here::Legacy => Verdict::Unconfirmed(Why::Legacy, None),
        Here::Other => {
            let (why, other) = bank::other_why(inst, address, places);
            Verdict::Unconfirmed(why, other)
        }
        Here::Known if s.elsewhere.is_empty() => Verdict::Known,
        Here::Known => Verdict::Elsewhere,
    }
}

/// The serial numbers of a pack on the CAN socket whose master reports String 1/2 (MAINT 1.3), and what they say
/// against the installation: a known board is recorded as seen (String 1 included), one that is not yet confirmed is
/// noted for „Übernehmen“. Returns the values and the identity view (None: „Nur ansehen“, or nothing to say).
pub(crate) fn serial_numbers(state: &AppState, link: &Link, bus: Bus, address: u8) -> Result<(openbms_proto::SerialNumbers, Option<bank::IdentityView>), String> {
    let mut s = with_conn(&state.conn, &state.flashing, |c| Ok(read_serials(&mut Conn { c, bus }, address)))??;
    // the master's String 1 as before when the binary one is missing
    if address == 0 && !matches!(s.battery, SerialValue::Set { .. }) {
        if let Ok(b) = with_conn(&state.conn, &state.flashing, |c| c.master_battery_serial()) {
            s.battery = b;
        }
    }
    let values = openbms_proto::SerialNumbers { battery: s.battery.clone(), bms: s.bms.clone() };
    let Some(inst) = link.installation.as_deref() else { return Ok((values, None)) };
    let view = match (&s.canonical, &s.bms) {
        (Some(serial), _) => {
            let facts = crate::maint::with_store(state, |st| Ok((st.current_refs(inst)?, st.serial_places(serial)?))).map_err(|e| format!("{e:?}"))?;
            match verdict(inst, address, serial, &facts.0, &facts.1) {
                Verdict::Unconfirmed(why, other) => Some(bank::uart1_unconfirmed(state, link, address, serial, &s.battery, why, other)),
                Verdict::Known | Verdict::Elsewhere => {
                    let now = now_ms();
                    for_link(state, link, "board", |st, inst| st.board_seen_canonical(inst, address, &s.bms, &s.battery, true, now));
                    Some(bank::IdentityView::of(&bank::Identity::Known, address, &s.bms))
                }
            }
        }
        (None, SerialValue::NoAnswer) => Some(bank::IdentityView::of(&bank::Identity::Failed, address, &s.bms)),
        (None, SerialValue::Unavailable) => None,
        // empty, the placeholder or bytes that are no identity: nothing to compare, nothing recorded
        (None, v) => Some(bank::IdentityView::of(&bank::Identity::Unchecked, address, v)),
    };
    Ok((values, view))
}

/// What the store must know for a write, read before its port session.
pub(crate) struct Facts {
    pub refs: Vec<BoardRef>,
    /// the master's battery serial stored for the installation
    pub master: Option<String>,
    /// every place the expected String 2 is known at
    pub places: Vec<SerialPlace>,
}

/// What a write's port session came to, and what the store keeps of it afterwards.
#[derive(Debug, Default)]
pub(crate) struct Session {
    pub result: Option<Result<ParamWrite, String>>,
    /// the checked String 2 the snapshots carry
    pub serial: Option<String>,
    pub snaps: Vec<(SnapshotKind, Vec<u8>)>,
    /// earlier current boards of the same serial at other addresses, shown to be gone there (another canonical String 2)
    pub gone: Vec<BoardRef>,
    /// the target is not (yet) one to write to: noted for „Übernehmen“
    pub unconfirmed: Option<(String, Why, Option<Other>)>,
}

/// The exchanges of `write_checked` inside the port session; snapshots are collected, the store gets them after.
struct SessionIo<'d, D: Dev> {
    dev: &'d mut D,
    address: u8,
    snaps: Vec<(SnapshotKind, Vec<u8>)>,
}

impl<D: Dev> ParamIo for SessionIo<'_, D> {
    fn read(&mut self, keep: bool) -> Result<Parameters, String> {
        let p = self.dev.params(self.address).map_err(uart1_error)?;
        if keep {
            self.snaps.push((SnapshotKind::Read, p.block.clone()));
        }
        Ok(p)
    }
    fn send(&mut self, block: &[u8]) -> Result<Sent, String> {
        Ok(uart1_sent(self.dev.write(self.address, block)))
    }
    fn snapshot(&mut self, kind: SnapshotKind, block: &[u8]) {
        self.snaps.push((kind, block.to_vec()));
    }
    fn uart1(&self) -> bool {
        true
    }
}

/// The refusal of a place that still has the same board recorded: `address`, its board row and when it became current,
/// for the explicit „Eintrag an Adresse … als nicht mehr vorhanden markieren“ (`bank_mark_gone`).
fn elsewhere(r: &BoardRef) -> String {
    serde_json::json!({ "kind": "uart1", "error": "elsewhere", "address": r.address, "board": r.id, "at": r.current_at }).to_string()
}

/// The port session of a write: master check, String 2 of the target (and of other places that list the same board),
/// the strict identity, then fresh read, write, acknowledgement and read-back (`write_checked`). Only device I/O and
/// comparisons; `facts` came from the store before.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_session(dev: &mut impl Dev, inst: &str, address: u8, expected: Option<&str>, facts: &Facts, base: Values, edited: Values, restore: bool) -> Session {
    let mut out = Session::default();
    let refuse = |out: &mut Session, e: String| out.result = Some(Err(e));
    match dev.master() {
        Ok(read) if crate::bms_memory::master_blocks(facts.master.as_deref(), &read) => {
            refuse(&mut out, uart1_refusal("other-master"));
            return out;
        }
        Err(e) if broken(&e) => {
            refuse(&mut out, e.to_string());
            return out;
        }
        _ => {}
    }
    let raw = match dev.serial(address, uart1::CMD_STRING2) {
        Ok(raw) => raw,
        Err(openbms_transport::Error::Proto(openbms_proto::Error::Uart1(0x81))) => {
            refuse(&mut out, uart1_refusal("no-identity"));
            return out;
        }
        Err(e) if broken(&e) => {
            refuse(&mut out, e.to_string());
            return out;
        }
        Err(_) => {
            refuse(&mut out, uart1_refusal("no-answer"));
            return out;
        }
    };
    let Some(serial) = serial::canonical(&raw) else {
        refuse(&mut out, uart1_refusal("unchecked"));
        return out;
    };
    // (b): the board the edited column was read from
    if expected != Some(serial.as_str()) {
        refuse(&mut out, uart1_refusal("changed"));
        return out;
    }
    let s = bank::strict(&serial, address, &facts.refs);
    match s.here {
        Here::Known => {}
        Here::Legacy | Here::Other => {
            let (why, other) = if s.here == Here::Legacy { (Why::Legacy, None) } else { bank::other_why(inst, address, &facts.places) };
            let bms = SerialValue::Set { value: serial.clone(), truncated: false };
            let view = bank::IdentityView::of(&bank::Identity::Unconfirmed { why, other: other.clone() }, address, &bms);
            out.unconfirmed = Some((serial, why, other));
            refuse(&mut out, bank::refusal(&view, None));
            return out;
        }
    }
    // (d): another place lists the same board as current; only another canonical String 2 there clears it
    for r in &s.elsewhere {
        match dev.serial(r.address, uart1::CMD_STRING2) {
            Ok(other) => match serial::canonical(&other) {
                Some(o) if o == serial => {
                    refuse(&mut out, serde_json::json!({ "kind": "uart1", "error": "duplicate", "address": r.address }).to_string());
                    return out;
                }
                Some(_) => out.gone.push(r.clone()),
                None => {
                    refuse(&mut out, elsewhere(r));
                    return out;
                }
            },
            Err(e) if broken(&e) => {
                refuse(&mut out, e.to_string());
                return out;
            }
            // 0x81 or no answer: not checkable, the user decides
            Err(_) => {
                refuse(&mut out, elsewhere(r));
                return out;
            }
        }
    }
    let mut io = SessionIo { dev, address, snaps: vec![] };
    out.result = Some(write_checked(&mut io, address, base, edited, restore));
    out.snaps = io.snaps;
    out.serial = Some(serial);
    out
}

/// A parameter write over the CAN socket (`write_parameters`): an open interrupted update of this address refuses it;
/// the store's facts first, then the port session with polling held, then what the store keeps.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write(state: &AppState, link: &Link, bus: Bus, address: u8, expected: Option<&str>, base: Values, edited: Values, restore: bool) -> Result<bank::Written<ParamWrite>, String> {
    let inst = link.installation.clone().ok_or("view only: writing to a pack needs a connection with an installation")?;
    uart1_pending(state, link, address)?;
    let facts = crate::maint::with_store(state, |s| {
        Ok(Facts {
            refs: s.current_refs(&inst)?,
            master: s.current_board(&inst, 0)?.and_then(|b| b.battery_serial),
            places: match expected {
                Some(e) => s.serial_places(e)?,
                None => vec![],
            },
        })
    })
    .map_err(|e| format!("{e:?}"))?;
    let _pause = crate::PollPause::hold(&state.params_pause);
    let session = with_link(&state.conn, &state.flashing, &state.link, link, |c| {
        Ok(write_session(&mut Conn { c, bus }, &inst, address, expected, &facts, base, edited, restore))
    })?;
    keep(state, link, address, &session);
    match session.result {
        Some(r) => r.map(|value| bank::Written { value, unchecked: false }).map_err(|e| with_gen(e, link.gen)),
        None => Err("no result".into()),
    }
}

/// An `elsewhere` refusal names the link generation its finding came from: „nicht mehr vorhanden“ applies only there.
fn with_gen(e: String, gen: u64) -> String {
    match serde_json::from_str::<serde_json::Value>(&e) {
        Ok(mut v) if v["kind"] == "uart1" && v["error"] == "elsewhere" => {
            v["gen"] = serde_json::json!(gen);
            v.to_string()
        }
        _ => e,
    }
}

/// After the port session: the places shown to be gone (only the rows checked, with their time), the snapshots with the
/// checked String 2, and an unconfirmed target for „Übernehmen“.
fn keep(state: &AppState, link: &Link, address: u8, session: &Session) {
    let now = now_ms();
    for g in &session.gone {
        for_link(state, link, "board gone", |s, inst| s.board_gone(inst, g.address, g.id, g.current_at, now));
    }
    // the target was checked strictly in this session: its snapshots are kept also while another pack of the
    // connection is foreign (bound to the installation and the link generation)
    if let Some(serial) = &session.serial {
        for (kind, block) in &session.snaps {
            for_link_checked(state, link, "parameter snapshot", |s, inst| s.add_param_snapshot_serial(inst, address, serial, *kind, block, now));
        }
    }
    if let Some((serial, why, other)) = &session.unconfirmed {
        bank::uart1_unconfirmed(state, link, address, serial, &SerialValue::NoAnswer, *why, other.clone());
    }
}

/// „Eintrag an Adresse … als nicht mehr vorhanden markieren“, after the user confirmed it: the board row `board` at
/// `address` that a write found still listed (its String 2 not checkable there) is no longer current. Only that row
/// with that time, so a board taken over there meanwhile stays.
#[tauri::command]
pub async fn bank_mark_gone(state: tauri::State<'_, AppState>, installation: String, address: u8, board: i64, at: Option<i64>, gen: u64) -> Result<bool, String> {
    mark_gone(&state, &installation, address, board, at, gen)
}

/// `gen`: the connection the finding came from; after a reconnect the place may answer again, so it is asked anew.
fn mark_gone(state: &AppState, installation: &str, address: u8, board: i64, at: Option<i64>, gen: u64) -> Result<bool, String> {
    let link = link_now(state);
    if link.installation.as_deref() != Some(installation) {
        return Err("the connection belongs to another installation than the one chosen; connect again".into());
    }
    if link.gen != gen {
        return Err(crate::LINK_CHANGED.into());
    }
    crate::maint::with_store(state, |s| s.board_gone(installation, address, board, at, now_ms())).map_err(|e| format!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn raw(text: &str) -> Vec<u8> {
        let mut v = text.as_bytes().to_vec();
        v.resize(serial::LEN, b' ');
        v
    }

    /// A bank as the binary service shows it: String 2/1 per address (None: 0x81, missing: no answer), the block, the
    /// write's answer and the read-back, and what was asked.
    #[derive(Default)]
    struct Bank {
        strings2: HashMap<u8, Option<Vec<u8>>>,
        strings1: HashMap<u8, Vec<u8>>,
        reads: Vec<openbms_transport::Result<Parameters>>,
        write: Option<openbms_transport::Result<()>>,
        master: Option<SerialValue>,
        asked: Vec<(u8, u8)>,
        written: Vec<Vec<u8>>,
    }

    impl Dev for Bank {
        fn serial(&mut self, address: u8, cmd: u8) -> openbms_transport::Result<Vec<u8>> {
            self.asked.push((address, cmd));
            let table = if cmd == uart1::CMD_STRING2 { self.strings2.get(&address).cloned() } else { self.strings1.get(&address).cloned().map(Some) };
            match table {
                Some(Some(r)) => Ok(r),
                Some(None) => Err(openbms_transport::Error::Proto(openbms_proto::Error::Uart1(0x81))),
                None => Err(openbms_transport::Error::Timeout(std::time::Duration::from_millis(800))),
            }
        }
        fn params(&mut self, address: u8) -> openbms_transport::Result<Parameters> {
            self.asked.push((address, uart1::CMD_READ));
            self.reads.remove(0)
        }
        fn write(&mut self, address: u8, block: &[u8]) -> openbms_transport::Result<()> {
            self.asked.push((address, uart1::CMD_WRITE));
            self.written.push(block.to_vec());
            self.write.take().expect("one write")
        }
        fn master(&mut self) -> openbms_transport::Result<SerialValue> {
            Ok(self.master.clone().unwrap_or(SerialValue::Unavailable))
        }
    }

    fn block(address: u8, p0: u16) -> Parameters {
        let mut b = vec![address, 0x3C];
        b.extend(p0.to_be_bytes());
        for i in 1..60u16 {
            b.extend((3000 + i).to_be_bytes());
        }
        b.push(0x1B);
        b.extend(60..87u8);
        b.push(0x08);
        b.extend([0u8; 8]);
        b.extend(b"SYN       ");
        Parameters::parse(&b).unwrap()
    }

    fn edit(p: &Parameters, p0: u16) -> (Values, Values) {
        let base = p.values();
        let mut edited = base.clone();
        edited.raws[0] = p0;
        (base, edited)
    }

    fn board(address: u8, serial: &str, canonical: Option<bool>) -> BoardRef {
        BoardRef { address, id: 100 + i64::from(address), bms_serial: serial.into(), aliases: vec![], canonical, current_at: Some(5) }
    }

    fn facts(refs: Vec<BoardRef>) -> Facts {
        Facts { refs, master: None, places: vec![] }
    }

    /// Address 1 holds "SYN-1", canonical, current; it answers String 2 and the read, takes the write, reads back.
    fn good(readback: u16) -> Bank {
        let mut b = Bank::default();
        b.strings2.insert(1, Some(raw("SYN-1")));
        b.reads = vec![Ok(block(1, 3600)), Ok(block(1, readback))];
        b.write = Some(Ok(()));
        b
    }

    fn kinds(s: &Session) -> Vec<SnapshotKind> {
        s.snaps.iter().map(|(k, _)| *k).collect()
    }

    fn err(s: &Session) -> serde_json::Value {
        serde_json::from_str(s.result.as_ref().unwrap().as_ref().unwrap_err()).unwrap()
    }

    #[test]
    fn a_strictly_known_target_is_written_in_one_session_with_snapshots_after_the_matching_read_back() {
        let mut dev = good(3650);
        let (base, edited) = edit(&block(1, 3600), 3650);
        let s = write_session(&mut dev, "A", 1, Some("SYN-1"), &facts(vec![board(1, "SYN-1", Some(true))]), base, edited, false);
        assert!(matches!(s.result, Some(Ok(ParamWrite::Written { read_back: Some(_), unanswered: false, .. }))));
        // master, String 2, fresh read, write, read-back: in this order, nothing else
        assert_eq!(dev.asked, [(1, uart1::CMD_STRING2), (1, uart1::CMD_READ), (1, uart1::CMD_WRITE), (1, uart1::CMD_READ)]);
        assert_eq!(kinds(&s), [SnapshotKind::Read, SnapshotKind::Written, SnapshotKind::Read]);
        assert_eq!(s.serial.as_deref(), Some("SYN-1"));
    }

    #[test]
    fn another_read_back_keeps_what_was_read_and_no_written() {
        let mut dev = good(3640);
        let (base, edited) = edit(&block(1, 3600), 3650);
        let s = write_session(&mut dev, "A", 1, Some("SYN-1"), &facts(vec![board(1, "SYN-1", Some(true))]), base, edited, false);
        assert!(matches!(s.result, Some(Ok(ParamWrite::UnclearRead { .. }))));
        assert_eq!(kinds(&s), [SnapshotKind::Read, SnapshotKind::Read]);
    }

    #[test]
    fn e2_keeps_a_refused_snapshot_and_no_live_switches() {
        let mut dev = good(3600);
        dev.write = Some(Err(openbms_transport::Error::Proto(openbms_proto::Error::Uart1(0xE2))));
        dev.reads.pop();
        let (base, edited) = edit(&block(1, 3600), 3650);
        let s = write_session(&mut dev, "A", 1, Some("SYN-1"), &facts(vec![board(1, "SYN-1", Some(true))]), base, edited, false);
        assert!(matches!(s.result, Some(Ok(ParamWrite::Refused { switches_live: false, .. }))));
        assert_eq!(kinds(&s), [SnapshotKind::Read, SnapshotKind::Refused]);
    }

    #[test]
    fn every_identity_refusal_comes_before_anything_is_written() {
        let known = || facts(vec![board(1, "SYN-1", Some(true))]);
        let (base, edited) = edit(&block(1, 3600), 3650);
        let run = |dev: &mut Bank, f: &Facts, expected: Option<&str>| write_session(dev, "A", 1, expected, f, base.clone(), edited.clone(), false);
        // 1.2: no String 2
        let mut d = good(3650);
        d.strings2.insert(1, None);
        assert_eq!(err(&run(&mut d, &known(), Some("SYN-1")))["error"], "no-identity");
        // no answer
        let mut d = good(3650);
        d.strings2.remove(&1);
        assert_eq!(err(&run(&mut d, &known(), Some("SYN-1")))["error"], "no-answer");
        // not canonical: leading padding, the placeholder, empty
        for r in [raw(" SYN-1"), raw("EMU110110S-XXXX-1"), raw("")] {
            let mut d = good(3650);
            d.strings2.insert(1, Some(r));
            assert_eq!(err(&run(&mut d, &known(), Some("SYN-1")))["error"], "unchecked");
        }
        // another board than the one the column was read from, or a column read without identity
        let mut d = good(3650);
        assert_eq!(err(&run(&mut d, &known(), Some("SYN-OTHER")))["error"], "changed");
        let mut d = good(3650);
        assert_eq!(err(&run(&mut d, &known(), None))["error"], "changed");
        // recorded before provenance: one confirmation first, noted for „Übernehmen“
        let mut d = good(3650);
        let s = run(&mut d, &facts(vec![board(1, "SYN-1", None)]), Some("SYN-1"));
        assert_eq!((err(&s)["kind"].as_str(), err(&s)["why"].as_str()), (Some("bank"), Some("legacy")));
        assert_eq!(s.unconfirmed.as_ref().map(|u| u.1), Some(Why::Legacy));
        // not the current board here (new)
        let mut d = good(3650);
        let s = run(&mut d, &facts(vec![board(1, "SYN-OLD", Some(true))]), Some("SYN-1"));
        assert_eq!(err(&s)["why"], "new");
        // the master behind the CAN socket is another bank's
        let mut d = good(3650);
        d.master = Some(SerialValue::Set { value: "BAT-B".into(), truncated: false });
        let f = Facts { master: Some("BAT-A".into()), ..known() };
        assert_eq!(err(&run(&mut d, &f, Some("SYN-1")))["error"], "other-master");
        // none of these wrote anything
        assert!(d.written.is_empty());
    }

    #[test]
    fn a_board_listed_at_another_address_too() {
        let (base, edited) = edit(&block(1, 3600), 3650);
        let refs = || facts(vec![board(1, "SYN-1", Some(true)), board(4, "SYN-1", Some(true))]);
        let run = |dev: &mut Bank| write_session(dev, "A", 1, Some("SYN-1"), &refs(), base.clone(), edited.clone(), false);
        // the old place shows another canonical board now: the old entry is gone, the write goes on
        let mut d = good(3650);
        d.strings2.insert(4, Some(raw("SYN-4")));
        let s = run(&mut d);
        assert!(matches!(s.result, Some(Ok(ParamWrite::Written { .. }))));
        assert_eq!(s.gone.iter().map(|g| (g.address, g.id)).collect::<Vec<_>>(), [(4, 104)]);
        // the same board answers there too: a real duplicate
        let mut d = good(3650);
        d.strings2.insert(4, Some(raw("SYN-1")));
        let s = run(&mut d);
        assert_eq!((err(&s)["error"].as_str(), err(&s)["address"].as_u64()), (Some("duplicate"), Some(4)));
        assert!(d.written.is_empty() && s.gone.is_empty());
        // 0x81 or silence there: not checkable, the user marks the entry gone explicitly
        for r in [Some(None), None] {
            let mut d = good(3650);
            if let Some(r) = r {
                d.strings2.insert(4, r);
            }
            let s = run(&mut d);
            let e = err(&s);
            assert_eq!((e["error"].as_str(), e["address"].as_u64(), e["board"].as_i64(), e["at"].as_i64()), (Some("elsewhere"), Some(4), Some(104), Some(5)));
            assert!(d.written.is_empty() && s.gone.is_empty());
        }
    }

    #[test]
    fn a_lost_acknowledgement_is_read_back_never_refused() {
        let mut dev = good(3650);
        dev.write = Some(Err(openbms_transport::Error::Incomplete));
        let (base, edited) = edit(&block(1, 3600), 3650);
        let s = write_session(&mut dev, "A", 1, Some("SYN-1"), &facts(vec![board(1, "SYN-1", Some(true))]), base, edited, false);
        assert!(matches!(s.result, Some(Ok(ParamWrite::Written { unanswered: true, .. }))));
        assert_eq!(kinds(&s), [SnapshotKind::Read, SnapshotKind::Written]);
    }

    #[test]
    fn serials_ask_string_1_only_after_a_string_2() {
        let mut d = Bank::default();
        d.strings2.insert(1, Some(raw("SYN-1")));
        d.strings1.insert(1, raw("BAT-1"));
        d.strings2.insert(2, None);
        let s = read_serials(&mut d, 1).unwrap();
        assert_eq!((s.canonical.as_deref(), &s.battery), (Some("SYN-1"), &SerialValue::Set { value: "BAT-1".into(), truncated: false }));
        // 1.2: 0x81, String 1 is not asked
        d.asked.clear();
        assert_eq!(read_serials(&mut d, 2).unwrap(), Serials { bms: SerialValue::Unavailable, battery: SerialValue::Unavailable, canonical: None });
        assert_eq!(d.asked, [(2, uart1::CMD_STRING2)]);
        // silence: no second wait
        d.asked.clear();
        assert_eq!(read_serials(&mut d, 3).unwrap().bms, SerialValue::NoAnswer);
        assert_eq!(d.asked.len(), 1);
        // String 1 missing: String 2 stays
        let mut d = Bank::default();
        d.strings2.insert(5, Some(raw("SYN-5")));
        let s = read_serials(&mut d, 5).unwrap();
        assert_eq!((s.canonical.as_deref(), s.battery), (Some("SYN-5"), SerialValue::NoAnswer));
    }

    #[test]
    fn reading_keeps_snapshots_only_for_a_strictly_known_board() {
        let refs = vec![board(1, "SYN-1", Some(true)), board(2, "SYN-2", None), board(3, "SYN-3", Some(true)), board(4, "SYN-3", Some(true))];
        assert_eq!(verdict("A", 1, "SYN-1", &refs, &[]), Verdict::Known);
        assert_eq!(verdict("A", 2, "SYN-2", &refs, &[]), Verdict::Unconfirmed(Why::Legacy, None));
        assert_eq!(verdict("A", 3, "SYN-3", &refs, &[]), Verdict::Elsewhere);
        assert_eq!(verdict("A", 5, "SYN-5", &refs, &[]), Verdict::Unconfirmed(Why::New, None));
    }

    #[test]
    fn a_foreign_other_pack_does_not_drop_the_checked_targets_snapshots() {
        let dir = std::env::temp_dir().join(format!("openbms-uart1-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let st = crate::maint_store::Store::open(&dir.join("maintenance.sqlite")).unwrap();
        let a = st.create_installation("A", None, vec![]).unwrap().id;
        let state = AppState::default();
        *state.maint.lock().unwrap() = Some(st);
        crate::new_link(&state, Some(a.clone()));
        let link = link_now(&state);
        bank::begin(&state, link.gen, false, None);
        // pack 2 of the connection belongs to another installation
        let set = |v: &str| SerialValue::Set { value: v.into(), truncated: false };
        bank::after_read(&state, &link, 2, &bank::Identity::Unconfirmed { why: Why::Foreign, other: None }, &set("SYN-B2"), &SerialValue::NoAnswer);
        assert!(bank::foreign(&state, link.gen));
        let session = Session { result: None, serial: Some("SYN-1".into()), snaps: vec![(SnapshotKind::Read, block(1, 3600).block)], ..Default::default() };
        keep(&state, &link, 1, &session);
        let rows = crate::maint::with_store(&state, |s| s.latest_snapshots(&a)).unwrap();
        assert_eq!(rows.iter().map(|r| r.address).collect::<Vec<_>>(), [1]);
        // a session of a connection replaced meanwhile keeps nothing
        crate::new_link(&state, Some(a.clone()));
        keep(&state, &link, 3, &Session { serial: Some("SYN-3".into()), snaps: vec![(SnapshotKind::Read, block(3, 3600).block)], ..Default::default() });
        assert_eq!(crate::maint::with_store(&state, |s| s.latest_snapshots(&a)).unwrap().len(), 1);
    }

    #[test]
    fn marking_gone_needs_the_connections_installation_and_generation() {
        let state = AppState::default();
        crate::new_link(&state, Some("a".into()));
        let gen = link_now(&state).gen;
        assert!(mark_gone(&state, "b", 4, 1, None, gen).unwrap_err().contains("another installation"));
        // a finding of an earlier connection of the same installation
        crate::new_link(&state, Some("a".into()));
        assert_eq!(mark_gone(&state, "a", 4, 1, None, gen).unwrap_err(), crate::LINK_CHANGED);
        // the refusal names its generation; others pass unchanged
        let e: serde_json::Value = serde_json::from_str(&with_gen(elsewhere(&board(4, "SYN-1", Some(true))), 7)).unwrap();
        assert_eq!((e["error"].as_str(), e["gen"].as_u64()), (Some("elsewhere"), Some(7)));
        assert_eq!(with_gen(uart1_refusal("changed"), 7), uart1_refusal("changed"));
    }
}
