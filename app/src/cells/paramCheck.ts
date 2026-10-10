import { effectiveChargeRange } from "./rules";
import type { Cell } from "./types";

/**
 * A BMS parameter against the cell's datasheet. "max": the parameter must not be above the limit,
 * "min": not below it, "maxAbs": by magnitude (the BMS keeps discharge currents negative),
 * "rated": the rated capacity should match the cells (a mismatch is a note, not a violation).
 */
export interface ParamLimit { kind: "max" | "maxAbs" | "min" | "rated"; limit: number }

/** Indices of the parameters with a datasheet limit (see KEYS in params.ts). */
const CELL_HIGH = [0, 4], CELL_LOW = [2, 6], PACK_HIGH = [10, 14], PACK_LOW = [12, 16];
const CHARGE_HOT = [20, 24], CHARGE_COLD = [22, 26], DISCHARGE_HOT = [28, 32], DISCHARGE_COLD = [30, 34];
const CHARGE_CURRENT = [50, 54], DISCHARGE_CURRENT = [52, 55];
export const RATED = 58, SERIES = 65;

/** Allowed difference between rated capacity and cells × nominal capacity. */
const RATED_TOLERANCE = 0.02;

/**
 * Datasheet limit per parameter index for this cell, cells in parallel and in series. Only alarm and
 * protection thresholds are checked: recovery values lie inside them by design.
 */
export function paramLimits(cell: Cell, parallel: number, series: number): Map<number, ParamLimit> {
  const out = new Map<number, ParamLimit>();
  const put = (indices: number[], kind: ParamLimit["kind"], limit: number | undefined) => {
    if (limit !== undefined) for (const i of indices) out.set(i, { kind, limit });
  };
  const v = cell.voltage, t = cell.temperature, c = cell.current;
  put(CELL_HIGH, "max", v?.charge_cutoff_v);
  put(CELL_LOW, "min", v?.discharge_cutoff_v);
  if (series > 0) {
    put(PACK_HIGH, "max", v?.charge_cutoff_v === undefined ? undefined : round(v.charge_cutoff_v * series, 2));
    put(PACK_LOW, "min", v?.discharge_cutoff_v === undefined ? undefined : round(v.discharge_cutoff_v * series, 2));
  }
  // with a derating table, its outer bounds are where charging is allowed at all
  const range = effectiveChargeRange(cell) ?? (t?.charge_min_c !== undefined && t.charge_max_c !== undefined ? { min: t.charge_min_c, max: t.charge_max_c } : undefined);
  put(CHARGE_HOT, "max", range?.max);
  put(CHARGE_COLD, "min", range?.min);
  put(DISCHARGE_HOT, "max", t?.discharge_max_c);
  put(DISCHARGE_COLD, "min", t?.discharge_min_c);
  put(CHARGE_CURRENT, "max", c?.max_continuous_charge_a === undefined ? undefined : c.max_continuous_charge_a * parallel);
  put(DISCHARGE_CURRENT, "maxAbs", c?.max_continuous_discharge_a === undefined ? undefined : c.max_continuous_discharge_a * parallel);
  put([RATED], "rated", cell.capacity?.nominal_ah === undefined ? undefined : cell.capacity.nominal_ah * parallel);
  return out;
}

const round = (x: number, d: number) => Math.round(x * 10 ** d) / 10 ** d;

export type ParamLevel = "ok" | "over" | "note";

/** Whether a parameter value keeps to its datasheet limit. */
export function checkParam(l: ParamLimit, value: number): ParamLevel {
  if (l.kind === "rated") return Math.abs(value - l.limit) <= l.limit * RATED_TOLERANCE ? "ok" : "note";
  // a small epsilon: values come from decimal displays (3.650 vs 3.65 × 16)
  if (l.kind === "min") return value < l.limit - 1e-6 ? "over" : "ok";
  const x = l.kind === "maxAbs" ? Math.abs(value) : value;
  return x > l.limit + 1e-6 ? "over" : "ok";
}

/** Every parameter of a source that leaves the datasheet: index → level (ok ones left out). */
export function paramFindings(cell: Cell, parallel: number, values: number[]): Map<number, ParamLevel> {
  const limits = paramLimits(cell, parallel, Math.round(values[SERIES] ?? 0));
  const out = new Map<number, ParamLevel>();
  for (const [i, l] of limits) {
    if (values[i] === undefined) continue;
    const r = checkParam(l, values[i]);
    if (r !== "ok") out.set(i, r);
  }
  return out;
}
