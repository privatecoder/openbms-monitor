//! External active balancers (spec docs/superpowers/specs/2026-10-17-cell-analysis-balancers.md, part A): the devices
//! of an installation (setting `devices`: definitions and assignments, each with revisions in time) and when an
//! assigned balancer works. Nothing is read from the device: its activity is computed from the recorded cells with its
//! manual settings. `Tracker::step` is the one activity rule; the timeline of the pack details and the cell analysis
//! use it.
//!
//! A single answer is decided with the state carried from the row before (hysteresis between `stop_mv` and
//! `trigger_mv`, and between `stop_cell_mv` and `start_mv`), within one session and without a gap of more than GAP_PERIODS poll periods. A minute row (compacted)
//! only by its extremes (`spread_lo/hi`, `i_lo/hi`, highest and lowest cell of the minute): active or inactive where
//! that is certain for every answer of the minute, unknown otherwise and without the range columns.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const MINUTE_MS: i64 = 60_000;
/// |current| (mA) of the `when` conditions: charging above +1 A, discharging below −1 A
pub(crate) const CHARGE_MA: i64 = 1_000;
/// a gap of more than this many poll periods ends the carried state
pub(crate) const GAP_PERIODS: i64 = 3;
/// poll period of a session without two single answers to measure it
pub(crate) const DEFAULT_PERIOD_MS: i64 = 10_000;
/// rows this long before the span only carry the state into it
pub(crate) const WARMUP_MS: i64 = 6 * 3_600_000;
/// spread (mV) from which a definition without a readable trigger starts (the UI's default)
const DEFAULT_TRIGGER_MV: i64 = 5;

/// When the balancer may work, by the pack current.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum When {
    #[default]
    Always,
    /// current > +1 A
    Charging,
    /// current ≥ −1 A
    NotDischarging,
}

/// The manual settings of an active balancer valid at a time (mV).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    /// the highest cell must be at least this to start
    pub start_mv: Option<i64>,
    /// highest cell below which it stops; None (or above the start): the start, no hysteresis
    pub stop_cell_mv: Option<i64>,
    /// spread from which it starts
    pub trigger_mv: i64,
    /// spread below which it stops; None (or above the trigger): the trigger, no hysteresis
    pub stop_mv: Option<i64>,
    pub when: When,
}

impl Rule {
    fn stop(&self) -> i64 {
        self.stop_mv.map_or(self.trigger_mv, |s| s.min(self.trigger_mv))
    }

    /// the highest cell below which it is off for certain (None: no cell condition)
    fn cell_stop(&self) -> Option<i64> {
        self.start_mv.map(|s| self.stop_cell_mv.map_or(s, |c| c.min(s)))
    }

    fn when_holds(&self, ma: i64) -> bool {
        match self.when {
            When::Always => true,
            When::Charging => ma > CHARGE_MA,
            When::NotDischarging => ma >= -CHARGE_MA,
        }
    }
}

/// One recorded row of a pack as stored (mV, mA).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub t: i64,
    pub session: i64,
    pub n: i64,
    pub current: Option<i64>,
    pub cell_max: Option<i64>,
    pub cell_min: Option<i64>,
    pub spread_lo: Option<i64>,
    pub spread_hi: Option<i64>,
    pub i_lo: Option<i64>,
    pub i_hi: Option<i64>,
    pub balancing: Option<i64>,
    pub bal_rows: Option<i64>,
    pub bal_n: Option<i64>,
}

impl Row {
    /// One answer with its own values; a minute row merged several (an imported one keeps n = 1 but has ranges that
    /// differ or balancing counts of more rows). Without the range columns it is neither: unknown.
    pub fn single(&self) -> bool {
        if self.n != 1 || self.bal_n.is_some_and(|n| n > 1) {
            return false;
        }
        matches!((self.spread_lo, self.spread_hi, self.i_lo, self.i_hi), (Some(a), Some(b), Some(c), Some(d)) if a == b && c == d)
    }
}

/// The computed state of an external balancer for one row.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Activity {
    Active,
    Inactive,
    Unknown,
}

/// The state carried from row to row; a new session or a gap starts inactive.
#[derive(Default, Debug)]
pub struct Tracker {
    last: Option<(i64, i64, Activity)>,
}

