//! „EXT — CCL-Abbaustufe (Master)“: the persistent CCL decrease step of EMU1101-EXT Full 1.6.7 and later (capability
//! bit 4 of the master's own 64/03; `openbms_proto::ccl`). A numeric EXT setting of its own, never part of the 87 OEM
//! parameters, a parameter file, a copy to other packs or a restore. Only the active master's value acts on the bank,
//! so the app reads and writes it at address 0 only (the firmware also answers slaves at their own address; nothing is
//! replicated), over the bus of the connection: the CAN socket, or RS485-1/2 with the master's polling stopped.
//!
//! The read asks a fresh 64/03 (bit 4), the master's String 2 and 64/07; a value is shown only from that answer. The
//! write is an explicit user action: the String 2 its value was read with must still be the installation's current
//! board at address 0 and answer there now, a fresh 64/03 must still show bit 4; over the CAN socket the strict identity
//! (`uart1_id::identity`) and a fresh binary 0x47 with Group0 bit 0 at 1, over RS485-1/2 the right-bank check of every
//! other RS485-1/2 write (`bank::check_target`; a polling master does not answer there, so nothing goes out) and no
//! release bit. Then 64/08 goes out once and a separate 64/07 reads the value back. A refusal (RTN 05/04/08) is named;
//! silence or any other answer leaves the outcome open and is never repeated. Nothing here runs on connect, reconnect,
//! a role change, a restore or a multi-pack write, and nothing halts a master.

use crate::bms_memory::{on_bus, uart1_error, uart1_refusal};
use crate::control::PackAction;
use crate::uart1_id::{identity, Conn, Dev, Expected, Facts, Session};
use crate::{with_link, AppState, Link};
use openbms_proto::access::{self, Action, Channel, Reason, State};
use openbms_proto::ccl::{self, CclStep};
use openbms_proto::maint::CAP_CCL_STEP;
use openbms_proto::{serial, uart1, ExtInfo, SerialValue};
use openbms_transport::{Bus, Connection, SwitchFailure};
use serde::Serialize;

/// The only address whose setting acts on the bank.
const MASTER: u8 = 0;

/// The ASCII exchanges of the setting on one channel; the connection, or a script in tests.
pub(crate) trait CclIo {
    fn channel(&self) -> Channel;
    fn ext_info(&mut self, address: u8) -> openbms_transport::Result<ExtInfo>;
    fn ccl_read(&mut self, address: u8) -> openbms_transport::Result<CclStep>;
    fn ccl_set(&mut self, address: u8, value: u8) -> Result<CclStep, SwitchFailure>;
    /// the master's String 2 as a board identity (binary A4 on the CAN socket, ASCII A4 on RS485-1/2); None: not
    /// checkable (no answer, not canonical, a placeholder)
    fn master_serial(&mut self) -> Option<String>;
}

impl CclIo for Conn<'_> {
    fn channel(&self) -> Channel {
        Channel::Uart1
    }
    fn ext_info(&mut self, address: u8) -> openbms_transport::Result<ExtInfo> {
        self.c.maint_info(address)
    }
    fn ccl_read(&mut self, address: u8) -> openbms_transport::Result<CclStep> {
        self.c.ccl_step(address)
    }
    fn ccl_set(&mut self, address: u8, value: u8) -> Result<CclStep, SwitchFailure> {
        self.c.set_ccl_step(address, value)
    }
    fn master_serial(&mut self) -> Option<String> {
        self.serial(MASTER, uart1::CMD_STRING2).ok().and_then(|raw| serial::canonical(&raw))
    }
}

/// The RS485-1/2 side: the same ASCII frames at the master's own address.
pub(crate) struct Ascii<'c> {
    pub c: &'c mut Connection,
}

impl CclIo for Ascii<'_> {
    fn channel(&self) -> Channel {
        Channel::Uart0
    }
    fn ext_info(&mut self, address: u8) -> openbms_transport::Result<ExtInfo> {
        self.c.maint_info(address)
    }
    fn ccl_read(&mut self, address: u8) -> openbms_transport::Result<CclStep> {
        self.c.ccl_step(address)
    }
    fn ccl_set(&mut self, address: u8, value: u8) -> Result<CclStep, SwitchFailure> {
        self.c.set_ccl_step(address, value)
    }
    fn master_serial(&mut self) -> Option<String> {
        match self.c.bms_serial_canonical(MASTER) {
            Ok((SerialValue::Set { value, truncated: false }, true)) => Some(value),
            _ => None,
        }
    }
}

