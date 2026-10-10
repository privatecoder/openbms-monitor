/**
 * Recordings (JSON Lines, see app/src-tauri/src/recorder.rs) read into columns per pack, and reduced
 * to a common time grid for the charts.
 */

/** Columns of one pack, one entry per recorded answer with telemetry. */
export interface PackSeries {
  t: number[];
  current: number[];
  soc: number[];
  voltage: number[];
  cellMax: number[];
  cellMin: number[];
  tempMax: number[];
  tempMin: number[];
  /** per cell: cells[i][n] = voltage of cell i in answer n */
  cells: number[][];
}

/** Columns of the master's system values. */
export interface SystemSeries {
  t: number[];
  current: number[];
  soc: number[];
  voltage: number[];
  ccl: number[];
  dcl: number[];
  cvl: number[];
  cellMax: number[];
  cellMin: number[];
}

export interface Recording {
  /** from the first line, if the file has one */
  start?: { packs: number[]; system: boolean; interval_s: number; bus: string };
  packs: Map<number, PackSeries>;
  system: SystemSeries;
  from: number;
  to: number;
  lines: number;
  /** lines that could not be read (e.g. a cut-off last line) */
  skipped: number;
  /** answers with an error instead of values */
  errors: number;
}

const emptyPack = (): PackSeries => ({ t: [], current: [], soc: [], voltage: [], cellMax: [], cellMin: [], tempMax: [], tempMin: [], cells: [] });
const emptySystem = (): SystemSeries => ({ t: [], current: [], soc: [], voltage: [], ccl: [], dcl: [], cvl: [], cellMax: [], cellMin: [] });

/* eslint-disable @typescript-eslint/no-explicit-any */
/** Reads a recording piece by piece, so files larger than a string can hold still work. */
export class RecordingParser {
  private rest = "";
  private r: Recording = { packs: new Map(), system: emptySystem(), from: Infinity, to: -Infinity, lines: 0, skipped: 0, errors: 0 };

  push(text: string) {
    const parts = (this.rest + text).split("\n");
    this.rest = parts.pop() ?? "";
    for (const l of parts) this.line(l);
  }

  finish(): Recording {
    if (this.rest.trim()) this.line(this.rest);
    this.rest = "";
    for (const p of this.r.packs.values()) sortByTime(p);
    sortByTime(this.r.system);
    return this.r;
  }

  private line(raw: string) {
    const l = raw.trim();
    if (!l) return;
    let o: any;
    try { o = JSON.parse(l); } catch { this.r.skipped++; return; }
    const t = o?.t;
    if (typeof t !== "number") { this.r.skipped++; return; }
    this.r.lines++;
    if (o.start) { this.r.start = o.start; return; }
    if (o.pack) {
      const tm = o.pack.telemetry;
      if (!tm) { this.r.errors++; return; }
      const a = o.pack.address as number;
      let p = this.r.packs.get(a);
      if (!p) { p = emptyPack(); this.r.packs.set(a, p); }
      const cv: number[] = tm.cell_voltages ?? [], ct: number[] = tm.cell_temperatures ?? [];
      // like the live view: the 1 mA idle current where the regular value reads 0
      const cur = tm.current !== 0 ? tm.current : (tm.idle_current_ma ?? 0) / 1000;
      p.t.push(t); p.current.push(cur); p.soc.push(tm.soc); p.voltage.push(tm.pack_voltage);
      p.cellMax.push(cv.length ? Math.max(...cv) : NaN); p.cellMin.push(cv.length ? Math.min(...cv) : NaN);
      p.tempMax.push(ct.length ? Math.max(...ct) : NaN); p.tempMin.push(ct.length ? Math.min(...ct) : NaN);
      const n = p.t.length - 1;
      cv.forEach((v, i) => { (p.cells[i] ??= [])[n] = v; });
      this.span(t);
      return;
    }
    if (o.system) {
      const v = o.system.values;
      if (!v) { this.r.errors++; return; }
      const s = this.r.system;
      s.t.push(t); s.current.push(v.current); s.soc.push(v.soc); s.voltage.push(v.voltage);
      s.ccl.push(v.charge_current_limit); s.dcl.push(v.discharge_current_limit); s.cvl.push(v.charge_voltage_limit);
      s.cellMax.push(v.highest_cell_voltage); s.cellMin.push(v.lowest_cell_voltage);
      this.span(t);
      return;
    }
    this.r.skipped++;
  }

  private span(t: number) {
    if (t < this.r.from) this.r.from = t;
    if (t > this.r.to) this.r.to = t;
  }
}
/* eslint-enable @typescript-eslint/no-explicit-any */

/** Lines are written in time order; only re-sort if a file was put together from pieces. */
function sortByTime(s: PackSeries | SystemSeries) {
  if (s.t.every((x, i) => i === 0 || s.t[i - 1] <= x)) return;
  const order = s.t.map((_, i) => i).sort((a, b) => s.t[a] - s.t[b]);
  const cols = s as unknown as Record<string, number[] | number[][]>;
  for (const [k, v] of Object.entries(cols)) {
    cols[k] = k === "cells" ? (v as number[][]).map((c) => order.map((i) => c[i])) : order.map((i) => (v as number[])[i]);
  }
}

