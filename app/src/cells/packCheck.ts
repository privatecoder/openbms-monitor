import { useEffect, useState } from "react";
import type { Telemetry } from "../api";
import { allowedChargeRate, effectiveChargeRange, toAmps } from "./rules";
import type { Cell } from "./types";

/** Cell model of a pack: database id and cells in parallel (in series comes from the telemetry). */
export interface Assignment { cell: string; parallel: number }
export type Assignments = Record<number, Assignment>;

const keyFor = (site: string) => `cellTypes:${site}`;

function read(site: string): Assignments {
  try {
    const raw = JSON.parse(localStorage.getItem(keyFor(site)) ?? "{}") as Record<string, Assignment>;
    return Object.fromEntries(Object.entries(raw).filter(([, a]) => a && typeof a.cell === "string" && a.parallel >= 1).map(([k, a]) => [Number(k), a]));
  } catch { return {}; }
}

/** Cell types of the packs, stored per installation (gateway or port) like the groups. */
export function useAssignments(site: string) {
  const [assigned, setAssigned] = useState<Assignments>(() => read(site));
  useEffect(() => setAssigned(read(site)), [site]);
  // other components of the same site (dashboard, pack detail) follow a change at once
  useEffect(() => {
    const f = (e: Event) => (e as CustomEvent<string>).detail === site && setAssigned(read(site));
    window.addEventListener("cell-assignments", f);
    return () => window.removeEventListener("cell-assignments", f);
  }, [site]);
  const save = (next: Assignments) => {
    setAssigned(next);
    try { localStorage.setItem(keyFor(site), JSON.stringify(next)); } catch { /* preview without storage */ }
    window.dispatchEvent(new CustomEvent("cell-assignments", { detail: site }));
  };
  /** Assign `a` to the packs, or remove their assignment with null. */
  const assign = (packs: number[], a: Assignment | null) => {
    const next = { ...assigned };
    for (const p of packs) if (a) next[p] = a; else delete next[p];
    save(next);
  };
  return { assigned, assign };
}

/** Cells in parallel that match the pack's rated capacity, if it is a whole multiple of the cell's. */
export function suggestParallel(cell: Cell, ratedAh: number | undefined): number | undefined {
  const ah = cell.capacity?.nominal_ah;
  if (!ah || !ratedAh) return undefined;
  const n = Math.round(ratedAh / ah);
  return n >= 1 && Math.abs(ratedAh - n * ah) <= ah * 0.02 ? n : undefined;
}

/**
 * Charge current per pack the datasheet allows at these cell temperatures and SOC. With a derating
 * table the lower value of the coldest and the warmest cell applies (rules.ts); without one, the
 * continuous charge current inside the charge temperature range and 0 outside it.
 * undefined: the datasheet gives no basis.
 */
export function allowedChargeA(cell: Cell, parallel: number, temps: number[], soc?: number): number | undefined {
  if (!temps.length) return undefined;
  const lo = Math.min(...temps), hi = Math.max(...temps);
  if (cell.charge_derating) {
    const rates = [allowedChargeRate(cell, lo, soc), allowedChargeRate(cell, hi, soc)].filter((r): r is number => r !== undefined);
    const amps = rates.map((r) => toAmps(cell, r, cell.charge_derating!.basis));
    if (!amps.length || amps.some((a) => a === undefined)) return undefined;
    const fromTable = Math.min(...(amps as number[]));
    const cap = cell.current?.max_continuous_charge_a;
    return (cap === undefined ? fromTable : Math.min(fromTable, cap)) * parallel;
  }
  const t = cell.temperature, cap = cell.current?.max_continuous_charge_a;
  if (cap === undefined || t?.charge_min_c === undefined || t.charge_max_c === undefined) return undefined;
  return lo >= t.charge_min_c && hi <= t.charge_max_c ? cap * parallel : 0;
}

/**
 * Discharge current per pack the datasheet allows at these cell temperatures: the continuous discharge
 * current inside the discharge temperature range, 0 outside it. undefined: the datasheet gives no basis.
 */
