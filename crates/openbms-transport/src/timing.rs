//! Every wait for a BMS answer, in one place. Two kinds:
//!
//! - **Answer times** (how long an answer may take on the wire): fixed floors here; over a network gateway (TCP) the
//!   connection measures how long valid answers really take and waits longer where they are slow
//!   (`Latency::budget`). Never shorter than the floor, never longer than the class's cap.
//! - **Fixed times** that are not network delay: device times (a restart, a maintenance change, the bootloader),
//!   protocol gaps and silence that is itself a finding (the restart watch). They stay as they are on every link.
//!
//! The bootloader and flash times live in `openbms_flash::session::Options`, the 64/xx state changes in
//! `openbms_flash::maint::dialog::Budgets`; both are fixed device times and do not learn from the link.

use std::collections::VecDeque;
use std::time::Duration;

const fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

// ---- answer times (floors; adaptive over TCP) ----

/// A Seplos ASCII answer (0x42, 0x44, 0x47, 0x51, A1–A6, 0x45, 0x4B …).
pub const ASCII_ANSWER: Duration = ms(1500);
/// A Modbus answer (FC03/04, master on the CAN socket).
pub const MODBUS_ANSWER: Duration = ms(1500);
/// The whole answer of the binary parameter service (0x47/A1 over UART1; a gateway may deliver it in several parts).
pub const BINARY_PARAMS: Duration = ms(3000);
/// A binary serial number answer (37 bytes): short, so packs without it do not stretch a connect.
pub const BINARY_SERIAL: Duration = ms(800);
/// A pack's own 64/03 and the master's at connect: older slaves stay silent, so each costs at most this.
pub const PACK_IDENTITY: Duration = ms(1500);
/// One try of the reachability check (0x51 to each address, stop at the first answer).
pub const REACH_TRY: Duration = ms(300);
/// The check whether a pack answers 0x47 (RS485-1/2 knows it, the CAN socket does not), and the bus scan per address.
pub const CAPABILITY_TRY: Duration = ms(600);
/// A probe whose silence is a finding (`REACH_TRY`, `CAPABILITY_TRY`, `PACK_IDENTITY`) learns at most up to this many
/// times its floor: a scan of silent addresses must stay within `PORT_WAIT`.
pub const PROBE_STRETCH: u32 = 2;
/// 64/02 and the start/stop answer of the patched master (`openbms_flash::maint::dialog::Budgets::answer`).
pub const MAINT_ANSWER: Duration = ms(600);

// ---- fixed times (never adaptive) ----

/// TCP connect including name resolution (a stale route blocks the OS for over a minute).
pub const CONNECT: Duration = ms(5000);
/// One read of the port: what arrived within this, or nothing yet.
pub const READ_CHUNK: Duration = ms(100);
/// A TCP write: a dead link must not block it either.
pub const TCP_WRITE: Duration = ms(1500);
/// Pause after each exchange; the BMS needs a short gap between requests.
pub const GAP: Duration = ms(100);
/// The answer to a restart (0x45 type 6): the board resets before it can answer, so a missing one is expected.
pub const RESTART_ANSWER: Duration = ms(300);
/// One 0x51 try of the restart watch; tied to `SILENT_FOR`, so a longer try would hide a short restart.
pub const WATCH_TRY: Duration = ms(600);
/// Silence that counts as an interruption in the restart watch: so many unanswered tries in a row, or this long.
pub const SILENT_TRIES: u32 = 2;
pub const SILENT_FOR: Duration = ms(1000);
/// How long a restarted pack may take to answer again.
pub const COMEBACK_LIMIT: Duration = ms(15_000);
/// From the master's start/stop answer until it shows the new state, and the pause between its status reads.
pub const MAINT_CHANGE: Duration = ms(3000);
pub const MAINT_INTERVAL: Duration = ms(300);
/// Minimum time after a maintenance stop when the slave count does not change (N = 0): one full master cycle.
pub const ZERO_SLAVES_SETTLE: Duration = ms(1100);
/// A short listen on the pack bus: a little more than one polling cycle of the master (every slave once a second).
pub const BUS_LISTEN: Duration = ms(1200);
/// Bootloader (`openbms_flash::session::Options`): after the reset until RFL, each bootloader acknowledgement, the
/// application's comeback after the flash.
pub const BOOT_DELAY: Duration = ms(50);
pub const BOOT_ACK: Duration = ms(10_000);
pub const BOOT_COMEBACK: Duration = ms(20_000);
/// The ASCII checks around a flash (before: 0x51/0x47; after: 0x42/0x44) and one 0x51 try while the application
/// comes back, with the pause between tries.
pub const FLASH_ASCII: Duration = ms(600);
pub const FLASH_REACH_TRY: Duration = ms(300);
pub const FLASH_REACH_PAUSE: Duration = ms(100);
/// After a failed flash: how long the pack may take to answer as an application again, and the pause between tries.
pub const RECOVERY: Duration = ms(6000);
pub const RECOVERY_PAUSE: Duration = ms(200);
/// Polling: how long one round may wait for packs that stay silent. Once its silent waits add up to this, the round
/// ends (one pack silent last time is still asked, in turns, so a pack that comes back is seen). Answers cost only
/// their own time, so between two answers of the bank lie at most this plus one longer wait: well within the UI's
/// 60 s for a lost connection, however long the learnt waits are.
pub const ROUND_SILENT_BUDGET: Duration = ms(30_000);
/// How long a user action waits for its pack, and for the port, before it reports „busy“ (app). The port wait covers
/// one pack scan of silent addresses at their longest (`CAPABILITY_TRY` × `PROBE_STRETCH` × 15) plus the bank check.
pub const PACK_WAIT: Duration = ms(5000);
pub const PORT_WAIT: Duration = ms(25_000);

