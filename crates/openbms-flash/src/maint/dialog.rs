//! CID2 0x64 with the patched master on the CAN socket bus (UART1, 9600 baud): read the maintenance
//! state, set or clear the marker, and wait until the master's next cycles show the change.

use super::Clock;
use crate::{link::Link, Error, Result};
use openbms_proto::{frame, MaintInfo, MaintStatus};
use std::time::Duration;

/// What the master at address 0 answered to the identity probe (`03` first, `02` only after RTN 05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// MAINT 1.1: the 22-byte identity
    Info(MaintInfo),
    /// `03` answered RTN 05 and `02` answered: MAINT 1.0 compatible
    Legacy(MaintStatus),
    /// RTN 04: the firmware does not know 0x64 (original or another build; RTN 04 does not prove which)
    No0x64,
    /// no valid answer; never read as original or 1.0
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Status,
    Start,
    Stop,
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
        Self { answer: Duration::from_millis(600), change: Duration::from_secs(3), interval: Duration::from_millis(300) }
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
    Timeout { after: Duration, last: MaintStatus },
}

/// Minimum time after `stop` when the slave count does not change (N = 0): one full master cycle.
const ZERO_SLAVES_SETTLE: Duration = Duration::from_millis(1100);

/// Which answers belong to the request sent: `03` gets 22 bytes or RTN 04/05; `02`, start and stop get two
/// bytes or a rejection. A late answer to an earlier request (other length), our own echo (CID2 0x64 in the
/// RTN field) and foreign traffic do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    Info,
    Status,
}

