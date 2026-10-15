/** Pack control (0x45, 0x49 index 59): pure helpers for the „Steuern“ block of the Parameters view. */
import type { Status } from "./api";
import { parseNumber } from "./calibration";

/** System status bit 5: switched off by command (0x45 type 4). */
export const OFF_BIT = 0x20;

export const commandedOff = (s: Status) => (s.system_status & OFF_BIT) !== 0;
/** Switch state bits: 0 discharge MOSFET, 1 charge MOSFET, 3 heater output. */
export const dischargeFree = (s: Status) => (s.switch_state & 0x01) !== 0;
export const chargeFree = (s: Status) => (s.switch_state & 0x02) !== 0;
export const heaterOn = (s: Status) => (s.switch_state & 0x08) !== 0;

/** Active protections and faults: what closes a MOSFET besides the off command. */
export const blockingAlarms = (s: Status) => s.alarms.filter((a) => a.severity === "protection" || a.severity === "fault").map((a) => a.id);

/** Reasons the firmware switches a pack off again right after „on“: cell or pack undervoltage, SOC protection. */
const OFF_REASONS = ["alarm.ev2.b3", "alarm.ev2.b7", "alarm.ev6.b3"];
export const offReasons = (s: Status) => s.alarms.map((a) => a.id).filter((id) => OFF_REASONS.includes(id));

/** Remaining capacity typed in Ah (comma or point) → 10 mAh for 0x49; null when empty, negative or too large. */
export function remainingRaw(text: string): number | null {
  const ah = parseNumber(text);
  if (ah === null || ah < 0) return null;
  const raw = Math.round(ah * 100);
  return raw <= 0xffff ? raw : null;
}

/** The field is usable: a value from 0 up to the full capacity (the firmware refuses more with RTN 06). */
export const remainingAllowed = (raw: number | null, fullAh: number | null | undefined) =>
  raw !== null && fullAh !== null && fullAh !== undefined && raw <= Math.round(fullAh * 100);

/** SOC in % the value gives, as the firmware computes it (remaining ÷ full). */
export const socOf = (raw: number, fullAh: number) => (fullAh > 0 ? (raw / (fullAh * 100)) * 100 : 0);

/** Under P79 (SOC protection, %) the BMS blocks discharging. Unknown P79: null. */
export const belowSocProtection = (soc: number, p79: number | null | undefined) => (p79 === null || p79 === undefined ? null : soc < p79);

/** Energy in kWh as text: MWh from 1000 kWh on. */
export function energyText(kwh: number, lang: string): string {
  const mwh = kwh >= 1000;
  const v = mwh ? kwh / 1000 : kwh;
  return `${new Intl.NumberFormat(lang, { maximumFractionDigits: mwh ? 1 : 0 }).format(v)} ${mwh ? "MWh" : "kWh"}`;
}

/** Outcome of a restart (backend `Restart`). */
export type RestartResult = { kind: "not_reachable" } | { kind: "back"; after_ms: number } | { kind: "no_interruption" } | { kind: "no_answer" };

/** Another action holds the pack, from the backend's JSON error; null for any other error. */
export function packBusy(e: unknown): { address: number; action: string } | null {
  let v: unknown = e;
  if (typeof e === "string") { try { v = JSON.parse(e); } catch { return null; } }
  if (!v || typeof v !== "object") return null;
  const o = v as { kind?: unknown; address?: unknown; action?: unknown };
  return o.kind === "pack-busy" && typeof o.address === "number" && typeof o.action === "string" ? { address: o.address, action: o.action } : null;
}

/** An error as text: „Pack 00 ist gerade mit … beschäftigt“ for a reserved pack, else as it came. */
export function errorText(e: unknown, t: (k: string, o?: Record<string, unknown>) => string): string {
  const b = packBusy(e);
  if (!b) return String(e);
  return t("ctl.busy", { pack: String(b.address).padStart(2, "0"), action: t(`ctl.action.${b.action}`) });
}
