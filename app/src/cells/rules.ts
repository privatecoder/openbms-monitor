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
 * Items of one axis that govern x: every band containing x (bounds inclusive, open bands
 * unbounded), plus the nearest point at or below x and the nearest at or above it. Outside
 * the points' range there is no lower or upper neighbour, so only bands can match.
 */
function govern<T>(items: T[], x: number, axis: (i: T) => Axis): T[] {
  const out = items.filter((i) => {
    const a = axis(i);
    return a.point === undefined && (a.min !== undefined || a.max !== undefined) && (a.min ?? -Infinity) <= x && x <= (a.max ?? Infinity);
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
const socAxis = (p: DeratingPoint): Axis =>
  p.soc_min_pct !== undefined && p.soc_min_pct === p.soc_max_pct ? { point: p.soc_min_pct } : { min: p.soc_min_pct, max: p.soc_max_pct };

/**
 * Allowed continuous charge rate (in the table's basis) at a cell temperature and SOC.
 * App rule: between two points, and on shared band bounds, the LOWER value applies; outside the
 * table charging is not allowed (0). Unknown SOC = the lowest value over all SOC bands.
 * Returns undefined if the cell has no derating table.
 */
export function allowedChargeRate(cell: Cell, tempC: number, soc?: number): number | undefined {
  const d = cell.charge_derating;
  if (!d) return undefined;
  if (d.interpolation === "linear") return linear(d.points, tempC, soc);
  const byTemp = govern(d.points, tempC, tempAxis);
  if (!byTemp.length) return 0;
  const hasSoc = byTemp.some((p) => p.soc_min_pct !== undefined || p.soc_max_pct !== undefined);
  const chosen = soc === undefined || !hasSoc ? byTemp : govern(byTemp, soc, socAxis);
  if (!chosen.length) return 0;
  return Math.min(...chosen.map((p) => p.value));
}

/** Only used when a datasheet explicitly allows interpolation; temperature points, SOC by lower rule. */
function linear(points: DeratingPoint[], tempC: number, soc?: number): number {
  const at = (t: number) => {
    const rows = points.filter((p) => p.temp_c === t);
    const chosen = soc === undefined ? rows : govern(rows, soc, socAxis);
    return chosen.length ? Math.min(...chosen.map((p) => p.value)) : 0;
  };
  const temps = [...new Set(points.map((p) => p.temp_c).filter((t): t is number => t !== undefined))].sort((a, b) => a - b);
  const lo = temps.filter((t) => t <= tempC).pop(), hi = temps.find((t) => t >= tempC);
  if (lo === undefined || hi === undefined) return 0;
  if (lo === hi) return at(lo);
  return at(lo) + ((at(hi) - at(lo)) * (tempC - lo)) / (hi - lo);
}

/**
 * Temperature range in which the table allows charging (> 0) at some SOC, scanned in 0.5 K
 * steps. (A full cell may not be chargeable at all; that is a SOC limit, not a temperature one.)
 */
export function effectiveChargeRange(cell: Cell): { min: number; max: number } | undefined {
  if (!cell.charge_derating) return undefined;
  let min: number | undefined, max: number | undefined;
  for (let t = -40; t <= 90; t += 0.5) {
    let best = 0;
    for (let soc = 0; soc <= 100; soc += 1) best = Math.max(best, allowedChargeRate(cell, t, soc) ?? 0);
    if (best > 0) {
      min ??= t;
      max = t;
    }
  }
  return min === undefined || max === undefined ? undefined : { min, max };
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