impl Tracker {
    /// The activity of `row` under `rule`; `gap_ms`: the longest time since the row before that still carries its state.
    pub fn step(&mut self, rule: &Rule, row: &Row, gap_ms: i64) -> Activity {
        let before = match self.last {
            Some((session, t, a)) if session == row.session && row.t - t <= gap_ms => a,
            _ => Activity::Inactive,
        };
        let now = if row.single() { single(rule, row, before) } else { minute(rule, row) };
        self.last = Some((row.session, row.t, now));
        now
    }
}

/// The tracker of a pack under its assignments in time: the balancer and settings valid at each row (`Devices::at`), and
/// a fresh (inactive) state whenever the device or its settings change, so a new balancer never inherits the state of
/// the one before.
#[derive(Default)]
pub struct Ext {
    tracker: Tracker,
    current: Option<(String, Rule)>,
}

impl Ext {
    /// The balancer assigned at the row and its activity; None: none assigned then.
    pub fn step<'a>(&mut self, devices: &'a Devices, pack: u8, row: &Row, gap_ms: i64) -> Option<(&'a Def, Activity)> {
        let Some((def, rule)) = devices.at(pack, row.t) else {
            *self = Ext::default();
            return None;
        };
        if self.current.as_ref().is_none_or(|(id, r)| *id != def.id || *r != rule) {
            self.tracker = Tracker::default();
            self.current = Some((def.id.clone(), rule));
        }
        Some((def, self.tracker.step(&rule, row, gap_ms)))
    }
}

fn single(rule: &Rule, row: &Row, before: Activity) -> Activity {
    let (Some(ma), Some(hi), Some(lo)) = (row.current, row.cell_max, row.cell_min) else { return Activity::Unknown };
    if !rule.when_holds(ma) || rule.cell_stop().is_some_and(|s| hi < s) {
        return Activity::Inactive;
    }
    let spread = hi - lo;
    if spread >= rule.trigger_mv && rule.start_mv.is_none_or(|s| hi >= s) {
        Activity::Active
    } else if spread < rule.stop() {
        Activity::Inactive
    } else {
        // between stop and trigger, or the highest cell between its stop and the start: as before (unknown stays
        // unknown)
        before
    }
}

fn minute(rule: &Rule, row: &Row) -> Activity {
    let (Some(s_lo), Some(s_hi), Some(i_lo), Some(i_hi)) = (row.spread_lo, row.spread_hi, row.i_lo, row.i_hi) else { return Activity::Unknown };
    // off for every answer: no current of the minute meets `when`, its highest cell stays below the cell stop, or every
    // spread is below the stop
    if !rule.when_holds(i_hi) || rule.cell_stop().zip(row.cell_max).is_some_and(|(s, hi)| hi < s) || s_hi < rule.stop() {
        return Activity::Inactive;
    }
    // on for every answer: every current meets `when`, even the lowest cell of the minute reaches the start, and every
    // spread reaches the trigger
    let start = match rule.start_mv {
        None => true,
        Some(s) => row.cell_min.is_some_and(|lo| lo >= s),
    };
    if rule.when_holds(i_lo) && start && s_lo >= rule.trigger_mv {
        Activity::Active
    } else {
        Activity::Unknown
    }
}

/// gaps kept per session for its poll period: memory stays at this many values per session, however long it runs
const PERIOD_SAMPLE: usize = 1_000;

/// The usual time between two answers per session: the median of the first PERIOD_SAMPLE gaps between its consecutive
/// single rows. Fed row by row in time order, so a long span needs no list of its rows.
#[derive(Default)]
pub struct Periods {
    gaps: HashMap<i64, Vec<i64>>,
    /// session and time of the row before, if it was a single answer
    last: Option<(i64, i64)>,
}

impl Periods {
    pub fn add(&mut self, row: &Row) {
        let single = row.single();
        if let (true, Some((session, t))) = (single, self.last) {
            if session == row.session && row.t > t {
                let g = self.gaps.entry(session).or_default();
                if g.len() < PERIOD_SAMPLE {
                    g.push(row.t - t);
                }
            }
        }
        self.last = single.then_some((row.session, row.t));
    }