/** Parse a whole recording held in memory (tests, small files). */
export const parseRecording = (text: string) => { const p = new RecordingParser(); p.push(text); return p.finish(); };

/** Typical time between two answers of a pack (median), for the finest useful grid. */
export function typicalInterval(r: Recording): number {
  const d: number[] = [];
  for (const p of r.packs.values()) for (let i = 1; i < p.t.length; i += Math.max(1, Math.floor(p.t.length / 500))) d.push(p.t[i] - p.t[i - 1]);
  for (let i = 1; i < r.system.t.length; i += Math.max(1, Math.floor(r.system.t.length / 500))) d.push(r.system.t[i] - r.system.t[i - 1]);
  if (!d.length) return 1000;
  d.sort((a, b) => a - b);
  return Math.max(200, d[Math.floor(d.length / 2)]);
}

/** A time grid between from and to: bucket start times (ms). */
export interface Grid { x: number[]; step: number; from: number; to: number }

/** At most maxPoints buckets, none finer than 1.5 × the typical interval (so most buckets hold an answer). */
export function makeGrid(from: number, to: number, interval: number, maxPoints = 1500): Grid {
  const span = Math.max(1, to - from);
  const step = Math.max(span / maxPoints, interval * 1.5);
  const n = Math.max(1, Math.ceil(span / step));
  return { x: Array.from({ length: n }, (_, i) => from + i * step), step, from, to };
}

export type Reduce = "mean" | "max" | "min";

/**
 * One column reduced onto the grid: mean (or max/min) of the answers in each bucket, null where there
 * are none. Short holes (up to `bridge` ms, e.g. one missed poll) take the previous value, longer ones
 * stay empty so the chart shows the gap.
 */
export function onGrid(t: number[], v: number[], g: Grid, how: Reduce = "mean", bridge = 0): (number | null)[] {
  const out: (number | null)[] = new Array(g.x.length).fill(null);
  let i = lowerBound(t, g.from);
  for (let b = 0; b < g.x.length; b++) {
    const end = b === g.x.length - 1 ? g.to + 1 : g.x[b] + g.step;
    let sum = 0, n = 0, m = how === "max" ? -Infinity : Infinity;
    for (; i < t.length && t[i] < end; i++) {
      const x = v[i];
      if (x === undefined || Number.isNaN(x)) continue;
      sum += x; n++;
      if (how === "max" ? x > m : x < m) m = x;
    }
    if (n) out[b] = how === "mean" ? sum / n : m;
  }
  if (bridge > 0) {
    const maxRun = Math.floor(bridge / g.step);
    let last: number | null = null, run = 0;
    for (let b = 0; b < out.length; b++) {
      if (out[b] !== null) {
        if (run && run <= maxRun && last !== null) for (let k = b - run; k < b; k++) out[k] = last;
        last = out[b]; run = 0;
      } else if (last !== null) run++;
    }
  }
  return out;
}

function lowerBound(a: number[], x: number): number {
  let lo = 0, hi = a.length;
  while (lo < hi) { const m = (lo + hi) >> 1; if (a[m] < x) lo = m + 1; else hi = m; }
  return lo;
}

/** Below this mean current per pack (A) the share is not meaningful (equalizing currents dominate). */
export const SHARE_MIN_A = 5;

/** Share of the packs that must have answered in a bucket for a meaningful mean. */
export const SHARE_MIN_PRESENT = 0.75;

/**
 * Each pack's current relative to the mean of the packs that answered in the same bucket (1 = average
 * share). Only where the mean is at least SHARE_MIN_A and at least SHARE_MIN_PRESENT of the packs answered.
 */
export function shares(currents: Map<number, (number | null)[]>): Map<number, (number | null)[]> {
  const cols = [...currents.values()];
  const n = cols[0]?.length ?? 0;
  const mean: (number | null)[] = Array.from({ length: n }, (_, b) => {
    const vs = cols.map((c) => c[b]).filter((x): x is number => x !== null);
    if (vs.length < 2 || vs.length < cols.length * SHARE_MIN_PRESENT) return null;
    const m = vs.reduce((s, x) => s + x, 0) / vs.length;
    return Math.abs(m) >= SHARE_MIN_A ? m : null;
  });
  return new Map([...currents].map(([a, c]) => [a, c.map((x, b) => (x === null || mean[b] === null ? null : x / mean[b]!))]));
}

/** A colour per pack address that stays the same across charts (golden-angle hues). */
export const packColor = (address: number, dark: boolean) =>
  `hsl(${Math.round((address * 137.508 + 190) % 360)} ${dark ? 62 : 58}% ${dark ? 64 : 42}%)`;
