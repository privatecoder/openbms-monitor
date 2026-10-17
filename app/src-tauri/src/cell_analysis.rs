//! Cell analysis of one pack from the recording (spec docs/superpowers/specs/2026-10-17-cell-analysis-balancers.md,
//! part B): per cell the deviation from the median cell of the same row in four situations (rest, top, bottom, under
//! load), the resistance from load steps, and a classification that says what the data allow and why.
//!
//! Raw rows only (single answers), and only those with the pack's usual cell count (the most frequent one in the
//! period); the others are counted. Pairs of rows (load steps, the rest predecessor, jumps) and the external balancer's
//! state are formed only within one session and without a gap of more than GAP_PERIODS poll periods. Rows where an
//! assigned external balancer is computed active (or not decidable) are left out of rest, top, bottom and load; a
//! cell's rest values while the BMS reports its balancing bit are left out too.
//!
//! Load steps and jumps are formed only between rows free of external balancer activity (computed active or not
//! decidable) and, per cell, free of the BMS's balancing bit, so balancing never reads as resistance or a contact fault.
//!
//! Three streaming passes, nothing per row is kept: the first (without the cell blobs) finds the poll periods and the
//! usual cell count, the second the load steps (resistance), the third the deviations (corrected by the resistance),
//! the heatmap, jumps and the order at the top. Medians come from fixed-bin histograms (half mV, 0.005 mΩ), so memory
//! is bounded by the cell count, not by the length of the recording.

use crate::balancer::{self, Activity, Devices, Ext, Periods, Row};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;

const MINUTE_MS: i64 = 60_000;
/// |current| (mA) below this at a row and the row before counts as rest (as in the health report)
const REST_MA: i64 = 2_000;
/// top: the highest cell at least this (mV) while charging
const TOP_MV: i64 = 3_400;
/// the order in which the cells pass this (mV) at the top
const ORDER_MV: i64 = 3_450;
/// bottom: the lowest cell at most this (mV) while discharging
const BOTTOM_MV: i64 = 3_150;
/// under load (the constant-offset check): |current| from this (mA), in the flat region between bottom and top
const LOAD_MA: i64 = 10_000;
/// a load step: the pack current changes by at least this (mA) between two consecutive rows
const STEP_MA: i64 = 20_000;
/// a situation needs this many rows, the resistance this many load steps
pub const MIN_ROWS: u32 = 30;
pub const MIN_STEPS: u32 = 10;
/// passend: rest within ± this (mV), resistance within ± R_OK of the median, top and bottom within ± END_OK_MV
pub const REST_OK_MV: f64 = 5.0;
pub const R_OK: f64 = 0.20;
pub const END_OK_MV: f64 = 15.0;
/// Widerstand erhöht from this share of the median, and at least R_RES_MV over the median |ΔI| of the steps above the
/// median (mΩ): the cell voltages come in whole mV, so a step of a mV or two is the resolution, not the cell
pub const R_HIGH: f64 = 1.30;
pub const R_RES_MV: f64 = 2.0;
/// a constant offset: rest, under load and top (or bottom) within this (mV) of each other
pub const OFFSET_TOL_MV: f64 = 5.0;
/// a jump: the deviation of a cell changes by at least this (mV) between consecutive rows with a steady current
const JUMP_MV: i64 = 20;
/// a steady current for the jump check: it changes by less than this (mA)
const JUMP_STEADY_MA: i64 = 5_000;
/// jumps from which a cell is auffällig
pub const JUMP_MIN: u32 = 3;
/// the deviation histograms keep ± this (mV) in half-mV bins, beyond counts at the edge (the heatmap clip is the UI's)
const HIST_MV: i64 = 1_000;
/// the resistance histograms: bins of this (mΩ) within ± R_SPAN bins (± 10 mΩ), beyond counts at the edge
const R_BIN_MOHM: f64 = 0.005;
const R_SPAN: i64 = 2_000;

// ---------------------------------------------------------------------------------------------
// classification

/// What the data say about one cell.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// passend
    Ok,
    /// Ruhewerte passend: rest (and the resistance, if known) within the limits, nothing at top or bottom: capacity not
    /// checked
    RestOk,
    /// Ladestand-Versatz: the same sign at top and bottom
    SocOffset,
    /// Versatz in Ruhe (Ladestand oder Messung): off at rest, nothing at top or bottom to tell which
    RestOffset,
    /// Kapazität vermutlich kleiner
    Capacity,
    /// mehrdeutig (Widerstand oder Kapazität)
    Ambiguous,
    /// Widerstand erhöht
    Resistance,
    /// auffällig (Messung/Kontakt?)
    Measurement,
    /// keine Aussage
    #[serde(rename = "none")]
    NoStatement,
}

/// Why, in values the UI words (mV, % of the median, counts).
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(tag = "key", rename_all = "snake_case")]
pub enum Reason {
    /// everything within the limits: rest, resistance, top and bottom
    Within,
    /// within the limits for what is there; `top`/`bottom`/`r`: whether that figure is there (steps: the cell's)
    Partial { top: bool, bottom: bool, r: bool, steps: u32 },
    /// Ruhewerte passend: rest within the limits, no top, bottom or resistance to judge the rest by
    RestOnly { rest: f64 },
    /// at rest off the median, nothing at top or bottom against it: SOC offset in the flat region or a measurement
    /// offset (told apart only with a charge over 3.40 V)
    Rest { rest: f64 },
    /// high (or low) at the top and at the bottom
    SameSign { top: f64, bottom: f64 },
    /// high at the top and low at the bottom, both corrected by the cell's resistance
    Capacity { top: f64, bottom: f64 },
    /// high at the top and low at the bottom, but the resistance is not known (too few load steps)
    Ambiguous { top: f64, bottom: f64, steps: u32 },
    Resistance { pct: f64, steps: u32 },
    /// the same offset at rest, under load and at the end(s) available
    Offset { rest: f64, load: f64 },
    Jumps { count: u32 },
    /// keine Aussage: fewer than MIN_ROWS rest rows for this cell
    NoRest { rows: u32 },
    /// keine Aussage: off at the top, no discharge under 3.15 V recorded to tell SOC from capacity
    TopOnly { top: f64 },
    /// keine Aussage: off at the bottom, no charge over 3.40 V recorded
    BottomOnly { bottom: f64 },
    /// keine Aussage: top and bottom fit no class (e.g. low at the top and high at the bottom)
    Pattern { top: f64, bottom: f64 },
    /// keine Aussage: resistance outside ± 20 % but below 130 %, or above it by less than the resolution (R_RES_MV)
    RSlight { pct: f64 },
}

