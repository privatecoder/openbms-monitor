//! Notifications (design `docs/superpowers/specs/2026-10-16-multi-write-health-alerts.md` part C, r2).
//!
//! The poll thread hands every answer of its connection (link generation) to `Notifier::answer` and calls `flush` after
//! each round; a ticker calls `tick` for the lost connection. The UI's merged live state (which keeps old values after
//! a failed poll) is never used. An answer counts only for a confirmed pack (`bank::notify_allowed`: on RS485-1/2 a
//! board known here, on the CAN socket a master whose battery serial matches the installation's); an unconfirmed one
//! forgets what was known about the pack.
//!
//! Triggers: a protection or fault alarm starts (again only after it cleared); a pack that answered misses
//! `SILENT_POLLS` polls in a row (again only after it answered); the rest cell spread stays above the threshold for
//! `SPREAD_HOLD_MS` (re-armed below threshold − `SPREAD_REARM_MV`); no confirmed answer for the set minutes while the
//! connection is wanted. At most one notification per pack and trigger (each alarm its own) per `RATE_MS`; what fires
//! in one round is joined per trigger. This module decides; `send` words it (`Texts`, from the UI) and shows it.

use openbms_proto::status::{ActiveAlarm, Severity};
use openbms_proto::{SerialValue, Status, Telemetry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const SILENT_POLLS: u32 = 3;
pub const SPREAD_HOLD_MS: i64 = 5 * 60_000;
pub const SPREAD_REARM_MV: f64 = 10.0;
/// rest as in the health report: |current| below 2 A, this answer and the one before
pub const REST_A: f64 = 2.0;
pub const RATE_MS: i64 = 10 * 60_000;

/// An installation's notification settings (key `notifications`); off by default.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub spread_mv: u32,
    pub lost_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, spread_mv: 50, lost_minutes: 5 }
    }
}

impl Settings {
    /// The stored text; none or an unreadable one is off.
    pub fn parse(text: Option<&str>) -> Self {
        let s: Self = text.and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
        Self { spread_mv: s.spread_mv.clamp(10, 1000), lost_minutes: s.lost_minutes.clamp(1, 24 * 60), ..s }
    }
}

/// CAN socket: the master's battery serial (Modbus, maybe only its first 22 characters) equals the one stored for the
/// installation's address 0.
pub fn master_matches(stored: Option<&str>, read: &SerialValue) -> bool {
    match (stored, read) {
        (Some(s), SerialValue::Set { value, truncated }) if !value.is_empty() => if *truncated { s.starts_with(value.as_str()) } else { s == value },
        _ => false,
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Trigger {
    /// `id` as in the status ("alarm.ev2.b1")
    Alarm { id: String },
    Silent,
    Spread,
    Lost,
}

/// One poll answer as far as notifications need it; None for a pack that did not answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    /// active protection and fault alarms; None: the status was not read
    pub alarms: Option<Vec<String>>,
    pub current: Option<f64>,
    pub spread_mv: Option<f64>,
}

impl Reading {
    pub fn of(t: Option<&Telemetry>, s: Option<&Status>) -> Option<Self> {
        if t.is_none() && s.is_none() {
            return None;
        }
        let spread = t.and_then(|t| {
            let max = t.cell_voltages.iter().copied().fold(f64::NAN, f64::max);
            let min = t.cell_voltages.iter().copied().fold(f64::NAN, f64::min);
            (!t.cell_voltages.is_empty()).then_some((max - min) * 1000.0)
        });
        Some(Self {
            alarms: s.map(|s| alarm_ids(&s.alarms)),
            current: t.map(|t| t.current),
            spread_mv: spread,
        })
    }
}

/// The ids of the active protection and fault alarms.
pub fn alarm_ids(alarms: &[ActiveAlarm]) -> Vec<String> {
    alarms.iter().filter(|a| matches!(a.severity, Severity::Protection | Severity::Fault)).map(|a| a.id.clone()).collect()
}

/// One notification: a trigger and the packs it fired for in one round.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Note {
    pub trigger: Trigger,
    pub packs: Vec<u8>,
    /// Spread: the highest rest spread of the packs, and the threshold
    pub spread_mv: Option<u32>,
    pub limit_mv: Option<u32>,
    /// Lost: the minutes set
    pub minutes: Option<u32>,
}

