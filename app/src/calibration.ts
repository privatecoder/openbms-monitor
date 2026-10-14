/** Calibration (0xA0) and CAN inverter protocol (0xA6): pure helpers for the Parameters view. */

/** What 0xA0 sends: sub 0 zero point, sub 1 current in 0.01 A, sub 2 mean cell voltage in mV. */
export type Calibration = { kind: "zero_point" } | { kind: "current"; centi_amps: number } | { kind: "voltage"; cell_mv: number };

/** The current calibration needs at least this much (A), in the reference and in the BMS reading. */
export const MIN_CURRENT_A = 10;

/** Protocols the app offers (0xA6). 7 MUST is left out: 06.04/06.05 store it but run 1, and a later update to ≥ 06.11
 *  then switches the CAN speed; 0x51 cannot tell these firmwares apart. */
export const CAN_PROTOCOLS: { n: number; name: string; text: string }[] = [
  { n: 1, name: "Pylontech/Deye/Luxpower/TBB", text: "CAN:PNG_DYE_Luxp_TBB" },
  { n: 2, name: "Growatt SPF/SPH", text: "CAN:Growatt_SPF_SPH" },
  { n: 3, name: "Victron", text: "CAN:Victron" },
  { n: 4, name: "SMA/Sofar", text: "CAN:SMA_SOFAR" },
  { n: 5, name: "Ginlong", text: "CAN:GINLONG" },
  { n: 6, name: "Studer", text: "CAN:Studer" },
];

/** The offered protocol a 0x51 text names, or undefined (7 MUST, 8…20 or an unknown text). */
export const protocolOf = (text: string | undefined) => CAN_PROTOCOLS.find((p) => p.text.toLowerCase() === text?.trim().toLowerCase());

/** A number typed with a decimal comma or point; null when empty or not a number. */
export function parseNumber(s: string): number | null {
  const v = s.trim().replace(",", ".");
  if (!v || !/^[+-]?(\d+\.?\d*|\.\d+)$/.test(v)) return null;
  return Number(v);
}

/** The current button: both the measured value and the BMS current at least 10 A in magnitude and the same direction.
 *  Without this the firmware accepts e.g. 0.01 A against 2 A and ruins the gain beyond one-step repair. */
export function currentAllowed(measured: number | null, bms: number | null | undefined): boolean {
  if (measured === null || bms === null || bms === undefined) return false;
  return Math.abs(measured) >= MIN_CURRENT_A && Math.abs(bms) >= MIN_CURRENT_A && Math.sign(measured) === Math.sign(bms);
}

/** 0.01 A for sub 1. */
export const centiAmps = (a: number) => Math.round(a * 100);

/** Mean cell voltage in mV for sub 2: pack voltage at P+/P− ÷ cells, rounded to mV; null for no usable input. */
export function cellMv(packVolts: number | null, cells: number): number | null {
  if (packVolts === null || !(packVolts > 0) || !(cells > 0)) return null;
  const mv = Math.round((packVolts * 1000) / cells);
  return mv <= 0xffff ? mv : null;
}

/** Arithmetic mean and spread (max − min) of the cell voltages in V, as the firmware compares them. */
export function cellStats(cells: number[]): { mean: number; spread: number } | null {
  if (!cells.length) return null;
  return { mean: cells.reduce((a, b) => a + b, 0) / cells.length, spread: Math.max(...cells) - Math.min(...cells) };
}

/** A write the BMS refused (RTN 06) or did not know (RTN 04), from the backend's JSON error; null for any other error. */
export function refusal(e: unknown): "refused" | "invalid" | null {
  let v: unknown = e;
  if (typeof e === "string") { try { v = JSON.parse(e); } catch { return null; } }
  const k = v && typeof v === "object" ? (v as { kind?: unknown }).kind : undefined;
  return k === "refused" || k === "invalid" ? k : null;
}

/** Event 8 bit of the "… not calibrated" flag each calibration clears. */
export const NOT_CALIBRATED_BIT: Record<Calibration["kind"], number> = { zero_point: 4, current: 3, voltage: 2 };

/** Whether the pack still reports that flag (status events, event 8 = index 7). */
export const notCalibrated = (events: number[] | undefined, kind: Calibration["kind"]) =>
  !!events && ((events[7] ?? 0) & (1 << NOT_CALIBRATED_BIT[kind])) !== 0;