/// The figures of one cell the classification works on. `r_rel`: resistance / pack median, None with fewer than
/// MIN_STEPS steps; `r_above`: resistance − pack median (mΩ), `r_bound`: R_RES_MV / median |ΔI| of the pack's steps
/// (mΩ); `top_corr`/`bottom_corr` only with `r_rel`. A situation with too few rows is None.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub rest: Option<f64>,
    pub rest_rows: u32,
    pub load: Option<f64>,
    pub top: Option<f64>,
    pub bottom: Option<f64>,
    pub top_corr: Option<f64>,
    pub bottom_corr: Option<f64>,
    pub r_rel: Option<f64>,
    pub r_above: Option<f64>,
    pub r_bound: Option<f64>,
    pub steps: u32,
    pub jumps: u32,
}

/// The class of a cell and the reason. Order: jumps and a constant offset (measurement) first, then resistance, then
/// the pattern of top and bottom, then rest.
pub fn classify(c: &Input) -> (Class, Reason) {
    if c.jumps >= JUMP_MIN {
        return (Class::Measurement, Reason::Jumps { count: c.jumps });
    }
    // with the resistance known, the ends are judged corrected
    let top = c.top_corr.or(c.top);
    let bottom = c.bottom_corr.or(c.bottom);
    if let (Some(rest), Some(load)) = (c.rest, c.load) {
        let near = |v: Option<f64>| v.is_none_or(|v| (v - rest).abs() <= OFFSET_TOL_MV);
        if rest.abs() > REST_OK_MV && (load - rest).abs() <= OFFSET_TOL_MV && (top.is_some() || bottom.is_some()) && near(top) && near(bottom) {
            return (Class::Measurement, Reason::Offset { rest, load });
        }
    }
    // high relative to the median and by more than the mV resolution allows over the steps' current
    let resolved = c.r_above.zip(c.r_bound).is_some_and(|(a, b)| a >= b);
    if let Some(r) = c.r_rel.filter(|&r| r >= R_HIGH && resolved) {
        return (Class::Resistance, Reason::Resistance { pct: r * 100.0, steps: c.steps });
    }
    let off = |v: Option<f64>| v.is_some_and(|v| v.abs() > END_OK_MV);
    if let (Some(t), Some(b)) = (top, bottom) {
        if t > END_OK_MV && b < -END_OK_MV {
            return match (c.top_corr, c.bottom_corr) {
                (Some(t), Some(b)) => (Class::Capacity, Reason::Capacity { top: t, bottom: b }),
                _ => (Class::Ambiguous, Reason::Ambiguous { top: t, bottom: b, steps: c.steps }),
            };
        }
        // an SOC offset shows on both ends with the same sign; resistance would give opposite signs
        if t.signum() == b.signum() && t.abs().max(b.abs()) > END_OK_MV && t.abs().min(b.abs()) > REST_OK_MV {
            return (Class::SocOffset, Reason::SameSign { top: t, bottom: b });
        }
        if off(top) || off(bottom) {
            return (Class::NoStatement, Reason::Pattern { top: t, bottom: b });
        }
    } else if let Some(t) = top.filter(|_| off(top)) {
        return (Class::NoStatement, Reason::TopOnly { top: t });
    } else if let Some(b) = bottom.filter(|_| off(bottom)) {
        return (Class::NoStatement, Reason::BottomOnly { bottom: b });
    }
    let Some(rest) = c.rest else { return (Class::NoStatement, Reason::NoRest { rows: c.rest_rows }) };
    if rest.abs() > REST_OK_MV {
        return (Class::RestOffset, Reason::Rest { rest });
    }
    // passend needs top or bottom: without either the capacity is not checked
    match (c.r_rel, top.is_some(), bottom.is_some()) {
        (Some(r), _, _) if (r - 1.0).abs() > R_OK => (Class::NoStatement, Reason::RSlight { pct: r * 100.0 }),
        (Some(_), true, true) => (Class::Ok, Reason::Within),
        (None, false, false) => (Class::RestOk, Reason::RestOnly { rest }),
        (Some(_), false, false) => (Class::RestOk, Reason::Partial { top: false, bottom: false, r: true, steps: c.steps }),
        (r, t, b) => (Class::Ok, Reason::Partial { top: t, bottom: b, r: r.is_some(), steps: c.steps }),
    }
}

// ---------------------------------------------------------------------------------------------
// aggregation

/// Values in fixed integer bins within ± `span`; the median without keeping the values. Memory: 2 × span + 1 counters,
/// allocated on the first value.
#[derive(Clone)]
struct Hist {
    span: i64,
    bins: Vec<u32>,
    n: u32,
}

impl Hist {
    fn new(span: i64) -> Hist {
        Hist { span, bins: vec![], n: 0 }
    }

    fn add(&mut self, v: i64) {
        if self.bins.is_empty() {
            self.bins = vec![0; (2 * self.span + 1) as usize];
        }
        self.bins[(v.clamp(-self.span, self.span) + self.span) as usize] += 1;
        self.n += 1;
    }

    /// The median in bin units, None below `min` values.
    fn median(&self, min: u32) -> Option<f64> {
        if self.n < min.max(1) {
            return None;
        }
        let (lo, hi) = ((self.n - 1) / 2, self.n / 2);
        let (mut seen, mut a, mut b) = (0u32, None, None);
        for (i, &c) in self.bins.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let v = i as i64 - self.span;
            if a.is_none() && seen + c > lo {
                a = Some(v);
            }
            if seen + c > hi {
                b = Some(v);
                break;
            }
            seen += c;
        }
        Some((a? + b?) as f64 / 2.0)
    }
}

/// Deviations in half mV.
fn dev_hist() -> Hist {
    Hist::new(2 * HIST_MV)
}

/// mV from a deviation histogram, None below MIN_ROWS.
fn dev_median(h: &Hist) -> Option<f64> {
    h.median(MIN_ROWS).map(|v| v / 2.0)
}

/// Twice the row's median cell (mV), so an even count stays an integer.
fn median2(cells: &[i64]) -> i64 {
    let mut s = cells.to_vec();
    s.sort_unstable();
    let m = s.len() / 2;
    if s.len().is_multiple_of(2) { s[m - 1] + s[m] } else { 2 * s[m] }
}

