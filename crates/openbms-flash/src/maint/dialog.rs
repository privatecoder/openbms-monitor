//! CID2 0x64 with the patched master on the CAN socket bus (UART1, 9600 baud): read its identity and state, set or
//! clear the marker, and wait until the master's next cycles show the change. A build with image flag bit 4 answers
//! status and clear also over RS485-1/2 at its own address (`dialog_at`).

use super::policy::Master;
use super::Clock;
use crate::{link::Link, Error, Result};
use openbms_proto::{frame, InfoAnswer, MaintInfo, MaintState, MarkerWord};
use openbms_transport::timing;
use std::time::Duration;

/// What the master at address 0 answered to `03`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// the format-2 or format-3 identity (any patch version; `policy::Master` decides what it offers)
    Info(MaintInfo),
    /// `MNT` with another format or length (the historic 22-byte format 1), or RTN 05: a patch the app does not support
    OtherPatch,
    /// RTN 04: the firmware does not know 0x64 (original or another build; RTN 04 does not prove which)
    No0x64,
    /// no valid answer; never read as original
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Status,
    Start,
    /// clear the marker; done once the master reports it off with `expected` effective slaves again (its DIP count)
    Stop { expected: u8 },
    /// clear the marker on a board alone (flashing one board): done once the master reports it off, whatever slave count
    /// its DIP switches give; the state returned is read after one more DIP cycle, so it shows whether the master polls
    Clear,
}

#[derive(Debug, Clone, Copy)]
pub struct Budgets {
    /// wait for one answer
    pub answer: Duration,
    /// from the valid start/stop reply until the master shows the new state
    pub change: Duration,
    /// pause between two status reads while waiting
    pub interval: Duration,
}

impl Default for Budgets {
    fn default() -> Self {
        Self { answer: timing::MAINT_ANSWER, change: timing::MAINT_CHANGE, interval: timing::MAINT_INTERVAL }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DialogError {
    #[error("connection: {0}")]
    Link(#[from] Error),
    #[error("the master does not know CID2 0x64 (RTN 04)")]
    Unsupported,
    #[error("the master rejected the request (RTN {0:02X})")]
    Rejected(u8),
    #[error("the master at address 0 does not answer")]
    NoAnswer,
    #[error("the master did not reach the requested state within {after:?}")]
    Timeout { after: Duration, last: MaintState },
}

/// Minimum time after `stop` when the slave count does not change (N = 0): one full master cycle.
pub const ZERO_SLAVES_SETTLE: Duration = timing::ZERO_SLAVES_SETTLE;

/// Which answers belong to the request sent: `03` gets 17, 18 or 19 bytes (or the historic 22, to name an unknown patch) or RTN
/// 04/05; `02`, start and stop get five bytes or a rejection. A late answer to an earlier request (other length), our own
/// echo (CID2 0x64 in the RTN field) and foreign traffic do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    Info,
    /// the state at this address
    Status(u8),
}

impl Bound {
    fn accepts(self, f: &frame::Frame) -> bool {
        let address = if let Bound::Status(a) = self { a } else { 0 };
        f.address == address
            && f.rtn != 0x64
            && match self {
                Bound::Info => (f.rtn == 0x00 && [MaintInfo::LEN, MaintInfo::LEN3, MaintInfo::LEN4, 22].contains(&f.info.len())) || f.rtn == 0x04 || f.rtn == 0x05,
                Bound::Status(_) => (f.rtn == 0x00 && f.info.len() == MaintState::LEN) || (0x01..=0x0F).contains(&f.rtn),
            }
    }
}

/// Send one request and return the first answer bound to it, or None after `timeout`.
fn exchange(link: &mut impl Link, clock: &impl Clock, request: &[u8], bound: Bound, timeout: Duration) -> Result<Option<frame::Frame>> {
    link.clear_input()?;
    link.write_raw(request)?;
    let deadline = clock.now() + timeout;
    let mut pending = Vec::new();
    let mut buf = [0u8; 256];
    while clock.now() < deadline {
        let n = link.read_raw(&mut buf)?;
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\r') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let Some(start) = line.iter().position(|b| *b == b'~') else { continue };
            if let Ok(f) = frame::parse(&line[start..]) {
                if bound.accepts(&f) {
                    return Ok(Some(f));
                }
            }
        }
        if pending.len() > 1024 {
            pending.clear();
        }
    }
    Ok(None)
}

/// One `02` status read (start/stop and the wait for their effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    Status(MaintState),
    Rtn(u8),
    NoAnswer,
}