/// The value as the UI sent it: an integer 1..=20, nothing rounded (a fraction, a Boolean, a text, null or out of range
/// is refused before anything else happens).
pub(crate) fn step_of(v: &serde_json::Value) -> Result<u8, String> {
    v.as_u64().and_then(|n| u8::try_from(n).ok()).and_then(|n| ccl::step(n).ok()).ok_or_else(|| uart1_refusal("ccl-bad-value"))
}

/// A confirmed read of the master's setting, with the master's String 2 read in the same session (canonical; None: not
/// checkable, then nothing is written).
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct CclRead {
    pub step: CclStep,
    pub serial: Option<String>,
}

/// A saved setting: the separate read-back after the acknowledged set.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct CclSaved {
    pub step: CclStep,
    pub serial: String,
}

/// An error JSON of the setting (`uart1.ccl-*` in the UI), with the value a read after the attempt found (`now`).
fn refusal(error: &str, now: Option<u8>) -> String {
    serde_json::json!({ "kind": "uart1", "error": error, "now": now }).to_string()
}

/// Silence of the master: over RS485-1/2 typically its own polling (it answers there only while halted or without
/// slaves), over the CAN socket no answer.
fn silent(channel: Channel) -> String {
    uart1_refusal(if channel == Channel::Uart0 { "ccl-silent" } else { "ccl-no-answer" })
}

/// A read that did not give a value: silence, a malformed answer, a refusal; never a stand-in value.
fn read_error(channel: Channel, e: openbms_transport::Error) -> String {
    use openbms_transport::Error as E;
    match e {
        E::Timeout(_) | E::Incomplete => silent(channel),
        E::Proto(openbms_proto::Error::Rtn(code)) => serde_json::json!({ "kind": "uart1", "error": "ccl-read-refused", "code": code }).to_string(),
        E::Proto(_) => uart1_refusal("ccl-malformed"),
        e => crate::error_text(e),
    }
}

/// The master's own fresh 64/03 announces the setting: a supported build with bit 4.
fn announces(dev: &mut impl CclIo) -> Result<ExtInfo, String> {
    match dev.ext_info(MASTER) {
        Ok(i) if i.supported() && i.cap(CAP_CCL_STEP) => Ok(i),
        Ok(_) | Err(openbms_transport::Error::Proto(_)) => Err(uart1_refusal("ccl-no-cap")),
        Err(openbms_transport::Error::Timeout(_) | openbms_transport::Error::Incomplete) => Err(silent(dev.channel())),
        Err(e) => Err(crate::error_text(e)),
    }
}

/// The read's port session on either channel: fresh 64/03, the master's String 2, 64/07.
pub(crate) fn read_session(dev: &mut impl CclIo) -> Result<CclRead, String> {
    announces(dev)?;
    let serial = dev.master_serial();
    let step = dev.ccl_read(MASTER).map_err(|e| read_error(dev.channel(), e))?;
    Ok(CclRead { step, serial })
}

/// The access facts of the master from its fresh identity and switches.
fn facts_of(info: &ExtInfo, group0: Option<u8>) -> access::Facts {
    access::Facts {
        patch: access::Patch::Version(info.patch), flags: Some(info.image_flags()), caps: Some(info.read_caps), group0, group7: None, address: MASTER,
        installation: true, marker: None, base: None, slaves: None,
    }
}

