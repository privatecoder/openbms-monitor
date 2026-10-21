//! Ein/aus, restart and the clock over the CAN socket (MAINT 1.5 with image flag bit 1): the same ASCII 0x45/0x4D/0x4E
//! as on RS485-1/2, answered by a patched pack at its own address. Offered only where this connection proved the flag for
//! exactly this pack (`profile::power_rtc`), never from a stored proof or a test send.
//!
//! A command that changes something first runs the identity check of a parameter write (`uart1_id::identity`, with the
//! installation's current board at the address as the expected String 2), then a fresh binary 0x47: Group0 bit 3 set
//! (locked) sends nothing. Identity, lock and command run in one port session under the pack's reservation with host
//! polling held. Reading the clock needs neither. A restart is sent once, never repeated; its comeback is watched after
//! the session (`control::pack_restart`).
//!
//! MAINT 1.6 keeps the same 0x45: with Group0 bit 4 at 0 an addressed off stores the pack cutoff, an addressed on clears
//! it (whatever bit 4). RTN 08 to ein/aus then means the lock could not be stored or verified (`switch_error`); the UI
//! reads 0x47 and the status again after every ein/aus.

use crate::bms_memory::{refusal_error, uart1_error, uart1_pending, uart1_refusal};
use crate::uart1_id::{identity, keep, Conn, Dev, Expected, Facts, Session};
use crate::{with_link, AppState, Link};
use openbms_proto::{BmsTime, Refusal};
use openbms_transport::Bus;
use std::time::Instant;

/// The ASCII exchanges of the power and clock service besides the binary ones; the connection, or a script in tests.
pub(crate) trait PowerDev: Dev {
    fn on_off(&mut self, address: u8, on: bool) -> Result<(), openbms_transport::SwitchFailure>;
    /// returns when the restart was sent (no answer expected)
    fn restart(&mut self, address: u8) -> openbms_transport::Result<Instant>;
    fn clock(&mut self, address: u8) -> openbms_transport::Result<BmsTime>;
    fn set_clock(&mut self, address: u8, time: BmsTime) -> openbms_transport::Result<()>;
}

impl PowerDev for Conn<'_> {
    fn on_off(&mut self, address: u8, on: bool) -> Result<(), openbms_transport::SwitchFailure> {
        self.c.on_off_phased(address, on)
    }
    fn restart(&mut self, address: u8) -> openbms_transport::Result<Instant> {
        self.c.restart(address)
    }
    fn clock(&mut self, address: u8) -> openbms_transport::Result<BmsTime> {
        self.c.clock(address)
    }
    fn set_clock(&mut self, address: u8, time: BmsTime) -> openbms_transport::Result<()> {
        self.c.set_clock(address, time)
    }
}

/// A command that changes the pack. No heater, clearing or remaining capacity: the patch offers none of them here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Act {
    /// `expect`: for off, the extended off (Group0 bit 4 at 0) the confirmation showed; a fresh read that differs sends
    /// nothing (`cutoff-changed`)
    Switch { on: bool, expect: Option<bool> },
    Restart,
    SetClock(BmsTime),
}

/// The way an ein/aus, restart or clock command takes on the bus of its link (`bms_memory::bound_bus`): this service on
/// the CAN socket, the RS485-1/2 one (right-bank check, 0x51) there. Never one in place of the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    Power,
    Ascii,
}