    pub fn finish(self) -> HashMap<i64, i64> {
        self.gaps
            .into_iter()
            .map(|(s, mut g)| {
                g.sort_unstable();
                (s, g[g.len() / 2])
            })
            .collect()
    }
}


// ---------------------------------------------------------------------------------------------
// devices

/// A definition's settings from a time on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefRev {
    pub from: i64,
    pub rule: Rule,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Def {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub revs: Vec<DefRev>,
    /// deleted at: its assignments end then, the time before stays computable
    pub to: Option<i64>,
}

/// The devices of an installation as stored by the UI (`devices.ts`), read leniently: an unreadable definition or
/// revision is left out, never the whole setting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Devices {
    pub defs: Vec<Def>,
    /// pack → (from, device id or none), in any order
    pub assign: BTreeMap<u8, Vec<(i64, Option<String>)>>,
}

#[derive(Deserialize)]
struct RawRev {
    from: i64,
    #[serde(default)]
    start_mv: Option<i64>,
    #[serde(default)]
    stop_cell_mv: Option<i64>,
    #[serde(default)]
    trigger_mv: Option<i64>,
    #[serde(default)]
    stop_mv: Option<i64>,
    #[serde(default)]
    when: When,
}

#[derive(Deserialize)]
struct RawAssign {
    from: i64,
    #[serde(default)]
    device: Option<String>,
}

impl Devices {
    pub fn parse(v: &Value) -> Devices {
        let defs = v["defs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                let revs = d["revs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| serde_json::from_value::<RawRev>(r.clone()).ok())
                    .map(|r| DefRev { from: r.from, rule: Rule { start_mv: r.start_mv, stop_cell_mv: r.stop_cell_mv, trigger_mv: r.trigger_mv.unwrap_or(DEFAULT_TRIGGER_MV), stop_mv: r.stop_mv, when: r.when } })
                    .collect();
                Some(Def { id: d["id"].as_str()?.to_string(), name: d["name"].as_str().unwrap_or("").to_string(), kind: d["kind"].as_str().unwrap_or("").to_string(), revs, to: d["to"].as_i64() })
            })
            .collect();
        let assign = v["assign"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(pack, list)| {
                let revs = list.as_array()?.iter().filter_map(|a| serde_json::from_value::<RawAssign>(a.clone()).ok()).map(|a| (a.from, a.device)).collect();
                Some((pack.parse().ok()?, revs))
            })
            .collect();
        Devices { defs, assign }
    }

    /// The active balancer of `pack` at `t` and its settings valid then; None: none assigned at that time.
    pub fn at(&self, pack: u8, t: i64) -> Option<(&Def, Rule)> {
        let (_, device) = self.assign.get(&pack)?.iter().filter(|(from, _)| *from <= t).max_by_key(|(from, _)| *from)?;
        let def = self.defs.iter().find(|d| Some(&d.id) == device.as_ref() && d.kind == "active-balancer")?;
        if def.to.is_some_and(|to| t >= to) {
            return None;
        }
        let rev = def.revs.iter().filter(|r| r.from <= t).max_by_key(|r| r.from).or_else(|| def.revs.iter().min_by_key(|r| r.from))?;
        Some((def, rev.rule))
    }

    /// The first time `pack` had a balancer assigned.
    pub fn first_assigned(&self, pack: u8) -> Option<i64> {
        self.assign.get(&pack)?.iter().filter(|(_, d)| d.is_some()).map(|(from, _)| *from).min()
    }
}

// ---------------------------------------------------------------------------------------------
// timeline

/// Time (ms) of one slice of the span per state. Time without a row (not recorded) is in none of them.
#[derive(Serialize, Default, Debug, Clone, PartialEq)]
pub struct Slice {
    pub from: i64,
    pub to: i64,
    /// external balancer, computed: active, inactive, not decidable, no balancer assigned
    pub active: i64,
    pub inactive: i64,
    pub unknown: i64,
    pub unassigned: i64,
    /// BMS balancing as reported: balancing (a minute row by its share bal_rows / bal_n), not balancing, no status
    pub bal: i64,
    pub idle: i64,
    pub no_status: i64,
    /// most cells balanced at once in a single answer
    pub cells: u32,
    /// minute rows that balanced at least once: only their share is known, not the cells at once
    pub minutes: bool,
}

