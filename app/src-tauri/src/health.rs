//! Health report from the recording (`history.sqlite`): per pack and local day the cell spread (all rows and at
//! rest), the last cycle count and the warmest cell; per pair the current share under load. Read-only, on a reader
//! connection; one pack (or pair) at a time over its primary key, so memory stays at one day's values.
//!
//! Spread and rest come from the range columns (`spread_lo/hi`, `i_lo/hi`): minute rows from before they were kept have
//! NULL there and are left out. A minute row keeps the minute's largest spread, a different measure than single
//! answers: a day with raw rows takes its percentiles from those alone, a day with only minute rows from the minute
//! maxima, and says which (`spread_raw`). Pair shares need single answers a few seconds apart, so only raw rows count
//! (`cell_analysis::raw`, by the row's contents, not its time); pair figures therefore cover the raw-data period only.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// |current| below this (mA) at both ends of a row's range counts as rest.
const REST_MA: i64 = 2_000;
/// Per pack (A) the pair needs for a share, as in the live check (`pairs.ts` PAIR_MIN_A); the pair's total twice that.
const PAIR_MIN_A: i64 = 20;
/// Two answers of a pair this close (ms) count as the same moment.
const MATCH_MS: i64 = 3_000;
/// SOC difference (points) from which a smaller share counts as equalizing (`pairs.ts` EQUALIZE_SOC).
const EQUALIZE_SOC: f64 = 3.0;
/// A day's share is marked outside 50 % ± this (points) over at least MARK_MINUTES minutes with samples (a minute
/// counts with one sample in it, not as a minute observed throughout).
const MARK_POINTS: f64 = 5.0;
const MARK_MINUTES: u32 = 30;
const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Pair {
    pub plus: u8,
    pub minus: u8,
}

#[derive(Serialize, Debug, Default, Clone, PartialEq)]
pub struct PackDay {
    /// local midnight
    pub day: i64,
    pub rows: u32,
    /// cell spread in mV over the day's rows (median, 95th percentile); None without range data
    pub spread_p50: Option<i64>,
    /// whether the spread figures (all and rest) come from raw rows; false: from minute maxima (no raw row that day)
    pub spread_raw: bool,
    pub spread_p95: Option<i64>,
    /// the same at rest only
    pub rest_rows: u32,
    pub rest_p50: Option<i64>,
    pub rest_p95: Option<i64>,
    /// the day's last cycle count
    pub cycles: Option<i64>,
    /// warmest cell of the day (°C)
    pub temp_max: Option<f64>,
    /// the BMS's balancing bits of the day: OR of the rows (bit n = cell n+1), rows with a bit set out of rows with a
    /// status (a minute row counts its own); None without any status
    pub bal_mask: Option<i64>,
    pub bal_rows: Option<i64>,
    pub bal_n: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct PackReport {
    pub pack: u8,
    pub days: Vec<PackDay>,
    /// cycles per 30 days over the period, without a rate across a counter drop; None below RATE_MIN_MS of counts
    pub cycles_per_30d: Option<f64>,
    /// the counted increase and the time it spans (ms), the same segments as the rate; None without any count
    pub cycles_gained: Option<i64>,
    pub cycles_span_ms: Option<i64>,
    /// times the counter dropped (cleared) in the period
    pub cycle_resets: u32,
}

#[derive(Serialize, Debug, Default, Clone, PartialEq)]
pub struct PairDay {
    pub day: i64,
    /// matched samples under load (both currents the same way) and the plus pack's mean share of them (%)
    pub samples: u32,
    pub share: Option<f64>,
    /// samples whose smaller share the states of charge explain (equalizing), left out of the judgement
    pub equalizing: u32,
    /// the rest: samples, minutes with samples, mean share of the plus pack (%)
    pub judged: u32,
    pub judged_minutes: u32,
    pub judged_share: Option<f64>,
    /// share outside 50 % ± 5 points over at least 30 minutes
    pub marked: bool,
}

#[derive(Serialize, Debug)]
pub struct PairReport {
    pub plus: u8,
    pub minus: u8,
    pub days: Vec<PairDay>,
}

#[derive(Serialize, Debug, Default)]
pub struct Report {
    pub from: i64,
    pub to: i64,
    pub packs: Vec<PackReport>,
    pub pairs: Vec<PairReport>,
    /// first row with spread data in the period (older minute rows have none)
    pub spread_from: Option<i64>,
    /// first raw row in the period (pair figures start there)
    pub raw_from: Option<i64>,
}

/// The directional equalizing rule of the live pair check (`pairs.ts` `equalizes`): the pack with the smaller share
/// (SOC `mine`) against its partner (`other`), the pair's total current (negative = discharging).
pub fn equalizes(total: f64, mine: f64, other: f64) -> bool {
    (mine - other).abs() >= EQUALIZE_SOC && ((total < 0.0 && mine < other) || (total > 0.0 && mine > other))
}

/// Local midnights from the day of `from` up to the first one at or after `to` (boundaries of the report's days).
pub fn local_days(from: i64, to: i64) -> Vec<i64> {
    use chrono::{Local, NaiveTime, TimeZone};
    let Some(mut d) = Local.timestamp_millis_opt(from).earliest().map(|x| x.date_naive()) else { return vec![] };
    let mut out = vec![];
    // a midnight a DST change skips: the first full hour of that day
    while let Some(ms) = (0..4).find_map(|h| Local.from_local_datetime(&d.and_time(NaiveTime::from_hms_opt(h, 0, 0)?)).earliest()).map(|x| x.timestamp_millis()) {
        out.push(ms);
        if ms >= to {
            break;
        }
        let Some(next) = d.succ_opt() else { break };
        d = next;
    }
    out
}

/// The report's period clamped to the site's data, as (from, to); None without data.
pub fn site_span(db: &Connection, site: &str, from: i64, to: i64) -> Result<Option<(i64, i64)>, String> {
    let (first, last): (Option<i64>, Option<i64>) =
        db.query_row("SELECT MIN(first), MAX(last) FROM sessions WHERE site = ?1", [site], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e2s)?;
    Ok(first.zip(last).map(|(f, l)| (from.max(f), to.min(l + 1))).filter(|(f, t)| f < t))
}

fn e2s(e: rusqlite::Error) -> String {
    e.to_string()
}

/// Nearest-rank percentile of sorted values.
fn pct(sorted: &[i64], p: f64) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let i = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len()) - 1;
    Some(sorted[i])
}