fn reply_of(answer: Option<frame::Frame>) -> Result<Reply> {
    Ok(match answer {
        None => Reply::NoAnswer,
        Some(f) if f.rtn == 0x00 => Reply::Status(MaintState::parse(&f.info)?),
        Some(f) => Reply::Rtn(f.rtn),
    })
}

fn status_read(link: &mut impl Link, clock: &impl Clock, b: Budgets) -> Result<Reply> {
    status_read_at(link, clock, 0, b)
}

fn status_read_at(link: &mut impl Link, clock: &impl Clock, address: u8, b: Budgets) -> Result<Reply> {
    let request = MaintState::request_at(address)?;
    reply_of(exchange(link, clock, &request, Bound::Status(address), b.answer)?)
}

/// Read the identity (`03` only). `Err` only for a link failure; a missing or malformed answer is `Failed`.
pub fn probe(link: &mut impl Link, clock: &impl Clock, b: Budgets) -> Result<Probe> {
    let request = MaintInfo::request()?;
    Ok(match exchange(link, clock, &request, Bound::Info, b.answer)? {
        None => Probe::Failed,
        Some(f) if f.rtn == 0x00 => match MaintInfo::classify(&f.info) {
            InfoAnswer::Known(i) => Probe::Info(i),
            InfoAnswer::OtherFormat => Probe::OtherPatch,
            InfoAnswer::Broken => Probe::Failed,
        },
        Some(f) if f.rtn == 0x04 => Probe::No0x64,
        Some(f) if f.rtn == 0x05 => Probe::OtherPatch,
        Some(_) => Probe::Failed,
    })
}

/// What the pack at an address answered to its own `03` (no `02` follows; that is the master's alone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackAnswer {
    /// a format-2 (17 bytes) or format-3 (18 bytes) identity
    Info(MaintInfo),
    /// any other valid `MNT` answer (another format or length, e.g. the historic 22-byte format 1)
    OtherMnt,
    /// silence, an error RTN (04, 05 or any other), or an answer without `MNT`: says nothing
    Undecided,
}

/// Which frame is the answer to the addressed `03`: VER 20, CID1 46, the address asked, RTN 00 and `MNT` in the INFO
/// (classified, whatever its length), or an error RTN (undecided). Our echo (CID2 0x64 in that field), foreign traffic,
/// other addresses and INFO without `MNT` are not.
fn pack_answer_of(f: &frame::Frame, address: u8) -> Option<PackAnswer> {
    if f.version != 0x20 || f.cid1 != 0x46 || f.address != address || f.rtn == 0x64 {
        return None;
    }
    if f.rtn != 0x00 {
        return Some(PackAnswer::Undecided);
    }
    if !f.info.starts_with(b"MNT") {
        return None;
    }
    Some(match MaintInfo::classify(&f.info) {
        InfoAnswer::Known(i) => PackAnswer::Info(i),
        _ => PackAnswer::OtherMnt,
    })
}