impl Slice {
    fn add(&mut self, ms: i64, ext: Option<Activity>, row: &Row) {
        match ext {
            Some(Activity::Active) => self.active += ms,
            Some(Activity::Inactive) => self.inactive += ms,
            Some(Activity::Unknown) => self.unknown += ms,
            None => self.unassigned += ms,
        }
        let rows = row.n.max(row.bal_n.unwrap_or(0));
        match (row.balancing, row.bal_n) {
            // a minute row: only its answers with a status are known (bal_n of n), split by bal_rows; the rest of the
            // minute has no data
            (Some(b), Some(n)) if rows > 1 => {
                let n = n.clamp(0, rows);
                let known = (ms as f64 * n as f64 / rows as f64).round() as i64;
                let on = if n > 0 { (known as f64 * row.bal_rows.unwrap_or(0).clamp(0, n) as f64 / n as f64).round() as i64 } else { 0 };
                self.bal += on;
                self.idle += known - on;
                self.no_status += ms - known;
                self.minutes |= b != 0;
            }
            (Some(_), None) if rows > 1 => self.no_status += ms,
            (Some(0), _) => self.idle += ms,
            (Some(b), _) => {
                self.bal += ms;
                self.cells = self.cells.max((b as u32).count_ones());
            }
            (None, _) => self.no_status += ms,
        }
    }
}

#[derive(Serialize, Default, Debug)]
pub struct Timeline {
    pub from: i64,
    pub to: i64,
    pub slices: Vec<Slice>,
    /// the whole span
    pub total: Slice,
    /// names of the balancers assigned within the span
    pub devices: Vec<String>,
    /// first assignment of a balancer to the pack (before it: „kein Balancer eingetragen“)
    pub first_assigned: Option<i64>,
    /// always false for now: no device is read (`profile` null), the activity is computed
    pub measured: bool,
}

/// The columns of a `Row`, in the order `read_row` takes them.
pub(crate) const ROW_COLS: &str = "t, session, n, current, cell_max, cell_min, spread_lo, spread_hi, i_lo, i_hi, balancing, bal_rows, bal_n";

/// A `Row` from the first 13 columns of a query that selects ROW_COLS first.
pub(crate) fn read_row(r: &rusqlite::Row) -> rusqlite::Result<Row> {
    Ok(Row {
        t: r.get(0)?,
        session: r.get(1)?,
        n: r.get(2)?,
        current: r.get(3)?,
        cell_max: r.get(4)?,
        cell_min: r.get(5)?,
        spread_lo: r.get(6)?,
        spread_hi: r.get(7)?,
        i_lo: r.get(8)?,
        i_hi: r.get(9)?,
        balancing: r.get(10)?,
        bal_rows: r.get(11)?,
        bal_n: r.get(12)?,
    })
}

/// The rows of a pack in a span, in time order, one at a time to `f`.
pub fn each_row(db: &Connection, site: &str, pack: u8, from: i64, to: i64, mut f: impl FnMut(Row)) -> Result<(), String> {
    let sql = format!("SELECT {ROW_COLS} FROM pack WHERE site = (SELECT id FROM sites WHERE site = ?1) AND pack = ?2 AND t >= ?3 AND t <= ?4 ORDER BY t");
    let mut st = db.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = st.query_map(params![site, pack, from, to], read_row).map_err(|e| e.to_string())?;
    for row in rows {
        f(row.map_err(|e| e.to_string())?);
    }
    Ok(())
}