/// Why the catalog does not let the write go over `channel` for `fw`: the capability (`ccl-no-cap`), Group0 bit 0
/// known off on the CAN socket (`write-locked`), another condition (`not-ready`); None: ready (the quiet bus of
/// RS485-1/2 is a note), or only the bit not read yet (the session reads it fresh).
pub(crate) fn not_ready(channel: Channel, fw: &access::Facts) -> Option<String> {
    let Some(route) = access::routes(Action::WriteCclStep).find(|r| r.channel == channel && r.implemented) else {
        return Some(uart1_refusal("not-ready"));
    };
    let d = access::decide(route, fw);
    if d.state == State::Ready {
        return None;
    }
    if d.reasons.iter().any(|r| matches!(r, Reason::Firmware { .. })) {
        return Some(uart1_refusal("ccl-no-cap"));
    }
    if d.reasons.iter().any(|r| matches!(r, Reason::Bit { group: 0, bit: access::G0_WRITE, known: Some(_), .. })) {
        return Some(uart1_refusal("write-locked"));
    }
    if d.state == State::Unknown && d.reasons.iter().all(|r| matches!(r, Reason::Bit { known: None, .. })) {
        return None;
    }
    Some(uart1_refusal("not-ready"))
}

/// The CAN socket write's port session: identity of the expected master board, fresh 64/03 with bit 4, fresh 0x47 with
/// Group0 bit 0, one 64/08, then a separate 64/07. Every refusal before the 64/08 sends nothing that changes the pack.
pub(crate) fn write_session<D: CclIo + Dev>(dev: &mut D, inst: &str, expected: &str, facts: &Facts, value: u8) -> Session<CclSaved> {
    let id = identity(dev, inst, MASTER, Expected::Board(expected), facts);
    let mut out = Session { gone: id.gone, unconfirmed: id.unconfirmed, ..Default::default() };
    let serial = match id.serial {
        Ok(s) => s,
        Err(e) => {
            out.result = Some(Err(e));
            return out;
        }
    };
    out.serial = Some(serial.clone());
    out.result = Some(save_uart1(dev, value).map(|step| CclSaved { step, serial }));
    out
}

fn save_uart1<D: CclIo + Dev>(dev: &mut D, value: u8) -> Result<CclStep, String> {
    let info = announces(dev)?;
    let fresh = dev.params(MASTER).map_err(uart1_error)?;
    if fresh.address != MASTER {
        return Err(format!("the pack at address {MASTER} answered with address {}", fresh.address));
    }
    if let Some(e) = not_ready(Channel::Uart1, &facts_of(&info, Some(fresh.function_switches[0]))) {
        return Err(e);
    }
    set_and_read(dev, value)
}

/// The RS485-1/2 write's port session (after the right-bank check): the master's fresh String 2 is the expected board,
/// a fresh 64/03 shows bit 4 (no release bit on this way), one 64/08, then a separate 64/07. A polling master does not
/// answer: nothing that changes it is sent.
pub(crate) fn write_session_uart0(dev: &mut impl CclIo, expected: &str, value: u8) -> Result<CclSaved, String> {
    let info = announces(dev)?;
    match dev.master_serial() {
        Some(s) if s == expected => {}
        _ => return Err(uart1_refusal("board-changed")),
    }
    if let Some(e) = not_ready(Channel::Uart0, &facts_of(&info, None)) {
        return Err(e);
    }
    set_and_read(dev, value).map(|step| CclSaved { step, serial: expected.to_string() })
}

/// One 64/08, never repeated, then one separate 64/07.
fn set_and_read(dev: &mut impl CclIo, value: u8) -> Result<CclStep, String> {
    match dev.ccl_set(MASTER, value) {
        // acknowledged with the value: confirmed only by the separate read
        Ok(_) => match dev.ccl_read(MASTER) {
            Ok(back) if back.down == value => Ok(back),
            Ok(back) => Err(refusal("ccl-readback-differs", Some(back.down))),
            Err(_) => Err(refusal("ccl-readback-failed", None)),
        },
        Err(f) if !f.sent => Err(crate::error_text(f.error)),
        Err(f) => {
            let unclear = f.unclear();
            let key = match f.error {
                openbms_transport::Error::Proto(openbms_proto::Error::Rtn(rtn)) if !unclear => match ccl::Refusal::of_rtn(rtn) {
                    Some(ccl::Refusal::InvalidArgument) => "ccl-invalid",
                    Some(ccl::Refusal::WriteDenied) => "ccl-denied",
                    Some(ccl::Refusal::Persistence) => "ccl-persist",
                    None => "ccl-unknown",
                },
                _ => "ccl-unknown",
            };
            // what the master holds now, read once (never a second set); nothing when that read fails
            Err(refusal(key, dev.ccl_read(MASTER).ok().map(|s| s.down)))
        }
    }
}

