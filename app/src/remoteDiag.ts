/** The reads behind the read-capability bits (calibration pages 0x64/05, stored SOC records and live energy 0x64/06): pure helpers for
 *  their display. Nothing here predicts what the BMS does next; it shows what the pack reported at the moment of reading. */
import type { CalPage, CalPageRead, Coefficients, SocRecord, SocStoreRead } from "./api";

export const PAGES: CalPage[] = ["active", "primary", "backup"];

/** One coefficient across the three pages: active, primary, backup (null: page missing or not strictly valid). */
export interface CalRow {
  /** i18n key below `rdiag.cal.row` and its parameters */
  key: "cell" | "voltage" | "gainA" | "gainB" | "zeroA" | "zeroB";
  n?: number;
  values: [number | null, number | null, number | null];
  /** the values shown differ; for current gain B only the stored copies count (the firmware sets the active value itself) */
  differs: boolean;
}

const coeffs = (r: CalPageRead | undefined): Coefficients | null => (r?.value ? r.value.coefficients : null);

const differ = (vs: (number | null)[]) => new Set(vs.filter((v): v is number => v !== null)).size > 1;

/** The rows of the calibration table, from whatever pages arrived. */
export function calRows(pages: CalPageRead[]): CalRow[] {
  const c = PAGES.map((p) => coeffs(pages.find((x) => x.page === p)));
  const row = (key: CalRow["key"], get: (k: Coefficients) => number, n?: number): CalRow => {
    const values = c.map((k) => (k ? get(k) : null)) as CalRow["values"];
    return { key, n, values, differs: key === "gainB" ? differ(values.slice(1)) : differ(values) };
  };
  const cells = Math.max(0, ...c.map((k) => k?.cells.length ?? 0));
  return [
    ...Array.from({ length: cells }, (_, i) => row("cell", (k) => k.cells[i], i + 1)),
    row("voltage", (k) => k.voltage[0], 1), row("voltage", (k) => k.voltage[1], 2),
    row("gainA", (k) => k.current_gain[0]), row("gainB", (k) => k.current_gain[1]),
    row("zeroA", (k) => k.zero[0]), row("zeroB", (k) => k.zero[1]),
  ];
}

/** The stored state as the pack reported it: live SOC, and the SOC of the copy its boot choice names, only when that copy
 *  is strictly valid with a full capacity (a copy the firmware accepts but of the wrong shape is never interpreted, and a
 *  valid other copy is not taken in its place). */
export interface SocView {
  live: number;
  boot: SocStoreRead["store"]["boot"];
  record: SocRecord | null;
  stored: number | null;
  /** stored − live in percentage points */
  delta: number | null;
}

export function socView(r: SocStoreRead): SocView {
  const s = r.store;
  const i = s.boot === "primary" ? 0 : s.boot === "backup" ? 1 : null;
  const record = r.records && i !== null && s.class[i] === "valid" ? s.records[i] : null;
  const stored = record && record.full_ah > 0 ? (record.remaining_ah / record.full_ah) * 100 : null;
  return { live: s.live_soc, boot: s.boot, record, stored, delta: stored === null ? null : stored - s.live_soc };
}

/** A deviation worth a warning colour (percentage points); only a display threshold, no gate. */
export const SOC_DELTA_WARN = 5;
