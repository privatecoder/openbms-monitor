//! CID2 0x64 with the patched master on the CAN socket bus (UART1, 9600 baud): read the maintenance
//! state, set or clear the marker, and wait until the master's next cycles show the change.

use super::Clock;
use crate::{link::Link, Error, Result};
use openbms_proto::{frame, MaintStatus};
use std::time::Duration;

/// What the master at address 0 answered to a 0x64 status request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Status(MaintStatus),
    /// RTN 04: the firmware does not know 0x64 (original or another build; RTN 04 does not prove which).
    Unsupported,
    /// any other RTN (05: format or token)
    Rejected(u8),
    /// no frame from address 0 within the answer time
    NoAnswer,
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

/// Send one request and return the first frame from address 0, or None after `timeout`.
/// A frame that carries CID2 0x64 is our own request echoed by the adapter, not an answer.
fn exchange(link: &mut impl Link, clock: &impl Clock, request: &[u8], timeout: Duration) -> Result<Option<frame::Frame>> {
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
                if f.address == 0 && f.rtn != 0x64 {
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

fn to_probe(answer: Option<frame::Frame>) -> Result<Probe> {
    Ok(match answer {
        None => Probe::NoAnswer,
        Some(f) if f.rtn == 0x00 => Probe::Status(MaintStatus::parse(&f.info)?),
        Some(f) if f.rtn == 0x04 => Probe::Unsupported,
        Some(f) => Probe::Rejected(f.rtn),
    })
}

/// Read the state once. `Err` only for a link failure; every master reaction is a `Probe`.
pub fn probe(link: &mut impl Link, clock: &impl Clock, b: Budgets) -> Result<Probe> {
    let request = MaintStatus::request()?;
    to_probe(exchange(link, clock, &request, b.answer)?)
}

fn status_of(p: Probe) -> std::result::Result<MaintStatus, DialogError> {
    match p {
        Probe::Status(s) => Ok(s),
        Probe::Unsupported => Err(DialogError::Unsupported),
        Probe::Rejected(r) => Err(DialogError::Rejected(r)),
        Probe::NoAnswer => Err(DialogError::NoAnswer),
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
        Action::Status => return status_of(probe(link, clock, b)?),
        Action::Start => MaintStatus::set_request(true).map_err(Error::from)?,
        Action::Stop => MaintStatus::set_request(false).map_err(Error::from)?,
    };
    let mut last = status_of(to_probe(exchange(link, clock, &request, b.answer)?)?)?;
    let since = clock.now();
    let reached = |s: MaintStatus| match action {
        Action::Start => s.marker && s.effective_slaves == 0,
        _ => !s.marker && expected_slaves.is_none_or(|n| s.effective_slaves == n),
    };
    loop {
        if clock.now().saturating_sub(since) > b.change {
            return Err(DialogError::Timeout { after: b.change, last });
        }
        link.wait(b.interval);
        match status_of(probe(link, clock, b)?) {
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
    use openbms_proto::frame;

    /// A patched master on UART1. A start or stop takes effect in the status `delay` later.
    struct Master { dip: u8, marker: bool, active: bool, switch_at: Option<Duration>, delay: Duration }

    impl Master {
        fn new(dip: u8, marker: bool, delay: Duration) -> Self {
            Self { dip, marker, active: marker, switch_at: None, delay }
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
            match f.info.as_slice() {
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
            Err(DialogError::Timeout { last, .. }) => assert_eq!(last, st(true, 11)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rtn_04_is_unsupported_and_silence_is_no_answer() {
        let t = VTime::default();
        let mut orig = FakeLink::new(t.clone(), |_, now| vec![(now + ms(40), answer(0, 0x04, &[]))]);
        assert_eq!(probe(&mut orig, &t, Budgets::default()).unwrap(), Probe::Unsupported);
        assert!(matches!(dialog(&mut orig, &t, Action::Start, None, Budgets::default()), Err(DialogError::Unsupported)));
        let mut silent = FakeLink::new(t.clone(), |_, _| vec![]);
        assert_eq!(probe(&mut silent, &t, Budgets::default()).unwrap(), Probe::NoAnswer);
    }

    #[test]
    fn a_broken_port_is_a_connection_error_not_a_status() {
        let (mut link, t) = link_with(Master::new(11, false, ms(0)));
        link.broken = true;
        assert!(matches!(dialog(&mut link, &t, Action::Status, None, Budgets::default()), Err(DialogError::Link(_))));
    }

    #[test]
    fn an_echo_of_the_request_is_not_taken_as_the_answer() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |req, now| vec![(now, req.to_vec()), (now + ms(40), answer(0, 0, &[0, 3]))]);
        assert_eq!(probe(&mut link, &t, Budgets::default()).unwrap(), Probe::Status(st(false, 3)));
    }
}