export function allowedDischargeA(cell: Cell, parallel: number, temps: number[]): number | undefined {
  const t = cell.temperature, cap = cell.current?.max_continuous_discharge_a;
  if (!temps.length || cap === undefined || t?.discharge_min_c === undefined || t.discharge_max_c === undefined) return undefined;
  return Math.min(...temps) >= t.discharge_min_c && Math.max(...temps) <= t.discharge_max_c ? cap * parallel : 0;
}

export type Level = "ok" | "near" | "over";
/** One comparison against the datasheet. value and limit in the unit of the check; dir: whether the value must stay below or above. */
export interface Check { id: "cellHigh" | "cellLow" | "tempCharge" | "tempDischarge" | "chargeCurrent" | "dischargeCurrent"; level: Level; value: number; limit: number; limit2?: number }

/** Margins at which a value counts as close to its limit. */
export const NEAR = { cellHigh: 0.05, cellLow: 0.2, temp: 5, current: 0.8 };

/** Current from which the pack counts as charging or discharging for the temperature checks (A). */
const FLOW_A = 0.5;

const level = (over: boolean, near: boolean): Level => (over ? "over" : near ? "near" : "ok");

/**
 * The pack's live values against the datasheet of its cells. current: the pack's current (+ = charging),
 * which decides whether a temperature outside a range is a violation or only a note.
 */
export function checkPack(cell: Cell, parallel: number, tm: Telemetry, current: number): Check[] {
  const out: Check[] = [];
  const v = cell.voltage, t = cell.temperature, c = cell.current;
  const cells = tm.cell_voltages, temps = tm.cell_temperatures;
  if (cells.length && v?.charge_cutoff_v !== undefined) {
    const hi = Math.max(...cells);
    out.push({ id: "cellHigh", value: hi, limit: v.charge_cutoff_v, level: level(hi > v.charge_cutoff_v, hi >= v.charge_cutoff_v - NEAR.cellHigh) });
  }
  if (cells.length && v?.discharge_cutoff_v !== undefined) {
    const lo = Math.min(...cells);
    out.push({ id: "cellLow", value: lo, limit: v.discharge_cutoff_v, level: level(lo < v.discharge_cutoff_v, lo <= v.discharge_cutoff_v + NEAR.cellLow) });
  }
  if (temps.length) {
    const lo = Math.min(...temps), hi = Math.max(...temps);
    // where the datasheet has a derating table, its range decides where charging is allowed at all
    const range = effectiveChargeRange(cell) ?? (t?.charge_min_c !== undefined && t.charge_max_c !== undefined ? { min: t.charge_min_c, max: t.charge_max_c } : undefined);
    // outside the range is a violation only while current flows in that direction; otherwise it means
    // "would not be allowed now" and counts as near
    const temp = (id: Check["id"], min: number, max: number, flowing: boolean) => {
      const worst = lo - min < max - hi ? lo : hi;
      const outside = lo < min || hi > max;
      out.push({ id, value: worst, limit: min, limit2: max, level: outside && flowing ? "over" : level(false, outside || lo < min + NEAR.temp || hi > max - NEAR.temp) });
    };
    if (range) temp("tempCharge", range.min, range.max, current > FLOW_A);
    if (t?.discharge_min_c !== undefined && t.discharge_max_c !== undefined) temp("tempDischarge", t.discharge_min_c, t.discharge_max_c, current < -FLOW_A);
  }
  const allowed = allowedChargeA(cell, parallel, temps, tm.soc);
  if (allowed !== undefined) {
    const a = Math.max(0, current);
    out.push({ id: "chargeCurrent", value: a, limit: allowed, level: level(a > allowed + 0.5, a > allowed * NEAR.current) });
  }
  if (c?.max_continuous_discharge_a !== undefined) {
    const a = Math.max(0, -current), lim = c.max_continuous_discharge_a * parallel;
    out.push({ id: "dischargeCurrent", value: a, limit: lim, level: level(a > lim, a > lim * NEAR.current) });
  }
  return out;
}

/** Recommended (standard) charge current of the pack in A, if the datasheet gives one. */
export const standardChargeA = (cell: Cell, parallel: number) => {
  const r = cell.current?.standard_charge;
  const a = r ? toAmps(cell, r.value, r.basis) : undefined;
  return a === undefined ? undefined : a * parallel;
};
