//! Pack bus (RS485-1/2) checks around maintenance: before an action the bus is quiet and the master
//! answers there; at the end every slave is polled by the master and answers again.

use super::Clock;
use crate::{link::Link, session, Error, Result};
use openbms_proto::frame;
use std::collections::BTreeSet;
use std::io::Write;
use std::time::Duration;

const ASCII_TIMEOUT: Duration = Duration::from_millis(600);

/// Fresh measurement: about 2 s without any byte, then 0x47 answered by address 0. 0x47 exists only on
/// UART0, so this also proves the channel is the pack bus and the master's dispatcher runs.
pub fn measure(link: &mut impl Link, log: &mut impl Write) -> Result<()> {
    session::quiet(link, log)?;
    session::ascii_query(link, 0, 0x47, log, ASCII_TIMEOUT).map_err(|e| match e {
        Error::Timeout { .. } => Error::MasterSilent,
        other => other,
    })?;
    Ok(())
}

/// Intra-pack traffic seen on the pack bus: polls of the master (CID2 0x5A, empty INFO) and answers of
/// the slaves (0x5A in the RTN field, 32 bytes INFO), by address.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Round {
    pub polled: BTreeSet<u8>,
    pub answered: BTreeSet<u8>,
}

impl Round {
    pub fn complete(&self, slaves: u8) -> bool {
        self.missing(slaves).is_empty()
    }

    /// Slave addresses without a poll or without an answer.
    pub fn missing(&self, slaves: u8) -> Vec<u8> {
        (1..=slaves).filter(|a| !self.polled.contains(a) || !self.answered.contains(a)).collect()
    }
}

/// INFO length of a slave's answer to the master's 0x5A poll.
const RECORD_LEN: usize = 32;

/// Listen for one full polling round of the master. A round starts with a poll of address 1 and ends with
/// the next one (the master polls 1..=N in turn; from code, to be confirmed on the bench). An answer counts
/// only when it has 32 bytes INFO and follows the master's poll of the same address with no other frame in
/// between. Returns the first complete round; on timeout the round with the most answers, for `missing`.
pub fn watch_round(link: &mut impl Link, clock: &impl Clock, log: &mut impl Write, slaves: u8, timeout: Duration) -> Result<Round> {
    link.clear_input()?;
    let deadline = clock.now() + timeout;
    let mut best = Round::default();
    let mut round: Option<Round> = None; // None until the first poll of address 1
    let mut last_poll: Option<u8> = None;
    let mut pending = Vec::new();
    let mut buf = [0u8; 512];
    while clock.now() < deadline {
        let n = link.read_raw(&mut buf)?;
        if n > 0 {
            writeln!(log, "RX {}", buf[..n].iter().map(|b| format!("{b:02X}")).collect::<String>())?;
        }
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\r') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let Some(start) = line.iter().position(|b| *b == b'~') else { continue };
            let Ok(f) = frame::parse(&line[start..]) else {
                last_poll = None;
                continue;
            };
            if f.rtn != 0x5A {
                last_poll = None;
                continue;
            }
            if f.info.is_empty() {
                if f.address == 1 {
                    if let Some(done) = round.take() {
                        if done.answered.len() > best.answered.len() {
                            best = done;
                        }
                    }
                    round = Some(Round::default());
                }
                if let Some(r) = round.as_mut() {
                    r.polled.insert(f.address);
                }
                last_poll = Some(f.address);
            } else {
                if let (Some(r), Some(p)) = (round.as_mut(), last_poll) {
                    if p == f.address && f.info.len() == RECORD_LEN {
                        r.answered.insert(f.address);
                    }
                }
                last_poll = None;
            }
            if round.as_ref().is_some_and(|r| r.complete(slaves)) {
                return Ok(round.unwrap_or_default());
            }
        }
        if pending.len() > 4096 {
            pending.clear();
        }
    }
    Ok(match round {
        Some(r) if r.answered.len() > best.answered.len() => r,
        _ => best,
    })
}