#[derive(Default, Debug)]
struct Pack {
    answered: bool,
    misses: u32,
    silent: bool,
    alarms: BTreeSet<String>,
    rest_before: bool,
    over_since: Option<i64>,
    spread_fired: bool,
}

/// The state of one connection's notifications; per-pack state lives for one link generation, the rate limit and the
/// lost watch for the installation (a reconnect after a lost link keeps them).
#[derive(Default, Debug)]
pub struct Notifier {
    gen: u64,
    installation: Option<String>,
    name: String,
    settings: Settings,
    packs: BTreeMap<u8, Pack>,
    pending: Vec<(Trigger, u8, Option<u32>)>,
    sent: HashMap<(Trigger, Option<u8>), i64>,
    /// the connection is wanted (not closed by the user or a maintenance step)
    armed: bool,
    /// the last confirmed answer, for the lost connection
    last_data: Option<i64>,
    lost_sent: bool,
}

impl Notifier {
    /// A new connection (generation `gen`) for `installation` (None: „Nur ansehen“, nothing is watched).
    pub fn begin(&mut self, gen: u64, installation: Option<String>, name: String, settings: Settings) {
        if self.installation != installation {
            self.sent.clear();
            self.last_data = None;
            self.lost_sent = false;
        }
        self.armed = installation.is_some();
        self.gen = gen;
        self.installation = installation;
        self.name = name;
        self.settings = settings;
        self.packs.clear();
        self.pending.clear();
    }