/// Ask the pack at `address` for its identity (`03` only) and wait at most `timeout`. `Err` only for a link failure.
pub fn probe_pack(link: &mut impl Link, clock: &impl Clock, address: u8, timeout: Duration) -> Result<PackAnswer> {
    let request = MaintInfo::request_at(address)?;
    link.clear_input()?;
    link.write_raw(&request)?;
    let deadline = clock.now() + timeout;
    let mut pending = Vec::new();
    let mut buf = [0u8; 256];
    while clock.now() < deadline {
        let n = link.read_raw(&mut buf)?;
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\r') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let Some(start) = line.iter().position(|b| *b == b'~') else { continue };
            if let Some(a) = frame::parse(&line[start..]).ok().and_then(|f| pack_answer_of(&f, address)) {
                return Ok(a);
            }
        }
        // the bounded frame size: an endless line is no answer
        if pending.len() > 1024 {
            pending.clear();
        }
    }
    Ok(PackAnswer::Undecided)
}

/// The master on this link: `03`, and only after it showed a build with the split status, the 5-byte `02` state.
pub fn read(link: &mut impl Link, clock: &impl Clock, b: Budgets) -> Result<Master> {
    let m = Master::of(Some(probe(link, clock, b)?));
    if !m.caps().is_some_and(|c| c.status_split) {
        return Ok(m);
    }
    let state = match status_read(link, clock, b)? {
        Reply::Status(s) => Some(s),
        _ => None,
    };
    Ok(m.with_state(state))
}

fn status_of(r: Reply) -> std::result::Result<MaintState, DialogError> {
    match r {
        Reply::Status(s) => Ok(s),
        Reply::Rtn(0x04) => Err(DialogError::Unsupported),
        Reply::Rtn(r) => Err(DialogError::Rejected(r)),
        Reply::NoAnswer => Err(DialogError::NoAnswer),
    }
}

/// The state a `start` or `stop` waits for: start with the marker on and no slaves, stop with the marker off and the
/// expected DIP slave count. A foreign marker word reaches neither.
pub fn reached(action: Action, s: MaintState) -> bool {
    match action {
        Action::Status => true,
        Action::Start => s.marker() == MarkerWord::On && s.effective_slaves == 0,
        Action::Stop { expected } => s.marker() == MarkerWord::Off && s.effective_slaves == expected,
        Action::Clear => s.marker() == MarkerWord::Off,
    }
}

/// Read the state (`Status`), or set (`Start`) / clear (`Stop`) the marker and wait until fresh `02` reads show it (the
/// immediate reply can still carry the old slave count). A stop to 0 slaves waits one full DIP cycle more.
pub fn dialog(link: &mut impl Link, clock: &impl Clock, action: Action, b: Budgets) -> std::result::Result<MaintState, DialogError> {
    dialog_at(link, clock, 0, action, b)
}