fn median_f(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable_by(|a, b| a.total_cmp(b));
    let m = v.len() / 2;
    Some(if v.len().is_multiple_of(2) { (v[m - 1] + v[m]) / 2.0 } else { v[m] })
}

/// A raw row: one answer (n = 1, kept from an export's `samples` on import), without ranges that differ or balancing
/// counts of more rows. The time on a minute boundary decides nothing: an answer polled on the full minute is raw. A row
/// without cells (no spread) or from before the range columns is raw too: it is counted, not taken for a minute.
pub(crate) fn raw(r: &Row) -> bool {
    r.n == 1 && r.bal_n.is_none_or(|n| n <= 1) && r.spread_lo == r.spread_hi && r.i_lo == r.i_hi
}

/// `raw` as an SQL expression over the pack table's columns (the health report's pair rows and spread days).
pub(crate) const RAW_SQL: &str = "(n = 1 AND IFNULL(bal_n, 0) <= 1 AND spread_lo IS spread_hi AND i_lo IS i_hi)";

/// Whether the BMS reported cell `k` (0-based) balancing in the row.
fn bit(balancing: Option<i64>, k: usize) -> bool {
    balancing.is_some_and(|b| k < 63 && b >> k & 1 == 1)
}

/// Whether the row is free of external balancer activity (none assigned, or computed inactive).
fn free(ext: Option<Activity>) -> bool {
    !matches!(ext, Some(Activity::Active | Activity::Unknown))
}

/// One pack row with its cells as stored (mV); None without cells.
struct Line {
    row: Row,
    cells: Option<Vec<i64>>,
}

fn each_line(db: &Connection, site: &str, pack: u8, from: i64, to: i64, mut f: impl FnMut(Line)) -> Result<(), String> {
    let sql = format!(
        "SELECT {}, cells FROM pack WHERE site = (SELECT id FROM sites WHERE site = ?1) AND pack = ?2 AND t >= ?3 AND t <= ?4 ORDER BY t",
        balancer::ROW_COLS
    );
    let mut st = db.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = st.query(params![site, pack, from, to]).map_err(|e| e.to_string())?;
    while let Some(r) = rows.next().map_err(|e| e.to_string())? {
        let row = balancer::read_row(r).map_err(|e| e.to_string())?;
        let cells = r.get::<_, Option<Vec<u8>>>(13).map_err(|e| e.to_string())?.map(|b| crate::history::unblob_u16(&b)).filter(|c| !c.is_empty());
        f(Line { row, cells });
    }
    Ok(())
}

/// The rows of the span without the cell blobs, with their cell count (length of the blob), one at a time.
fn each_head(db: &Connection, site: &str, pack: u8, from: i64, to: i64, mut f: impl FnMut(Row, usize)) -> Result<(), String> {
    let sql = format!(
        "SELECT {}, IFNULL(length(cells), 0) / 2 FROM pack WHERE site = (SELECT id FROM sites WHERE site = ?1) AND pack = ?2 AND t >= ?3 AND t <= ?4 ORDER BY t",
        balancer::ROW_COLS
    );
    let mut st = db.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = st.query(params![site, pack, from, to]).map_err(|e| e.to_string())?;
    while let Some(r) = rows.next().map_err(|e| e.to_string())? {
        f(balancer::read_row(r).map_err(|e| e.to_string())?, r.get::<_, i64>(13).map_err(|e| e.to_string())? as usize);
    }
    Ok(())
}

/// The longest time (ms) a row still pairs with the one before in its session.
fn gap_of(periods: &HashMap<i64, i64>, r: &Row) -> i64 {
    balancer::GAP_PERIODS * if r.single() { periods.get(&r.session).copied().unwrap_or(balancer::DEFAULT_PERIOD_MS) } else { MINUTE_MS }
}

/// One cell's result.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct CellResult {
    /// 1-based
    pub cell: u8,
    /// median deviation from the row median (mV) per situation; None with fewer than MIN_ROWS rows
    pub rest: Option<f64>,
    pub rest_rows: u32,
    pub load: Option<f64>,
    pub top: Option<f64>,
    pub bottom: Option<f64>,
    /// top and bottom corrected by the cell's own resistance (`dev − (R_cell − R_median) × I`); None without it
    pub top_corr: Option<f64>,
    pub bottom_corr: Option<f64>,
    /// ΔU/ΔI median over the load steps (mΩ) and as % of the pack median; None with fewer than MIN_STEPS steps
    pub r_mohm: Option<f64>,
    /// load steps of this cell (the pack's minus those with its BMS bit at either end)
    pub steps: u32,
    pub r_pct: Option<f64>,
    /// mean place when passing 3.45 V (1 = first) over `Analysis::charges` charges
    pub order: Option<f64>,
    pub jumps: u32,
    pub class: Class,
    pub reason: Reason,
}

#[derive(Serialize, Debug, Default)]
pub struct Analysis {
    pub from: i64,
    pub to: i64,
    /// the pack's usual cell count (most frequent in the period); 0 without raw rows with cells
    pub cell_count: usize,
    /// raw rows with the usual count (analysed)
    pub rows: u64,
    /// raw rows without complete cell values (another count or none): ohne vollständige Zellwerte
    pub incomplete: u64,
    /// compacted (minute) rows in the period: left out, only raw rows count
    pub minute_rows: u64,
    /// rows left out of rest/top/bottom/load: external balancer computed active, or not decidable
    pub balancer_active: u64,
    pub balancer_unknown: u64,
    /// whether a balancer was assigned at some time of the period
    pub balancer: bool,
    /// rows per situation (before the per-cell BMS bit exclusion at rest)
    pub rest_rows: u64,
    pub top_rows: u64,
    pub bottom_rows: u64,
    pub load_rows: u64,
    /// rest values of a cell left out because the BMS balanced it; rest rows without a status (bits unknown)
    pub bms_left_out: u64,
    pub rest_no_status: u64,
    /// load steps free of external balancer activity, and the pack median of the cells' resistance (mΩ)
    pub steps: u32,
    pub r_median: Option<f64>,
    /// charges that passed 3.45 V (for the order)
    pub charges: u32,
    pub cells: Vec<CellResult>,
    /// mean deviation from the row median (mV) per cell and slice of [from, to]; empty with 0 slices
    pub heat: Vec<Vec<Option<f32>>>,
}

