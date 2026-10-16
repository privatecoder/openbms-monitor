//! Frame capture: every byte a `Connection` writes or reads while a capture runs, kept in memory for an export.
//!
//! Off by default; one atomic check per write and read then. Recording copies the bytes and pushes an `Arc<Entry>`
//! under a short lock; nothing is decoded or written to disk while capturing. Status, snapshot and clear hold the lock
//! only for counters, pointer copies or a swap. Recording never fails an exchange: a full capture stops itself, a
//! poisoned lock is skipped.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The capture stops by itself at this many entries …
pub const MAX_ENTRIES: usize = 100_000;
/// … or this many captured bytes, whichever comes first.
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
/// One entry keeps at most this much; the rest is only counted.
pub const MAX_ENTRY: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Tx,
    Rx,
    /// an `ascii`/`modbus_read` exchange ended without its answer: the waited time and the address asked
    Timeout { waited_ms: u64, address: u8 },
    /// I/O error or gateway closed
    Error(String),
    /// bootloader traffic left out (bytes sent, bytes received)
    Skipped { sent: usize, received: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// wall-clock time, ms since the Unix epoch
    pub wall_ms: i64,
    /// monotonic ms since the capture started
    pub offset_ms: u64,
    /// the connection's id (unique per process) and label
    pub conn: u64,
    pub label: Arc<str>,
    pub kind: Kind,
    pub bytes: Vec<u8>,
    /// bytes beyond `MAX_ENTRY` that were not kept
    pub truncated: usize,
}

/// Why a capture stopped by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Full {
    Entries,
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    pub running: bool,
    pub entries: usize,
    pub bytes: usize,
    pub timeouts: usize,
    /// ms between the first and the last entry
    pub span_ms: u64,
    pub full: Option<Full>,
}

/// The entries at one moment (pointer copies) and what the export header needs.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub entries: Vec<Arc<Entry>>,
    pub timeouts: usize,
    pub full: Option<Full>,
}

struct Inner {
    entries: Vec<Arc<Entry>>,
    bytes: usize,
    timeouts: usize,
    /// the instant offsets count from; None before the first start and after a clear while stopped
    base: Option<Instant>,
    full: Option<Full>,
}

impl Inner {
    const EMPTY: Inner = Inner { entries: Vec::new(), bytes: 0, timeouts: 0, base: None, full: None };
}

pub struct Capture {
    on: AtomicBool,
    inner: Mutex<Inner>,
}

