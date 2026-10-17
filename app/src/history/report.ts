/** Summaries, flags and CSV of the health report; the per-day aggregates come from the database (health.rs). */
import type { CellAnalysis, HealthReport, PackDay } from "../api";
import { maskCells, reasonCsv } from "../cellAnalysis";

const DAY = 86_400_000;

export const PERIODS = [7, 30, 90, "all"] as const;
export type Period = (typeof PERIODS)[number];

/** A pack counts cycles this much faster than the bank median before it is flagged. */
export const CYCLE_RATE_ABOVE = 0.25;
/** The counter counts whole cycles: a flag needs the bank median to have gained this many in the period and the pack
 *  this many whole cycles more than the median, so one counter step never decides it. The rate itself needs 7 days
 *  of counts (health.rs RATE_MIN_MS). */
export const CYCLE_MEDIAN_GAIN_MIN = 8;
export const CYCLE_ABOVE_MIN = 2;
/** Rest spread (P95) flagged above this factor of the bank median and at least SPREAD_ABOVE_MV above it. */
export const SPREAD_FACTOR = 1.5;
export const SPREAD_ABOVE_MV = 10;
/** Rest spread rising by this much (mV) over the period. */
export const SPREAD_RISE_MV = 10;
/** Warmest cell of the period this much (K) above the bank median. */
export const WARM_ABOVE_C = 5;
/** A day's rest P95 counts for the bank comparison and the trend from this many rest rows that day; the trend needs
 *  TREND_DAYS_MIN such days. */
export const REST_ROWS_MIN = 30;
export const TREND_DAYS_MIN = 3;

/** [from, to] of a period up to now; "all" from the start of the installation's recording. */
export function periodRange(p: Period, now: number, siteFrom: number | null): [number, number] {
  return [p === "all" ? siteFrom ?? now - DAY : now - p * DAY, now];
}

export type Trend = { dir: "up" | "down" | "flat"; change: number };

/** Change of a daily series over its span (least-squares line over the days); below `flat` it counts as flat.
 *  undefined with fewer than 3 values. */
export function trend(points: { day: number; v: number | null }[], flat: number): Trend | undefined {
  const p = points.filter((x): x is { day: number; v: number } => x.v !== null);
  if (p.length < 3) return undefined;
  const xs = p.map((x) => (x.day - p[0].day) / DAY), n = p.length;
  const mx = xs.reduce((a, b) => a + b, 0) / n, my = p.reduce((a, x) => a + x.v, 0) / n;
  let sxy = 0, sxx = 0;
  p.forEach((x, i) => { sxy += (xs[i] - mx) * (x.v - my); sxx += (xs[i] - mx) ** 2; });
  const change = sxx ? (sxy / sxx) * (xs[n - 1] - xs[0]) : 0;
  return { dir: Math.abs(change) < flat ? "flat" : change > 0 ? "up" : "down", change };
}