/// The installation's current board at address 0, which `expected` (the String 2 the shown value was read with) must be:
/// a value of a former master is never written.
fn expected_board(state: &AppState, inst: &str, expected: &str) -> Result<String, String> {
    let stored = crate::maint::with_store(state, |s| Ok(s.current_board(inst, MASTER)?.map(|b| b.bms_serial))).map_err(|e| format!("{e:?}"))?;
    let stored = stored.ok_or_else(|| uart1_refusal("no-board"))?;
    if stored != expected {
        return Err(uart1_refusal("board-changed"));
    }
    Ok(stored)
}

/// One write of the setting over `channel` (`on_bus`); the caller holds the master's reservation.
pub(crate) fn run(state: &AppState, link: &Link, channel: Channel, value: u8, expected: &str) -> Result<CclSaved, String> {
    let inst = link.installation.as_deref().ok_or("view only: writing to a pack needs a connection with an installation")?;
    if channel == Channel::Uart0 {
        let stored = expected_board(state, inst, expected)?;
        // the right-bank check of every RS485-1/2 write: a polling master stays silent and nothing goes out
        let checked = crate::bank::check_target(state, link, MASTER)?;
        if checked.unchecked {
            return Err(uart1_refusal("unchecked"));
        }
        return with_link(&state.conn, &state.flashing, &state.link, link, |c| Ok(write_session_uart0(&mut Ascii { c }, &stored, value)))?;
    }
    // the capability this connection proved for the master (its profile); the session asks it fresh again
    let fw = crate::profile::access_facts(crate::profile::pack_patch(state, link, MASTER, None).as_ref(), None, None, MASTER);
    if let Some(e) = not_ready(Channel::Uart1, &fw) {
        return Err(e);
    }
    let stored = expected_board(state, inst, expected)?;
    let (inst, facts) = crate::uart1_id::prepare(state, link, MASTER, Some(&stored))?;
    let session = crate::uart1_id::run_session(state, link, MASTER, |c| write_session(&mut Conn { c, bus: Bus::CanSocketBus }, &inst, &stored, &facts, value))?;
    session.result.unwrap_or_else(|| Err("no result".into()))
}

/// „Lesen“: the master's setting as it is stored now, over the connection's bus. Needs no installation and no release
/// bit.
#[tauri::command]
pub async fn ccl_step_read(state: tauri::State<'_, AppState>) -> Result<CclRead, String> {
    let PackAction { guard: _guard, link, bus } = PackAction::read(&state, MASTER, "ccl_read")?;
    let channel = on_bus(Action::ReadCclStep, bus)?;
    with_link(&state.conn, &state.flashing, &state.link, &link, |c| {
        Ok(if channel == Channel::Uart0 { read_session(&mut Ascii { c }) } else { read_session(&mut Conn { c, bus: Bus::CanSocketBus }) })
    })?
}

