import type { Basis, Cell, DeratingPoint } from "./types";

/** Amperes for a rate: C over the nominal capacity, P over energy / nominal voltage. */
export function toAmps(cell: Cell, value: number, basis: Basis): number | undefined {
  const ah = cell.capacity?.nominal_ah;
  if (basis === "C") return ah === undefined ? undefined : value * ah;
  const v = cell.voltage?.nominal_v;
  const wh = cell.capacity?.nominal_energy_wh ?? (ah !== undefined && v !== undefined ? ah * v : undefined);
  return wh === undefined || v === undefined ? undefined : (value * wh) / v;
}

type Axis = { point?: number; min?: number; max?: number };

/**
 * Items of one axis that govern x: every band containing x (bounds inclusive; a missing bound is
 * open), plus the nearest point at or below x and the nearest at or above it. Outside the points'
 * range there is no lower or upper neighbour, so only bands can match.
 */
function govern<T>(items: T[], x: number, axis: (i: T) => Axis): T[] {
  const out = items.filter((i) => {
    const a = axis(i);
    return a.point === undefined && (a.min ?? -Infinity) <= x && x <= (a.max ?? Infinity);
  });
  const pts = items.filter((i) => axis(i).point !== undefined);
  if (pts.length) {
    const vals = [...new Set(pts.map((i) => axis(i).point!))].sort((a, b) => a - b);
    const lo = vals.filter((v) => v <= x).pop(), hi = vals.find((v) => v >= x);
    if (lo !== undefined && hi !== undefined) out.push(...pts.filter((i) => axis(i).point === lo || axis(i).point === hi));
  }
  return out;
}

const tempAxis = (p: DeratingPoint): Axis => ({ point: p.temp_c, min: p.temp_min_c, max: p.temp_max_c });
/** A row without SOC bounds applies to every SOC (band 0–100 %). */
const socAxis = (p: DeratingPoint): Axis =>
  p.soc_min_pct !== undefined && p.soc_min_pct === p.soc_max_pct ? { point: p.soc_min_pct } : { min: p.soc_min_pct ?? 0, max: p.soc_max_pct ?? 100 };
/** Rows governing a SOC; unknown SOC = all rows (the minimum is then the worst case). */
const bySoc = (rows: DeratingPoint[], soc?: number) => (soc === undefined ? rows : govern(rows, soc, socAxis));
const minOf = (vals: number[]) => (vals.length ? Math.min(...vals) : 0);

/**
 * Allowed continuous charge rate (in the table's basis) at a cell temperature and SOC.
 * App rule: between two points, and on shared band bounds, the LOWER value applies; outside the
 * table charging is not allowed (0). Unknown SOC = the lowest value over all SOC rows.
 * Returns undefined if the cell has no derating table.
 */
export function allowedChargeRate(cell: Cell, tempC: number, soc?: number): number | undefined {
  const d = cell.charge_derating;
  if (!d) return undefined;
  if (d.interpolation === "linear") return linear(d.points, tempC, soc);
  return minOf(bySoc(govern(d.points, tempC, tempAxis), soc).map((p) => p.value));
}

/**
 * Only when a datasheet explicitly allows interpolation: temperature bands apply as constant values,
 * temperature points are interpolated linearly per SOC row group; SOC still uses the lower rule.
 */
function linear(points: DeratingPoint[], tempC: number, soc?: number): number {
  const rows = bySoc(points, soc);
  const vals = rows.filter((p) => p.temp_c === undefined && (p.temp_min_c ?? -Infinity) <= tempC && tempC <= (p.temp_max_c ?? Infinity)).map((p) => p.value);
  const groups = new Map<string, DeratingPoint[]>();
  for (const p of rows) if (p.temp_c !== undefined) {
    const k = `${p.soc_min_pct ?? ""}|${p.soc_max_pct ?? ""}`;
    groups.set(k, [...(groups.get(k) ?? []), p]);
  }
  for (const g of groups.values()) {
    const at = (t: number) => minOf(g.filter((p) => p.temp_c === t).map((p) => p.value));
    const temps = [...new Set(g.map((p) => p.temp_c!))].sort((a, b) => a - b);
    const lo = temps.filter((t) => t <= tempC).pop(), hi = temps.find((t) => t >= tempC);
    if (lo === undefined || hi === undefined) continue;
    vals.push(lo === hi ? at(lo) : at(lo) + ((at(hi) - at(lo)) * (tempC - lo)) / (hi - lo));
  }
  return minOf(vals);
}