/// Both lanes of a pack over [from, to] in `n` equal slices. A row covers the time until the next row of its session,
/// at most GAP_PERIODS poll periods (a minute row: minutes); after that one period. Two passes over the rows (poll
/// periods, then the lanes), none kept: a month of answers stays out of memory.
pub fn timeline(db: &Connection, site: &str, pack: u8, from: i64, to: i64, devices: &Devices, n: usize) -> Result<Timeline, String> {
    let to = to.max(from + 1);
    let mut periods = Periods::default();
    each_row(db, site, pack, from - WARMUP_MS, to, |r| periods.add(&r))?;
    let periods = periods.finish();
    let n = n.clamp(1, 2000);
    let width = (to - from) as f64 / n as f64;
    let edge = |i: usize| from + (i as f64 * width).round() as i64;
    let mut slices: Vec<Slice> = (0..n).map(|i| Slice { from: edge(i), to: edge(i + 1), ..Default::default() }).collect();
    let mut total = Slice { from, to, ..Default::default() };
    let mut ext_state = Ext::default();
    let mut names = BTreeSet::new();
    // a row is drawn once the next one tells where it ends
    let mut draw = |row: &Row, next: Option<&Row>| {
        let span = if row.single() { periods.get(&row.session).copied().unwrap_or(DEFAULT_PERIOD_MS) } else { MINUTE_MS };
        let gap = GAP_PERIODS * span;
        let end = next.filter(|x| x.session == row.session && x.t - row.t <= gap).map_or(row.t + span, |x| x.t);
        let ext = ext_state.step(devices, pack, row, gap).map(|(def, a)| {
            if row.t >= from {
                names.insert(def.name.clone());
            }
            a
        });
        let (a, b) = (row.t.max(from), end.min(to));
        if b <= a {
            return;
        }
        total.add(b - a, ext, row);
        let mut k = (((a - from) as f64 / width) as usize).min(n - 1);
        while k < n && slices[k].from < b {
            let (x, y) = (a.max(slices[k].from), b.min(slices[k].to));
            if y > x {
                slices[k].add(y - x, ext, row);
            }
            k += 1;
        }
    };
    let mut prev: Option<Row> = None;
    each_row(db, site, pack, from - WARMUP_MS, to, |r| {
        if let Some(p) = prev.take() {
            draw(&p, Some(&r));
        }
        prev = Some(r);
    })?;
    if let Some(p) = prev {
        draw(&p, None);
    }
    Ok(Timeline { from, to, slices, total, devices: names.into_iter().collect(), first_assigned: devices.first_assigned(pack), measured: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use Activity::*;

    const RULE: Rule = Rule { start_mv: None, stop_cell_mv: None, trigger_mv: 10, stop_mv: Some(5), when: When::Always };

    /// A single answer: spread from 3300 mV up, current in mA.
    fn answer(t: i64, session: i64, spread: i64, ma: i64) -> Row {
        Row { t, session, n: 1, current: Some(ma), cell_max: Some(3300 + spread), cell_min: Some(3300), spread_lo: Some(spread), spread_hi: Some(spread), i_lo: Some(ma), i_hi: Some(ma), ..Default::default() }
    }

    fn minute_row(t: i64, spread: (i64, i64), ma: (i64, i64), cells: (i64, i64)) -> Row {
        Row { t, session: 1, n: 6, current: Some((ma.0 + ma.1) / 2), cell_max: Some(cells.1), cell_min: Some(cells.0), spread_lo: Some(spread.0), spread_hi: Some(spread.1), i_lo: Some(ma.0), i_hi: Some(ma.1), ..Default::default() }
    }

    fn run(rule: &Rule, rows: &[Row]) -> Vec<Activity> {
        let mut tr = Tracker::default();
        rows.iter().map(|r| tr.step(rule, r, 30_000)).collect()
    }

    #[test]
    fn starts_at_the_trigger_and_stops_below_the_stop() {
        let rows: Vec<Row> = [4, 10, 7, 5, 4, 7, 12].iter().enumerate().map(|(i, &s)| answer(i as i64 * 10_000, 1, s, 0)).collect();
        assert_eq!(run(&RULE, &rows), vec![Inactive, Active, Active, Active, Inactive, Inactive, Active]);
        // without a stop (or one above the trigger): no hysteresis
        for stop in [None, Some(20)] {
            assert_eq!(run(&Rule { stop_mv: stop, ..RULE }, &rows), vec![Inactive, Active, Inactive, Inactive, Inactive, Inactive, Active]);
        }
    }

    #[test]
    fn when_and_start_hold_on_every_row() {
        let charging = Rule { when: When::Charging, ..RULE };
        assert_eq!(run(&charging, &[answer(0, 1, 20, 1_000), answer(10_000, 1, 20, 1_001), answer(20_000, 1, 20, -5_000)]), vec![Inactive, Active, Inactive]);
        let not_dis = Rule { when: When::NotDischarging, ..RULE };
        assert_eq!(run(&not_dis, &[answer(0, 1, 20, -1_000), answer(10_000, 1, 20, -1_001), answer(20_000, 1, 20, 40_000)]), vec![Active, Inactive, Active]);
        // the highest cell (3300 + spread) must reach the start
        let start = Rule { start_mv: Some(3_320), ..RULE };
        assert_eq!(run(&start, &[answer(0, 1, 15, 0), answer(10_000, 1, 20, 0), answer(20_000, 1, 19, 0)]), vec![Inactive, Active, Inactive]);
    }

    #[test]
    fn a_cell_stop_below_the_start_keeps_it_running_down_to_the_cell_stop() {
        // start at a highest cell of 3320, stop below 3310 (as NEEY RunVol / StopVol): between them as before
        let rule = Rule { start_mv: Some(3_320), stop_cell_mv: Some(3_310), ..RULE };
        let rows: Vec<Row> = [15, 25, 15, 8, 15, 25].iter().enumerate().map(|(i, &s)| answer(i as i64 * 10_000, 1, s, 0)).collect();
        assert_eq!(run(&rule, &rows), vec![Inactive, Active, Active, Inactive, Inactive, Active]);
        // a cell stop above the start is the start
        assert_eq!(run(&Rule { stop_cell_mv: Some(3_400), ..rule }, &rows), vec![Inactive, Active, Inactive, Inactive, Inactive, Active]);
        // a minute whose highest cell stays between the cell stop and the start is not off for certain
        let m = minute_row(0, (20, 20), (0, 0), (3_305, 3_315));
        assert_eq!(run(&rule, std::slice::from_ref(&m)), vec![Unknown]);
        assert_eq!(run(&Rule { stop_cell_mv: None, ..rule }, &[m]), vec![Inactive]);
    }

    #[test]
    fn a_new_session_or_a_gap_starts_inactive() {
        // active, then in the band between stop and trigger: carried only within the session and without a gap
        assert_eq!(run(&RULE, &[answer(0, 1, 12, 0), answer(10_000, 1, 7, 0)]), vec![Active, Active]);
        assert_eq!(run(&RULE, &[answer(0, 1, 12, 0), answer(10_000, 2, 7, 0)]), vec![Active, Inactive]);
        assert_eq!(run(&RULE, &[answer(0, 1, 12, 0), answer(40_001, 1, 7, 0)]), vec![Active, Inactive]);
    }

    #[test]
    fn a_row_without_cells_is_unknown_until_the_spread_decides() {
        let blind = Row { cell_max: None, cell_min: None, ..answer(10_000, 1, 0, 0) };
        let rows = [answer(0, 1, 12, 0), blind, answer(20_000, 1, 7, 0), answer(30_000, 1, 12, 0), answer(40_000, 1, 7, 0)];
        assert_eq!(run(&RULE, &rows), vec![Active, Unknown, Unknown, Active, Active]);
        let rows = [Row { cell_max: None, cell_min: None, ..answer(0, 1, 0, 0) }, answer(10_000, 1, 4, 0)];
        assert_eq!(run(&RULE, &rows), vec![Unknown, Inactive]);
    }

    #[test]
    fn a_minute_row_is_decided_only_where_its_extremes_are_certain() {
        let one = |rule: &Rule, r: Row| run(rule, &[r])[0];
        let cells = (3_300, 3_330);
        assert_eq!(one(&RULE, minute_row(0, (10, 20), (0, 0), cells)), Active);
        assert_eq!(one(&RULE, minute_row(0, (1, 4), (0, 0), cells)), Inactive);
        assert_eq!(one(&RULE, minute_row(0, (4, 20), (0, 0), cells)), Unknown);
        // the state before does not carry into a minute
        assert_eq!(run(&RULE, &[answer(0, 1, 12, 0), minute_row(10_000, (6, 9), (0, 0), cells)]), vec![Active, Unknown]);
        let charging = Rule { when: When::Charging, ..RULE };
        assert_eq!(one(&charging, minute_row(0, (12, 20), (2_000, 9_000), cells)), Active);
        assert_eq!(one(&charging, minute_row(0, (12, 20), (-500, 9_000), cells)), Unknown);
        assert_eq!(one(&charging, minute_row(0, (12, 20), (-9_000, 1_000), cells)), Inactive);
        let start = Rule { start_mv: Some(3_320), ..RULE };
        assert_eq!(one(&start, minute_row(0, (12, 20), (0, 0), (3_300, 3_330))), Unknown, "the lowest cell of the minute is below the start");
        assert_eq!(one(&start, minute_row(0, (12, 20), (0, 0), (3_321, 3_340))), Active);
        assert_eq!(one(&start, minute_row(0, (12, 20), (0, 0), (3_290, 3_310))), Inactive);
        // an old minute row without the range columns
        assert_eq!(one(&RULE, Row { spread_lo: None, spread_hi: None, i_lo: None, i_hi: None, ..minute_row(0, (12, 20), (0, 0), cells) }), Unknown);
        // an imported minute row (n = 1) with ranges that differ is a minute row
        assert_eq!(one(&RULE, Row { n: 1, ..minute_row(0, (4, 20), (0, 0), cells) }), Unknown);
    }

    #[test]
    fn the_settings_and_assignments_valid_at_the_time() {
        let rev = |from: i64, trigger: i64| json!({ "from": from, "start_mv": null, "trigger_mv": trigger, "stop_mv": null, "when": "charging", "max_a": 4 });
        let d = Devices::parse(&json!({
            "defs": [
                { "id": "a", "name": "Balancer A", "kind": "active-balancer", "profile": null, "revs": [rev(100, 10), rev(200, 20)], "to": 300 },
                { "id": "b", "name": "B", "kind": "active-balancer", "profile": null, "revs": [rev(50, 7), { "from": "broken" }] },
                "not a definition",
            ],
            "assign": { "3": [{ "from": 100, "device": "a" }, { "from": 400, "device": "b" }, { "from": 500, "device": null }], "x": [] },
        }));
        assert_eq!(d.defs.len(), 2);
        assert_eq!(d.at(3, 99), None, "before the first assignment");
        let (def, rule) = d.at(3, 150).unwrap();
        assert_eq!((def.name.as_str(), rule.trigger_mv, rule.when), ("Balancer A", 10, When::Charging));
        assert_eq!(d.at(3, 250).unwrap().1.trigger_mv, 20, "the revision from 200 on");
        assert_eq!(d.at(3, 300), None, "deleted at 300: the assignment ends");
        assert_eq!(d.at(3, 450).unwrap().0.id, "b");
        assert_eq!(d.at(3, 500), None);
        assert_eq!(d.at(4, 450), None);
        assert_eq!(d.first_assigned(3), Some(100));
        assert_eq!(Devices::parse(&Value::Null), Devices::default());
    }

    #[test]
    fn poll_period_is_the_median_gap_of_single_answers() {
        let rows = [answer(0, 1, 0, 0), answer(5_000, 1, 0, 0), answer(10_000, 1, 0, 0), answer(60_000, 1, 0, 0), answer(0, 2, 0, 0), answer(2_000, 2, 0, 0)];
        let mut p = Periods::default();
        rows.iter().for_each(|r| p.add(r));
        let p = p.finish();
        assert_eq!((p[&1], p[&2]), (5_000, 2_000));
    }

    #[test]
    fn the_timeline_splits_both_lanes_over_the_span() {
        let dir = std::env::temp_dir().join(format!("openbms-balancer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut h = crate::history::History::default();
        h.open(&dir.join("history.sqlite")).unwrap();
        h.set_settings(crate::history::Settings { enabled: true, ..Default::default() }).unwrap();
        h.connected("inst", "can");
        // 10 answers 10 s apart: spread 12 mV for the first five (cells 1 and 2 balancing), then 2 mV; the last without status
        for i in 0..10 {
            let top = if i < 5 { 3.312 } else { 3.302 };
            let status = match i {
                0..5 => json!({ "alarms": [], "balancing": 0b11 }),
                9 => Value::Null,
                _ => json!({ "alarms": [], "balancing": 0 }),
            };
            h.pack(1_000_000 + i * 10_000, 2, &json!({ "address": 2, "telemetry": { "cell_voltages": [3.300, top], "current": 0.0 }, "status": status }));
        }
        let db = crate::history::reader(&dir.join("history.sqlite")).unwrap();
        let devices = Devices::parse(&json!({
            "defs": [{ "id": "a", "name": "Balancer A", "kind": "active-balancer", "profile": null, "revs": [{ "from": 0, "trigger_mv": 10, "when": "always" }] }],
            "assign": { "2": [{ "from": 1_020_000, "device": "a" }] },
        }));
        let tl = timeline(&db, "inst", 2, 1_000_000, 1_100_000, &devices, 10).unwrap();
        // before the assignment at 1 020 000 nothing is computed; the last row covers one poll period
        assert_eq!((tl.total.unassigned, tl.total.active, tl.total.inactive, tl.total.unknown), (20_000, 30_000, 50_000, 0));
        assert_eq!((tl.total.bal, tl.total.idle, tl.total.no_status, tl.total.cells), (50_000, 40_000, 10_000, 2));
        assert_eq!(tl.slices.len(), 10);
        assert_eq!((tl.slices[0].unassigned, tl.slices[2].active, tl.slices[5].inactive), (10_000, 10_000, 10_000));
        assert_eq!(tl.devices, vec!["Balancer A".to_string()]);
        assert_eq!((tl.first_assigned, tl.measured), (Some(1_020_000), false));
        assert_eq!(tl.slices.iter().map(|s| s.active + s.inactive + s.unassigned).sum::<i64>(), 100_000);
    }

    #[test]
    fn a_minute_row_reports_only_its_share() {
        let mut s = Slice::default();
        let row = Row { balancing: Some(0b1010), bal_rows: Some(2), bal_n: Some(4), ..minute_row(0, (1, 2), (0, 0), (3_300, 3_302)) };
        s.add(60_000, None, &row);
        // 4 of the 6 answers had a status: 40 s known, half of it balancing; 20 s without data
        assert_eq!((s.bal, s.idle, s.no_status, s.cells, s.minutes), (20_000, 20_000, 20_000, 0, true));
        // one status answer of ten (balancing): 10 % known, never the whole minute
        let mut s = Slice::default();
        let row = Row { n: 10, balancing: Some(0b1), bal_rows: Some(1), bal_n: Some(1), ..minute_row(0, (1, 2), (0, 0), (3_300, 3_302)) };
        s.add(60_000, None, &row);
        assert_eq!((s.bal, s.idle, s.no_status, s.cells), (6_000, 0, 54_000, 0));
    }

    #[test]
    fn a_new_balancer_or_new_settings_start_inactive() {
        let rev = |trigger: i64, stop: i64| json!([{ "from": 0, "trigger_mv": trigger, "stop_mv": stop, "when": "always" }]);
        let d = Devices::parse(&json!({
            "defs": [
                { "id": "a", "name": "A", "kind": "active-balancer", "revs": [{ "from": 0, "trigger_mv": 5, "when": "always" }, { "from": 30_000, "trigger_mv": 10, "stop_mv": 5, "when": "always" }] },
                { "id": "b", "name": "B", "kind": "active-balancer", "revs": rev(10, 5) },
            ],
            "assign": { "1": [{ "from": 0, "device": "a" }, { "from": 50_000, "device": "b" }] },
        }));
        let mut e = Ext::default();
        let mut step = |t: i64, spread: i64| e.step(&d, 1, &answer(t, 1, spread, 0), 30_000).map(|(def, a)| (def.id.clone(), a));
        assert_eq!(step(0, 7), Some(("a".into(), Active)));
        assert_eq!(step(10_000, 7), Some(("a".into(), Active)));
        // A's new revision (trigger 10, stop 5): 7 mV is in its band, but it never started under these settings
        assert_eq!(step(30_000, 7), Some(("a".into(), Inactive)));
        assert_eq!(step(40_000, 12), Some(("a".into(), Active)));
        // B takes over at 7 mV: inside B's band too, but B has not started
        assert_eq!(step(50_000, 7), Some(("b".into(), Inactive)));
    }
}