impl Default for Capture {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-wide capture every `Connection` records into.
pub static GLOBAL: Capture = Capture::new();

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_conn_id() -> u64 {
    NEXT_CONN.fetch_add(1, Ordering::Relaxed)
}

fn wall_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl Capture {
    pub const fn new() -> Self {
        Self { on: AtomicBool::new(false), inner: Mutex::new(Inner::EMPTY) }
    }

    pub fn is_running(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Start or continue the capture. A full capture stays stopped until it is cleared.
    pub fn start(&self) -> Status {
        if let Ok(mut i) = self.inner.lock() {
            if i.full.is_none() {
                i.base.get_or_insert_with(Instant::now);
                self.on.store(true, Ordering::Relaxed);
            }
        }
        self.status()
    }

    pub fn stop(&self) -> Status {
        self.on.store(false, Ordering::Relaxed);
        self.status()
    }

    /// Throw the entries away; a running capture goes on from zero. The old list is dropped outside the lock.
    pub fn clear(&self) -> Status {
        let old = self.inner.lock().ok().map(|mut i| {
            let base = self.is_running().then(Instant::now);
            std::mem::replace(&mut *i, Inner { base, ..Inner::EMPTY })
        });
        drop(old);
        self.status()
    }

    pub fn status(&self) -> Status {
        let running = self.is_running();
        let Ok(i) = self.inner.lock() else { return Status { running, ..Status::default() } };
        let span_ms = match (i.entries.first(), i.entries.last()) {
            (Some(a), Some(b)) => b.offset_ms.saturating_sub(a.offset_ms),
            _ => 0,
        };
        Status { running, entries: i.entries.len(), bytes: i.bytes, timeouts: i.timeouts, span_ms, full: i.full }
    }

    /// The entries as they are now, for an export: pointer copies under the lock, formatting happens outside.
    pub fn snapshot(&self) -> Snapshot {
        let Ok(i) = self.inner.lock() else { return Snapshot::default() };
        Snapshot { entries: i.entries.clone(), timeouts: i.timeouts, full: i.full }
    }

    /// Record one event. Cheap when off; never fails.
    pub(crate) fn record(&self, conn: u64, label: &Arc<str>, kind: Kind, bytes: &[u8]) {
        if !self.is_running() {
            return;
        }
        let now = Instant::now();
        let wall_ms = wall_ms();
        let keep = bytes.len().min(MAX_ENTRY);
        let data = bytes[..keep].to_vec();
        let truncated = bytes.len() - keep;
        let Ok(mut i) = self.inner.lock() else { return };
        // stopped or cleared while this event was on its way
        let (Some(base), true) = (i.base, self.is_running()) else { return };
        if i.entries.len() >= MAX_ENTRIES || i.bytes + data.len() > MAX_BYTES {
            i.full = Some(if i.entries.len() >= MAX_ENTRIES { Full::Entries } else { Full::Bytes });
            self.on.store(false, Ordering::Relaxed);
            return;
        }
        if matches!(kind, Kind::Timeout { .. }) {
            i.timeouts += 1;
        }
        i.bytes += data.len();
        let offset_ms = now.saturating_duration_since(base).as_millis() as u64;
        i.entries.push(Arc::new(Entry { wall_ms, offset_ms, conn, label: label.clone(), kind, bytes: data, truncated }));
    }
}

/// What a `Connection` keeps for the capture: where it records, its id and label, and the bootloader pause.
pub(crate) struct Tap {
    pub sink: &'static Capture,
    pub id: u64,
    pub label: Arc<str>,
    paused: bool,
    skipped: (usize, usize),
}

impl Tap {
    pub fn new(sink: &'static Capture, label: String) -> Self {
        Self { sink, id: next_conn_id(), label: label.into(), paused: false, skipped: (0, 0) }
    }

    pub fn tx(&mut self, bytes: &[u8]) {
        if self.paused {
            self.skipped.0 += bytes.len();
        } else {
            self.sink.record(self.id, &self.label, Kind::Tx, bytes);
        }
    }

    pub fn rx(&mut self, bytes: &[u8]) {
        if self.paused {
            self.skipped.1 += bytes.len();
        } else {
            self.sink.record(self.id, &self.label, Kind::Rx, bytes);
        }
    }

    /// Timeouts and errors; none while paused (the bootloader session reports its own).
    pub fn event(&mut self, kind: Kind) {
        if !self.paused {
            self.sink.record(self.id, &self.label, kind, &[]);
        }
    }

    /// Pause (bootloader) or resume; resuming notes how much was left out.
    pub fn pause(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        if paused {
            self.skipped = (0, 0);
        } else {
            let (sent, received) = std::mem::take(&mut self.skipped);
            self.sink.record(self.id, &self.label, Kind::Skipped { sent, received }, &[]);
        }
    }
}

/// One line of an export: a TX, an assembled RX line, or an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub wall_ms: i64,
    pub offset_ms: u64,
    pub conn: u64,
    pub label: Arc<str>,
    pub kind: Kind,
    pub bytes: Vec<u8>,
    pub truncated: usize,
}

impl Line {
    fn from(e: &Entry) -> Self {
        Line { wall_ms: e.wall_ms, offset_ms: e.offset_ms, conn: e.conn, label: e.label.clone(), kind: e.kind.clone(), bytes: e.bytes.clone(), truncated: e.truncated }
    }
}

/// Assemble the received chunks into lines, per connection: after an ASCII request (or none yet) RX is cut after each
/// `\r`; after a binary request (Modbus) the chunks up to the next event of that connection make one line. A line
/// carries the time of its first chunk; the lines come out in that order.
pub fn lines(entries: &[Arc<Entry>]) -> Vec<Line> {
    struct Conn {
        binary: bool,
        pending: Option<(usize, Line)>,
    }
    let mut conns: HashMap<u64, Conn> = HashMap::new();
    let mut out: Vec<(usize, Line)> = Vec::new();
    for (seq, e) in entries.iter().enumerate() {
        let c = conns.entry(e.conn).or_insert(Conn { binary: false, pending: None });
        if e.kind != Kind::Rx {
            out.extend(c.pending.take());
            if e.kind == Kind::Tx {
                c.binary = e.bytes.first().is_some_and(|b| *b != b'~');
            }
            out.push((seq, Line::from(e)));
            continue;
        }
        let (_, p) = c.pending.get_or_insert_with(|| (seq, Line { bytes: Vec::new(), truncated: 0, ..Line::from(e) }));
        p.bytes.extend_from_slice(&e.bytes);
        p.truncated += e.truncated;
        if c.binary {
            continue;
        }
        while let Some(end) = c.pending.as_ref().and_then(|(_, p)| p.bytes.iter().position(|b| *b == b'\r')) {
            let (s, mut p) = c.pending.take().expect("pending line");
            let rest = p.bytes.split_off(end + 1);
            out.push((s, p));
            if !rest.is_empty() {
                c.pending = Some((seq, Line { bytes: rest, truncated: 0, ..Line::from(e) }));
            }
        }
    }
    out.extend(conns.into_values().filter_map(|c| c.pending));
    out.sort_by_key(|(seq, _)| *seq);
    out.into_iter().map(|(_, l)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink() -> &'static Capture {
        Box::leak(Box::new(Capture::new()))
    }

    fn label(s: &str) -> Arc<str> {
        s.into()
    }

    #[test]
    fn records_only_while_running_and_continues_after_a_stop() {
        let c = sink();
        let l = label("USB test");
        c.record(1, &l, Kind::Tx, b"x");
        assert_eq!(c.status().entries, 0);
        assert!(c.start().running);
        c.record(1, &l, Kind::Tx, b"ab");
        c.record(1, &l, Kind::Timeout { waited_ms: 300, address: 3 }, &[]);
        let s = c.stop();
        assert_eq!((s.running, s.entries, s.bytes, s.timeouts), (false, 2, 2, 1));
        c.record(1, &l, Kind::Tx, b"c");
        assert_eq!(c.status().entries, 2);
        c.start();
        c.record(1, &l, Kind::Rx, b"d");
        let snap = c.snapshot();
        assert_eq!(snap.entries.len(), 3);
        assert!(snap.entries[2].offset_ms >= snap.entries[0].offset_ms);
    }

    #[test]
    fn clear_empties_and_keeps_running() {
        let c = sink();
        let l = label("TCP test");
        c.start();
        c.record(1, &l, Kind::Tx, b"ab");
        let snap = c.snapshot();
        let s = c.clear();
        assert_eq!((s.running, s.entries, s.bytes), (true, 0, 0));
        // a snapshot taken before still has its entries
        assert_eq!(snap.entries.len(), 1);
        c.record(1, &l, Kind::Tx, b"c");
        assert_eq!(c.status().entries, 1);
        c.stop();
        assert_eq!(c.clear().entries, 0);
        c.record(1, &l, Kind::Tx, b"c");
        assert_eq!(c.status().entries, 0);
    }

    #[test]
    fn one_entry_keeps_4_kib_and_counts_the_rest() {
        let c = sink();
        c.start();
        c.record(1, &label("x"), Kind::Tx, &vec![0x55; MAX_ENTRY + 123]);
        let e = c.snapshot().entries[0].clone();
        assert_eq!((e.bytes.len(), e.truncated), (MAX_ENTRY, 123));
        assert_eq!(c.status().bytes, MAX_ENTRY);
    }

    #[test]
    fn stops_itself_at_the_entry_limit_and_stays_full_until_cleared() {
        let c = sink();
        let l = label("x");
        c.start();
        for _ in 0..MAX_ENTRIES {
            c.record(1, &l, Kind::Rx, b"");
        }
        assert!(c.is_running());
        c.record(1, &l, Kind::Rx, b"a");
        let s = c.status();
        assert_eq!((s.running, s.entries, s.full), (false, MAX_ENTRIES, Some(Full::Entries)));
        assert!(!c.start().running);
        let s = c.clear();
        assert_eq!((s.entries, s.full), (0, None));
        assert!(c.start().running);
    }

    #[test]
    fn stops_itself_at_the_byte_limit() {
        let c = sink();
        let l = label("x");
        c.start();
        let chunk = vec![0u8; MAX_ENTRY];
        for _ in 0..MAX_BYTES / MAX_ENTRY {
            c.record(1, &l, Kind::Rx, &chunk);
        }
        assert!(c.is_running());
        c.record(1, &l, Kind::Rx, b"a");
        let s = c.status();
        assert_eq!((s.running, s.bytes, s.full), (false, MAX_BYTES, Some(Full::Bytes)));
    }

    #[test]
    fn the_pause_counts_instead_of_recording_and_notes_it_on_resume() {
        let c = sink();
        c.start();
        let mut t = Tap::new(c, "USB test".into());
        t.tx(b"~20004651\r");
        t.pause(true);
        t.tx(b"::::RFL");
        t.rx(b"::::RFB..");
        t.event(Kind::Timeout { waited_ms: 1, address: 0 });
        t.pause(false);
        t.rx(b"~2000\r");
        let kinds: Vec<Kind> = c.snapshot().entries.iter().map(|e| e.kind.clone()).collect();
        assert_eq!(kinds, vec![Kind::Tx, Kind::Skipped { sent: 7, received: 9 }, Kind::Rx]);
        assert_eq!(c.status().timeouts, 0);
    }

    fn entry(conn: u64, kind: Kind, bytes: &[u8], t: u64) -> Arc<Entry> {
        Arc::new(Entry { wall_ms: t as i64, offset_ms: t, conn, label: format!("c{conn}").into(), kind, bytes: bytes.to_vec(), truncated: 0 })
    }

    #[test]
    fn ascii_lines_are_cut_at_cr_across_chunks_and_per_connection() {
        let es = vec![
            entry(1, Kind::Tx, b"~20004642E00200FD37\r", 0),
            entry(2, Kind::Tx, b"~20014642E00201FD35\r", 1),
            entry(1, Kind::Rx, b"~2000460010", 10),
            entry(2, Kind::Rx, b"~200146", 11),
            entry(1, Kind::Rx, b"AB\r~2005", 12),
            entry(2, Kind::Rx, b"0000FDAB\r", 13),
            entry(1, Kind::Rx, b"4600\rtail", 14),
            entry(1, Kind::Timeout { waited_ms: 1500, address: 0 }, b"", 20),
        ];
        let ls = lines(&es);
        let view: Vec<(u64, u64, &[u8])> = ls.iter().map(|l| (l.conn, l.offset_ms, l.bytes.as_slice())).collect();
        assert_eq!(view, vec![
            (1, 0, &b"~20004642E00200FD37\r"[..]),
            (2, 1, b"~20014642E00201FD35\r"),
            (1, 10, b"~2000460010AB\r"),
            (2, 11, b"~2001460000FDAB\r"),
            (1, 12, b"~20054600\r"),
            (1, 14, b"tail"),
            (1, 20, b""),
        ]);
        assert_eq!(ls[6].kind, Kind::Timeout { waited_ms: 1500, address: 0 });
    }

    #[test]
    fn binary_answers_stay_one_line_up_to_the_next_event() {
        // a Modbus answer may carry 0x0D; it is not cut there
        let es = vec![
            entry(1, Kind::Tx, &[0x01, 0x03, 0x10, 0x00, 0x00, 0x15, 0x80, 0xC5], 0),
            entry(1, Kind::Rx, &[0x01, 0x03, 0x2A, 0x0D], 5),
            entry(1, Kind::Rx, &[0x00, 0x0D, 0x7E], 6),
            entry(1, Kind::Tx, b"~20004651\r", 9),
            entry(1, Kind::Rx, b"~20\r", 10),
        ];
        let ls = lines(&es);
        assert_eq!(ls.len(), 4);
        assert_eq!((ls[1].offset_ms, ls[1].bytes.as_slice()), (5, &[0x01, 0x03, 0x2A, 0x0D, 0x00, 0x0D, 0x7E][..]));
        assert_eq!(ls[3].bytes, b"~20\r");
        // a binary run still pending at the end is kept
        let ls = lines(&es[..3]);
        assert_eq!(ls.len(), 2);
    }
}