impl Bound {
    fn accepts(self, f: &frame::Frame) -> bool {
        f.address == 0
            && match self {
                Bound::Info => (f.rtn == 0x00 && f.info.len() == MaintInfo::LEN) || f.rtn == 0x04 || f.rtn == 0x05,
                Bound::Status => (f.rtn == 0x00 && f.info.len() == 2) || (0x01..=0x0F).contains(&f.rtn),
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
    Status(MaintStatus),
    Rtn(u8),
    NoAnswer,
}

fn reply_of(answer: Option<frame::Frame>) -> Result<Reply> {
    Ok(match answer {
        None => Reply::NoAnswer,
        Some(f) if f.rtn == 0x00 => Reply::Status(MaintStatus::parse(&f.info)?),
        Some(f) => Reply::Rtn(f.rtn),
    })
}

fn status_read(link: &mut impl Link, clock: &impl Clock, b: Budgets) -> Result<Reply> {
    let request = MaintStatus::request()?;
    reply_of(exchange(link, clock, &request, Bound::Status, b.answer)?)
}

/// Read the identity: `03` first; after RTN 05 the `02` status (MAINT 1.0 compatible). `Err` only for a link
/// failure; a missing or malformed answer is `Failed`, never another image.
pub fn probe(link: &mut impl Link, clock: &impl Clock, b: Budgets) -> Result<Probe> {
    let request = MaintInfo::request()?;
    Ok(match exchange(link, clock, &request, Bound::Info, b.answer)? {
        None => Probe::Failed,
        Some(f) if f.rtn == 0x00 => MaintInfo::parse(&f.info).map(Probe::Info).unwrap_or(Probe::Failed),
        Some(f) if f.rtn == 0x04 => Probe::No0x64,
        Some(_) => match status_read(link, clock, b)? {
            Reply::Status(s) => Probe::Legacy(s),
            _ => Probe::Failed,
        },
    })
}

fn status_of(r: Reply) -> std::result::Result<MaintStatus, DialogError> {
    match r {
        Reply::Status(s) => Ok(s),
        Reply::Rtn(0x04) => Err(DialogError::Unsupported),
        Reply::Rtn(r) => Err(DialogError::Rejected(r)),
        Reply::NoAnswer => Err(DialogError::NoAnswer),
    }
}

/// Read the state (`Status`), or set (`Start`) / clear (`Stop`) the marker and wait until the master shows
/// it. `expected_slaves` (Stop only): the DIP slave count the master must report again; None = any.
pub fn dialog(
    link: &mut impl Link,
    clock: &impl Clock,
    action: Action,
    expected_slaves: Option<u8>,
    b: Budgets,
) -> std::result::Result<MaintStatus, DialogError> {
    let request = match action {
        Action::Status => return status_of(status_read(link, clock, b)?),
        Action::Start => MaintStatus::set_request(true).map_err(Error::from)?,
        Action::Stop => MaintStatus::set_request(false).map_err(Error::from)?,
    };
    let mut last = status_of(reply_of(exchange(link, clock, &request, Bound::Status, b.answer)?)?)?;
    let since = clock.now();
    let left = || b.change.saturating_sub(clock.now().saturating_sub(since));
    let reached = |s: MaintStatus| match action {
        Action::Start => s.marker && s.effective_slaves == 0,
        _ => !s.marker && expected_slaves.is_none_or(|n| s.effective_slaves == n),
    };
    loop {
        if left().is_zero() {
            return Err(DialogError::Timeout { after: b.change, last });
        }
        link.wait(b.interval.min(left()));
        if left().is_zero() {
            return Err(DialogError::Timeout { after: b.change, last });
        }
        let pb = Budgets { answer: b.answer.min(left()), ..b };
        match status_of(status_read(link, clock, pb)?) {
            Ok(s) => {
                last = s;
                if reached(s) {
                    if action == Action::Stop && s.effective_slaves == 0 {
                        link.wait(ZERO_SLAVES_SETTLE.saturating_sub(clock.now().saturating_sub(since)));
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
    use openbms_proto::{frame, MarkerWord};

    /// The 22 INFO bytes of a MAINT 1.1 identity answer.
    fn mnt(marker_word: u32, slaves: u8) -> Vec<u8> {
        let mut v = b"MNT".to_vec();
        v.extend_from_slice(&[1, 16, 6, 13, 1, 1]);
        v.extend_from_slice(&marker_word.to_le_bytes());
        v.push(slaves);
        v.extend_from_slice(&[0xFE, 0x38, 0x19, 0x85, 0xBF, 0xFF, 0x8A, 0xD0]);
        v
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Build { Maint11, Maint10 }

    /// A patched master on UART1. A start or stop takes effect in the status `delay` later.
    struct Master { build: Build, dip: u8, marker: bool, active: bool, switch_at: Option<Duration>, delay: Duration }

    impl Master {
        fn new(dip: u8, marker: bool, delay: Duration) -> Self {
            Self { build: Build::Maint11, dip, marker, active: marker, switch_at: None, delay }
        }
        fn reply(&mut self, req: &[u8], now: Duration) -> Vec<(Duration, Vec<u8>)> {
            if self.switch_at.is_some_and(|t| now >= t) {
                self.active = self.marker;
                self.switch_at = None;
            }
            let Ok(f) = frame::parse(req) else { return vec![] };
            if f.rtn != 0x64 {
                return vec![];
            }
            let slaves = if self.active { 0 } else { self.dip };
            match f.info.as_slice() {
                [0x03] if self.build == Build::Maint11 => {
                    return vec![(now + ms(40), answer(0, 0x00, &mnt(if self.marker { 0 } else { u32::MAX }, slaves)))];
                }
                [0x02] => {}
                [on @ (0 | 1), 0xC3, 0x5A] => {
                    self.marker = *on == 1;
                    self.switch_at = Some(now + self.delay);
                }
                _ => return vec![(now + ms(40), answer(0, 0x05, &[]))],
            }
            let slaves = if self.active { 0 } else { self.dip };
            vec![(now + ms(40), answer(0, 0x00, &[u8::from(self.marker), slaves]))]
        }
    }

    fn link_with(mut m: Master) -> (FakeLink, VTime) {
        let t = VTime::default();
        (FakeLink::new(t.clone(), move |req, now| m.reply(req, now)), t)
    }

    fn st(marker: bool, slaves: u8) -> MaintStatus {
        MaintStatus { marker, effective_slaves: slaves }
    }

    fn probe_of(link: &mut FakeLink) -> Probe {
        let t = link.time.clone();
        probe(link, &t, Budgets::default()).unwrap()
    }

    #[test]
    fn start_waits_until_the_master_reports_the_marker() {
        let (mut link, t) = link_with(Master::new(11, false, ms(1000)));
        let s = dialog(&mut link, &t, Action::Start, None, Budgets::default()).unwrap();
        assert_eq!(s, st(true, 0));
        assert!(t.now() >= ms(1000));
    }

    #[test]
    fn stop_waits_for_the_expected_slave_count() {
        let (mut link, t) = link_with(Master::new(11, true, ms(1000)));
        assert_eq!(dialog(&mut link, &t, Action::Stop, Some(11), Budgets::default()).unwrap(), st(false, 11));
    }

    #[test]
    fn stop_with_zero_slaves_waits_at_least_1_1_s() {
        let (mut link, t) = link_with(Master::new(0, true, ms(200)));
        assert_eq!(dialog(&mut link, &t, Action::Stop, Some(0), Budgets::default()).unwrap(), st(false, 0));
        assert!(t.now() >= ms(1100));
    }

    #[test]
    fn a_change_that_never_shows_times_out_with_the_last_state() {
        let (mut link, t) = link_with(Master::new(11, false, ms(10_000)));
        match dialog(&mut link, &t, Action::Start, None, Budgets::default()) {
            // 1/N: the marker is set, the master still polls until its next cycle
            Err(DialogError::Timeout { last, .. }) => assert_eq!(last, st(true, 11)),
            other => panic!("{other:?}"),
        }
        // 40 ms answer latency + the 3 s change budget, plus at most a read step of rounding
        assert!(t.now() <= ms(40) + ms(3000) + ms(100), "{:?}", t.now());
    }

    #[test]
    fn probe_reads_the_identity_of_maint_1_1() {
        let (mut link, _) = link_with(Master::new(3, false, ms(0)));
        let Probe::Info(i) = probe_of(&mut link) else { panic!() };
        assert_eq!((i.marker(), i.effective_slaves, i.build_id_hex().as_str()), (MarkerWord::Off, 3, "fe381985bfff8ad0"));
        assert_eq!(link.sent, vec![MaintInfo::request().unwrap()]);
    }

    #[test]
    fn probe_falls_back_to_02_only_after_rtn_05() {
        let mut m = Master::new(3, true, ms(0));
        m.build = Build::Maint10;
        let (mut link, _) = link_with(m);
        assert_eq!(probe_of(&mut link), Probe::Legacy(st(true, 0)));
        assert_eq!(link.sent, vec![MaintInfo::request().unwrap(), MaintStatus::request().unwrap()]);
    }

    #[test]
    fn rtn_04_is_no_0x64_and_silence_is_failed() {
        let t = VTime::default();
        let mut orig = FakeLink::new(t.clone(), |_, now| vec![(now + ms(40), answer(0, 0x04, &[]))]);
        assert_eq!(probe(&mut orig, &t, Budgets::default()).unwrap(), Probe::No0x64);
        assert_eq!(orig.sent.len(), 1, "no 02 after RTN 04");
        assert!(matches!(dialog(&mut orig, &t, Action::Start, None, Budgets::default()), Err(DialogError::Unsupported)));
        let mut silent = FakeLink::new(t.clone(), |_, _| vec![]);
        assert_eq!(probe(&mut silent, &t, Budgets::default()).unwrap(), Probe::Failed);
    }

    #[test]
    fn a_malformed_03_answer_is_failed_not_another_image() {
        let t = VTime::default();
        let mut bad = mnt(u32::MAX, 0);
        bad[0] = b'X';
        let mut link = FakeLink::new(t.clone(), move |_, now| vec![(now + ms(40), answer(0, 0x00, &bad))]);
        assert_eq!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Failed);
        // RTN 05 to 03, but 02 stays silent: not 1.0
        let mut half = FakeLink::new(t.clone(), |req, now| match frame::parse(req) {
            Ok(f) if f.info == [0x03] => vec![(now + ms(40), answer(0, 0x05, &[]))],
            _ => vec![],
        });
        assert_eq!(probe(&mut half, &t, Budgets::default()).unwrap(), Probe::Failed);
    }

    #[test]
    fn answers_of_the_wrong_length_are_not_bound_to_03() {
        // a delayed two-byte status (the answer to an earlier 02) arrives first: ignored, the identity counts
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(10), answer(0, 0x00, &[0, 3])), (now + ms(80), answer(0, 0x00, &mnt(u32::MAX, 3)))]);
        assert!(matches!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Info(_)));
        // only the two-byte answer: no answer to 03
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(10), answer(0, 0x00, &[0, 3]))]);
        assert_eq!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Failed);
        // a 22-byte answer during a 02 read is not a status
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(10), answer(0, 0x00, &mnt(0, 0)))]);
        assert!(matches!(dialog(&mut link, &t, Action::Status, None, Budgets::default()), Err(DialogError::NoAnswer)));
    }

    #[test]
    fn a_broken_port_is_a_connection_error_not_a_status() {
        let (mut link, t) = link_with(Master::new(11, false, ms(0)));
        link.broken = true;
        assert!(matches!(dialog(&mut link, &t, Action::Status, None, Budgets::default()), Err(DialogError::Link(_))));
        assert!(probe(&mut link, &t, Budgets::default()).is_err());
    }

    #[test]
    fn an_echo_of_the_request_is_not_taken_as_the_answer() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |req, now| vec![(now, req.to_vec()), (now + ms(40), answer(0, 0, &mnt(u32::MAX, 3)))]);
        assert!(matches!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Info(i) if i.effective_slaves == 3));
        let mut link = FakeLink::new(t.clone(), |req, now| vec![(now, req.to_vec()), (now + ms(40), answer(0, 0, &[0, 3]))]);
        assert_eq!(dialog(&mut link, &t, Action::Status, None, Budgets::default()).unwrap(), st(false, 3));
    }

    #[test]
    fn foreign_traffic_before_the_status_is_skipped() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, now| vec![(now + ms(300), answer(0, 0, &[0, 3]))]);
        link.inject(ms(100), &answer(0, 0x00, &[1, 2, 3, 4]));
        let foreign_req = frame::encode_request(0, 0x42, &[]).unwrap();
        link.inject(ms(150), &foreign_req);
        assert_eq!(dialog(&mut link, &t, Action::Status, None, Budgets::default()).unwrap(), st(false, 3));
    }
}