/// `dialog` with the pack at `address`: over RS485-1/2 (image flag bit 4) status and `Clear` at any pack's own address;
/// the CAN socket knows only the master (0).
pub fn dialog_at(link: &mut impl Link, clock: &impl Clock, address: u8, action: Action, b: Budgets) -> std::result::Result<MaintState, DialogError> {
    let request = match action {
        Action::Status => return status_of(status_read_at(link, clock, address, b)?),
        Action::Start => MaintState::set_request_at(address, true).map_err(Error::from)?,
        Action::Stop { .. } | Action::Clear => MaintState::set_request_at(address, false).map_err(Error::from)?,
    };
    let mut last = status_of(reply_of(exchange(link, clock, &request, Bound::Status(address), b.answer)?)?)?;
    let since = clock.now();
    let left = || b.change.saturating_sub(clock.now().saturating_sub(since));
    loop {
        if left().is_zero() {
            return Err(DialogError::Timeout { after: b.change, last });
        }
        link.wait(b.interval.min(left()));
        if left().is_zero() {
            return Err(DialogError::Timeout { after: b.change, last });
        }
        let pb = Budgets { answer: b.answer.min(left()), ..b };
        match status_of(status_read_at(link, clock, address, pb)?) {
            Ok(s) => {
                last = s;
                if reached(action, s) {
                    if action == (Action::Stop { expected: 0 }) {
                        link.wait(ZERO_SLAVES_SETTLE.saturating_sub(clock.now().saturating_sub(since)));
                    }
                    if action == Action::Clear {
                        // the DIP count shows only with the next cycle; a lost read keeps the state seen (the flash's
                        // own quiet check still hears a polling master)
                        link.wait(ZERO_SLAVES_SETTLE.saturating_sub(clock.now().saturating_sub(since)));
                        return Ok(status_read_at(link, clock, address, b).ok().and_then(|r| status_of(r).ok()).unwrap_or(s));
                    }
                    return Ok(s);
                }
            }
            // one lost answer while waiting is not final; the budget decides
            Err(DialogError::NoAnswer) => {}
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maint::testkit::{answer, ms, FakeLink, VTime};
    use crate::maint::Clock;
    use openbms_proto::frame;

    /// The 17 INFO bytes of a format-2 identity.
    fn mnt(patch: [u8; 2], id: [u8; 8]) -> Vec<u8> {
        let mut v = b"MNT".to_vec();
        v.extend_from_slice(&[2, 16, 6, 13, patch[0], patch[1]]);
        v.extend_from_slice(&id);
        v
    }
    const ID15: [u8; 8] = [0x90, 0x9A, 0x60, 0x4E, 0x60, 0xE9, 0x22, 0x5B];

    fn state(marker: bool, slaves: u8) -> Vec<u8> {
        let mut v = (if marker { 0u32 } else { u32::MAX }).to_le_bytes().to_vec();
        v.push(slaves);
        v
    }

    /// A patched master on UART1. A start or stop takes effect in the status `delay` later; `refuse_start`: RTN 05.
    /// `addr`: the address it answers at (0 on the CAN socket; any pack's own over RS485-1/2 with image flag bit 4).
    struct Master15 { dip: u8, marker: bool, active: bool, switch_at: Option<Duration>, delay: Duration, refuse_start: bool, addr: u8 }

    impl Master15 {
        fn new(dip: u8, marker: bool, delay: Duration) -> Self {
            Self { dip, marker, active: marker, switch_at: None, delay, refuse_start: false, addr: 0 }
        }
        fn reply(&mut self, req: &[u8], now: Duration) -> Vec<(Duration, Vec<u8>)> {
            if self.switch_at.is_some_and(|t| now >= t) {
                self.active = self.marker;
                self.switch_at = None;
            }
            let Ok(f) = frame::parse(req) else { return vec![] };
            if f.rtn != 0x64 || f.address != self.addr {
                return vec![];
            }
            let a = self.addr;
            match f.info.as_slice() {
                [0x03] => return vec![(now + ms(40), answer(a, 0x00, &mnt([1, 5], ID15)))],
                [0x02] => {}
                [1, 0xC3, 0x5A] if self.refuse_start => return vec![(now + ms(40), answer(a, 0x05, &[]))],
                [on @ (0 | 1), 0xC3, 0x5A] => {
                    self.marker = *on == 1;
                    self.switch_at = Some(now + self.delay);
                }
                _ => return vec![(now + ms(40), answer(a, 0x05, &[]))],
            }
            let slaves = if self.active { 0 } else { self.dip };
            vec![(now + ms(40), answer(a, 0x00, &state(self.marker, slaves)))]
        }
    }

    fn link_with(mut m: Master15) -> (FakeLink, VTime) {
        let t = VTime::default();
        (FakeLink::new(t.clone(), move |req, now| m.reply(req, now)), t)
    }

    fn st(marker: bool, slaves: u8) -> MaintState {
        MaintState::parse(&state(marker, slaves)).unwrap()
    }

    #[test]
    fn start_waits_until_the_master_reports_the_marker_and_no_slaves() {
        let (mut link, t) = link_with(Master15::new(11, false, ms(1000)));
        assert_eq!(dialog(&mut link, &t, Action::Start, Budgets::default()).unwrap(), st(true, 0));
        assert!(t.now() >= ms(1000));
    }

    #[test]
    fn stop_waits_for_the_expected_slave_count() {
        let (mut link, t) = link_with(Master15::new(11, true, ms(1000)));
        assert_eq!(dialog(&mut link, &t, Action::Stop { expected: 11 }, Budgets::default()).unwrap(), st(false, 11));
        // another N never counts as reached: the master keeps reporting 11
        let (mut link, t) = link_with(Master15::new(11, true, ms(100)));
        assert!(matches!(dialog(&mut link, &t, Action::Stop { expected: 3 }, Budgets::default()), Err(DialogError::Timeout { last, .. }) if last == st(false, 11)));
    }

    #[test]
    fn clear_needs_only_the_marker_off_whatever_slave_count() {
        let (mut link, t) = link_with(Master15::new(3, true, ms(500)));
        // the marker word is off at once; the state after one more DIP cycle shows the master polling its DIP count
        assert_eq!(dialog(&mut link, &t, Action::Clear, Budgets::default()).unwrap(), st(false, 3));
        assert!(t.now() >= ms(1100));
        let (mut link, t) = link_with(Master15::new(0, true, ms(500)));
        assert_eq!(dialog(&mut link, &t, Action::Clear, Budgets::default()).unwrap(), st(false, 0));
    }

    #[test]
    fn status_and_clear_at_a_pack_address_over_rs485() {
        let mut m = Master15::new(0, true, ms(300));
        m.addr = 7;
        let (mut link, t) = link_with(m);
        assert_eq!(dialog_at(&mut link, &t, 7, Action::Status, Budgets::default()).unwrap(), st(true, 0));
        assert_eq!(dialog_at(&mut link, &t, 7, Action::Clear, Budgets::default()).unwrap(), st(false, 0));
        assert!(link.sent.iter().all(|r| frame::parse(r).unwrap().address == 7));
        // another address does not answer: no state, never another pack's
        let mut m = Master15::new(0, true, ms(0));
        m.addr = 7;
        let (mut link, t) = link_with(m);
        assert!(matches!(dialog_at(&mut link, &t, 3, Action::Status, Budgets::default()), Err(DialogError::NoAnswer)));
    }

    #[test]
    fn stop_with_zero_slaves_waits_at_least_1_1_s() {
        let (mut link, t) = link_with(Master15::new(0, true, ms(200)));
        assert_eq!(dialog(&mut link, &t, Action::Stop { expected: 0 }, Budgets::default()).unwrap(), st(false, 0));
        assert!(t.now() >= ms(1100));
    }

    #[test]
    fn a_refused_start_is_rtn_05() {
        let mut m = Master15::new(3, false, ms(0));
        m.refuse_start = true;
        let (mut link, t) = link_with(m);
        assert!(matches!(dialog(&mut link, &t, Action::Start, Budgets::default()), Err(DialogError::Rejected(5))));
    }

    #[test]
    fn a_change_that_never_shows_times_out_with_the_last_state() {
        let (mut link, t) = link_with(Master15::new(11, false, ms(10_000)));
        match dialog(&mut link, &t, Action::Start, Budgets::default()) {
            // the marker is set, the master still polls until its next cycle
            Err(DialogError::Timeout { last, .. }) => assert_eq!(last, st(true, 11)),
            other => panic!("{other:?}"),
        }
        assert!(t.now() <= ms(40) + ms(3000) + ms(100), "{:?}", t.now());
    }

    #[test]
    fn reading_the_master_takes_03_then_02() {
        let (mut link, t) = link_with(Master15::new(3, false, ms(0)));
        let m = read(&mut link, &t, Budgets::default()).unwrap();
        assert_eq!((m.marker(), m.effective_slaves()), (Some(MarkerWord::Off), Some(3)));
        assert!(m.may_halt());
        assert_eq!(link.sent, vec![MaintInfo::request().unwrap(), MaintState::request().unwrap()]);
    }

    #[test]
    fn a_format_3_identity_is_bound_and_read_then_02() {
        let t = VTime::default();
        let mut id3 = b"MNT".to_vec();
        id3.extend_from_slice(&[3, 16, 6, 13, 1, 5, 1, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 1]);
        let mut link = FakeLink::new(t.clone(), move |req, now| match frame::parse(req) {
            Ok(f) if f.info == [0x03] => vec![(now + ms(40), answer(0, 0x00, &id3))],
            Ok(f) if f.info == [0x02] => vec![(now + ms(40), answer(0, 0x00, &state(false, 2)))],
            _ => vec![],
        });
        let m = read(&mut link, &t, Budgets::default()).unwrap();
        assert!(matches!(m, Master::Patched { patch, .. } if patch.info.hard_stop == Some(true) && patch.info.format == 3));
        assert_eq!(m.effective_slaves(), Some(2));
        // format 4 (18 bytes): unknown patch, no 02
        let mut id4 = b"MNT".to_vec();
        id4.extend_from_slice(&[4, 16, 6, 13, 1, 5, 1, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 1]);
        let mut link = FakeLink::new(t.clone(), move |_, now| vec![(now + ms(40), answer(0, 0x00, &id4))]);
        assert_eq!(read(&mut link, &t, Budgets::default()).unwrap(), Master::UnknownPatch(None));
        assert_eq!(link.sent.len(), 1);
    }

    fn mnt3(patch: [u8; 2], flags: u8) -> Vec<u8> {
        let mut v = b"MNT".to_vec();
        v.extend_from_slice(&[3, 16, 6, 13, patch[0], patch[1], 1, 2, 3, 4, 5, 6, 7, 8, flags]);
        v
    }

    fn pack_link(answer_of: impl Fn(u8) -> Vec<(Duration, Vec<u8>)> + 'static) -> (FakeLink, VTime) {
        let t = VTime::default();
        let link = FakeLink::new(t.clone(), move |req, now| match frame::parse(req) {
            Ok(f) if f.info == [0x03] => answer_of(f.address).into_iter().map(|(d, b)| (now + d, b)).collect(),
            _ => vec![],
        });
        (link, t)
    }

    #[test]
    fn a_pack_answers_its_own_addressed_03_and_no_02_follows() {
        let (mut link, t) = pack_link(|a| vec![(ms(40), answer(a, 0x00, &mnt3([1, 5], 1)))]);
        let r = probe_pack(&mut link, &t, 7, ms(600)).unwrap();
        assert!(matches!(r, PackAnswer::Info(i) if i.hard_stop == Some(true) && i.format == 3));
        assert_eq!(link.sent, vec![MaintInfo::request_at(7).unwrap()]);
        // MNT02 with 17 bytes
        let (mut link, t) = pack_link(|a| vec![(ms(40), answer(a, 0x00, &mnt([1, 5], ID15)))]);
        assert!(matches!(probe_pack(&mut link, &t, 3, ms(600)).unwrap(), PackAnswer::Info(i) if i.hard_stop.is_none()));
    }

    #[test]
    fn a_measured_slave_answer_at_address_1_is_read() {
        let measured = b"~20014600A0244D4E540310060D0105297F9F2AA8402E9F00F5D1\r".to_vec();
        let (mut link, t) = pack_link(move |a| if a == 1 { vec![(ms(40), measured.clone())] } else { vec![] });
        let PackAnswer::Info(i) = probe_pack(&mut link, &t, 1, ms(600)).unwrap() else { panic!() };
        assert_eq!((i.format, i.base, i.patch, i.build_id_hex().as_str(), i.hard_stop), (3, [16, 6, 13], [1, 5], "297f9f2aa8402e9f", Some(false)));
        assert!(matches!(super::super::policy::PackFinding::of(PackAnswer::Info(i)), super::super::policy::PackFinding::Proven { listed: Some("16.06.13 MAINT 1.5"), .. }));
        // the same answer does not belong to address 2
        assert_eq!(probe_pack(&mut link, &t, 2, ms(600)).unwrap(), PackAnswer::Undecided);
    }

    #[test]
    fn another_valid_mnt_is_classified_errors_and_silence_are_undecided() {
        // the historic 22-byte format 1, format 4, MNT03 with a wrong length: classified as another MNT
        let mut f1 = b"MNT".to_vec();
        f1.extend_from_slice(&[1, 16, 6, 13, 1, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0xFE, 0x38, 0x19, 0x85, 0xBF, 0xFF, 0x8A, 0xD0]);
        let mut f4 = mnt3([1, 5], 0);
        f4[3] = 4;
        let short = mnt3([1, 5], 0)[..17].to_vec();
        for info in [f1, f4, short] {
            let (mut link, t) = pack_link(move |a| vec![(ms(40), answer(a, 0x00, &info))]);
            assert_eq!(probe_pack(&mut link, &t, 5, ms(600)).unwrap(), PackAnswer::OtherMnt);
        }
        // RTN 04, RTN 05, another error, silence, INFO without MNT: undecided
        for rtn in [0x04, 0x05, 0x02] {
            let (mut link, t) = pack_link(move |a| vec![(ms(40), answer(a, rtn, &[]))]);
            assert_eq!(probe_pack(&mut link, &t, 5, ms(600)).unwrap(), PackAnswer::Undecided);
        }
        let (mut link, t) = pack_link(|_| vec![]);
        assert_eq!(probe_pack(&mut link, &t, 5, ms(600)).unwrap(), PackAnswer::Undecided);
        assert!(t.now() >= ms(600) && t.now() <= ms(800), "{:?}", t.now());
        let (mut link, t) = pack_link(|a| vec![(ms(40), answer(a, 0x00, &[1, 2, 3, 4]))]);
        assert_eq!(probe_pack(&mut link, &t, 5, ms(600)).unwrap(), PackAnswer::Undecided);
    }

    #[test]
    fn only_the_address_asked_and_never_our_echo_answers() {
        // another address's answer first, our echo, then ours
        let (mut link, t) = pack_link(|a| vec![(ms(10), answer(a + 1, 0x00, &mnt3([1, 5], 0))), (ms(20), MaintInfo::request_at(a).unwrap()), (ms(60), answer(a, 0x00, &mnt3([1, 5], 0)))]);
        assert!(matches!(probe_pack(&mut link, &t, 4, ms(600)).unwrap(), PackAnswer::Info(_)));
        // only another address answers: undecided
        let (mut link, t) = pack_link(|a| vec![(ms(10), answer(a + 1, 0x00, &mnt3([1, 5], 0)))]);
        assert_eq!(probe_pack(&mut link, &t, 4, ms(600)).unwrap(), PackAnswer::Undecided);
        assert!(probe_pack(&mut link, &t, 22, ms(600)).is_err(), "address beyond 21");
    }

    #[test]
    fn no_02_without_a_supported_identity() {
        let t = VTime::default();
        // the historic 22-byte format 1: unknown patch, never decoded, no 02
        let mut f1 = b"MNT".to_vec();
        f1.extend_from_slice(&[1, 16, 6, 13, 1, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0xFE, 0x38, 0x19, 0x85, 0xBF, 0xFF, 0x8A, 0xD0]);
        let mut link = FakeLink::new(t.clone(), move |_, now| vec![(now + ms(40), answer(0, 0x00, &f1))]);
        assert_eq!(read(&mut link, &t, Budgets::default()).unwrap(), Master::UnknownPatch(None));
        assert_eq!(link.sent.len(), 1);
        // format 2 with patch 1.4: unknown patch
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(40), answer(0, 0x00, &mnt([1, 4], [7; 8])))]);
        assert!(matches!(read(&mut link, &t, Budgets::default()).unwrap(), Master::UnknownPatch(Some(_))));
        assert_eq!(link.sent.len(), 1);
        // RTN 05 to 03: a patch that knows 0x64 but not this identity
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(40), answer(0, 0x05, &[]))]);
        assert_eq!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::OtherPatch);
    }

    #[test]
    fn rtn_04_is_no_0x64_and_silence_is_failed() {
        let t = VTime::default();
        let mut orig = FakeLink::new(t.clone(), |_, now| vec![(now + ms(40), answer(0, 0x04, &[]))]);
        assert_eq!(probe(&mut orig, &t, Budgets::default()).unwrap(), Probe::No0x64);
        assert_eq!(orig.sent.len(), 1, "no 02 after RTN 04");
        assert!(matches!(dialog(&mut orig, &t, Action::Start, Budgets::default()), Err(DialogError::Unsupported)));
        let mut silent = FakeLink::new(t.clone(), |_, _| vec![]);
        assert_eq!(probe(&mut silent, &t, Budgets::default()).unwrap(), Probe::Failed);
        assert_eq!(read(&mut silent, &t, Budgets::default()).unwrap(), Master::Unknown);
    }

    #[test]
    fn a_malformed_03_answer_is_failed_not_another_image() {
        let t = VTime::default();
        let mut bad = mnt([1, 5], ID15);
        bad[0] = b'X';
        let mut link = FakeLink::new(t.clone(), move |_, now| vec![(now + ms(40), answer(0, 0x00, &bad))]);
        assert_eq!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Failed);
    }

    #[test]
    fn answers_of_the_wrong_length_are_not_bound() {
        // a delayed five-byte state (the answer to an earlier 02) arrives first: ignored, the identity counts
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(10), answer(0, 0x00, &state(false, 3))), (now + ms(80), answer(0, 0x00, &mnt([1, 5], ID15)))]);
        assert!(matches!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Info(_)));
        // only the state: no answer to 03
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(10), answer(0, 0x00, &state(false, 3)))]);
        assert_eq!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Failed);
        // an identity or an old two-byte status during a 02 read is not a state
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(10), answer(0, 0x00, &mnt([1, 5], ID15))), (now + ms(20), answer(0, 0x00, &[0, 3]))]);
        assert!(matches!(dialog(&mut link, &t, Action::Status, Budgets::default()), Err(DialogError::NoAnswer)));
    }

    #[test]
    fn a_broken_port_is_a_connection_error_not_a_status() {
        let (mut link, t) = link_with(Master15::new(11, false, ms(0)));
        link.broken = true;
        assert!(matches!(dialog(&mut link, &t, Action::Status, Budgets::default()), Err(DialogError::Link(_))));
        assert!(probe(&mut link, &t, Budgets::default()).is_err());
    }

    #[test]
    fn an_echo_of_the_request_is_not_taken_as_the_answer() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |req, now| vec![(now, req.to_vec()), (now + ms(40), answer(0, 0, &mnt([1, 5], ID15)))]);
        assert!(matches!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Info(i) if i.patch == [1, 5]));
        let mut link = FakeLink::new(t.clone(), |req, now| vec![(now, req.to_vec()), (now + ms(40), answer(0, 0, &state(false, 3)))]);
        assert_eq!(dialog(&mut link, &t, Action::Status, Budgets::default()).unwrap(), st(false, 3));
    }

    #[test]
    fn foreign_traffic_before_the_status_is_skipped() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(300), answer(0, 0, &state(false, 3)))]);
        link.inject(ms(100), &answer(0, 0x00, &[1, 2, 3, 4]));
        let foreign_req = frame::encode_request(0, 0x42, &[]).unwrap();
        link.inject(ms(150), &foreign_req);
        assert_eq!(dialog(&mut link, &t, Action::Status, Budgets::default()).unwrap(), st(false, 3));
    }

    #[test]
    fn a_foreign_marker_word_reaches_neither_start_nor_stop() {
        let foreign = MaintState { marker_word: 0x1234_5678, effective_slaves: 0 };
        assert!(!reached(Action::Start, foreign));
        assert!(!reached(Action::Stop { expected: 0 }, foreign));
        assert!(reached(Action::Stop { expected: 0 }, st(false, 0)));
        assert!(!reached(Action::Stop { expected: 3 }, st(false, 0)));
    }
}
