/** Display helpers of the cell analysis (`cell_analysis.rs` computes; this words and colours it). */
import type { CellAnalysis, CellReason, CellResult } from "./api";

/** The heatmap's colour scale is symmetric and clipped at ± this (mV). */
export const HEAT_CLIP_MV = 20;
/** The backend's minimum figures, for the wording of „keine Aussage“. */
export const MIN_ROWS = 30;
export const MIN_STEPS = 10;

type Rgb = [number, number, number];

export function hexRgb(hex: string): Rgb | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const v = parseInt(m[1], 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
}

/** The colour of a deviation: from `neutral` at 0 towards `low` (below the median) or `high` (above), full at the clip;
 *  null without a value (not recorded). */
export function heatRgb(mv: number | null, neutral: Rgb, low: Rgb, high: Rgb): Rgb | null {
  if (mv === null || !Number.isFinite(mv)) return null;
  const f = Math.min(1, Math.abs(mv) / HEAT_CLIP_MV), end = mv < 0 ? low : high;
  return neutral.map((c, i) => Math.round(c + (end[i] - c) * f)) as Rgb;
}

/** A deviation with its sign („+12 mV“, „−3 mV“); "–" without a value. */
export function signedMv(v: number | null | undefined, lang: string): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return "–";
  const r = Math.round(v * 10) / 10;
  const s = Math.abs(r).toLocaleString(lang, { maximumFractionDigits: Math.abs(r) < 10 ? 1 : 0 });
  return `${r > 0 ? "+" : r < 0 ? "−" : "±"}${s} mV`;
}

/** The i18n key and values of a reason; mV values worded with their sign, % rounded. `partial` names what is missing
 *  as keys under cell.missing (`missing`), which `reasonText` words. */
export function reasonArgs(r: CellReason, lang: string): [string, Record<string, string | number | string[]>] {
  const mv = (v: number) => signedMv(v, lang), pct = (v: number) => `${Math.round(v)} %`;
  if (r.key === "partial") {
    const missing = [!r.top && "cell.missing.top", !r.bottom && "cell.missing.bottom", !r.r && "cell.missing.r"].filter((x): x is string => !!x);
    return ["cell.reason.partial", { steps: r.steps, minSteps: MIN_STEPS, missing }];
  }
  const { key, ...rest } = r;
  const vals: Record<string, string | number> = { min: MIN_ROWS, minSteps: MIN_STEPS };
  for (const [k, v] of Object.entries(rest) as [string, number][]) vals[k] = k === "pct" ? pct(v) : ["steps", "count", "rows"].includes(k) ? v : mv(v);
  return [`cell.reason.${key}`, vals];
}

/** A reason in words. */
export function reasonText(t: (key: string, vals?: Record<string, unknown>) => string, r: CellReason, lang: string): string {
  const [key, { missing: keys, ...vals }] = reasonArgs(r, lang);
  const missing = Array.isArray(keys) ? keys.map((k) => t(k, vals)).join(", ") : undefined;
  return t(key, { ...vals, missing });
}

/** Classes the report does not name: matching, or matching as far as measured (counted apart in the summary). */
export const QUIET_CLASSES: CellResult["class"][] = ["ok", "rest_ok"];

/** The end values a cell is judged by: corrected where the resistance is known. */
export const topOf = (c: CellResult) => c.top_corr ?? c.top;
export const bottomOf = (c: CellResult) => c.bottom_corr ?? c.bottom;

/** The cells to name in the report: everything not „passend“. When every cell has no statement for the same reason, one
 *  entry stands for the pack. */
export function reportCells(a: CellAnalysis): { all: CellResult | null; cells: CellResult[] } {
  const off = a.cells.filter((c) => !QUIET_CLASSES.includes(c.class));
  if (off.length === a.cells.length && off.length > 0 && off.every((c) => c.class === "none" && c.reason.key === off[0].reason.key))
    return { all: off[0], cells: [] };
  return { all: null, cells: off };
}

/** Reason key and values for the CSV: key, then name=value pairs separated by spaces. */
export function reasonCsv(r: CellReason): string {
  const { key, ...rest } = r;
  return [key, ...Object.entries(rest).map(([k, v]) => `${k}=${typeof v === "boolean" ? v : Math.round((v as number) * 100) / 100}`)].join(" ");
}

/** Cells of a bitmask as a list („3 7“; bit n = cell n+1). */
export function maskCells(mask: number | null | undefined): string {
  if (mask === null || mask === undefined) return "";
  const out: number[] = [];
  for (let i = 0; i < 53 && 2 ** i <= mask; i++) if (Math.floor(mask / 2 ** i) % 2) out.push(i + 1);
  return out.join(" ");
}