/// „Speichern“: one explicit set of the master's setting (`value` 1..=20, an integer), read back separately.
#[tauri::command]
pub async fn ccl_step_write(state: tauri::State<'_, AppState>, installation: Option<String>, value: serde_json::Value, expected: String) -> Result<CclSaved, String> {
    let value = step_of(&value)?;
    let PackAction { guard: _guard, link, bus } = PackAction::write(&state, installation.as_deref(), MASTER, "ccl_write")?;
    let channel = on_bus(Action::WriteCclStep, bus)?;
    run(&state, &link, channel, value, &expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uart1_id::tests::{block, board, facts, raw, with_group0, Bank};
    use openbms_proto::{Parameters, SerialValue};
    use openbms_transport::Error as E;
    use std::time::Duration;

    const FULL_167: &str = "4558540510060D010607F445666C205184A8FE001F";
    const FULL_166: &str = "4558540510060D0106062508B93F45C29F31FE000F";
    const LIGHT_156: &str = "4558540510060D010506FE7CBA82F861F3373A000F";

    fn info(hex: &str) -> ExtInfo {
        ExtInfo::parse(&(0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect::<Vec<_>>()).unwrap()
    }

    fn step(down: u8) -> CclStep {
        CclStep { down, up: 1, min: 1, max: 20 }
    }

    /// The master behind the CAN socket: its binary side (`Bank`), its 64/03, the answers to 64/07 (in order) and to
    /// 64/08, and what went out.
    struct Script {
        bank: Bank,
        info: Option<openbms_transport::Result<ExtInfo>>,
        reads: Vec<openbms_transport::Result<CclStep>>,
        set: Option<Result<CclStep, SwitchFailure>>,
        sent: Vec<String>,
        channel: Channel,
    }

    impl Dev for Script {
        fn serial(&mut self, address: u8, cmd: u8) -> openbms_transport::Result<Vec<u8>> {
            self.bank.serial(address, cmd)
        }
        fn params(&mut self, address: u8) -> openbms_transport::Result<Parameters> {
            self.sent.push("47".into());
            self.bank.params(address)
        }
        fn write(&mut self, address: u8, block: &[u8]) -> openbms_transport::Result<()> {
            self.bank.write(address, block)
        }
        fn master(&mut self) -> openbms_transport::Result<SerialValue> {
            self.bank.master()
        }
    }

    impl CclIo for Script {
        fn channel(&self) -> Channel {
            self.channel
        }
        fn master_serial(&mut self) -> Option<String> {
            if self.channel == Channel::Uart1 {
                return self.bank.serial(MASTER, uart1::CMD_STRING2).ok().and_then(|raw| serial::canonical(&raw));
            }
            self.sent.push("A4".into());
            self.bank.strings2.get(&MASTER).cloned().flatten().and_then(|raw| serial::canonical(&raw))
        }
        fn ext_info(&mut self, address: u8) -> openbms_transport::Result<ExtInfo> {
            assert_eq!(address, MASTER, "only the master is asked");
            self.sent.push("64/03".into());
            self.info.take().unwrap_or(Err(E::Timeout(Duration::from_millis(1))))
        }
        fn ccl_read(&mut self, address: u8) -> openbms_transport::Result<CclStep> {
            assert_eq!(address, MASTER);
            self.sent.push("64/07".into());
            if self.reads.is_empty() { Err(E::Timeout(Duration::from_millis(1))) } else { self.reads.remove(0) }
        }
        fn ccl_set(&mut self, address: u8, value: u8) -> Result<CclStep, SwitchFailure> {
            assert_eq!(address, MASTER);
            self.sent.push(format!("64/08 {value:02X}"));
            self.set.take().expect("one set")
        }
    }

    /// Master "SYN-M" at address 0, Full 1.6.7, Group0 `g0`; the set answers `set`, the 64/07 reads `reads`.
    fn script(g0: u8, set: Result<CclStep, SwitchFailure>, reads: Vec<openbms_transport::Result<CclStep>>) -> Script {
        let mut bank = Bank::default();
        bank.strings2.insert(0, Some(raw("SYN-M")));
        bank.reads = vec![Ok(with_group0(&block(0, 3600), g0))];
        Script { bank, info: Some(Ok(info(FULL_167))), reads, set: Some(set), sent: vec![], channel: Channel::Uart1 }
    }

    fn save_as(d: &mut Script, value: u8) -> Session<CclSaved> {
        write_session(d, "A", "SYN-M", &facts(vec![board(0, "SYN-M", Some(true))]), value)
    }

    fn err(s: &Session<CclSaved>) -> serde_json::Value {
        serde_json::from_str(s.result.as_ref().unwrap().as_ref().unwrap_err()).unwrap()
    }

    #[test]
    fn the_value_is_an_integer_1_to_20_never_rounded() {
        use serde_json::json;
        for (v, want) in [(json!(1), Some(1)), (json!(10), Some(10)), (json!(20), Some(20))] {
            assert_eq!(step_of(&v).ok(), want, "{v}");
        }
        for bad in [json!(0), json!(21), json!(-1), json!(10.5), json!(10.0), json!(true), json!(false), json!("10"), json!(null), json!([10]), json!({ "v": 10 }), json!(256), json!(266)] {
            assert_eq!(step_of(&bad), Err(uart1_refusal("ccl-bad-value")), "{bad}");
        }
    }

    #[test]
    fn a_save_goes_out_once_after_identity_capability_and_bit_0_then_reads_back() {
        for value in [1u8, 10, 20] {
            let mut d = script(0x01, Ok(step(value)), vec![Ok(step(value))]);
            let s = save_as(&mut d, value);
            assert_eq!(s.result, Some(Ok(CclSaved { step: step(value), serial: "SYN-M".into() })));
            assert_eq!(d.sent, ["64/03", "47", &format!("64/08 {value:02X}"), "64/07"]);
            // the identity first: String 2 of the master, nothing else asked of the binary service but the fresh 0x47
            assert_eq!(d.bank.asked, [(0, uart1::CMD_STRING2), (0, uart1::CMD_READ)]);
            assert!(d.bank.written.is_empty(), "no parameter block goes out");
        }
    }

    #[test]
    fn nothing_is_set_without_bit_0_the_capability_or_the_expected_board() {
        // Group0 bit 0 off: refused here, nothing sent
        let mut d = script(0xFE, Ok(step(10)), vec![]);
        assert_eq!(err(&save_as(&mut d, 10))["error"], "write-locked");
        assert!(!d.sent.iter().any(|s| s.starts_with("64/08")));
        // Full 1.6.6 and Light 1.5.6 (mask 000F), an unanswered 64/03
        for (i, key) in [(Some(Ok(info(FULL_166))), "ccl-no-cap"), (Some(Ok(info(LIGHT_156))), "ccl-no-cap"), (None, "ccl-no-answer")] {
            let mut d = script(0xFF, Ok(step(10)), vec![]);
            d.info = i;
            assert_eq!(err(&save_as(&mut d, 10))["error"], key);
            assert!(!d.sent.iter().any(|s| s.starts_with("64/08") || s == "47"));
        }
        // another board at address 0 than the one the value was read with
        let mut d = script(0xFF, Ok(step(10)), vec![]);
        d.bank.strings2.insert(0, Some(raw("SYN-OTHER")));
        assert_eq!(err(&save_as(&mut d, 10))["error"], "board-changed");
        assert!(d.sent.is_empty());
        // a block of another address says nothing about the master's bit 0
        let mut d = script(0xFF, Ok(step(10)), vec![]);
        d.bank.reads = vec![Ok(with_group0(&block(1, 3600), 0xFF))];
        assert!(save_as(&mut d, 10).result.unwrap().unwrap_err().contains("answered with address 1"));
        assert!(!d.sent.iter().any(|s| s.starts_with("64/08")));
    }

    #[test]
    fn refusals_are_named_and_followed_by_one_read_never_a_second_set() {
        for (rtn, key) in [(0x05, "ccl-invalid"), (0x04, "ccl-denied"), (0x08, "ccl-persist")] {
            let mut d = script(0xFF, Err(SwitchFailure { sent: true, error: E::Proto(openbms_proto::Error::Rtn(rtn)) }), vec![Ok(step(1))]);
            let e = err(&save_as(&mut d, 10));
            assert_eq!((e["error"].as_str(), e["now"].as_u64()), (Some(key), Some(1)), "{rtn:02X}");
            assert_eq!(d.sent.iter().filter(|s| s.starts_with("64/08")).count(), 1);
        }
    }

    #[test]
    fn silence_or_a_malformed_answer_leaves_the_outcome_open() {
        for error in [E::Timeout(Duration::from_millis(1500)), E::Incomplete, E::Proto(openbms_proto::Error::Layout("x")), E::Closed] {
            // the read after it finds the new value: still open, not success
            let mut d = script(0xFF, Err(SwitchFailure { sent: true, error }), vec![Ok(step(10))]);
            let e = err(&save_as(&mut d, 10));
            assert_eq!((e["error"].as_str(), e["now"].as_u64()), (Some("ccl-unknown"), Some(10)));
            assert_eq!(d.sent.iter().filter(|s| s.starts_with("64/08")).count(), 1, "never repeated");
        }
        // and when that read fails too, no value is claimed
        let mut d = script(0xFF, Err(SwitchFailure { sent: true, error: E::Timeout(Duration::from_millis(1)) }), vec![]);
        assert!(err(&save_as(&mut d, 10))["now"].is_null());
        // the request never left: nothing sent, no read-back
        let mut d = script(0xFF, Err(SwitchFailure { sent: false, error: E::Closed }), vec![]);
        assert_eq!(save_as(&mut d, 10).result.unwrap().unwrap_err(), E::Closed.to_string());
        assert!(!d.sent.contains(&"64/07".to_string()));
    }

    #[test]
    fn the_read_back_must_show_the_value() {
        let mut d = script(0xFF, Ok(step(10)), vec![Ok(step(1))]);
        let e = err(&save_as(&mut d, 10));
        assert_eq!((e["error"].as_str(), e["now"].as_u64()), (Some("ccl-readback-differs"), Some(1)));
        let mut d = script(0xFF, Ok(step(10)), vec![Err(E::Timeout(Duration::from_millis(1)))]);
        assert_eq!(err(&save_as(&mut d, 10))["error"], "ccl-readback-failed");
    }

    #[test]
    fn a_read_shows_only_a_confirmed_value() {
        let mut d = script(0x00, Ok(step(1)), vec![Ok(step(7))]);
        assert_eq!(read_session(&mut d), Ok(CclRead { step: step(7), serial: Some("SYN-M".into()) }));
        assert_eq!(d.sent, ["64/03", "64/07"]);
        // silence, a malformed answer, a refusal: an error, never the default 1
        for (r, key) in [(E::Timeout(Duration::from_millis(1)), "ccl-no-answer"), (E::Proto(openbms_proto::Error::Layout("x")), "ccl-malformed"), (E::Proto(openbms_proto::Error::Rtn(4)), "ccl-read-refused")] {
            let mut d = script(0x00, Ok(step(1)), vec![Err(r)]);
            let e: serde_json::Value = serde_json::from_str(&read_session(&mut d).unwrap_err()).unwrap();
            assert_eq!(e["error"], key);
        }
        // a build without bit 4 is not asked 64/07
        let mut d = script(0x00, Ok(step(1)), vec![Ok(step(1))]);
        d.info = Some(Ok(info(FULL_166)));
        assert_eq!(read_session(&mut d).unwrap_err(), uart1_refusal("ccl-no-cap"));
        assert_eq!(d.sent, ["64/03"]);
        // a master without a checkable String 2: the value, but no serial to write against
        let mut d = script(0x00, Ok(step(1)), vec![Ok(step(3))]);
        d.bank.strings2.insert(0, None);
        assert_eq!(read_session(&mut d).unwrap().serial, None);
    }

    #[test]
    fn the_profile_check_before_the_session() {
        let f = |caps: Option<u16>, g0: Option<u8>| access::Facts { caps, group0: g0, ..facts_of(&info(FULL_167), None) };
        assert_eq!(not_ready(Channel::Uart1, &f(Some(0x1F), None)), None, "bit 0 not read yet: the session reads it");
        assert_eq!(not_ready(Channel::Uart1, &f(Some(0x1F), Some(0x01))), None);
        assert_eq!(not_ready(Channel::Uart1, &f(Some(0x1F), Some(0xFE))), Some(uart1_refusal("write-locked")));
        assert_eq!(not_ready(Channel::Uart1, &f(Some(0x0F), None)), Some(uart1_refusal("ccl-no-cap")));
        // a stored proof carries no mask: not proven on this connection, not offered
        assert_eq!(not_ready(Channel::Uart1, &f(None, None)), Some(uart1_refusal("ccl-no-cap")));
    }

    #[test]
    fn without_the_capability_on_this_link_nothing_is_written() {
        let state = AppState::default();
        crate::new_link(&state, Some("A".into()));
        let link = crate::link_now(&state);
        crate::bank::begin(&state, link.gen, false, None);
        // no master profile on this link: refused before any frame, also with the right serial
        assert_eq!(run(&state, &link, Channel::Uart1, 10, "SYN-M").unwrap_err(), uart1_refusal("ccl-no-cap"));
        // „Nur ansehen“ writes nothing
        let view = Link { installation: None, ..link };
        assert!(run(&state, &view, Channel::Uart1, 10, "SYN-M").unwrap_err().contains("view only"));
    }

    /// The master on RS485-1/2: the same script, the ASCII side only (no binary service, no Group0 read).
    fn uart0(set: Result<CclStep, SwitchFailure>, reads: Vec<openbms_transport::Result<CclStep>>) -> Script {
        Script { channel: Channel::Uart0, bank: { let mut b = Bank::default(); b.strings2.insert(0, Some(raw("SYN-M"))); b }, ..script(0xFF, set, reads) }
    }

    #[test]
    fn over_rs485_a_save_needs_no_release_bit_and_goes_out_once() {
        for value in [1u8, 10, 20] {
            let mut d = uart0(Ok(step(value)), vec![Ok(step(value))]);
            assert_eq!(write_session_uart0(&mut d, "SYN-M", value), Ok(CclSaved { step: step(value), serial: "SYN-M".into() }));
            // no binary 0x47: Group0 bit 0 is not a condition of this way
            assert_eq!(d.sent, ["64/03", "A4", &format!("64/08 {value:02X}"), "64/07"]);
        }
        // the release bit off changes nothing here (it gates only the CAN socket)
        let fw = access::Facts { group0: Some(0xFE), ..facts_of(&info(FULL_167), None) };
        assert_eq!((not_ready(Channel::Uart0, &fw), not_ready(Channel::Uart1, &fw)), (None, Some(uart1_refusal("write-locked"))));
        assert_eq!(not_ready(Channel::Uart0, &facts_of(&info(FULL_166), None)), Some(uart1_refusal("ccl-no-cap")));
    }

    #[test]
    fn over_rs485_a_polling_master_gets_nothing() {
        // a master polling its slaves does not answer on RS485-1/2: silence, nothing that changes it is sent
        let mut d = uart0(Ok(step(10)), vec![]);
        d.info = None;
        assert_eq!(write_session_uart0(&mut d, "SYN-M", 10), Err(uart1_refusal("ccl-silent")));
        assert_eq!(d.sent, ["64/03"]);
        let mut d = uart0(Ok(step(10)), vec![]);
        d.info = None;
        assert_eq!(read_session(&mut d), Err(uart1_refusal("ccl-silent")));
        // another board at address 0, or none readable: refused before the set
        for s2 in [Some(raw("SYN-OTHER")), None] {
            let mut d = uart0(Ok(step(10)), vec![]);
            d.bank.strings2.insert(0, s2);
            assert_eq!(write_session_uart0(&mut d, "SYN-M", 10), Err(uart1_refusal("board-changed")));
            assert!(!d.sent.iter().any(|s| s.starts_with("64/08")));
        }
        // the refusals and the open outcome as on the CAN socket, never repeated
        let mut d = uart0(Err(SwitchFailure { sent: true, error: E::Proto(openbms_proto::Error::Rtn(0x08)) }), vec![Ok(step(1))]);
        let e: serde_json::Value = serde_json::from_str(&write_session_uart0(&mut d, "SYN-M", 10).unwrap_err()).unwrap();
        assert_eq!((e["error"].as_str(), e["now"].as_u64()), (Some("ccl-persist"), Some(1)));
        let mut d = uart0(Err(SwitchFailure { sent: true, error: E::Timeout(Duration::from_millis(1)) }), vec![]);
        let e: serde_json::Value = serde_json::from_str(&write_session_uart0(&mut d, "SYN-M", 10).unwrap_err()).unwrap();
        assert_eq!(e["error"], "ccl-unknown");
        assert_eq!(d.sent.iter().filter(|s| s.starts_with("64/08")).count(), 1);
        // a confirmed read over RS485-1/2
        let mut d = uart0(Ok(step(1)), vec![Ok(step(4))]);
        assert_eq!(read_session(&mut d), Ok(CclRead { step: step(4), serial: Some("SYN-M".into()) }));
    }

    #[test]
    fn only_the_explicit_save_sets_the_step() {
        // no connect, reconnect, role change, restore, parameter or multi-pack path reaches the set: it is called here only
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") && path.file_name().unwrap() != "ccl.rs" {
                let src = std::fs::read_to_string(&path).unwrap();
                for call in ["set_ccl_step", "ccl_set(", "ccl::run(", "ccl::write_session", "ccl_step_write("] {
                    assert!(!src.contains(call), "{} calls {call}", path.display());
                }
            }
        }
    }
}