// ---- learning ----

/// What kind of answer a sample belongs to: their sizes and baud rates differ too much for one average.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Ascii,
    Modbus,
    Binary,
}

impl Class {
    pub const ALL: [Class; 3] = [Class::Ascii, Class::Modbus, Class::Binary];

    /// The longest answer time learning may reach.
    pub const fn cap(self) -> Duration {
        match self {
            Class::Ascii | Class::Modbus => ms(6000),
            Class::Binary => ms(8000),
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Samples kept per class, and how many are needed before they count.
pub const WINDOW: usize = 32;
pub const MIN_SAMPLES: usize = 8;
/// Added to the 90th percentile: at least this, or half the percentile.
pub const MARGIN: Duration = ms(250);

/// The answer times of valid answers on one open connection (RAM only, gone with the connection). Only a complete
/// answer that belongs to the request is a sample; a timeout, an echo, a foreign frame or a broken one never is.
#[derive(Debug, Default, Clone)]
pub struct Latency {
    adaptive: bool,
    samples: [VecDeque<Duration>; 3],
}

/// One class as the UI shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ClassView {
    pub class: Class,
    pub samples: usize,
    /// None: too few samples yet
    pub p90_ms: Option<u64>,
    pub max_ms: Option<u64>,
    /// the answer time a normal request of this class waits now
    pub budget_ms: u64,
}

impl Latency {
    /// `adaptive`: learn (a network gateway); else every budget is its floor.
    pub fn new(adaptive: bool) -> Self {
        Self { adaptive, samples: Default::default() }
    }

    pub fn adaptive(&self) -> bool {
        self.adaptive
    }

    pub fn record(&mut self, class: Class, took: Duration) {
        let s = &mut self.samples[class.index()];
        if s.len() == WINDOW {
            s.pop_front();
        }
        s.push_back(took);
    }

    fn p90(&self, class: Class) -> Option<Duration> {
        let s = &self.samples[class.index()];
        if s.len() < MIN_SAMPLES {
            return None;
        }
        let mut v: Vec<Duration> = s.iter().copied().collect();
        v.sort();
        // nearest rank: the smallest sample at or above 90 %
        Some(v[(v.len() * 9).div_ceil(10) - 1])
    }

    /// How long to wait for an answer of `class` whose fixed time is `floor`: the floor until enough answers came,
    /// then the 90th percentile plus a margin if that is longer, at most the class's cap (or the floor if that is
    /// higher). Never below the floor.
    pub fn budget(&self, class: Class, floor: Duration) -> Duration {
        if !self.adaptive {
            return floor;
        }
        match self.p90(class) {
            Some(p) => (p + MARGIN.max(p / 2)).min(class.cap()).max(floor),
            None => floor,
        }
    }

    /// `budget` for a probe whose silence is a finding: at most `PROBE_STRETCH` times its floor.
    pub fn probe_budget(&self, class: Class, floor: Duration) -> Duration {
        self.budget(class, floor).min(floor * PROBE_STRETCH)
    }

    /// The longer wait of a read of a pack that answered before (TCP): twice the budget, capped.
    pub fn retry_budget(&self, class: Class, floor: Duration) -> Duration {
        (self.budget(class, floor) * 2).min(class.cap().max(floor))
    }

    pub fn view(&self, floor: impl Fn(Class) -> Duration) -> Vec<ClassView> {
        Class::ALL
            .iter()
            .map(|&c| {
                let s = &self.samples[c.index()];
                ClassView {
                    class: c,
                    samples: s.len(),
                    p90_ms: self.p90(c).map(|d| d.as_millis() as u64),
                    max_ms: s.iter().max().map(|d| d.as_millis() as u64),
                    budget_ms: self.budget(c, floor(c)).as_millis() as u64,
                }
            })
            .collect()
    }
}

/// The floor of a normal request of each class (what `ClassView::budget_ms` refers to).
pub fn normal_floor(class: Class) -> Duration {
    match class {
        Class::Ascii => ASCII_ANSWER,
        Class::Modbus => MODBUS_ANSWER,
        Class::Binary => BINARY_PARAMS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(adaptive: bool, class: Class, samples: &[u64]) -> Latency {
        let mut l = Latency::new(adaptive);
        for s in samples {
            l.record(class, ms(*s));
        }
        l
    }

    #[test]
    fn a_local_link_keeps_every_floor() {
        let l = with(false, Class::Ascii, &[4000; 20]);
        assert_eq!(l.budget(Class::Ascii, ASCII_ANSWER), ASCII_ANSWER);
        assert_eq!(l.retry_budget(Class::Ascii, ASCII_ANSWER), ms(3000));
    }

    #[test]
    fn too_few_samples_keep_the_floor() {
        let l = with(true, Class::Ascii, &[2000; 7]);
        assert_eq!(l.budget(Class::Ascii, ASCII_ANSWER), ASCII_ANSWER);
    }

    #[test]
    fn fast_answers_never_shorten_the_floor() {
        let l = with(true, Class::Ascii, &[80; 32]);
        assert_eq!(l.budget(Class::Ascii, ASCII_ANSWER), ASCII_ANSWER);
        // a short probe still gets the margin above the answers actually seen
        assert_eq!(l.budget(Class::Ascii, REACH_TRY), ms(330));
    }

    #[test]
    fn a_probe_stretches_at_most_twice_its_floor() {
        let l = with(true, Class::Ascii, &[2000; 10]);
        assert_eq!(l.budget(Class::Ascii, REACH_TRY), ms(3000));
        assert_eq!(l.probe_budget(Class::Ascii, REACH_TRY), ms(600));
        assert_eq!(l.probe_budget(Class::Ascii, PACK_IDENTITY), ms(3000));
        assert_eq!(Latency::new(false).probe_budget(Class::Ascii, CAPABILITY_TRY), CAPABILITY_TRY);
        // a scan of 15 silent addresses at the longest still fits the port wait
        assert!(CAPABILITY_TRY * PROBE_STRETCH * 15 < PORT_WAIT);
    }

    #[test]
    fn slow_answers_lengthen_by_the_90th_percentile_plus_margin() {
        // 9 of 10 within 1200 ms, one at 1400: p90 = 1200, margin max(250, 600) = 600
        let l = with(true, Class::Ascii, &[100, 200, 300, 400, 500, 600, 800, 1000, 1200, 1400]);
        assert_eq!(l.budget(Class::Ascii, ASCII_ANSWER), ms(1800));
        // a short probe learns from the same answers
        assert_eq!(l.budget(Class::Ascii, REACH_TRY), ms(1800));
    }

    #[test]
    fn the_cap_holds_and_a_higher_floor_wins_over_it() {
        let l = with(true, Class::Ascii, &[5000; 10]);
        assert_eq!(l.budget(Class::Ascii, ASCII_ANSWER), Class::Ascii.cap());
        assert_eq!(l.budget(Class::Ascii, ms(9000)), ms(9000));
        assert_eq!(l.retry_budget(Class::Ascii, ASCII_ANSWER), Class::Ascii.cap());
    }

    #[test]
    fn classes_learn_apart_and_the_window_forgets() {
        let mut l = with(true, Class::Binary, &[2900; 10]);
        assert_eq!(l.budget(Class::Ascii, ASCII_ANSWER), ASCII_ANSWER);
        assert_eq!(l.budget(Class::Binary, BINARY_PARAMS), ms(2900 + 1450));
        for _ in 0..WINDOW {
            l.record(Class::Binary, ms(100));
        }
        assert_eq!(l.budget(Class::Binary, BINARY_PARAMS), BINARY_PARAMS);
    }

    #[test]
    fn the_view_shows_samples_and_budgets() {
        let l = with(true, Class::Modbus, &[100, 200, 300, 400, 500, 600, 700, 2000]);
        let v = l.view(normal_floor);
        let m = v.iter().find(|c| c.class == Class::Modbus).unwrap();
        assert_eq!((m.samples, m.p90_ms, m.max_ms, m.budget_ms), (8, Some(2000), Some(2000), 3000));
        assert_eq!(v.iter().find(|c| c.class == Class::Ascii).unwrap().p90_ms, None);
    }
}