/// Places of the cells passing ORDER_MV in one charge: cells by the time they first passed (ties share a place),
/// those that never did after them.
fn places(first: &[Option<i64>]) -> Option<Vec<u32>> {
    let passed: Vec<i64> = first.iter().flatten().copied().collect();
    if passed.is_empty() {
        return None;
    }
    Some(first.iter().map(|f| f.map_or(passed.len() as u32 + 1, |t| 1 + passed.iter().filter(|&&x| x < t).count() as u32)).collect())
}

/// Adds the places of a finished charge (not spoiled, at least one cell passed).
fn finish_charge(c: Option<(Vec<Option<i64>>, bool)>, order_sum: &mut [u64], order_n: &mut u32) {
    if let Some((first, false)) = c {
        if let Some(p) = places(&first) {
            order_sum.iter_mut().zip(p).for_each(|(s, p)| *s += p as u64);
            *order_n += 1;
        }
    }
}

/// The analysis of `pack` over [from, to]; `slices`: heatmap columns (0 = none).
pub fn analyse(db: &Connection, site: &str, pack: u8, from: i64, to: i64, devices: &Devices, slices: usize) -> Result<Analysis, String> {
    let to = to.max(from + 1);
    let mut out = Analysis { from, to, ..Default::default() };
    let start = from - balancer::WARMUP_MS;

    // pass 1, without the blobs: poll periods and the usual cell count
    let mut periods = Periods::default();
    let mut counts: HashMap<usize, u64> = HashMap::new();
    each_head(db, site, pack, start, to, |row, count| {
        periods.add(&row);
        if row.t >= from && raw(&row) && count > 0 {
            *counts.entry(count).or_default() += 1;
        }
    })?;
    let periods = periods.finish();
    // the most frequent count; a tie goes to the larger
    let Some((&n, _)) = counts.iter().max_by_key(|(&k, &v)| (v, k)) else {
        // nothing to analyse; still count what is there
        each_head(db, site, pack, from, to, |row, _| if raw(&row) { out.incomplete += 1 } else { out.minute_rows += 1 })?;
        return Ok(out);
    };
    out.cell_count = n;

    // pass 2: load steps between consecutive raw rows of a session with the usual cells, both free of external
    // balancer activity; a cell's step is left out when its BMS bit is set at either end. ΔU/ΔI (mV/A = mΩ) per cell
    // into fixed bins.
    let mut r_hist = vec![Hist::new(R_SPAN); n];
    // |ΔI| of the steps in 0.1 A bins (up to 500 A), for the resolution bound of the resistance
    let mut di_hist = Hist::new(5_000);
    {
        let mut ext = Ext::default();
        // the row before (raw rows with the usual cells only; anything else breaks the pair)
        struct Before {
            t: i64,
            session: i64,
            current: Option<i64>,
            cells: Vec<i64>,
            balancing: Option<i64>,
            /// free of external activity
            free: bool,
        }
        let mut before: Option<Before> = None;
        each_line(db, site, pack, start, to, |l| {
            let row = &l.row;
            let gap = gap_of(&periods, row);
            let act = ext.step(devices, pack, row, gap).map(|(_, a)| a);
            let Some(cells) = l.cells.filter(|c| raw(row) && c.len() == n) else {
                before = None;
                return;
            };
            if let (Some(b), Some(i)) = (&before, row.current) {
                if let Some(bi) = b.current.filter(|bi| row.t >= from && b.session == row.session && row.t - b.t <= gap && (i - bi).abs() >= STEP_MA && b.free && free(act)) {
                    out.steps += 1;
                    let di = (i - bi) as f64 / 1000.0;
                    di_hist.add((di.abs() * 10.0).round() as i64);
                    for (k, h) in r_hist.iter_mut().enumerate() {
                        if !bit(b.balancing, k) && !bit(row.balancing, k) {
                            h.add(((cells[k] - b.cells[k]) as f64 / di / R_BIN_MOHM).round() as i64);
                        }
                    }
                }
            }
            before = Some(Before { t: row.t, session: row.session, current: row.current, cells, balancing: row.balancing, free: free(act) });
        })?;
    }
    let r: Vec<Option<f64>> = r_hist.iter().map(|h| h.median(MIN_STEPS).map(|v| v * R_BIN_MOHM)).collect();
    let cell_steps: Vec<u32> = r_hist.iter().map(|h| h.n).collect();
    drop(r_hist);
    out.r_median = median_f(&mut r.iter().flatten().copied().collect::<Vec<_>>());
    // R_cell − R_median per cell (mΩ), for the correction of top and bottom
    let dr: Vec<Option<f64>> = r.iter().map(|x| x.zip(out.r_median).map(|(a, m)| a - m)).collect();
    // what the whole-mV cell voltages allow over the steps' median current (mΩ)
    let r_bound = di_hist.median(1).filter(|&d| d > 0.0).map(|d| R_RES_MV / (d / 10.0));

    // pass 3: deviations, heatmap, jumps, order at the top
    let hists = || vec![dev_hist(); n];
    let (mut rest, mut load, mut top, mut bottom, mut top_c, mut bottom_c) = (hists(), hists(), hists(), hists(), hists(), hists());
    let mut jumps = vec![0u32; n];
    let (mut order_sum, mut order_n) = (vec![0u64; n], 0u32);
    let slices = slices.min(2000);
    let width = (to - from) as f64 / slices.max(1) as f64;
    let mut heat = if slices > 0 { vec![vec![(0f64, 0u32); slices]; n] } else { vec![] };
    let mut ext = Ext::default();
    // the row before in time (any): the row, its deviations if it had the usual cells, whether free of external activity
    let mut before: Option<(Row, Option<Vec<i64>>, bool)> = None;
    // one charge towards the top: armed once the highest cell is below TOP_MV; first time each cell passed ORDER_MV;
    // spoiled if a balancer row fell into it
    let mut charge: Option<(Vec<Option<i64>>, bool)> = None;
    each_line(db, site, pack, start, to, |l| {
        let row = &l.row;
        let gap = gap_of(&periods, row);
        let ext_now = ext.step(devices, pack, row, gap).map(|(_, a)| a);
        if ext_now.is_some() && row.t >= from {
            out.balancer = true;
        }
        let paired = before.as_ref().filter(|(b, _, _)| b.session == row.session && row.t - b.t <= gap && raw(b));
        let prev_i = paired.and_then(|(b, _, _)| b.current);
        let prev = paired.and_then(|(b, d, f)| d.clone().map(|d| (d, b.balancing, *f)));
        if !raw(row) {
            if row.t >= from {
                out.minute_rows += 1;
            }
            finish_charge(charge.take(), &mut order_sum, &mut order_n);
            before = Some((row.clone(), None, free(ext_now)));
            return;
        }
        let Some(cells) = l.cells.as_ref().filter(|c| c.len() == n) else {
            if row.t >= from {
                out.incomplete += 1;
            }
            before = Some((row.clone(), None, free(ext_now)));
            return;
        };
        let m2 = median2(cells);
        let dev: Vec<i64> = cells.iter().map(|c| 2 * c - m2).collect();
        if row.t < from {
            before = Some((row.clone(), Some(dev), free(ext_now)));
            return;
        }
        out.rows += 1;
        if slices > 0 {
            let k = (((row.t - from) as f64 / width) as usize).min(slices - 1);
            for (h, d) in heat.iter_mut().zip(&dev) {
                h[k].0 += *d as f64 / 2.0;
                h[k].1 += 1;
            }
        }
        match ext_now {
            Some(Activity::Active) => out.balancer_active += 1,
            Some(Activity::Unknown) => out.balancer_unknown += 1,
            _ => {}
        }
        let left_out = !free(ext_now);
        let (hi, lo) = (*cells.iter().max().unwrap(), *cells.iter().min().unwrap());
        // a new charge towards the top once the highest cell is below it; a session change or gap ends one
        if paired.is_none() || hi < TOP_MV {
            finish_charge(charge.take(), &mut order_sum, &mut order_n);
        }
        if hi < TOP_MV {
            charge = Some((vec![None; n], false));
        }
        if let Some(ma) = row.current {
            let corrected = |d: i64, k: usize| dr[k].map(|x| d - (2.0 * x * ma as f64 / 1000.0).round() as i64);
            if !left_out {
                if ma.abs() < REST_MA && prev_i.is_some_and(|p| p.abs() < REST_MA) {
                    out.rest_rows += 1;
                    if row.balancing.is_none() {
                        out.rest_no_status += 1;
                    }
                    for (k, &d) in dev.iter().enumerate() {
                        if bit(row.balancing, k) {
                            out.bms_left_out += 1;
                        } else {
                            rest[k].add(d);
                        }
                    }
                }
                let ends = [(hi >= TOP_MV && ma > balancer::CHARGE_MA, &mut top, &mut top_c, &mut out.top_rows), (lo <= BOTTOM_MV && ma < -balancer::CHARGE_MA, &mut bottom, &mut bottom_c, &mut out.bottom_rows)];
                for (on, h, hc, count) in ends {
                    if on {
                        *count += 1;
                        for (k, &d) in dev.iter().enumerate() {
                            h[k].add(d);
                            if let Some(c) = corrected(d, k) {
                                hc[k].add(c);
                            }
                        }
                    }
                }
                if ma.abs() >= LOAD_MA && hi < TOP_MV && lo > BOTTOM_MV {
                    out.load_rows += 1;
                    for (k, &d) in dev.iter().enumerate() {
                        load[k].add(corrected(d, k).unwrap_or(d));
                    }
                }
            }
            if let Some((first, spoiled)) = charge.as_mut().filter(|_| ma > balancer::CHARGE_MA) {
                *spoiled |= left_out;
                for (f, &c) in first.iter_mut().zip(cells) {
                    if f.is_none() && c >= ORDER_MV {
                        *f = Some(row.t);
                    }
                }
            }
            // jumps: a cell's deviation moves by JUMP_MV with a steady current, and no other cell does except a direct
            // neighbour (a sense wire is shared by two cells); both rows free of external activity and of the cell's
            // BMS bit, so balancing switching on or off is no jump
            if let (Some((pd, pb, pfree)), Some(pi)) = (&prev, prev_i) {
                if (ma - pi).abs() < JUMP_STEADY_MA && *pfree && !left_out {
                    let big: Vec<usize> = dev.iter().zip(pd).enumerate().filter(|(_, (a, b))| (*a - *b).abs() >= 2 * JUMP_MV).map(|(k, _)| k).collect();
                    if big.len() == 1 || (big.len() == 2 && big[1] == big[0] + 1) {
                        big.iter().filter(|&&k| !bit(*pb, k) && !bit(row.balancing, k)).for_each(|&k| jumps[k] += 1);
                    }
                }
            }
        }
        before = Some((row.clone(), Some(dev), !left_out));
    })?;
    finish_charge(charge.take(), &mut order_sum, &mut order_n);
    out.charges = order_n;
    out.heat = heat.into_iter().map(|h| h.into_iter().map(|(s, c)| (c > 0).then(|| (s / c as f64) as f32)).collect()).collect();
    out.cells = (0..n)
        .map(|k| {
            let r_rel = r[k].zip(out.r_median).filter(|(_, m)| *m > 0.0).map(|(a, m)| a / m);
            let input = Input {
                rest: dev_median(&rest[k]),
                rest_rows: rest[k].n,
                load: dev_median(&load[k]),
                top: dev_median(&top[k]),
                bottom: dev_median(&bottom[k]),
                top_corr: r_rel.and(dev_median(&top_c[k])),
                bottom_corr: r_rel.and(dev_median(&bottom_c[k])),
                r_rel,
                r_above: dr[k],
                r_bound,
                steps: cell_steps[k],
                jumps: jumps[k],
            };
            let (class, reason) = classify(&input);
            CellResult {
                cell: k as u8 + 1,
                rest: input.rest,
                rest_rows: input.rest_rows,
                load: input.load,
                top: input.top,
                bottom: input.bottom,
                top_corr: input.top_corr,
                bottom_corr: input.bottom_corr,
                r_mohm: r[k],
                steps: cell_steps[k],
                r_pct: r_rel.map(|x| x * 100.0),
                order: (order_n > 0).then(|| order_sum[k] as f64 / order_n as f64),
                jumps: jumps[k],
                class,
                reason,
            }
        })
        .collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(rest: f64, load: f64, top: Option<f64>, bottom: Option<f64>, r_rel: Option<f64>) -> Input {
        // the correction exists only with a known resistance; here it changes nothing (R at the median)
        // R at the pack median 0.5 mΩ, steps of 40 A (2 mV / 40 A = 0.05 mΩ)
        let r_above = r_rel.map(|r| (r - 1.0) * 0.5);
        Input { rest: Some(rest), rest_rows: 200, load: Some(load), top, bottom, top_corr: r_rel.and(top), bottom_corr: r_rel.and(bottom), r_rel, r_above, r_bound: Some(0.05), steps: if r_rel.is_some() { 40 } else { 4 }, jumps: 0 }
    }

    #[test]
    fn classification_vectors() {
        use Class::*;
        let cases: Vec<(&str, Input, Class, Reason)> = vec![
            ("matching", input(1.0, 1.5, Some(4.0), Some(-3.0), Some(1.05)), Ok, Reason::Within),
            ("matching, resistance unknown", input(1.0, 1.5, Some(4.0), Some(-3.0), None), Ok, Reason::Partial { top: true, bottom: true, r: false, steps: 4 }),
            ("matching, no bottom", input(1.0, 1.5, Some(4.0), None, Some(1.0)), Ok, Reason::Partial { top: true, bottom: false, r: true, steps: 40 }),
            ("rest only: never plain matching", input(1.0, 1.5, None, None, None), RestOk, Reason::RestOnly { rest: 1.0 }),
            ("SOC offset: high at both ends", input(3.0, 3.0, Some(30.0), Some(20.0), Some(1.0)), SocOffset, Reason::SameSign { top: 30.0, bottom: 20.0 }),
            ("offset at rest only: SOC or measurement", input(-8.0, -9.0, None, None, Some(1.0)), RestOffset, Reason::Rest { rest: -8.0 }),
            ("rest and resistance fine, no ends: capacity not checked", input(0.0, 0.0, None, None, Some(1.0)), RestOk, Reason::Partial { top: false, bottom: false, r: true, steps: 40 }),
            ("smaller capacity, corrected", input(0.0, 0.0, Some(40.0), Some(-30.0), Some(1.1)), Capacity, Reason::Capacity { top: 40.0, bottom: -30.0 }),
            ("ambiguous without enough steps", input(0.0, 0.0, Some(30.0), Some(-30.0), None), Ambiguous, Reason::Ambiguous { top: 30.0, bottom: -30.0, steps: 4 }),
            ("higher resistance wins over the pattern", input(0.0, 0.0, Some(30.0), Some(-30.0), Some(1.5)), Resistance, Reason::Resistance { pct: 150.0, steps: 40 }),
            ("measurement offset", input(40.0, 41.0, Some(38.0), None, Some(1.0)), Measurement, Reason::Offset { rest: 40.0, load: 41.0 }),
            ("jumps", Input { jumps: 5, ..input(0.0, 0.0, None, None, Some(1.0)) }, Measurement, Reason::Jumps { count: 5 }),
            ("no rest", Input { rest: None, rest_rows: 12, ..input(0.0, 0.0, None, None, None) }, NoStatement, Reason::NoRest { rows: 12 }),
            ("top only", input(0.0, 0.0, Some(25.0), None, Some(1.0)), NoStatement, Reason::TopOnly { top: 25.0 }),
            ("bottom only", input(0.0, 0.0, None, Some(-25.0), Some(1.0)), NoStatement, Reason::BottomOnly { bottom: -25.0 }),
            ("low at the top, high at the bottom", input(0.0, 0.0, Some(-25.0), Some(25.0), Some(1.0)), NoStatement, Reason::Pattern { top: -25.0, bottom: 25.0 }),
            ("resistance a little high", input(0.0, 0.0, None, None, Some(1.25)), NoStatement, Reason::RSlight { pct: 125.0 }),
        ];
        for (name, c, class, reason) in cases {
            let (k, r) = classify(&c);
            assert_eq!(k, class, "{name}");
            match (r, reason) {
                (Reason::Resistance { pct: a, .. }, Reason::Resistance { pct: b, .. }) | (Reason::RSlight { pct: a }, Reason::RSlight { pct: b }) => assert!((a - b).abs() < 1e-9, "{name}"),
                (r, reason) => assert_eq!(r, reason, "{name}"),
            }
        }
        // the resistance explains the raw ends: corrected inside ± 15 mV, no capacity statement
        let c = Input { top_corr: Some(5.0), bottom_corr: Some(-5.0), ..input(0.0, 0.0, Some(25.0), Some(-25.0), Some(1.15)) };
        assert_eq!(classify(&c), (Ok, Reason::Within));
        // without the correction the same raw ends stay ambiguous
        assert_eq!(classify(&Input { top_corr: None, bottom_corr: None, r_rel: None, ..c }).0, Ambiguous);
        // steps of 20 A: 2 mV are 0.1 mΩ; a median of 0.2 mΩ and the cell 0.28 mΩ (140 %) is within the resolution
        let at20 = |r: f64| Input { r_rel: Some(r / 0.2), r_above: Some(r - 0.2), r_bound: Some(0.1), ..input(0.0, 0.0, None, None, None) };
        assert_eq!(classify(&at20(0.28)).0, NoStatement);
        assert!(matches!(classify(&at20(0.28)).1, Reason::RSlight { .. }));
        assert_eq!(classify(&at20(0.32)).0, Resistance);
    }

    #[test]
    fn histogram_median_in_half_millivolts() {
        let mut h = dev_hist();
        for v in [-3, 1, 1, 7] {
            for _ in 0..10 {
                h.add(v);
            }
        }
        assert_eq!(dev_median(&h), Some(0.5));
        h.add(1);
        assert_eq!(dev_median(&h), Some(0.5));
        let mut few = dev_hist();
        few.add(4);
        assert_eq!(dev_median(&few), None, "fewer than MIN_ROWS rows");
        let mut r = Hist::new(R_SPAN);
        (0..9).for_each(|_| r.add(100));
        assert_eq!(r.median(MIN_STEPS), None, "fewer than MIN_STEPS steps");
        r.add(9_999);
        assert_eq!(r.median(MIN_STEPS).map(|v| v * R_BIN_MOHM), Some(0.5), "beyond the span counts at the edge");
        assert_eq!(median2(&[3300, 3310, 3320, 3400]), 6630);
        assert_eq!(places(&[Some(5), None, Some(3), Some(5)]), Some(vec![2, 4, 1, 2]));
    }

    const CELLS: usize = 12;
    const T0: i64 = 1_000_001;
    const STEP: i64 = 10_000;
    const GAP: i64 = 300_000;
    const R: f64 = 0.5;

    /// Synthetic recording of one 12-cell pack, rows 10 s apart, blocks separated by 5 minutes:
    /// cell 2 twice the resistance, 3 an SOC offset, 4 a constant measurement offset, 5 a smaller capacity,
    /// 6 low while the BMS balances it, 7 high while an external balancer works, 8 jumps.
    struct Rec {
        c: Connection,
        t: i64,
        pack: u8,
    }

    impl Rec {
        fn row(&mut self, session: i64, ma: i64, cells: Option<Vec<i64>>, balancing: Option<i64>) {
            let (hi, lo) = cells.as_ref().map_or((None, None), |c| (c.iter().max().copied(), c.iter().min().copied()));
            let spread = hi.zip(lo).map(|(a, b)| a - b);
            self.c
                .execute(
                    "INSERT INTO pack(site, pack, t, session, n, current, cell_max, cell_min, cells, spread_lo, spread_hi, i_lo, i_hi, balancing, bal_rows, bal_n)
                     VALUES(1, ?11, ?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?7, ?3, ?3, ?8, ?9, ?10)",
                    params![self.t, session, ma, hi, lo, cells.map(|c| c.iter().flat_map(|&v| (v as u16).to_le_bytes()).collect::<Vec<u8>>()), spread, balancing,
                        balancing.map(|b| i64::from(b != 0)), balancing.map(|_| 1), self.pack],
                )
                .unwrap();
            self.t += STEP;
        }

        /// cells at `base` plus R × I plus each cell's offset in this region
        fn cells(base: i64, ma: i64, extra: &[(usize, i64)]) -> Vec<i64> {
            (1..=CELLS)
                .map(|k| {
                    let r = if k == 2 { 2.0 * R } else { R };
                    base + (r * ma as f64 / 1000.0).round() as i64 + extra.iter().filter(|e| e.0 == k).map(|e| e.1).sum::<i64>()
                })
                .collect()
        }

        fn gap(&mut self) {
            self.t += GAP;
        }
    }

    #[test]
    fn analysis_of_a_synthetic_pack() {
        let dir = std::env::temp_dir().join(format!("openbms-cells-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("history.sqlite");
        crate::history::History::default().open(&path).unwrap();
        let c = Connection::open(&path).unwrap();
        c.execute_batch("INSERT INTO sites(id, site) VALUES(1, 'a'); INSERT INTO sessions(id, site, bus, source, first, last) VALUES(1, 'a', 'can', '', 0, 1), (2, 'a', 'can', '', 0, 1);").unwrap();
        let mut rec = Rec { c, t: T0, pack: 4 };
        let flat = [(3, 3), (4, 40)];
        // rest: 60 rows, cell 8 jumping 30 mV in ten of them
        for i in 0..60 {
            let jump = if (10..20).contains(&i) && i % 2 == 1 { 30 } else { 0 };
            rec.row(1, 0, Some(Rec::cells(3300, 0, &[(3, 3), (4, 40), (8, jump)])), Some(0));
        }
        // rest while the BMS balances cell 6 (reads 20 mV low): its values left out
        for _ in 0..80 {
            rec.row(1, 0, Some(Rec::cells(3300, 0, &[(3, 3), (4, 40), (6, -20)])), Some(1 << 5));
        }
        // rest while the external balancer works (cell 7 30 mV high): left out
        let bal_from = rec.t;
        for _ in 0..80 {
            rec.row(1, 0, Some(Rec::cells(3300, 0, &[(3, 3), (4, 40), (7, 30)])), Some(0));
        }
        let bal_to = rec.t;
        rec.gap();
        // load steps 0 / 50 A: 39 steps
        for i in 0..40 {
            let ma = if i % 2 == 0 { 0 } else { 50_000 };
            rec.row(1, ma, Some(Rec::cells(3300, ma, &flat)), Some(0));
        }
        rec.gap();
        // charging towards the top: 15 rows in the flat region (arms the charge), then 40 at the top
        for _ in 0..15 {
            rec.row(1, 30_000, Some(Rec::cells(3300, 30_000, &flat)), Some(0));
        }
        for _ in 0..40 {
            rec.row(1, 30_000, Some(Rec::cells(3420, 30_000, &[(3, 30), (4, 40), (5, 40)])), Some(0));
        }
        rec.gap();
        // discharging at the bottom
        for _ in 0..40 {
            rec.row(1, -30_000, Some(Rec::cells(3100, -30_000, &[(3, 20), (4, 40), (5, -30)])), Some(0));
        }
        // a new session 10 s later at +50 A: no load step across the sessions
        rec.row(2, 50_000, Some(Rec::cells(3300, 50_000, &flat)), Some(0));
        rec.gap();
        // incomplete: another cell count and none
        for _ in 0..3 {
            rec.row(2, 0, Some(vec![3300; CELLS - 1]), Some(0));
        }
        rec.row(2, 0, None, Some(0));
        // a compacted minute row
        rec.c.execute("INSERT INTO pack(site, pack, t, session, n, current) VALUES(1, 4, ?1, 2, 6, 0)", [(rec.t / MINUTE_MS + 1) * MINUTE_MS]).unwrap();
        let end = rec.t + 2 * MINUTE_MS;

        let devices = Devices::parse(&json!({
            "defs": [{ "id": "b", "name": "B", "kind": "active-balancer", "profile": null, "revs": [{ "from": 0, "trigger_mv": 25, "when": "always" }] }],
            "assign": { "4": [{ "from": bal_from, "device": "b" }, { "from": bal_to, "device": null }] },
        }));
        let db = crate::history::reader(&path).unwrap();
        let a = analyse(&db, "a", 4, T0, end, &devices, 10).unwrap();
        assert_eq!(a.cell_count, CELLS);
        assert_eq!((a.rows, a.incomplete, a.minute_rows), (356, 4, 1));
        assert_eq!((a.balancer, a.balancer_active, a.balancer_unknown), (true, 80, 0));
        assert_eq!((a.rest_rows, a.top_rows, a.bottom_rows, a.load_rows), (139, 40, 40, 36));
        assert_eq!((a.bms_left_out, a.rest_no_status), (80, 0));
        assert_eq!(a.steps, 39, "the step across the sessions is not one");
        assert_eq!(a.r_median, Some(R));
        assert_eq!(a.charges, 1);
        let cell = |k: usize| &a.cells[k - 1];
        assert_eq!((cell(2).class, cell(2).r_mohm, cell(2).r_pct), (Class::Resistance, Some(2.0 * R), Some(200.0)));
        // cell 2's raw ends come from its resistance; corrected they are at the median
        assert_eq!((cell(2).top, cell(2).top_corr, cell(2).bottom, cell(2).bottom_corr), (Some(15.0), Some(0.0), Some(-15.0), Some(0.0)));
        assert_eq!((cell(3).class, cell(3).reason), (Class::SocOffset, Reason::SameSign { top: 30.0, bottom: 20.0 }));
        assert_eq!((cell(4).class, cell(4).reason), (Class::Measurement, Reason::Offset { rest: 40.0, load: 40.0 }));
        assert_eq!((cell(5).class, cell(5).reason), (Class::Capacity, Reason::Capacity { top: 40.0, bottom: -30.0 }));
        assert_eq!((cell(8).class, cell(8).jumps), (Class::Measurement, 10));
        // the BMS-balanced and the external-balancer values are left out: both cells match
        assert_eq!((cell(6).class, cell(6).rest, cell(6).rest_rows), (Class::Ok, Some(0.0), 59));
        assert_eq!((cell(7).class, cell(7).rest, cell(7).rest_rows), (Class::Ok, Some(0.0), 139));
        for k in [1, 9, 10, 11, 12] {
            assert_eq!((cell(k).class, cell(k).reason), (Class::Ok, Reason::Within), "cell {k}");
        }
        // order at 3.45 V: cells 2–5 pass together, the others never
        assert_eq!((cell(4).order, cell(1).order), (Some(1.0), Some(5.0)));
        assert_eq!((a.heat.len(), a.heat[0].len()), (CELLS, 10));
        assert_eq!(a.heat[3][0], Some(40.0));

        // without the balancer assigned its rows count: cell 7 reads high at rest
        let a = analyse(&db, "a", 4, T0, end, &Devices::default(), 0).unwrap();
        assert_eq!((a.balancer, a.balancer_active, a.rest_rows), (false, 0, 219));
        assert!(a.heat.is_empty());
        // too few rows: a reason, never a class
        let short = analyse(&db, "a", 4, T0, T0 + 20 * STEP, &Devices::default(), 0).unwrap();
        assert_eq!((short.cells[0].class, short.cells[0].reason), (Class::NoStatement, Reason::NoRest { rows: 20 }));
        assert!(analyse(&db, "nobody", 4, T0, end, &Devices::default(), 0).unwrap().cells.is_empty());
    }

    fn fresh(name: &str) -> (std::path::PathBuf, Connection) {
        let dir = std::env::temp_dir().join(format!("openbms-cells-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("history.sqlite");
        crate::history::History::default().open(&path).unwrap();
        let c = Connection::open(&path).unwrap();
        c.execute_batch("INSERT INTO sites(id, site) VALUES(1, 'a'); INSERT INTO sessions(id, site, bus, source, first, last) VALUES(1, 'a', 'can', '', 0, 1);").unwrap();
        (path, c)
    }

    /// Load steps 0 / 50 A where cell 3 reads 30 mV higher on the 50 A rows (moved by a balancer, not by its
    /// resistance), then rest rows where cell 5 switches by 25 mV every row. `bits`: the BMS reports cells 3 and 5
    /// balancing on exactly those rows.
    fn balancing_pack(rec: &mut Rec, bits: bool) -> (i64, i64) {
        let from = rec.t;
        for i in 0..40 {
            let (ma, moved) = if i % 2 == 1 { (50_000, true) } else { (0, false) };
            rec.row(1, ma, Some(Rec::cells(3300, ma, &[(3, if moved { 30 } else { 0 })])), Some(if bits && moved { 1 << 2 } else { 0 }));
        }
        rec.gap();
        for i in 0..40 {
            let on = i % 2 == 1;
            rec.row(1, 0, Some(Rec::cells(3300, 0, &[(5, if on { 25 } else { 0 })])), Some(if bits && on { 1 << 4 } else { 0 }));
        }
        (from, rec.t)
    }

    #[test]
    fn balancing_is_neither_resistance_nor_a_jump() {
        let (path, c) = fresh("balancing");
        let mut rec = Rec { c, t: T0, pack: 1 };
        let (from, to) = balancing_pack(&mut rec, false);
        rec.pack = 2;
        rec.t = T0;
        balancing_pack(&mut rec, true);
        let db = crate::history::reader(&path).unwrap();
        // nothing known about balancing: cell 3's moves read as resistance, cell 5's switching as jumps
        let plain = analyse(&db, "a", 1, from, to, &Devices::default(), 0).unwrap();
        assert_eq!(plain.steps, 39);
        assert_eq!((plain.cells[2].r_pct.map(f64::round), plain.cells[2].class), (Some(220.0), Class::Resistance));
        assert_eq!((plain.cells[4].jumps, plain.cells[4].class), (39, Class::Measurement));
        // an external balancer that works whenever the spread reaches 20 mV: no step and no jump has both ends free
        let devices = Devices::parse(&json!({
            "defs": [{ "id": "b", "name": "B", "kind": "active-balancer", "revs": [{ "from": 0, "trigger_mv": 20, "when": "always" }] }],
            "assign": { "1": [{ "from": 0, "device": "b" }] },
        }));
        let ext = analyse(&db, "a", 1, from, to, &devices, 0).unwrap();
        assert_eq!((ext.steps, ext.cells[2].r_mohm, ext.cells[4].jumps), (0, None, 0));
        assert_ne!(ext.cells[2].class, Class::Resistance);
        // the BMS bits: only the balanced cell's steps and jumps are left out
        let bms = analyse(&db, "a", 2, from, to, &Devices::default(), 0).unwrap();
        assert_eq!((bms.steps, bms.cells[2].steps, bms.cells[0].steps), (39, 0, 39));
        assert_eq!((bms.cells[2].r_mohm, bms.cells[0].r_mohm), (None, Some(R)));
        // its rest values with the bit set are left out too: too few left for a statement
        assert_eq!((bms.cells[4].jumps, bms.cells[4].class), (0, Class::NoStatement));
    }

    #[test]
    fn answers_on_the_full_minute_are_raw() {
        let (path, c) = fresh("minute");
        let mut rec = Rec { c, t: 100 * MINUTE_MS, pack: 7 };
        for _ in 0..40 {
            rec.row(1, 0, Some(Rec::cells(3300, 0, &[])), Some(0));
            rec.t += MINUTE_MS - STEP;
        }
        let a = analyse(&crate::history::reader(&path).unwrap(), "a", 7, 0, rec.t, &Devices::default(), 0).unwrap();
        assert_eq!((a.cell_count, a.rows, a.minute_rows, a.rest_rows), (CELLS, 40, 0, 39));
        assert_eq!(a.cells[0].class, Class::RestOk);
    }
}