export function median(xs: number[]): number | undefined {
  if (!xs.length) return undefined;
  const s = [...xs].sort((a, b) => a - b), m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

/** Median of the values of the others (with two packs: the partner's), so a candidate never dilutes its own reference. */
export function othersMedian<T>(all: T[], self: T, f: (x: T) => number): number | undefined {
  return median(all.filter((y) => y !== self).map(f));
}

/** The days a pack's rest trend may use: of the kind (raw rows or minute maxima, health.rs `spread_raw`) of its last
 *  day with rest values and only after the last day of the other kind, never across the change; each with at least
 *  REST_ROWS_MIN rest rows. */
export function trendDays(days: PackDay[]): PackDay[] {
  const spread = days.filter((d) => d.spread_p95 !== null);
  const rest = spread.filter((d) => d.rest_p95 !== null);
  if (!rest.length) return [];
  const kind = rest[rest.length - 1].spread_raw;
  let i = spread.length;
  while (i > 0 && spread[i - 1].spread_raw === kind) i--;
  const out = spread.slice(i).filter((d) => d.rest_p95 !== null && d.rest_rows >= REST_ROWS_MIN);
  return out.length >= TREND_DAYS_MIN ? out : [];
}

/** The last non-null value of a daily series. */
export function lastOf<T>(days: T[], f: (d: T) => number | null): number | null {
  for (let i = days.length - 1; i >= 0; i--) { const v = f(days[i]); if (v !== null) return v; }
  return null;
}

/** What is unusual about a pack, said in words by the view (hist.rep.flag.<key>). */
export type PackFlag =
  | { key: "cycles"; pct: number }
  | { key: "spread"; mv: number; median: number }
  | { key: "spreadRising"; mv: number }
  | { key: "warm"; c: number; median: number }
  | { key: "counterCleared"; count: number };

export interface PackRow {
  pack: number; restP50: number | null; restP95: number | null; loadP95: number | null; restTrend?: Trend;
  cycles: number | null; rate: number | null; tempMax: number | null; flags: PackFlag[];
  /** counts recorded, but too short a span for a rate */
  rateShort: boolean;
}

/** One row per pack: last values, the rest spread's trend and the flags against the other packs (each compared with
 *  the median of the others). The rest spread is compared only on the latest day with rest values in the bank, from
 *  packs with REST_ROWS_MIN rest rows that day and of one kind (raw rows when any has them); the warmest cell only over
 *  the days every pack with temperatures has some. */
export function packRows(r: HealthReport): PackRow[] {
  const restDay = Math.max(-Infinity, ...r.packs.flatMap((p) => p.days.filter((d) => d.rest_p95 !== null).map((d) => d.day)));
  const withTemp = r.packs.filter((p) => p.days.some((d) => d.temp_max !== null));
  const tempDays = new Set(withTemp[0]?.days.filter((d) => d.temp_max !== null).map((d) => d.day));
  for (const p of withTemp) { const own = new Set(p.days.filter((d) => d.temp_max !== null).map((d) => d.day)); for (const d of tempDays) if (!own.has(d)) tempDays.delete(d); }
  const rows = r.packs.map((p) => {
    const temps = p.days.map((d) => d.temp_max).filter((v): v is number => v !== null);
    const common = p.days.filter((d) => d.temp_max !== null && tempDays.has(d.day)).map((d) => d.temp_max!);
    const cmp = p.days.find((d) => d.day === restDay && d.rest_p95 !== null && d.rest_rows >= REST_ROWS_MIN);
    return {
      pack: p.pack, restP50: lastOf(p.days, (d) => d.rest_p50), restP95: lastOf(p.days, (d) => d.rest_p95),
      loadP95: lastOf(p.days, (d) => d.spread_p95), restTrend: trend(trendDays(p.days).map((d) => ({ day: d.day, v: d.rest_p95 })), 3),
      cycles: lastOf(p.days, (d) => d.cycles), rate: p.cycles_per_30d, tempMax: temps.length ? Math.max(...temps) : null,
      flags: [] as PackFlag[], resets: p.cycle_resets, gained: p.cycles_gained, span: p.cycles_span_ms,
      rateShort: p.cycles_per_30d === null && p.cycles_gained !== null,
      restCmp: cmp ? { v: cmp.rest_p95!, raw: cmp.spread_raw } : null, tempCmp: common.length ? Math.max(...common) : null,
    };
  });
  type Row = (typeof rows)[number];
  const rawKind = rows.some((x) => x.restCmp?.raw);
  const restRows = rows.filter((x) => x.restCmp !== null && x.restCmp.raw === rawKind);
  const tempRows = rows.filter((x) => x.tempCmp !== null);
  // cycles are compared only over the same window: every rated pack spans about the same time, no counter cleared
  const rated = rows.filter((x) => x.rate !== null && x.gained !== null && x.span !== null);
  const spans = rated.map((x) => x.span!);
  const cyclesComparable = rated.length >= 2 && rows.every((x) => x.resets === 0) && Math.max(...spans) - Math.min(...spans) <= DAY;
  for (const x of rows) {
    const rateMed = othersMedian(rated, x, (y: Row) => y.rate!), gainMed = othersMedian(rated, x, (y: Row) => y.gained!);
    if (cyclesComparable && rated.includes(x) && rateMed && gainMed !== undefined && gainMed >= CYCLE_MEDIAN_GAIN_MIN
      && x.rate! > rateMed * (1 + CYCLE_RATE_ABOVE) && x.gained! - gainMed >= CYCLE_ABOVE_MIN)
      x.flags.push({ key: "cycles", pct: Math.round((x.rate! / rateMed - 1) * 100) });
    const restMed = othersMedian(restRows, x, (y: Row) => y.restCmp!.v);
    if (restRows.includes(x) && restMed !== undefined && x.restCmp!.v > restMed * SPREAD_FACTOR && x.restCmp!.v >= restMed + SPREAD_ABOVE_MV)
      x.flags.push({ key: "spread", mv: x.restCmp!.v, median: restMed });
    if (x.restTrend && x.restTrend.change >= SPREAD_RISE_MV) x.flags.push({ key: "spreadRising", mv: Math.round(x.restTrend.change) });
    const tempMed = othersMedian(tempRows, x, (y: Row) => y.tempCmp!);
    if (tempRows.includes(x) && tempMed !== undefined && x.tempCmp! >= tempMed + WARM_ABOVE_C)
      x.flags.push({ key: "warm", c: x.tempCmp!, median: tempMed });
    if (x.resets > 0) x.flags.push({ key: "counterCleared", count: x.resets });
  }
  return rows.map(({ resets: _r, gained: _g, span: _s, restCmp: _c, tempCmp: _t, ...x }) => x);
}

export interface PairRow {
  plus: number; minus: number; share: number | null; samples: number; markedDays: number; trend?: Trend;
  /** the pack carrying more on the last marked day and its share (%) */
  flag?: { pack: number; share: number; days: number };
}

/** The share a pair day is judged by: without the samples equalizing explains, else all. */
const dayShare = (d: { judged_share: number | null; share: number | null }) => d.judged_share ?? d.share;

/** One row per pair: last share, its trend, the days marked and who carries more. */
export function pairRows(r: HealthReport): PairRow[] {
  return r.pairs.map((p) => {
    const marked = p.days.filter((d) => d.marked);
    const last = marked.length ? dayShare(marked[marked.length - 1]) : null;
    return {
      plus: p.plus, minus: p.minus, share: lastOf(p.days, dayShare), samples: p.days.reduce((a, d) => a + d.samples, 0),
      markedDays: marked.length, trend: trend(p.days.map((d) => ({ day: d.day, v: dayShare(d) })), 2),
      flag: last === null ? undefined : last >= 50 ? { pack: p.plus, share: last, days: marked.length } : { pack: p.minus, share: 100 - last, days: marked.length },
    };
  });
}

/** Local date as YYYY-MM-DD. */
export function localDate(ms: number) {
  const d = new Date(ms), z = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${z(d.getMonth() + 1)}-${z(d.getDate())}`;
}

/** The report as one CSV: a row per pack and day (with the BMS balancing bits), per pack the cycle rate, a row per pair
 *  and day, and with the cell analyses per pack one row for the pack's figures and one per cell. */
export function reportCsv(r: HealthReport, cells: Record<number, CellAnalysis> = {}): string {
  const cols = ["kind", "pack", "partner", "date", "rows", "spread_p50_mv", "spread_p95_mv", "rest_rows", "rest_p50_mv", "rest_p95_mv", "cycles",
    "temp_max_c", "cycles_per_30d", "cycle_resets", "cycles_gained", "cycles_span_days", "samples", "share_pct", "equalizing", "judged", "judged_minutes", "judged_share_pct", "marked",
    "bal_status_rows", "bal_rows", "bal_pct", "bal_cells",
    "cell", "class", "reason", "rest_mv", "load_mv", "top_mv", "top_corr_mv", "bottom_mv", "bottom_corr_mv", "r_mohm", "r_pct", "order", "jumps",
    "cell_count", "incomplete", "balancer_left_out", "bms_left_out", "top_rows", "bottom_rows", "load_rows", "steps", "charges"];
  const v = (x: number | string | boolean | null | undefined) => (x === null || x === undefined ? "" : typeof x === "number" ? String(Math.round(x * 100) / 100) : String(x));
  const line = (o: Record<string, number | string | boolean | null | undefined>) => cols.map((c) => v(o[c])).join(",");
  const out = [cols.join(",")];
  for (const p of r.packs) {
    for (const d of p.days) out.push(line({ kind: "pack", pack: p.pack, date: localDate(d.day), rows: d.rows, spread_p50_mv: d.spread_p50, spread_p95_mv: d.spread_p95,
      rest_rows: d.rest_rows, rest_p50_mv: d.rest_p50, rest_p95_mv: d.rest_p95, cycles: d.cycles, temp_max_c: d.temp_max,
      bal_status_rows: d.bal_n, bal_rows: d.bal_rows, bal_pct: d.bal_n ? ((d.bal_rows ?? 0) / d.bal_n) * 100 : null, bal_cells: maskCells(d.bal_mask) }));
    out.push(line({ kind: "pack_period", pack: p.pack, cycles_per_30d: p.cycles_per_30d, cycle_resets: p.cycle_resets,
      cycles_gained: p.cycles_gained, cycles_span_days: p.cycles_span_ms === null ? null : p.cycles_span_ms / DAY }));
    const a = cells[p.pack];
    if (!a) continue;
    out.push(line({ kind: "cells_period", pack: p.pack, rows: a.rows, rest_rows: a.rest_rows, cell_count: a.cell_count, incomplete: a.incomplete,
      balancer_left_out: a.balancer_active + a.balancer_unknown, bms_left_out: a.bms_left_out, top_rows: a.top_rows, bottom_rows: a.bottom_rows,
      load_rows: a.load_rows, steps: a.steps, charges: a.charges, r_mohm: a.r_median }));
    for (const c of a.cells) out.push(line({ kind: "cell", pack: p.pack, cell: c.cell, class: c.class, reason: reasonCsv(c.reason), rest_rows: c.rest_rows,
      rest_mv: c.rest, load_mv: c.load, top_mv: c.top, top_corr_mv: c.top_corr, bottom_mv: c.bottom, bottom_corr_mv: c.bottom_corr,
      r_mohm: c.r_mohm, r_pct: c.r_pct, steps: c.steps, order: c.order, jumps: c.jumps }));
  }
  for (const p of r.pairs) for (const d of p.days) out.push(line({ kind: "pair", pack: p.plus, partner: p.minus, date: localDate(d.day), samples: d.samples,
    share_pct: d.share, equalizing: d.equalizing, judged: d.judged, judged_minutes: d.judged_minutes, judged_share_pct: d.judged_share, marked: d.marked ? 1 : 0 }));
  return out.join("\n") + "\n";
}