/// Index of the day `t` falls in, moving `at` forward (rows come in time order).
fn day_index(days: &[i64], at: &mut usize, t: i64) -> Option<usize> {
    while *at + 1 < days.len() && t >= days[*at + 1] {
        *at += 1;
    }
    (*at + 1 < days.len() && t >= days[*at]).then_some(*at)
}

/// The report for `site` over the days `days` (boundaries: n days need n + 1 values).
pub fn report(db: &Connection, site: &str, days: &[i64], pairs: &[Pair]) -> Result<Report, String> {
    let mut out = Report { from: days.first().copied().unwrap_or(0), to: days.last().copied().unwrap_or(0), ..Report::default() };
    let site_id: Option<i64> = db.query_row("SELECT id FROM sites WHERE site = ?1", [site], |r| r.get(0)).optional().map_err(e2s)?;
    let Some(site_id) = site_id.filter(|_| days.len() >= 2) else { return Ok(out) };
    let packs: Vec<u8> = db
        .prepare("SELECT pack FROM site_packs WHERE site = ?1 ORDER BY pack")
        .and_then(|mut s| s.query_map([site], |r| r.get(0))?.collect())
        .map_err(e2s)?;
    for pack in packs {
        let (rep, spread_from, raw_from) = pack_report(db, site_id, pack, days)?;
        out.spread_from = min_opt(out.spread_from, spread_from);
        out.raw_from = min_opt(out.raw_from, raw_from);
        if !rep.days.is_empty() {
            out.packs.push(rep);
        }
    }
    for p in pairs {
        out.pairs.push(PairReport { plus: p.plus, minus: p.minus, days: pair_days(db, site_id, *p, days)? });
    }
    Ok(out)
}

fn min_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// One day's values of a pack while they are collected; spreads of raw rows and of minute rows apart.
#[derive(Default)]
struct DayAcc {
    rows: u32,
    all: Vec<i64>,
    rest: Vec<i64>,
    all_min: Vec<i64>,
    rest_min: Vec<i64>,
    cycles: Option<(i64, i64)>,
    temp_max: Option<i64>,
    bal_mask: Option<i64>,
    bal_rows: Option<i64>,
    bal_n: Option<i64>,
}
impl DayAcc {
    fn finish(self, day: i64) -> PackDay {
        // raw rows when the day has any, else the minute maxima: the two never mix in one percentile
        let spread_raw = !self.all.is_empty();
        let (mut all, mut rest) = if spread_raw { (self.all, self.rest) } else { (self.all_min, self.rest_min) };
        all.sort_unstable();
        rest.sort_unstable();
        PackDay {
            day,
            rows: self.rows,
            spread_p50: pct(&all, 0.5),
            spread_p95: pct(&all, 0.95),
            spread_raw,
            rest_rows: rest.len() as u32,
            rest_p50: pct(&rest, 0.5),
            rest_p95: pct(&rest, 0.95),
            cycles: self.cycles.map(|c| c.1),
            temp_max: self.temp_max.map(|v| v as f64 / 10.0),
            bal_mask: self.bal_mask,
            bal_rows: self.bal_rows,
            bal_n: self.bal_n,
        }
    }
}

