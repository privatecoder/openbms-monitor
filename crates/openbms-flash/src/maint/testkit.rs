//! Fake link with virtual time: a read with nothing due advances time by 100 ms (the real port's read
//! timeout), `wait` advances by its duration. A responder answers each written request.

use crate::{link::Link, Error, Result};
use openbms_proto::frame;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Default)]
pub struct VTime(Rc<Cell<Duration>>);

impl VTime {
    pub fn advance(&self, d: Duration) {
        self.0.set(self.0.get() + d);
    }
}

impl super::Clock for VTime {
    fn now(&self) -> Duration {
        self.0.get()
    }
}

type Responder = Box<dyn FnMut(&[u8], Duration) -> Vec<(Duration, Vec<u8>)>>;

pub struct FakeLink {
    pub time: VTime,
    incoming: Vec<(Duration, Vec<u8>)>,
    responder: Responder,
    pub sent: Vec<Vec<u8>>,
    /// every write fails (a closed port)
    pub broken: bool,
}

impl FakeLink {
    pub fn new(time: VTime, responder: impl FnMut(&[u8], Duration) -> Vec<(Duration, Vec<u8>)> + 'static) -> Self {
        Self { time, incoming: Vec::new(), responder: Box::new(responder), sent: Vec::new(), broken: false }
    }

    /// Bytes that arrive at `at` without a request (another sender on the bus).
    #[allow(dead_code)] // used by the bus checks (later task)
    pub fn inject(&mut self, at: Duration, bytes: &[u8]) {
        self.incoming.push((at, bytes.to_vec()));
        self.incoming.sort_by_key(|(t, _)| *t);
    }

    fn now(&self) -> Duration {
        use super::Clock;
        self.time.now()
    }
}

impl Link for FakeLink {
    fn clear_input(&mut self) -> Result<()> {
        let now = self.now();
        self.incoming.retain(|(t, _)| *t > now);
        Ok(())
    }

    fn write_raw(&mut self, bytes: &[u8]) -> Result<()> {
        if self.broken {
            return Err(Error::Transport("port closed".into()));
        }
        self.sent.push(bytes.to_vec());
        let now = self.now();
        let replies = (self.responder)(bytes, now);
        self.incoming.extend(replies);
        self.incoming.sort_by_key(|(t, _)| *t);
        Ok(())
    }

    fn read_raw(&mut self, out: &mut [u8]) -> Result<usize> {
        let now = self.now();
        if let Some(pos) = self.incoming.iter().position(|(t, _)| *t <= now) {
            let (_, bytes) = self.incoming.remove(pos);
            let n = bytes.len().min(out.len());
            out[..n].copy_from_slice(&bytes[..n]);
            if n < bytes.len() {
                self.incoming.insert(0, (now, bytes[n..].to_vec()));
            }
            return Ok(n);
        }
        self.time.advance(Duration::from_millis(100));
        Ok(0)
    }

    fn wait(&mut self, d: Duration) {
        self.time.advance(d);
    }
}

/// An ASCII answer frame from `address` with `rtn` and binary `info`.
pub fn answer(address: u8, rtn: u8, info: &[u8]) -> Vec<u8> {
    let hex: String = info.iter().map(|b| format!("{b:02X}")).collect();
    let len = frame::length_field(hex.len()).unwrap();
    let body = format!("20{address:02X}46{rtn:02X}{len:04X}{hex}");
    format!("~{body}{:04X}\r", frame::checksum(body.as_bytes())).into_bytes()
}

pub fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