    /// The connection ended. `lost`: the link died and the UI reconnects, the lost watch goes on; else (closed by the
    /// user, taken by a maintenance step) nothing is watched any more.
    pub fn end(&mut self, lost: bool) {
        self.packs.clear();
        self.pending.clear();
        if !lost {
            self.armed = false;
            self.last_data = None;
            self.lost_sent = false;
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The settings of `installation` changed; only the watched one counts.
    pub fn set_settings(&mut self, installation: &str, settings: Settings) {
        if self.installation.as_deref() == Some(installation) {
            self.settings = settings;
        }
    }

    /// One poll of `address` on connection `gen`; `confirmed`: its bank verdict allows notifications.
    pub fn answer(&mut self, gen: u64, address: u8, now: i64, confirmed: bool, reading: Option<&Reading>) {
        if gen != self.gen || self.installation.is_none() {
            return;
        }
        if !confirmed {
            self.packs.remove(&address);
            return;
        }
        let p = self.packs.entry(address).or_default();
        let Some(r) = reading else {
            if p.answered && !p.silent {
                p.misses += 1;
                if p.misses >= SILENT_POLLS {
                    p.silent = true;
                    self.pending.push((Trigger::Silent, address, None));
                }
            }
            return;
        };
        self.last_data = Some(now);
        self.lost_sent = false;
        p.answered = true;
        p.misses = 0;
        p.silent = false;
        if let Some(alarms) = &r.alarms {
            let active: BTreeSet<String> = alarms.iter().cloned().collect();
            for id in active.difference(&p.alarms) {
                self.pending.push((Trigger::Alarm { id: id.clone() }, address, None));
            }
            p.alarms = active;
        }
        if let (Some(i), Some(mv)) = (r.current, r.spread_mv) {
            let rest = i > -REST_A && i < REST_A;
            let at_rest = rest && p.rest_before;
            p.rest_before = rest;
            let limit = f64::from(self.settings.spread_mv);
            if !at_rest {
                // under load (or just after it) the spread says little: the five minutes start again at rest
                p.over_since = None;
            } else if mv > limit {
                let since = *p.over_since.get_or_insert(now);
                if !p.spread_fired && now - since >= SPREAD_HOLD_MS {
                    p.spread_fired = true;
                    self.pending.push((Trigger::Spread, address, Some(mv.round() as u32)));
                }
            } else {
                p.over_since = None;
                if mv < limit - SPREAD_REARM_MV {
                    p.spread_fired = false;
                }
            }
        }
    }

    /// The end of a polling round: what fired, joined per trigger, without what the rate limit holds back. Nothing
    /// while switched off (what fired meanwhile is not sent later).
    pub fn flush(&mut self, now: i64) -> Vec<Note> {
        let pending = std::mem::take(&mut self.pending);
        if !self.settings.enabled {
            return vec![];
        }
        let mut notes: BTreeMap<Trigger, Note> = BTreeMap::new();
        for (trigger, address, mv) in pending {
            if !self.allow(&trigger, Some(address), now) {
                continue;
            }
            let limit = (trigger == Trigger::Spread).then_some(self.settings.spread_mv);
            let n = notes.entry(trigger.clone()).or_insert(Note { trigger, packs: vec![], spread_mv: None, limit_mv: limit, minutes: None });
            if !n.packs.contains(&address) {
                n.packs.push(address);
            }
            n.spread_mv = n.spread_mv.max(mv);
        }
        let mut out: Vec<Note> = notes.into_values().collect();
        out.iter_mut().for_each(|n| n.packs.sort_unstable());
        out
    }

    /// The lost connection: no confirmed answer for the set minutes while the connection is wanted; once until data
    /// comes again.
    pub fn tick(&mut self, now: i64) -> Option<Note> {
        if !self.armed || !self.settings.enabled || self.lost_sent {
            return None;
        }
        let last = self.last_data?;
        if now - last < i64::from(self.settings.lost_minutes) * 60_000 {
            return None;
        }
        self.lost_sent = true;
        self.allow(&Trigger::Lost, None, now).then(|| Note { trigger: Trigger::Lost, packs: vec![], spread_mv: None, limit_mv: None, minutes: Some(self.settings.lost_minutes) })
    }

    /// Polling paused on purpose (a BMS history read): that time is no lost connection.
    pub fn touch(&mut self, now: i64) {
        if self.last_data.is_some() {
            self.last_data = Some(now);
        }
    }

    fn allow(&mut self, trigger: &Trigger, address: Option<u8>, now: i64) -> bool {
        let key = (trigger.clone(), address);
        if self.sent.get(&key).is_some_and(|t| now - t < RATE_MS) {
            return false;
        }
        self.sent.insert(key, now);
        true
    }
}

/// The wording, from the UI in its language (`notify_texts`); English until it arrives. Placeholders: `{{name}}`
/// installation, `{{packs}}` pack list, `{{alarm}}`, `{{mv}}`, `{{limit}}`, `{{min}}`.
#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Texts {
    pub title: String,
    pub alarm_one: String,
    pub alarm_other: String,
    pub silent_one: String,
    pub silent_other: String,
    pub spread_one: String,
    pub spread_other: String,
    pub lost: String,
    pub test: String,
    /// alarm id → its name
    pub alarms: HashMap<String, String>,
}

impl Default for Texts {
    fn default() -> Self {
        Self {
            title: "OpenBMS Monitor: {{name}}".into(),
            alarm_one: "Pack {{packs}}: {{alarm}}".into(),
            alarm_other: "{{alarm}} on packs {{packs}}".into(),
            silent_one: "Pack {{packs}} stopped answering".into(),
            silent_other: "Packs {{packs}} stopped answering".into(),
            spread_one: "Pack {{packs}}: cell spread at rest {{mv}} mV (threshold {{limit}} mV)".into(),
            spread_other: "Cell spread at rest above {{limit}} mV on packs {{packs}}".into(),
            lost: "No data for {{min}} min: connection lost".into(),
            test: "Test notification: notifications work.".into(),
            alarms: HashMap::new(),
        }
    }
}

fn fill(template: &str, vars: &[(&str, String)]) -> String {
    vars.iter().fold(template.to_string(), |s, (k, v)| s.replace(&format!("{{{{{k}}}}}"), v))
}

/// Title and body of a notification.
pub fn render(note: &Note, texts: &Texts, name: &str) -> (String, String) {
    let packs = note.packs.iter().map(|a| format!("{a:02}")).collect::<Vec<_>>().join(", ");
    let one = note.packs.len() == 1;
    let template = match (&note.trigger, one) {
        (Trigger::Alarm { .. }, true) => &texts.alarm_one,
        (Trigger::Alarm { .. }, false) => &texts.alarm_other,
        (Trigger::Silent, true) => &texts.silent_one,
        (Trigger::Silent, false) => &texts.silent_other,
        (Trigger::Spread, true) => &texts.spread_one,
        (Trigger::Spread, false) => &texts.spread_other,
        (Trigger::Lost, _) => &texts.lost,
    };
    let alarm = match &note.trigger {
        Trigger::Alarm { id } => texts.alarms.get(id).cloned().unwrap_or_else(|| id.clone()),
        _ => String::new(),
    };
    let num = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
    let vars = [("name", name.to_string()), ("packs", packs), ("alarm", alarm), ("mv", num(note.spread_mv)), ("limit", num(note.limit_mv)), ("min", num(note.minutes))];
    (fill(&texts.title, &vars), fill(template, &vars))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;
    const OV: &str = "alarm.ev2.b1";

    fn on() -> Settings {
        Settings { enabled: true, ..Default::default() }
    }

    fn watching(gen: u64) -> Notifier {
        let mut n = Notifier::default();
        n.begin(gen, Some("inst-a".into()), "A".into(), on());
        n
    }

    fn idle(spread_mv: f64) -> Reading {
        Reading { alarms: Some(vec![]), current: Some(0.0), spread_mv: Some(spread_mv) }
    }

    fn alarm(ids: &[&str]) -> Reading {
        Reading { alarms: Some(ids.iter().map(|s| s.to_string()).collect()), ..idle(10.0) }
    }

    fn triggers(notes: &[Note]) -> Vec<(Trigger, Vec<u8>)> {
        notes.iter().map(|n| (n.trigger.clone(), n.packs.clone())).collect()
    }

    fn ov() -> Trigger {
        Trigger::Alarm { id: OV.into() }
    }

    #[test]
    fn an_alarm_fires_once_per_start_until_it_cleared() {
        let mut n = watching(1);
        n.answer(1, 3, 0, true, Some(&alarm(&[OV])));
        assert_eq!(triggers(&n.flush(0)), vec![(ov(), vec![3])]);
        // still active: nothing more
        n.answer(1, 3, MIN, true, Some(&alarm(&[OV])));
        assert!(n.flush(MIN).is_empty());
        // a status not read changes nothing
        n.answer(1, 3, 2 * MIN, true, Some(&Reading { alarms: None, ..idle(10.0) }));
        assert!(n.flush(2 * MIN).is_empty());
        // cleared and back after the rate limit: fires again
        n.answer(1, 3, 3 * MIN, true, Some(&alarm(&[])));
        n.answer(1, 3, 11 * MIN, true, Some(&alarm(&[OV])));
        assert_eq!(triggers(&n.flush(11 * MIN)), vec![(ov(), vec![3])]);
    }

    #[test]
    fn warnings_and_info_never_fire() {
        // event 2 bit 0 is a warning, bit 1 a protection; event 4 bit 6 info, bit 7 a fault
        let active = openbms_proto::status::active_alarms(&[0, 0b0000_0011, 0, 0b1100_0000, 0, 0, 0, 0]);
        assert_eq!(alarm_ids(&active), vec![OV.to_string(), "alarm.ev4.b7".to_string()]);
        assert_eq!(Reading::of(None, None), None, "no answer");
    }

    #[test]
    fn a_silent_pack_fires_after_three_polls_and_again_only_after_it_answered() {
        let mut n = watching(1);
        // never answered on this connection: no notice
        for t in 0..5 {
            n.answer(1, 5, t, true, None);
        }
        assert!(n.flush(5).is_empty());
        n.answer(1, 5, 10, true, Some(&idle(10.0)));
        n.answer(1, 5, 20, true, None);
        n.answer(1, 5, 30, true, None);
        assert!(n.flush(30).is_empty());
        n.answer(1, 5, 40, true, None);
        assert_eq!(triggers(&n.flush(40)), vec![(Trigger::Silent, vec![5])]);
        n.answer(1, 5, 50, true, None);
        assert!(n.flush(50).is_empty(), "once");
        // answered again, silent again after the rate limit: fires again
        n.answer(1, 5, 11 * MIN, true, Some(&idle(10.0)));
        for t in 1..=3 {
            n.answer(1, 5, 11 * MIN + t, true, None);
        }
        assert_eq!(triggers(&n.flush(11 * MIN + 3)), vec![(Trigger::Silent, vec![5])]);
    }

    #[test]
    fn the_rest_spread_fires_after_five_minutes_and_rearms_below_the_hysteresis() {
        let mut n = watching(1);
        let poll = |n: &mut Notifier, t: i64, r: Reading| {
            n.answer(1, 2, t, true, Some(&r));
            n.flush(t)
        };
        // the first rest sample after load does not count (the one before was not at rest)
        assert!(poll(&mut n, 0, Reading { current: Some(-30.0), ..idle(80.0) }).is_empty());
        assert!(poll(&mut n, MIN, idle(60.0)).is_empty());
        assert!(poll(&mut n, 2 * MIN, idle(60.0)).is_empty(), "the five minutes start here");
        assert!(poll(&mut n, 6 * MIN, idle(61.0)).is_empty());
        let notes = poll(&mut n, 7 * MIN, idle(62.0));
        assert_eq!(triggers(&notes), vec![(Trigger::Spread, vec![2])]);
        assert_eq!((notes[0].spread_mv, notes[0].limit_mv), (Some(62), Some(50)));
        // below the threshold but within the hysteresis: not re-armed
        assert!(poll(&mut n, 8 * MIN, idle(45.0)).is_empty());
        for t in 9..=20 {
            assert!(poll(&mut n, t * MIN, idle(60.0)).is_empty(), "minute {t}");
        }
        // below threshold − 10 mV: re-armed, fires after another five minutes over it
        assert!(poll(&mut n, 21 * MIN, idle(39.0)).is_empty());
        assert!(poll(&mut n, 22 * MIN, idle(60.0)).is_empty());
        assert_eq!(triggers(&poll(&mut n, 27 * MIN, idle(60.0))), vec![(Trigger::Spread, vec![2])]);
    }

    #[test]
    fn load_restarts_the_five_minutes() {
        let mut n = watching(1);
        n.answer(1, 2, 0, true, Some(&idle(60.0)));
        n.answer(1, 2, MIN, true, Some(&idle(60.0)));
        n.answer(1, 2, 4 * MIN, true, Some(&Reading { current: Some(25.0), ..idle(90.0) }));
        n.answer(1, 2, 5 * MIN, true, Some(&idle(60.0)));
        n.answer(1, 2, 6 * MIN, true, Some(&idle(60.0)));
        assert!(n.flush(6 * MIN).is_empty());
        n.answer(1, 2, 11 * MIN, true, Some(&idle(60.0)));
        assert_eq!(triggers(&n.flush(11 * MIN)), vec![(Trigger::Spread, vec![2])]);
    }

    #[test]
    fn the_lost_connection_fires_after_the_set_minutes_once_until_data_comes() {
        let mut n = watching(1);
        assert_eq!(n.tick(60 * MIN), None, "no data yet: nothing was lost");
        n.answer(1, 0, 0, true, Some(&idle(10.0)));
        assert_eq!(n.tick(4 * MIN), None);
        let note = n.tick(5 * MIN).unwrap();
        assert_eq!((note.trigger, note.minutes), (Trigger::Lost, Some(5)));
        assert_eq!(n.tick(6 * MIN), None, "once");
        // the UI reconnects (a new generation of the same installation): the watch goes on
        n.end(true);
        n.begin(2, Some("inst-a".into()), "A".into(), on());
        n.answer(2, 0, 20 * MIN, true, Some(&idle(10.0)));
        assert!(n.tick(25 * MIN).is_some(), "data came, lost again");
        // closed by the user: nothing is watched
        n.answer(2, 0, 30 * MIN, true, Some(&idle(10.0)));
        n.end(false);
        assert_eq!(n.tick(90 * MIN), None);
    }

    #[test]
    fn a_history_read_is_no_lost_connection() {
        let mut n = watching(1);
        n.answer(1, 0, 0, true, Some(&idle(10.0)));
        n.touch(4 * MIN);
        assert_eq!(n.tick(6 * MIN), None);
        assert!(n.tick(9 * MIN).is_some());
    }

    #[test]
    fn at_most_one_notice_per_pack_and_trigger_in_ten_minutes() {
        let mut n = watching(1);
        n.answer(1, 3, 0, true, Some(&alarm(&[OV])));
        assert_eq!(n.flush(0).len(), 1);
        n.answer(1, 3, MIN, true, Some(&alarm(&[])));
        n.answer(1, 3, 2 * MIN, true, Some(&alarm(&[OV])));
        assert!(n.flush(2 * MIN).is_empty(), "the same pack and alarm within ten minutes");
        // another alarm of the same pack, and the same alarm of another pack, are their own
        n.answer(1, 3, 3 * MIN, true, Some(&alarm(&[OV, "alarm.ev2.b5"])));
        n.answer(1, 4, 3 * MIN, true, Some(&alarm(&[OV])));
        assert_eq!(triggers(&n.flush(3 * MIN)), vec![(ov(), vec![4]), (Trigger::Alarm { id: "alarm.ev2.b5".into() }, vec![3])]);
    }

    #[test]
    fn several_packs_in_one_round_are_one_notice() {
        let mut n = watching(1);
        n.answer(1, 4, 0, true, Some(&alarm(&[OV])));
        n.answer(1, 3, 0, true, Some(&alarm(&[OV])));
        n.answer(1, 7, 0, true, Some(&alarm(&[])));
        let notes = n.flush(0);
        assert_eq!(triggers(&notes), vec![(ov(), vec![3, 4])]);
        let texts = Texts { alarms: HashMap::from([(OV.to_string(), "Cell overvoltage protection".to_string())]), ..Default::default() };
        assert_eq!(render(&notes[0], &texts, "Bank A"), ("OpenBMS Monitor: Bank A".into(), "Cell overvoltage protection on packs 03, 04".into()));
    }

    #[test]
    fn nothing_fires_for_an_unconfirmed_or_unchecked_pack() {
        let mut n = watching(1);
        n.answer(1, 3, 0, false, Some(&alarm(&[OV])));
        n.answer(1, 3, 0, false, Some(&idle(10.0)));
        for t in 1..5 {
            n.answer(1, 3, t, false, None);
        }
        assert!(n.flush(5).is_empty());
        assert_eq!(n.tick(60 * MIN), None, "no confirmed data: no lost connection either");
        // confirmed later: a fresh start for the pack
        n.answer(1, 3, 10, true, Some(&alarm(&[OV])));
        assert_eq!(triggers(&n.flush(10)), vec![(ov(), vec![3])]);
    }

    #[test]
    fn the_can_socket_counts_only_with_the_installations_master() {
        let cut = SerialValue::Set { value: "BAT-0123456789-ABCDEFG".into(), truncated: true };
        assert!(master_matches(Some("BAT-0123456789-ABCDEFGHIJ"), &cut));
        assert!(!master_matches(Some("BAT-OTHER"), &cut));
        assert!(!master_matches(None, &cut), "none stored for the installation");
        let full = SerialValue::Set { value: "BAT-1".into(), truncated: false };
        assert!(master_matches(Some("BAT-1"), &full));
        assert!(!master_matches(Some("BAT-10"), &full), "complete: equal, not a prefix");
        for v in [SerialValue::Empty, SerialValue::Unavailable, SerialValue::NoAnswer, SerialValue::Set { value: String::new(), truncated: true }] {
            assert!(!master_matches(Some("BAT-1"), &v), "{v:?}");
        }
    }

    #[test]
    fn a_new_generation_starts_clean_and_old_answers_are_ignored() {
        let mut n = watching(1);
        n.answer(1, 3, 0, true, Some(&alarm(&[OV])));
        n.answer(1, 5, 0, true, Some(&idle(10.0)));
        n.answer(1, 5, 1, true, None);
        n.answer(1, 5, 2, true, None);
        n.begin(2, Some("inst-a".into()), "A".into(), on());
        assert!(n.flush(2).is_empty(), "pending of the old connection dropped");
        // an answer of the old connection changes nothing; the silent count starts again
        n.answer(1, 5, 3, true, None);
        n.answer(2, 5, 4, true, None);
        assert!(n.flush(4).is_empty(), "pack 5 has not answered on this connection");
        // another installation: the rate limit starts again as well
        n.begin(3, Some("inst-b".into()), "B".into(), on());
        n.answer(3, 3, 5, true, Some(&alarm(&[OV])));
        assert_eq!(triggers(&n.flush(5)), vec![(ov(), vec![3])]);
        // „Nur ansehen“: nothing
        n.begin(4, None, String::new(), on());
        n.answer(4, 3, 6, true, Some(&alarm(&["alarm.ev2.b5"])));
        assert!(n.flush(6).is_empty());
    }

    #[test]
    fn switched_off_nothing_is_sent_and_nothing_later() {
        let mut n = Notifier::default();
        n.begin(1, Some("inst-a".into()), "A".into(), Settings::default());
        n.answer(1, 3, 0, true, Some(&alarm(&[OV])));
        assert!(n.flush(0).is_empty());
        n.set_settings("inst-other", on());
        n.answer(1, 0, 0, true, Some(&idle(10.0)));
        assert_eq!(n.tick(30 * MIN), None);
        n.set_settings("inst-a", on());
        assert!(n.flush(MIN).is_empty(), "the alarm that started while off is not sent later");
        assert!(n.tick(30 * MIN).is_some());
    }

    #[test]
    fn settings_default_off_and_stay_in_range() {
        assert_eq!(Settings::parse(None), Settings { enabled: false, spread_mv: 50, lost_minutes: 5 });
        assert_eq!(Settings::parse(Some("not json")), Settings::default());
        assert_eq!(Settings::parse(Some(r#"{"enabled":true}"#)), on());
        assert_eq!(Settings::parse(Some(r#"{"enabled":true,"spread_mv":0,"lost_minutes":0}"#)), Settings { enabled: true, spread_mv: 10, lost_minutes: 1 });
        let s = Settings { enabled: true, spread_mv: 40, lost_minutes: 15 };
        assert_eq!(Settings::parse(Some(&serde_json::to_string(&s).unwrap())), s);
    }

    #[test]
    fn texts_name_one_pack_or_several() {
        let note = |trigger, packs: Vec<u8>| Note { trigger, packs, spread_mv: Some(62), limit_mv: Some(50), minutes: Some(5) };
        let t = Texts::default();
        assert_eq!(render(&note(ov(), vec![3]), &t, "A").1, "Pack 03: alarm.ev2.b1", "an unknown alarm keeps its id");
        assert_eq!(render(&note(Trigger::Silent, vec![5, 6]), &t, "A").1, "Packs 05, 06 stopped answering");
        assert_eq!(render(&note(Trigger::Spread, vec![2]), &t, "A").1, "Pack 02: cell spread at rest 62 mV (threshold 50 mV)");
        assert_eq!(render(&note(Trigger::Lost, vec![]), &t, "A").1, "No data for 5 min: connection lost");
        // partial texts from the UI keep the defaults for the rest
        let de: Texts = serde_json::from_str(r#"{"silent_one":"Pack {{packs}} antwortet nicht mehr"}"#).unwrap();
        assert_eq!(render(&note(Trigger::Silent, vec![5]), &de, "A").1, "Pack 05 antwortet nicht mehr");
        assert_eq!(render(&note(Trigger::Silent, vec![5, 6]), &de, "A").1, "Packs 05, 06 stopped answering");
    }
}