/// Whether the pack answers as an application (0x51 and 0x47 in one attempt) within `within`. Used after a
/// failed flash: a pack that answers is not stuck in its bootloader. A pack just out of its bootloader may
/// answer 0x51 before 0x47 is ready, so the pair is retried until `within` has passed; each query's timeout
/// is clamped to the time left.
pub fn app_answers(link: &mut impl Link, log: &mut impl Write, address: u8, within: Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        let left = |cap: Duration| within.saturating_sub(start.elapsed()).min(cap);
        if session::ascii_query(link, address, 0x51, log, left(Duration::from_millis(300))).is_ok()
            && session::ascii_query(link, address, 0x47, log, left(ASCII_TIMEOUT)).is_ok()
        {
            return true;
        }
        if start.elapsed() >= within {
            return false;
        }
        link.wait(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maint::testkit::{answer, ms, FakeLink, VTime};
    use openbms_proto::frame;

    fn poll(a: u8) -> Vec<u8> {
        frame::encode_request(a, 0x5A, &[]).unwrap()
    }

    fn record(a: u8) -> Vec<u8> {
        answer(a, 0x5A, &[0u8; 32])
    }

    /// A quiet master that answers 0x47 (and 0x51) at address 0.
    fn quiet_master(req: &[u8], now: Duration) -> Vec<(Duration, Vec<u8>)> {
        match frame::parse(req) {
            Ok(f) if f.address == 0 && (f.rtn == 0x47 || f.rtn == 0x51) => vec![(now + ms(60), answer(0, 0, &[1, 2, 3]))],
            _ => vec![],
        }
    }

    #[test]
    fn measure_passes_on_a_quiet_bus_with_the_master_answering() {
        let mut link = FakeLink::new(VTime::default(), quiet_master);
        assert!(measure(&mut link, &mut Vec::new()).is_ok());
    }

    #[test]
    fn measure_fails_when_the_master_still_polls() {
        let mut link = FakeLink::new(VTime::default(), quiet_master);
        link.inject(ms(500), &poll(1));
        assert!(matches!(measure(&mut link, &mut Vec::new()), Err(Error::BusBusy(_))));
    }

    #[test]
    fn measure_fails_when_only_0x51_is_answered() {
        // the CAN socket bus: 0x51 yes, 0x47 no
        let mut link = FakeLink::new(VTime::default(), |req, now| match frame::parse(req) {
            Ok(f) if f.rtn == 0x51 => vec![(now + ms(60), answer(0, 0, &[1]))],
            _ => vec![],
        });
        assert!(matches!(measure(&mut link, &mut Vec::new()), Err(Error::MasterSilent)));
    }

    /// Inject one master poll and (optionally) the slave's answer 50 ms later.
    fn exchange(link: &mut FakeLink, at: Duration, a: u8, answer: Option<Vec<u8>>) {
        link.inject(at, &poll(a));
        if let Some(r) = answer {
            link.inject(at + ms(50), &r);
        }
    }

    #[test]
    fn a_round_is_complete_only_with_poll_and_answer_of_every_slave() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        exchange(&mut link, ms(100), 1, Some(record(1)));
        exchange(&mut link, ms(300), 2, Some(record(2)));
        exchange(&mut link, ms(500), 3, None); // slave 3 polled, never answers
        exchange(&mut link, ms(800), 1, Some(record(1))); // next round starts
        let r = watch_round(&mut link, &t, &mut Vec::new(), 3, ms(2000)).unwrap();
        assert!(!r.complete(3));
        assert_eq!(r.missing(3), vec![3]);
    }

    #[test]
    fn a_complete_round_returns_before_the_timeout() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        link.inject(ms(100), &poll(1));
        link.inject(ms(120), b"\x00garbage\r");
        link.inject(ms(150), &record(1));
        let r = watch_round(&mut link, &t, &mut Vec::new(), 1, ms(5000)).unwrap();
        assert!(r.complete(1));
        use crate::maint::Clock;
        assert!(t.now() < ms(1000));
    }

    #[test]
    fn answers_count_only_right_after_the_poll_of_the_same_address() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        link.inject(ms(50), &record(1)); // answer before any poll
        exchange(&mut link, ms(100), 1, None);
        exchange(&mut link, ms(300), 2, Some(record(1))); // address 1 answers the poll of 2
        exchange(&mut link, ms(500), 1, Some(answer(1, 0x5A, &[0u8; 8]))); // INFO not 32 bytes
        let r = watch_round(&mut link, &t, &mut Vec::new(), 2, ms(1500)).unwrap();
        assert!(r.answered.is_empty());
        assert!(!r.complete(2));
    }

    #[test]
    fn polls_and_answers_of_two_rounds_do_not_add_up() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        // round 1: slave 2 silent; round 2: slave 1 silent
        exchange(&mut link, ms(100), 1, Some(record(1)));
        exchange(&mut link, ms(300), 2, None);
        exchange(&mut link, ms(600), 1, None);
        exchange(&mut link, ms(800), 2, Some(record(2)));
        let r = watch_round(&mut link, &t, &mut Vec::new(), 2, ms(1500)).unwrap();
        assert!(!r.complete(2));
        assert_eq!(r.answered.len(), 1);
    }

    fn counting(ok47_from_attempt: u32) -> impl FnMut(&[u8], Duration) -> Vec<(Duration, Vec<u8>)> {
        let n47 = std::cell::Cell::new(0u32);
        move |req, now| match frame::parse(req) {
            Ok(f) if f.rtn == 0x51 => vec![(now + ms(20), answer(f.address, 0, &[1]))],
            Ok(f) if f.rtn == 0x47 => {
                n47.set(n47.get() + 1);
                if n47.get() >= ok47_from_attempt { vec![(now + ms(20), answer(f.address, 0, &[1]))] } else { vec![] }
            }
            _ => vec![],
        }
    }

    #[test]
    fn app_answers_true_when_both_answer_at_once() {
        let mut link = FakeLink::new(VTime::default(), counting(1));
        assert!(app_answers(&mut link, &mut Vec::new(), 1, ms(800)));
    }

    #[test]
    fn app_answers_false_for_a_silent_pack_within_the_time_plus_one_query() {
        let mut link = FakeLink::new(VTime::default(), |_, _| vec![]);
        let t = std::time::Instant::now();
        assert!(!app_answers(&mut link, &mut Vec::new(), 1, ms(500)));
        assert!(t.elapsed() < ms(500) + ms(600) + ms(250), "{:?}", t.elapsed());
    }

    #[test]
    fn app_answers_retries_when_0x47_is_not_ready_yet() {
        let mut link = FakeLink::new(VTime::default(), counting(2));
        assert!(app_answers(&mut link, &mut Vec::new(), 1, ms(2000)));
    }
}