/// A pack's days, its first row with spread data and its first raw row.
fn pack_report(db: &Connection, site: i64, pack: u8, days: &[i64]) -> Result<(PackReport, Option<i64>, Option<i64>), String> {
    let sql = format!(
        "SELECT t, {}, cycles, temp_max, spread_hi, i_lo, i_hi, balancing, bal_rows, bal_n FROM pack WHERE site = ?1 AND pack = ?2 AND t >= ?3 AND t < ?4 ORDER BY t",
        crate::cell_analysis::RAW_SQL
    );
    let mut st = db.prepare_cached(&sql).map_err(e2s)?;
    let mut rows = st.query(params![site, pack, days[0], days[days.len() - 1]]).map_err(e2s)?;
    let (mut spread_from, mut raw_from) = (None, None);
    let mut out = vec![];
    let mut cur: Option<(usize, DayAcc)> = None;
    let mut at = 0;
    // whether the previous row was at rest (raw rows need it too: a row right after load still sags)
    let mut prev_rest = false;
    // each day's last count with its time, for the rate
    let mut counts: Vec<(i64, i64)> = vec![];
    while let Some(r) = rows.next().map_err(e2s)? {
        let t: i64 = r.get(0).map_err(e2s)?;
        let Some(d) = day_index(days, &mut at, t) else { continue };
        let raw: bool = r.get(1).map_err(e2s)?;
        if raw && raw_from.is_none() {
            raw_from = Some(t);
        }
        if cur.as_ref().is_none_or(|c| c.0 != d) {
            if let Some((i, acc)) = cur.take() {
                counts.extend(acc.cycles);
                out.push(acc.finish(days[i]));
            }
            cur = Some((d, DayAcc::default()));
        }
        let acc = &mut cur.as_mut().unwrap().1;
        acc.rows += 1;
        if let Some(c) = r.get::<_, Option<i64>>(2).map_err(e2s)? {
            acc.cycles = Some((t, c));
        }
        if let Some(v) = r.get::<_, Option<i64>>(3).map_err(e2s)? {
            acc.temp_max = Some(acc.temp_max.map_or(v, |m| m.max(v)));
        }
        if let Some(b) = r.get::<_, Option<i64>>(7).map_err(e2s)? {
            acc.bal_mask = Some(acc.bal_mask.unwrap_or(0) | b);
        }
        for (sum, col) in [(&mut acc.bal_rows, 8), (&mut acc.bal_n, 9)] {
            if let Some(v) = r.get::<_, Option<i64>>(col).map_err(e2s)? {
                *sum = Some(sum.unwrap_or(0) + v);
            }
        }
        let spread: Option<i64> = r.get(4).map_err(e2s)?;
        let (lo, hi): (Option<i64>, Option<i64>) = (r.get(5).map_err(e2s)?, r.get(6).map_err(e2s)?);
        let at_rest = lo.zip(hi).is_some_and(|(lo, hi)| lo > -REST_MA && hi < REST_MA);
        if let Some(s) = spread {
            spread_from.get_or_insert(t);
            let (all, rest) = if raw { (&mut acc.all, &mut acc.rest) } else { (&mut acc.all_min, &mut acc.rest_min) };
            all.push(s);
            if at_rest && (!raw || prev_rest) {
                rest.push(s);
            }
        }
        prev_rest = at_rest;
    }
    if let Some((i, acc)) = cur.take() {
        counts.extend(acc.cycles);
        out.push(acc.finish(days[i]));
    }
    let c = cycle_rate(&counts);
    Ok((PackReport { pack, days: out, cycles_per_30d: c.rate, cycles_gained: c.gained, cycles_span_ms: c.span_ms, cycle_resets: c.resets }, spread_from, raw_from))
}

/// Least span of counts for a rate: the counter counts whole cycles, one step in a day or two reads as 25 per 30 days.
pub const RATE_MIN_MS: i64 = 7 * DAY_MS;

/// Cycles counted in a period: rate per 30 days, increase, span (ms) and drops.
#[derive(Debug, PartialEq)]
pub struct CycleRate {
    pub rate: Option<f64>,
    pub gained: Option<i64>,
    pub span_ms: Option<i64>,
    pub resets: u32,
}