/** All values of one axis where the table can change, plus the midpoints between them. */
function probes(bounds: number[], lo: number, hi: number): number[] {
  const b = [...new Set(bounds.filter((x) => x >= lo && x <= hi))].sort((x, y) => x - y);
  const out = [...b];
  for (let i = 1; i < b.length; i++) out.push((b[i - 1] + b[i]) / 2);
  return out;
}

/**
 * One temperature interval in which charging is allowed. min/max are the bounds; minExcl/maxExcl
 * mark a bound that itself is excluded (e.g. "above 0 °C"); openLow/openHigh mark an open table end.
 */
export interface ChargeInterval { min: number; max: number; minExcl: boolean; maxExcl: boolean; openLow: boolean; openHigh: boolean }

/** Where a linear segment between (t0, v0) and (t1, v1) crosses zero, if it does inside the segment. */
function crossings(cell: Cell): number[] {
  const d = cell.charge_derating;
  if (!d || d.interpolation !== "linear") return [];
  const temps = [...new Set(d.points.map((p) => p.temp_c).filter((x): x is number => x !== undefined))].sort((a, b) => a - b);
  const out: number[] = [];
  for (let i = 1; i < temps.length; i++) {
    // the value is linear in between, so it is positive on the whole open segment if one end is
    out.push(temps[i - 1] + (temps[i] - temps[i - 1]) * 0.001, temps[i] - (temps[i] - temps[i - 1]) * 0.001);
  }
  return out;
}

/**
 * Temperature intervals in which the table allows charging (> 0) at some SOC. Under the lower rule
 * the value is constant between two table bounds, so evaluating every bound and the midpoints between
 * them is exact; linear tables are also probed just inside each segment. Gaps between intervals mean
 * no charging. (A full cell may not be chargeable at all; that is a SOC limit, not a temperature one.)
 */
export function chargeIntervals(cell: Cell): ChargeInterval[] | undefined {
  const d = cell.charge_derating;
  if (!d) return undefined;
  const tb = d.points.flatMap((p) => [p.temp_c, p.temp_min_c, p.temp_max_c]).filter((x): x is number => x !== undefined);
  if (!tb.length) return [];
  const tLo = Math.min(...tb), tHi = Math.max(...tb);
  const bounds = new Set(tb);
  const temps = [...new Set([tLo - 1, ...probes(tb, tLo, tHi), ...crossings(cell), tHi + 1])].sort((a, b) => a - b);
  const socs = probes([0, 100, ...d.points.flatMap((p) => [p.soc_min_pct, p.soc_max_pct]).filter((x): x is number => x !== undefined)], 0, 100);
  const pos = temps.map((t) => socs.some((s) => (allowedChargeRate(cell, t, s) ?? 0) > 0));
  const out: ChargeInterval[] = [];
  for (let i = 0; i < temps.length; i++) {
    if (!pos[i] || (i > 0 && pos[i - 1])) continue;
    let j = i;
    while (j + 1 < temps.length && pos[j + 1]) j++;
    const first = temps[i], last = temps[j];
    // an interval that starts or ends between two bounds excludes the neighbouring bound
    const startsInside = !bounds.has(first) && first !== tLo - 1;
    const endsInside = !bounds.has(last) && last !== tHi + 1;
    out.push({
      min: startsInside ? temps[i - 1] : first === tLo - 1 ? tLo : first,
      max: endsInside ? temps[j + 1] : last === tHi + 1 ? tHi : last,
      minExcl: startsInside,
      maxExcl: endsInside,
      openLow: first === tLo - 1,
      openHigh: last === tHi + 1,
    });
  }
  return out;
}

/** Outer bounds of all charge intervals (for summaries); undefined if charging is never allowed. */
export function effectiveChargeRange(cell: Cell): { min: number; max: number } | undefined {
  const iv = chargeIntervals(cell);
  if (!iv?.length) return undefined;
  return { min: iv[0].min, max: iv[iv.length - 1].max };
}

/** Required values that are missing or not verified. A value that is absent but has a verified provenance note ("not specified in datasheet", "no derating table") counts as checked. */
export function missingRequired(cell: Cell, required: readonly string[]): string[] {
  const get = (path: string) => path.split(".").reduce<unknown>((o, k) => (o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined), cell);
  return required.filter((path) => {
    const prov = cell.provenance?.[path];
    if (get(path) !== undefined) return !prov?.verified;
    return !(prov?.verified && prov.note);
  });
}