pub(crate) fn way(action: openbms_proto::access::Action, bus: Bus) -> Result<Way, String> {
    Ok(match crate::bms_memory::on_bus(action, bus)? {
        openbms_proto::access::Channel::Uart1 => Way::Power,
        _ => Way::Ascii,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Done {
    Switched,
    /// sent at this instant, the start of the silence the comeback watch measures
    Restarted(Instant),
    /// the clock read back after setting it
    Clock(BmsTime),
}

/// The errors of the service as JSON for the UI (`uart1Error` in control.ts): RTN 05 (locked meanwhile, or data the
/// pack does not take) as `power-locked` with the code, RTN 08 `rtc-failed` (the clock chip did not answer), silence
/// `power-no-answer` (whether the command took effect shows the next read); RTN 04/06 as the refusals of RS485-1/2.
pub(crate) fn power_error(e: openbms_transport::Error) -> String {
    match e {
        openbms_transport::Error::Proto(openbms_proto::Error::Rtn(0x05)) => serde_json::json!({ "kind": "uart1", "error": "power-locked", "code": 5 }).to_string(),
        openbms_transport::Error::Proto(openbms_proto::Error::Rtn(0x08)) => uart1_refusal("rtc-failed"),
        openbms_transport::Error::Timeout(_) | openbms_transport::Error::Incomplete => uart1_refusal("power-no-answer"),
        openbms_transport::Error::Proto(p) => Refusal::from_error(&p).map(refusal_error).unwrap_or_else(|| openbms_transport::Error::Proto(p).to_string()),
        e => e.to_string(),
    }
}

/// Ein/aus of a build with the stored pack cutoff (MAINT 1.6) answers RTN 08 when storing or verifying the lock failed
/// (`control::cutoff_failed`): never a success or a release. A request that left and then lost its link (gateway
/// closed, I/O) is open like silence (`power-lost`); every other error as `power_error`.
pub(crate) fn switch_error(f: openbms_transport::SwitchFailure, on: bool) -> String {
    match f.error {
        openbms_transport::Error::Proto(openbms_proto::Error::Rtn(0x08)) => uart1_refusal(crate::control::cutoff_failed(on)),
        openbms_transport::Error::Timeout(_) | openbms_transport::Error::Incomplete => power_error(f.error),
        _ if f.unclear() => uart1_refusal("power-lost"),
        e => power_error(e),
    }
}

/// The port session of one command: the strict identity, a fresh binary 0x47 with Group0 bit 3 clear, the command (for
/// the clock its read-back). Every refusal before the command sends nothing to the pack.
pub(crate) fn session(dev: &mut impl PowerDev, inst: &str, address: u8, expected: &str, facts: &Facts, act: Act) -> Session<Done> {
    let id = identity(dev, inst, address, Expected::Board(expected), facts);
    let mut out = Session { gone: id.gone, unconfirmed: id.unconfirmed, ..Default::default() };
    let serial = match id.serial {
        Ok(s) => s,
        Err(e) => {
            out.result = Some(Err(e));
            return out;
        }
    };
    out.serial = Some(serial);
    out.result = Some(command(dev, address, act));
    out
}

fn command(dev: &mut impl PowerDev, address: u8, act: Act) -> Result<Done, String> {
    let fresh = dev.params(address).map_err(uart1_error)?;
    // byte 0 is the pack's own address: the lock of another pack says nothing here
    if fresh.address != address {
        return Err(format!("the pack at address {address} answered with address {}", fresh.address));
    }
    if crate::profile::power_locked(fresh.function_switches[0]) {
        return Err(uart1_refusal("power-locked"));
    }
    match act {
        // a soft off confirmed must not become a stored cutoff, nor the reverse
        Act::Switch { on: false, expect } if crate::profile::extended_changed(expect, fresh.function_switches[0]) => Err(uart1_refusal("cutoff-changed")),
        Act::Switch { on, .. } => dev.on_off(address, on).map(|_| Done::Switched).map_err(|e| switch_error(e, on)),
        // once, no retry: a timeout proves nothing either way
        Act::Restart => dev.restart(address).map(Done::Restarted).map_err(power_error),
        Act::SetClock(time) => {
            dev.set_clock(address, time).map_err(power_error)?;
            dev.clock(address).map(Done::Clock).map_err(power_error)
        }
    }
}

/// The clock of a pack over the CAN socket, free like on RS485-1/2 (no identity, no lock), only with the image flag.
pub(crate) fn clock(state: &AppState, link: &Link, address: u8) -> Result<BmsTime, String> {
    if !crate::profile::power_rtc(state, link, address) {
        return Err(uart1_refusal("power-unsupported"));
    }
    with_link(&state.conn, &state.flashing, &state.link, link, |c| Ok(c.clock(address).map_err(power_error)))?
}

/// One command over the CAN socket; the caller holds the pack's reservation (a restart until its comeback watch ends).
/// Refused without the image flag, without a current board recorded at the address, or with an open interrupted update
/// there; the store's facts first, then the port session with polling held, then what the store keeps.
pub(crate) fn run(state: &AppState, link: &Link, address: u8, act: Act) -> Result<Done, String> {
    let inst = link.installation.clone().ok_or("view only: writing to a pack needs a connection with an installation")?;
    if !crate::profile::power_rtc(state, link, address) {
        return Err(uart1_refusal("power-unsupported"));
    }
    uart1_pending(state, link, address)?;
    let (expected, facts) = crate::maint::with_store(state, |s| {
        let expected = s.current_board(&inst, address)?.map(|b| b.bms_serial);
        let facts = Facts {
            refs: s.current_refs(&inst)?,
            master: s.current_board(&inst, 0)?.and_then(|b| b.battery_serial),
            places: match &expected {
                Some(e) => s.serial_places(e)?,
                None => vec![],
            },
            // no parameter block goes out here
            group0: crate::profile::group0_rule(true, None),
        };
        Ok((expected, facts))
    })
    .map_err(|e| format!("{e:?}"))?;
    let expected = expected.ok_or_else(|| uart1_refusal("no-board"))?;
    let session = {
        let _pause = crate::PollPause::hold(&state.params_pause);
        with_link(&state.conn, &state.flashing, &state.link, link, |c| Ok(session(&mut Conn { c, bus: Bus::CanSocketBus }, &inst, address, &expected, &facts, act)))?
    };
    keep(state, link, address, &session);
    session.result.unwrap_or_else(|| Err("no result".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uart1_id::tests::{block, board, facts, raw, with_group0, Bank};
    use openbms_proto::{uart1, Parameters, SerialValue};
    use openbms_transport::Error as E;

    /// The binary side of a bank (`Bank`) and the answers of the ASCII commands, with what was sent.
    struct Script {
        bank: Bank,
        answer: Option<openbms_transport::Result<()>>,
        read_back: Option<openbms_transport::Result<BmsTime>>,
        sent: Vec<&'static str>,
    }

    impl Dev for Script {
        fn serial(&mut self, address: u8, cmd: u8) -> openbms_transport::Result<Vec<u8>> {
            self.bank.serial(address, cmd)
        }
        fn params(&mut self, address: u8) -> openbms_transport::Result<Parameters> {
            self.bank.params(address)
        }
        fn write(&mut self, address: u8, block: &[u8]) -> openbms_transport::Result<()> {
            self.bank.write(address, block)
        }
        fn master(&mut self) -> openbms_transport::Result<SerialValue> {
            self.bank.master()
        }
    }

    impl PowerDev for Script {
        fn on_off(&mut self, _address: u8, on: bool) -> Result<(), openbms_transport::SwitchFailure> {
            self.sent.push(if on { "on" } else { "off" });
            self.answer.take().expect("one command").map_err(|error| openbms_transport::SwitchFailure { sent: true, error })
        }
        fn restart(&mut self, _address: u8) -> openbms_transport::Result<Instant> {
            self.sent.push("restart");
            self.answer.take().expect("one command").map(|_| Instant::now())
        }
        fn clock(&mut self, _address: u8) -> openbms_transport::Result<BmsTime> {
            self.sent.push("clock");
            self.read_back.take().expect("one read-back")
        }
        fn set_clock(&mut self, _address: u8, _time: BmsTime) -> openbms_transport::Result<()> {
            self.sent.push("set_clock");
            self.answer.take().expect("one command")
        }
    }

    const TIME: BmsTime = BmsTime { year: 2026, month: 3, day: 14, hour: 9, minute: 26, second: 53 };

    /// Address 1 holds "SYN-1", canonical and current; its Group0 is `g0`; the command is answered `answer`.
    fn script(g0: u8, answer: openbms_transport::Result<()>) -> Script {
        let mut bank = Bank::default();
        bank.strings2.insert(1, Some(raw("SYN-1")));
        bank.reads = vec![Ok(with_group0(&block(1, 3600), g0))];
        Script { bank, answer: Some(answer), read_back: Some(Ok(TIME)), sent: vec![] }
    }

    fn run(d: &mut Script, act: Act) -> Session<Done> {
        session(d, "A", 1, "SYN-1", &facts(vec![board(1, "SYN-1", Some(true))]), act)
    }

    fn err(s: &Session<Done>) -> serde_json::Value {
        serde_json::from_str(s.result.as_ref().unwrap().as_ref().unwrap_err()).unwrap()
    }

    #[test]
    fn released_commands_go_out_after_identity_and_a_fresh_lock_read() {
        let mut d = script(0xF7, Ok(()));
        let s = run(&mut d, Act::Switch { on: false, expect: None });
        assert_eq!(s.result, Some(Ok(Done::Switched)));
        assert_eq!(d.bank.asked, [(1, uart1::CMD_STRING2), (1, uart1::CMD_READ)]);
        assert_eq!(d.sent, ["off"]);
        assert_eq!(s.serial.as_deref(), Some("SYN-1"));
        // the clock: set, then read back
        let mut d = script(0xF7, Ok(()));
        assert_eq!(run(&mut d, Act::SetClock(TIME)).result, Some(Ok(Done::Clock(TIME))));
        assert_eq!(d.sent, ["set_clock", "clock"]);
    }

    #[test]
    fn a_locked_pack_gets_nothing() {
        // the old FF parameters lock power and clock (bit 3 set)
        for act in [Act::Switch { on: true, expect: None }, Act::Restart, Act::SetClock(TIME)] {
            let mut d = script(0xFF, Ok(()));
            let s = run(&mut d, act);
            assert_eq!(err(&s)["error"], "power-locked");
            assert!(err(&s).get("code").is_none(), "refused here, not by the pack");
            assert!(d.sent.is_empty(), "{act:?}");
        }
        // a block of another address says nothing about this pack's lock
        let mut d = script(0xF7, Ok(()));
        d.bank.reads = vec![Ok(with_group0(&block(2, 3600), 0xF7))];
        assert!(run(&mut d, Act::Restart).result.unwrap().unwrap_err().contains("answered with address 2"));
        assert!(d.sent.is_empty());
    }

    #[test]
    fn every_identity_refusal_comes_before_anything_is_sent() {
        let check = |d: &mut Script, s: Session<Done>, e: &str| {
            assert_eq!(err(&s)["error"], e);
            assert!(d.sent.is_empty() && !d.bank.asked.contains(&(1, uart1::CMD_READ)), "{e}");
        };
        // another board than the installation's at this address
        let mut d = script(0xF7, Ok(()));
        d.bank.strings2.insert(1, Some(raw("SYN-OTHER")));
        let s = run(&mut d, Act::Switch { on: true, expect: None });
        check(&mut d, s, "board-changed");
        // no String 2, silence, no identity
        let mut d = script(0xF7, Ok(()));
        d.bank.strings2.insert(1, None);
        let s = run(&mut d, Act::Restart);
        check(&mut d, s, "no-identity");
        let mut d = script(0xF7, Ok(()));
        d.bank.strings2.remove(&1);
        let s = run(&mut d, Act::Restart);
        check(&mut d, s, "no-answer");
        let mut d = script(0xF7, Ok(()));
        d.bank.strings2.insert(1, Some(raw("EMU110110S-XXXX-1")));
        let s = run(&mut d, Act::SetClock(TIME));
        check(&mut d, s, "unchecked");
        // the master behind the CAN socket is another bank's
        let mut d = script(0xF7, Ok(()));
        d.bank.master = Some(SerialValue::Set { value: "SYN-BAT-B".into(), truncated: false });
        let f = Facts { master: Some("SYN-BAT-A".into()), ..facts(vec![board(1, "SYN-1", Some(true))]) };
        let s = session(&mut d, "A", 1, "SYN-1", &f, Act::Switch { on: false, expect: None });
        check(&mut d, s, "other-master");
        // recorded before provenance: one confirmation first, noted for „Übernehmen“
        let mut d = script(0xF7, Ok(()));
        let s = session(&mut d, "A", 1, "SYN-1", &facts(vec![board(1, "SYN-1", None)]), Act::Restart);
        assert_eq!((err(&s)["kind"].as_str(), err(&s)["why"].as_str()), (Some("bank"), Some("legacy")));
        assert!(s.unconfirmed.is_some() && d.sent.is_empty());
        // the same board answers at another address too
        let mut d = script(0xF7, Ok(()));
        d.bank.strings2.insert(4, Some(raw("SYN-1")));
        let s = session(&mut d, "A", 1, "SYN-1", &facts(vec![board(1, "SYN-1", Some(true)), board(4, "SYN-1", Some(true))]), Act::Restart);
        check(&mut d, s, "duplicate");
    }

    #[test]
    fn rtn_05_and_08_are_named() {
        let mut d = script(0xF7, Err(E::Proto(openbms_proto::Error::Rtn(0x05))));
        let e = err(&run(&mut d, Act::Switch { on: true, expect: None }));
        assert_eq!((e["error"].as_str(), e["code"].as_u64()), (Some("power-locked"), Some(5)));
        let mut d = script(0xF7, Err(E::Proto(openbms_proto::Error::Rtn(0x08))));
        assert_eq!(err(&run(&mut d, Act::SetClock(TIME)))["error"], "rtc-failed");
        assert_eq!(d.sent, ["set_clock"], "no read-back after a refusal");
        // set, but the read-back fails at the clock chip
        let mut d = script(0xF7, Ok(()));
        d.read_back = Some(Err(E::Proto(openbms_proto::Error::Rtn(0x08))));
        assert_eq!(err(&run(&mut d, Act::SetClock(TIME)))["error"], "rtc-failed");
        // ein/aus: RTN 08 is a failed store of the pack cutoff, in both directions; RTN 05 stays the refusal
        for on in [false, true] {
            let mut d = script(0xE7, Err(E::Proto(openbms_proto::Error::Rtn(0x08))));
            assert_eq!(err(&run(&mut d, Act::Switch { on, expect: None }))["error"], crate::control::cutoff_failed(on));
        }
        let mut d = script(0xE7, Err(E::Proto(openbms_proto::Error::Rtn(0x05))));
        assert_eq!(err(&run(&mut d, Act::Switch { on: false, expect: None }))["code"], 5);
        // silence, RS485-1/2 refusals, a broken link
        assert_eq!(power_error(E::Timeout(std::time::Duration::from_millis(1500))), uart1_refusal("power-no-answer"));
        assert_eq!(power_error(E::Proto(openbms_proto::Error::Rtn(0x04))), r#"{"kind":"invalid"}"#);
        assert_eq!(power_error(E::Closed), E::Closed.to_string());
    }

    #[test]
    fn a_broken_link_after_sending_leaves_ein_aus_open() {
        for answer in [E::Closed, E::Io(std::io::Error::from(std::io::ErrorKind::ConnectionReset))] {
            let mut d = script(0xF7, Err(answer));
            assert_eq!(err(&run(&mut d, Act::Switch { on: false, expect: Some(false) }))["error"], "power-lost");
            assert_eq!(d.sent, ["off"], "sent once, never repeated");
        }
        let mut d = script(0xF7, Err(E::Timeout(std::time::Duration::from_millis(1500))));
        assert_eq!(err(&run(&mut d, Act::Switch { on: true, expect: None }))["error"], "power-no-answer");
        // the request never left: not executed
        let before = openbms_transport::SwitchFailure { sent: false, error: E::Closed };
        assert_eq!(switch_error(before, false), E::Closed.to_string());
    }

    #[test]
    fn a_changed_extended_off_sends_nothing() {
        // confirmed as soft off (bit 4 at 1), the pack has the extended off on now, and the reverse
        for (g0, expect) in [(0xE7, false), (0xF7, true)] {
            let mut d = script(g0, Ok(()));
            let s = run(&mut d, Act::Switch { on: false, expect: Some(expect) });
            assert_eq!(err(&s)["error"], "cutoff-changed", "{g0:02X}");
            assert!(d.sent.is_empty());
        }
        // as confirmed: sent
        for (g0, expect) in [(0xE7, true), (0xF7, false)] {
            let mut d = script(g0, Ok(()));
            assert_eq!(run(&mut d, Act::Switch { on: false, expect: Some(expect) }).result, Some(Ok(Done::Switched)));
            assert_eq!(d.sent, ["off"]);
        }
        // on releases whatever bit 4 shows
        let mut d = script(0xE7, Ok(()));
        assert_eq!(run(&mut d, Act::Switch { on: true, expect: Some(false) }).result, Some(Ok(Done::Switched)));
    }

    #[test]
    fn a_restart_is_sent_once_and_its_silence_is_no_failure() {
        let mut d = script(0xF7, Ok(()));
        assert!(matches!(run(&mut d, Act::Restart).result, Some(Ok(Done::Restarted(_)))));
        assert_eq!(d.sent, ["restart"]);
        // a refusal is reported, never repeated
        let mut d = script(0xF7, Err(E::Proto(openbms_proto::Error::Rtn(0x05))));
        assert_eq!(err(&run(&mut d, Act::Restart))["code"], 5);
        assert_eq!(d.sent, ["restart"]);
    }

    #[test]
    fn without_the_image_flag_on_this_link_nothing_is_offered() {
        let state = AppState::default();
        crate::new_link(&state, Some("A".into()));
        let link = crate::link_now(&state);
        crate::bank::begin(&state, link.gen, false, None);
        // a slave without a live 64/03 (a stored proof carries no image flag): refused before any frame
        assert_eq!(run_err(&state, &link, 3), uart1_refusal("power-unsupported"));
        assert_eq!(clock(&state, &link, 3).unwrap_err(), uart1_refusal("power-unsupported"));
        // a slave whose own 64/03 shows the flag passes this check (and fails later only for the missing store)
        let flagged = openbms_flash::maint::policy::PackFinding::of(openbms_flash::maint::dialog::PackAnswer::Info(openbms_proto::MaintInfo {
            format: 3, base: [16, 6, 13], patch: [1, 5], build_id: [7; 8], hard_stop: Some(false), power_rtc: true, pack_cutoff: false, reserved_flags: 0,
        }));
        crate::bank::set_live(&state, link.gen, 3, flagged);
        assert!(crate::profile::power_rtc(&state, &link, 3));
        assert_ne!(run_err(&state, &link, 3), uart1_refusal("power-unsupported"));
    }

    fn run_err(state: &AppState, link: &Link, address: u8) -> String {
        super::run(state, link, address, Act::Switch { on: true, expect: None }).unwrap_err()
    }
}