/// Cycles from (time, count) points in time order. A drop (counter cleared) starts a new segment; the increase and
/// the span add up over the segments, never across a drop. The rate is None while they span less than RATE_MIN_MS.
pub fn cycle_rate(points: &[(i64, i64)]) -> CycleRate {
    let (mut gained, mut span, mut resets) = (0i64, 0i64, 0u32);
    let mut start: Option<(i64, i64)> = None;
    let mut last: Option<(i64, i64)> = None;
    for &(t, c) in points {
        if let Some((lt, lc)) = last {
            if c < lc {
                resets += 1;
                if let Some((st, sc)) = start {
                    gained += lc - sc;
                    span += lt - st;
                }
                start = Some((t, c));
            }
        } else {
            start = Some((t, c));
        }
        last = Some((t, c));
    }
    let Some(((st, sc), (lt, lc))) = start.zip(last) else { return CycleRate { rate: None, gained: None, span_ms: None, resets } };
    gained += lc - sc;
    span += lt - st;
    CycleRate { rate: (span >= RATE_MIN_MS).then(|| gained as f64 * 30.0 * DAY_MS as f64 / span as f64), gained: Some(gained), span_ms: Some(span), resets }
}

/// Raw rows of one pack: time, current (mA), SOC (‰).
type Sample = (i64, Option<i64>, Option<i64>);

/// One day's pair samples while they are collected.
#[derive(Default)]
struct PairAcc {
    samples: u32,
    sum: f64,
    equalizing: u32,
    judged: u32,
    judged_sum: f64,
    minutes: u32,
    last_minute: Option<i64>,
}
impl PairAcc {
    fn finish(&self, day: i64) -> PairDay {
        let judged_share = (self.judged > 0).then(|| self.judged_sum / self.judged as f64 * 100.0);
        PairDay {
            day,
            samples: self.samples,
            share: (self.samples > 0).then(|| self.sum / self.samples as f64 * 100.0),
            equalizing: self.equalizing,
            judged: self.judged,
            judged_minutes: self.minutes,
            judged_share,
            marked: self.minutes >= MARK_MINUTES && judged_share.is_some_and(|s| (s - 50.0).abs() > MARK_POINTS),
        }
    }
}

