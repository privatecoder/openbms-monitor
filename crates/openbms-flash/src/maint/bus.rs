//! Pack bus (RS485-1/2) around maintenance: a short listen tells a quiet bus from a polling master, and after
//! a failed flash whether the pack answers as an application again.

use super::Clock;
use crate::{link::Link, session, Result};
use openbms_proto::frame;
use std::collections::BTreeSet;
use std::io::Write;
use std::time::Duration;

const ASCII_TIMEOUT: Duration = Duration::from_millis(600);

/// What a short listen on the pack bus saw. Sends nothing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Traffic {
    /// bytes received
    pub bytes: usize,
    /// addresses the master polled (CID2 0x5A, empty INFO)
    pub polled: BTreeSet<u8>,
}

impl Traffic {
    pub fn quiet(&self) -> bool {
        self.bytes == 0
    }
}

/// Listen for `window` without sending. The master polls every slave once a second, so a window of a little
/// more than a second sees every polled address.
pub fn listen(link: &mut impl Link, clock: &impl Clock, window: Duration) -> Result<Traffic> {
    link.clear_input()?;
    let deadline = clock.now() + window;
    let mut seen = Traffic::default();
    let mut pending = Vec::new();
    let mut buf = [0u8; 512];
    while clock.now() < deadline {
        let n = link.read_raw(&mut buf)?;
        seen.bytes += n;
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\r') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let Some(start) = line.iter().position(|b| *b == b'~') else { continue };
            if let Ok(f) = frame::parse(&line[start..]) {
                if f.rtn == 0x5A && f.info.is_empty() {
                    seen.polled.insert(f.address);
                }
            }
        }
        if pending.len() > 4096 {
            pending.clear();
        }
    }
    Ok(seen)
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

    #[test]
    fn a_quiet_bus_and_a_polling_master() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        let seen = listen(&mut link, &t, ms(1200)).unwrap();
        assert!(seen.quiet() && seen.polled.is_empty());
        assert!(link.sent.is_empty(), "listening sends nothing");
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        for (at, a) in [(1300, 1), (1450, 2), (1600, 3)] {
            link.inject(ms(at), &poll(a));
        }
        link.inject(ms(1500), &answer(1, 0x5A, &[0u8; 32]));
        let seen = listen(&mut link, &t, ms(1200)).unwrap();
        assert!(!seen.quiet());
        assert_eq!(seen.polled.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn other_traffic_is_not_quiet_but_names_no_poll() {
        let t = VTime::default();
        let mut link = FakeLink::new(t.clone(), |_, _| vec![]);
        link.inject(ms(100), b"\x00garbage\r");
        let seen = listen(&mut link, &t, ms(1200)).unwrap();
        assert!(!seen.quiet() && seen.polled.is_empty());
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