/// A pair's days: each raw plus-pack row with the minus-pack row nearest in time (at most MATCH_MS apart), both read
/// in time order side by side. A sample counts with both currents the same way and their sum at least twice PAIR_MIN_A.
fn pair_days(db: &Connection, site: i64, pair: Pair, days: &[i64]) -> Result<Vec<PairDay>, String> {
    let sql = format!("SELECT t, current, soc FROM pack WHERE site = ?1 AND pack = ?2 AND t >= ?3 AND t < ?4 AND {} ORDER BY t", crate::cell_analysis::RAW_SQL);
    let (mut sp, mut sm) = (db.prepare(&sql).map_err(e2s)?, db.prepare(&sql).map_err(e2s)?);
    let (lo, hi) = (days[0] - MATCH_MS, days[days.len() - 1] + MATCH_MS);
    let mut plus = sp.query(params![site, pair.plus, days[0], days[days.len() - 1]]).map_err(e2s)?;
    let mut minus = sm.query(params![site, pair.minus, lo, hi]).map_err(e2s)?;
    let row = |r: &rusqlite::Row| -> Result<Sample, String> { Ok((r.get(0).map_err(e2s)?, r.get(1).map_err(e2s)?, r.get(2).map_err(e2s)?)) };
    let mut next_minus = || -> Result<Option<Sample>, String> { minus.next().map_err(e2s)?.map(row).transpose() };
    // the last minus row at or before the plus row's time and the first after it
    let (mut before, mut after): (Option<Sample>, Option<Sample>) = (None, next_minus()?);
    let mut out = vec![];
    let mut cur: Option<(usize, PairAcc)> = None;
    let mut at = 0;
    while let Some(r) = plus.next().map_err(e2s)? {
        let a = row(r)?;
        while after.is_some_and(|m| m.0 <= a.0) {
            before = after;
            after = next_minus()?;
        }
        let b = match (before, after) {
            (Some(x), Some(y)) => Some(if a.0 - x.0 <= y.0 - a.0 { x } else { y }),
            (x, y) => x.or(y),
        };
        let Some(b) = b.filter(|b| (b.0 - a.0).abs() <= MATCH_MS) else { continue };
        let (Some(ia), Some(ib)) = (a.1, b.1) else { continue };
        let total = ia + ib;
        // load sharing needs both currents the same way: against each other the packs equalize, they do not share
        if ia.signum() != ib.signum() || total.abs() < 2 * PAIR_MIN_A * 1000 {
            continue;
        }
        let Some(d) = day_index(days, &mut at, a.0) else { continue };
        if cur.as_ref().is_none_or(|c| c.0 != d) {
            if let Some((i, acc)) = cur.take() {
                out.push(acc.finish(days[i]));
            }
            cur = Some((d, PairAcc::default()));
        }
        let acc = &mut cur.as_mut().unwrap().1;
        let share = ia.abs() as f64 / (ia.abs() + ib.abs()) as f64;
        acc.samples += 1;
        acc.sum += share;
        // the pack with the smaller share and the partner, by SOC (%)
        let soc = |s: Option<i64>| s.map(|v| v as f64 / 10.0);
        let (mine, other) = if share < 0.5 { (soc(a.2), soc(b.2)) } else { (soc(b.2), soc(a.2)) };
        let explained = share != 0.5 && mine.zip(other).is_some_and(|(m, o)| equalizes(total as f64, m, o));
        if explained {
            acc.equalizing += 1;
        } else {
            acc.judged += 1;
            acc.judged_sum += share;
            let minute = a.0.div_euclid(MINUTE_MS);
            if acc.last_minute != Some(minute) {
                acc.minutes += 1;
                acc.last_minute = Some(minute);
            }
        }
    }
    if let Some((i, acc)) = cur.take() {
        out.push(acc.finish(days[i]));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;
    use std::path::PathBuf;

    /// A fresh history file with sites "a" (key 1) and "b" (key 2), sessions spanning the first ten days.
    fn db(name: &str) -> (PathBuf, Connection) {
        let dir = std::env::temp_dir().join(format!("openbms-health-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("history.sqlite");
        History::default().open(&path).unwrap();
        let c = Connection::open(&path).unwrap();
        c.execute_batch(&format!(
            "INSERT INTO sites(id, site) VALUES(1, 'a'), (2, 'b');
             INSERT INTO sessions(id, site, bus, source, first, last) VALUES(1, 'a', 'can', '', 0, {d}), (2, 'b', 'can', '', 0, {d});",
            d = 10 * DAY_MS
        ))
        .unwrap();
        (path, c)
    }

    /// One row: range columns as given (None = a minute row from before they were kept).
    #[allow(clippy::too_many_arguments)]
    fn row(c: &Connection, site: i64, pack: u8, t: i64, n: i64, current: i64, soc: i64, cycles: Option<i64>, spread: Option<(i64, i64)>, i: Option<(i64, i64)>) {
        let name = if site == 1 { "a" } else { "b" };
        c.execute("INSERT OR IGNORE INTO site_packs(site, pack) VALUES(?1, ?2)", params![name, pack]).unwrap();
        c.execute(
            "INSERT INTO pack(site, pack, t, session, n, current, soc, cycles, temp_max, spread_lo, spread_hi, i_lo, i_hi)
             VALUES(?1, ?2, ?3, ?1, ?4, ?5, ?6, ?7, 250, ?8, ?9, ?10, ?11)",
            params![site, pack, t, n, current, soc, cycles, spread.map(|s| s.0), spread.map(|s| s.1), i.map(|x| x.0), i.map(|x| x.1)],
        )
        .unwrap();
    }
    fn raw(c: &Connection, site: i64, pack: u8, t: i64, current: i64, spread: i64) {
        row(c, site, pack, t, 1, current, 500, Some(10), Some((spread, spread)), Some((current, current)));
    }
    fn utc_days(n: i64) -> Vec<i64> {
        (0..=n).map(|d| d * DAY_MS).collect()
    }

    #[test]
    fn equalizing_rule_matches_the_frontend_vectors() {
        let vectors: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../src/equalize.vectors.json")).unwrap();
        assert!(vectors.len() >= 5);
        for v in vectors {
            let f = |k: &str| v[k].as_f64().unwrap();
            assert_eq!(equalizes(f("total"), f("mine"), f("other")), v["equalizes"].as_bool().unwrap(), "{}", v["case"]);
        }
    }

    #[test]
    fn rest_leaves_out_the_row_after_load_and_minute_rows_under_load() {
        let (path, c) = db("rest");
        let t0 = 1_000;
        // under load (spread 40), the first row at rest still sagging (30), then settled at rest (10)
        raw(&c, 1, 0, t0, -50_000, 40);
        raw(&c, 1, 0, t0 + 5_000, -50_000, 40);
        raw(&c, 1, 0, t0 + 10_000, 0, 30);
        for k in 3..6 {
            raw(&c, 1, 0, t0 + k * 5_000, 300, 10);
        }
        // the next day only minute rows: one at rest (its largest spread counts), one with load inside the minute, one
        // from before the ranges
        let m = DAY_MS + 10 * MINUTE_MS;
        row(&c, 1, 0, m, 6, 0, 500, Some(10), Some((8, 12)), Some((-1_000, 1_500)));
        row(&c, 1, 0, m + MINUTE_MS, 6, 0, 500, Some(10), Some((9, 25)), Some((-1_000, 5_000)));
        row(&c, 1, 0, m + 2 * MINUTE_MS, 6, 0, 500, Some(10), None, None);
        let r = report(&crate::history::reader(&path).unwrap(), "a", &utc_days(2), &[]).unwrap();
        let d = &r.packs[0].days[0];
        assert_eq!((d.rows, d.rest_rows, d.spread_raw), (6, 3, true));
        assert_eq!((d.rest_p50, d.rest_p95), (Some(10), Some(10)));
        // raw rows: 40 40 30 10 10 10
        assert_eq!((d.spread_p50, d.spread_p95), (Some(10), Some(40)));
        let d = &r.packs[0].days[1];
        assert_eq!((d.rows, d.rest_rows, d.spread_raw), (3, 1, false));
        assert_eq!((d.rest_p95, d.spread_p50, d.spread_p95), (Some(12), Some(12), Some(25)));
        assert_eq!(r.spread_from, Some(t0));
        assert_eq!(r.raw_from, Some(t0));
    }

    #[test]
    fn a_day_with_raw_rows_leaves_its_minute_maxima_out() {
        let (path, c) = db("compaction");
        // a minute at rest: 59 answers at 10 mV and one at 50 mV; compacted, the minute keeps 50 mV
        for k in 0..60 {
            raw(&c, 1, 0, 1_000 + k * 1_000, 0, if k == 59 { 50 } else { 10 });
        }
        row(&c, 1, 0, 2 * 3_600_000, 60, 0, 500, Some(10), Some((10, 50)), Some((0, 0)));
        row(&c, 1, 0, DAY_MS + 2 * 3_600_000, 60, 0, 500, Some(10), Some((10, 50)), Some((0, 0)));
        let r = report(&crate::history::reader(&path).unwrap(), "a", &utc_days(2), &[]).unwrap();
        let d = &r.packs[0].days;
        // the raw day: P95 of the answers, the minute row of the same day left out
        assert_eq!((d[0].spread_raw, d[0].rest_rows, d[0].rest_p95, d[0].spread_p95), (true, 59, Some(10), Some(10)));
        // the compacted day: the minute maximum, marked as such
        assert_eq!((d[1].spread_raw, d[1].rest_rows, d[1].rest_p95), (false, 1, Some(50)));
    }

    #[test]
    fn raw_rows_by_their_contents_not_the_clock() {
        let (path, c) = db("raw-clock");
        // single answers polled on the full minute are raw
        for k in 0..3 {
            raw(&c, 1, 0, (5 + k) * MINUTE_MS, -30_000, 5);
            raw(&c, 1, 1, (5 + k) * MINUTE_MS + 1_000, -20_000, 5);
        }
        let db = crate::history::reader(&path).unwrap();
        let r = report(&db, "a", &utc_days(1), &[Pair { plus: 0, minus: 1 }]).unwrap();
        assert_eq!((r.pairs[0].days[0].samples, r.raw_from, r.packs[0].days[0].spread_raw), (3, Some(5 * MINUTE_MS), true));
        // the SQL test agrees with the cell analysis' one: a compacted minute, ranges that differ, balancing of more rows
        row(&c, 1, 2, 10 * MINUTE_MS, 6, 0, 500, None, Some((5, 5)), Some((0, 0)));
        row(&c, 1, 2, 11 * MINUTE_MS, 1, 0, 500, None, Some((5, 9)), Some((0, 0)));
        row(&c, 1, 2, 12 * MINUTE_MS, 1, 0, 500, None, Some((5, 5)), Some((0, 900)));
        row(&c, 1, 2, 13 * MINUTE_MS, 1, 0, 500, None, Some((5, 5)), Some((0, 0)));
        row(&c, 1, 2, 14 * MINUTE_MS, 1, 0, 500, None, None, None);
        c.execute("UPDATE pack SET bal_n = 6 WHERE pack = 2 AND t = ?1", [13 * MINUTE_MS]).unwrap();
        let sql = format!("SELECT {}, {} FROM pack WHERE pack = 2 ORDER BY t", crate::balancer::ROW_COLS, crate::cell_analysis::RAW_SQL);
        let mut st = db.prepare(&sql).unwrap();
        let got: Vec<(bool, bool)> = st.query_map([], |r| Ok((crate::cell_analysis::raw(&crate::balancer::read_row(r)?), r.get(13)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(got, vec![(false, false), (false, false), (false, false), (false, false), (true, true)]);
    }

    #[test]
    fn currents_against_each_other_are_no_load_sharing() {
        let (path, c) = db("opposite");
        // one answer in each of 30 minutes, +100 A against −40 A: 60 A in total, but the packs work against each other
        for k in 0..30 {
            let t = 1_000 + k * MINUTE_MS;
            raw(&c, 1, 0, t, 100_000, 5);
            raw(&c, 1, 1, t + 1_000, -40_000, 5);
        }
        let r = report(&crate::history::reader(&path).unwrap(), "a", &utc_days(1), &[Pair { plus: 0, minus: 1 }]).unwrap();
        assert!(r.pairs[0].days.is_empty());
    }

    #[test]
    fn cycle_rate_is_split_at_a_counter_drop() {
        let d = DAY_MS;
        // 100 → 110 in 10 days, cleared, 2 → 11 in 9 days: 19 cycles in 19 days, nothing across the drop
        let c = cycle_rate(&[(0, 100), (10 * d, 110), (11 * d, 2), (20 * d, 11)]);
        assert_eq!((c.resets, c.gained, c.span_ms), (1, Some(19), Some(19 * d)));
        assert!((c.rate.unwrap() - 30.0).abs() < 1e-9);
        // one step in a day or two is the counter's resolution, not a rate; from seven days there is one
        assert_eq!(cycle_rate(&[(0, 5), (d + d / 5, 6)]), CycleRate { rate: None, gained: Some(1), span_ms: Some(d + d / 5), resets: 0 });
        assert!((cycle_rate(&[(0, 5), (7 * d, 12)]).rate.unwrap() - 30.0).abs() < 1e-9);
        assert_eq!(cycle_rate(&[]), CycleRate { rate: None, gained: None, span_ms: None, resets: 0 });
        // the same through the report: each day's last count
        let (path, c) = db("cycles");
        for (day, cycles) in [(0, 100), (1, 101), (2, 102), (3, 1), (4, 2)] {
            row(&c, 1, 3, day * d + 1_000, 1, 0, 500, Some(cycles - 1), None, None);
            row(&c, 1, 3, day * d + 2_000, 1, 0, 500, Some(cycles), None, None);
        }
        let r = report(&crate::history::reader(&path).unwrap(), "a", &utc_days(5), &[]).unwrap();
        let p = &r.packs[0];
        assert_eq!(p.days.iter().map(|x| x.cycles.unwrap()).collect::<Vec<_>>(), vec![100, 101, 102, 1, 2]);
        assert_eq!(p.cycle_resets, 1);
        // 2 cycles in 2 days, then 1 in 1 day: 3 in 3 days, too short for a rate
        assert_eq!((p.cycles_per_30d, p.cycles_gained, p.cycles_span_ms), (None, Some(3), Some(3 * d)));
        assert_eq!(p.days[0].temp_max, Some(25.0));
    }

    #[test]
    fn pairs_match_raw_rows_within_three_seconds() {
        let (path, c) = db("pairs");
        // 40 minutes every 10 s: plus -30 A, minus -20 A 2.5 s later (share 60 %)
        for k in 0..240 {
            let t = 1_000 + k * 10_000;
            raw(&c, 1, 0, t, -30_000, 5);
            raw(&c, 1, 1, t + 2_500, -20_000, 5);
        }
        // the next hour: the partner 4 s away (no match), then too little load (25 A in total)
        for k in 0..60 {
            let t = 3 * 3_600_000 + k * 10_000;
            raw(&c, 1, 0, t, -30_000, 5);
            raw(&c, 1, 1, t + 4_000, -20_000, 5);
            raw(&c, 1, 0, t + 3_600_000, -15_000, 5);
            raw(&c, 1, 1, t + 3_600_000 + 1_000, -10_000, 5);
        }
        // compacted minutes are never matched
        row(&c, 1, 0, 6 * 3_600_000, 6, -30_000, 500, None, None, None);
        row(&c, 1, 1, 6 * 3_600_000, 6, -20_000, 500, None, None, None);
        let r = report(&crate::history::reader(&path).unwrap(), "a", &utc_days(1), &[Pair { plus: 0, minus: 1 }]).unwrap();
        let d = &r.pairs[0].days[0];
        assert_eq!((d.samples, d.judged, d.equalizing, d.judged_minutes), (240, 240, 0, 40));
        assert!((d.share.unwrap() - 60.0).abs() < 1e-9);
        assert!(d.marked, "10 points off over 40 minutes");
    }

    #[test]
    fn a_share_the_states_of_charge_explain_is_not_marked() {
        let (path, c) = db("equalize");
        for k in 0..240 {
            let t = 1_000 + k * 10_000;
            // discharging, the minus pack gives less and is 10 points emptier
            row(&c, 1, 0, t, 1, -30_000, 700, None, None, None);
            row(&c, 1, 1, t + 1_000, 1, -20_000, 600, None, None, None);
        }
        let r = report(&crate::history::reader(&path).unwrap(), "a", &utc_days(1), &[Pair { plus: 0, minus: 1 }]).unwrap();
        let d = &r.pairs[0].days[0];
        assert_eq!((d.samples, d.equalizing, d.judged, d.judged_share, d.marked), (240, 240, 0, None, false));
        // charging the same way is not explained
        let (path, c) = db("equalize-charge");
        for k in 0..240 {
            let t = 1_000 + k * 10_000;
            row(&c, 1, 0, t, 1, 30_000, 700, None, None, None);
            row(&c, 1, 1, t + 1_000, 1, 20_000, 600, None, None, None);
        }
        let r = report(&crate::history::reader(&path).unwrap(), "a", &utc_days(1), &[Pair { plus: 0, minus: 1 }]).unwrap();
        assert!(r.pairs[0].days[0].marked);
    }

    #[test]
    fn only_the_sites_rows_in_the_period_count() {
        let (path, c) = db("isolation");
        raw(&c, 1, 0, DAY_MS + 1_000, 0, 10);
        raw(&c, 1, 0, 3 * DAY_MS + 1_000, 0, 99);
        raw(&c, 2, 0, DAY_MS + 2_000, 0, 77);
        raw(&c, 2, 5, DAY_MS + 2_000, 0, 77);
        let db = crate::history::reader(&path).unwrap();
        let days: Vec<i64> = (1..=2).map(|d| d * DAY_MS).collect();
        let r = report(&db, "a", &days, &[]).unwrap();
        assert_eq!(r.packs.len(), 1);
        assert_eq!(r.packs[0].days.iter().map(|d| (d.day, d.spread_p50)).collect::<Vec<_>>(), vec![(DAY_MS, Some(10))]);
        assert_eq!(report(&db, "b", &days, &[]).unwrap().packs.iter().map(|p| p.pack).collect::<Vec<_>>(), vec![0, 5]);
        assert!(report(&db, "nobody", &days, &[]).unwrap().packs.is_empty());
        // the period is clamped to the site's sessions
        assert_eq!(site_span(&db, "a", -5, 100 * DAY_MS).unwrap(), Some((0, 10 * DAY_MS + 1)));
        assert_eq!(site_span(&db, "nobody", 0, 1).unwrap(), None);
    }

    #[test]
    fn the_balancing_bits_of_a_day() {
        let (path, c) = db("balancing");
        for (k, bits) in [Some(0b100), Some(0), None, Some(0b1)].into_iter().enumerate() {
            raw(&c, 1, 0, 1_000 + k as i64 * 10_000, 0, 5);
            c.execute("UPDATE pack SET balancing = ?1, bal_rows = ?2, bal_n = ?3 WHERE t = ?4", params![bits, bits.map(|b| i64::from(b != 0)), bits.map(|_| 1), 1_000 + k as i64 * 10_000]).unwrap();
        }
        // a minute row: bits of its rows ORed, 2 of 6 with a bit set
        row(&c, 1, 0, 10 * MINUTE_MS, 6, 0, 500, None, Some((5, 5)), Some((0, 0)));
        c.execute("UPDATE pack SET balancing = 8, bal_rows = 2, bal_n = 6 WHERE t = ?1", [10 * MINUTE_MS]).unwrap();
        raw(&c, 1, 0, DAY_MS + 1_000, 0, 5);
        let r = report(&crate::history::reader(&path).unwrap(), "a", &utc_days(2), &[]).unwrap();
        let d = &r.packs[0].days;
        assert_eq!((d[0].bal_mask, d[0].bal_rows, d[0].bal_n), (Some(0b1101), Some(4), Some(9)));
        assert_eq!((d[1].bal_mask, d[1].bal_rows, d[1].bal_n), (None, None, None), "no status: unknown, not 0");
    }

    #[test]
    fn local_days_cover_the_period() {
        let d = local_days(5 * DAY_MS, 8 * DAY_MS);
        assert!(d.len() >= 4 && d[0] <= 5 * DAY_MS && *d.last().unwrap() >= 8 * DAY_MS);
        assert!(d.windows(2).all(|w| (w[1] - w[0] - DAY_MS).abs() <= 3_600_000));
    }
}
