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

/** Events that close the discharge MOSFET (firmware 16.06.13 `0x7184`): cell/pack undervoltage, discharge and
 *  ambient temperature, power stage, discharge overcurrent, transient overcurrent, short circuit and their locks,
 *  SOC protection, output connection fault. */
const DISCHARGE_BLOCKERS = ["alarm.ev2.b3", "alarm.ev2.b7", "alarm.ev3.b5", "alarm.ev3.b7", "alarm.ev4.b1", "alarm.ev4.b3", "alarm.ev4.b4",
  "alarm.ev5.b3", "alarm.ev5.b4", "alarm.ev5.b5", "alarm.ev5.b6", "alarm.ev5.b7", "alarm.ev6.b3", "alarm.ev6.b6"];
/** Events that close the charge MOSFET (`0x6FFA`): cell/pack overvoltage, charge and ambient temperature, power stage,
 *  charge overcurrent, charger overvoltage, cell too low to charge, reverse polarity, output connection fault. */
const CHARGE_BLOCKERS = ["alarm.ev2.b1", "alarm.ev2.b5", "alarm.ev3.b1", "alarm.ev3.b3", "alarm.ev4.b1", "alarm.ev4.b3", "alarm.ev4.b4",
  "alarm.ev5.b1", "alarm.ev6.b0", "alarm.ev6.b4", "alarm.ev6.b5", "alarm.ev6.b6"];

/** Active events that block discharging or charging, in alarm order. */
export const blockingAlarms = (s: Status, dir: "discharge" | "charge") => {
  const ids = dir === "discharge" ? DISCHARGE_BLOCKERS : CHARGE_BLOCKERS;
  return s.alarms.map((a) => a.id).filter((id) => ids.includes(id));
};

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
export type RestartResult = { kind: "not_reachable" } | { kind: "back"; after_ms: number } | { kind: "no_interruption" } | { kind: "no_answer" }
  | { kind: "sent_link_lost"; error: string };

const kindOf = (e: unknown): unknown => {
  let v: unknown = e;
  if (typeof e === "string") { try { v = JSON.parse(e); } catch { return undefined; } }
  return v && typeof v === "object" ? (v as { kind?: unknown }).kind : undefined;
};

/** The pack did not answer a control command (typically a master polling its slaves). */
export const noAnswer = (e: unknown) => kindOf(e) === "no-answer";
/** The connection changed during the action; nothing more was sent. */
export const linkChanged = (e: unknown) => kindOf(e) === "link-changed";

/** Another action holds the pack, from the backend's JSON error; null for any other error. */
export function packBusy(e: unknown): { address: number; action: string } | null {
  let v: unknown = e;
  if (typeof e === "string") { try { v = JSON.parse(e); } catch { return null; } }
  if (!v || typeof v !== "object") return null;
  const o = v as { kind?: unknown; address?: unknown; action?: unknown };
  return o.kind === "pack-busy" && typeof o.address === "number" && typeof o.action === "string" ? { address: o.address, action: o.action } : null;
}

/** A write refused before anything was sent because the pack's bank is not confirmed or could not be checked
 *  (backend `bank.rs`); null for any other error. */
export type BankRefusal = { state: "unconfirmed" | "failed"; address: number; why?: "new" | "moved" | "foreign"; installation?: { id: string; name: string } | null; error?: string };
export function bankRefusal(e: unknown): BankRefusal | null {
  let v: unknown = e;
  if (typeof e === "string") { try { v = JSON.parse(e); } catch { return null; } }
  if (!v || typeof v !== "object") return null;
  const o = v as { kind?: unknown; state?: unknown; address?: unknown };
  return o.kind === "bank" && (o.state === "unconfirmed" || o.state === "failed") && typeof o.address === "number" ? (v as BankRefusal) : null;
}

/** The text of a bank refusal: what the pack is and where to confirm it; always says that nothing was written. */
export function bankText(b: BankRefusal, t: (k: string, o?: Record<string, unknown>) => string): string {
  const pack = String(b.address).padStart(2, "0");
  if (b.state === "failed") return b.error === "no-answer" ? t("bank.refused.silent", { pack }) : t("bank.refused.failed", { pack, err: b.error ?? "" });
  if (b.why === "foreign") return t("bank.refused.foreign", { pack, name: b.installation?.name ?? "" });
  return t(b.why === "moved" ? "bank.refused.moved" : "bank.refused.new", { pack });
}

/** The text of a failed write: a bank refusal as it is (it says that nothing was written), any other error through
 *  `wrap` (e.g. „Nicht geschrieben: …“). */
export const writeError = (e: unknown, t: (k: string, o?: Record<string, unknown>) => string, wrap: (err: string) => string) =>
  bankRefusal(e) ? errorText(e, t) : wrap(errorText(e, t));

/** An error as text: „Pack 00 ist gerade mit … beschäftigt“ for a reserved pack, the texts for a silent pack, a
 *  changed connection and a bank refusal, else as it came. */
export function errorText(e: unknown, t: (k: string, o?: Record<string, unknown>) => string): string {
  const bank = bankRefusal(e);
  if (bank) return bankText(bank, t);
  if (noAnswer(e)) return t("ctl.silent");
  if (linkChanged(e)) return t("ctl.linkChanged");
  const b = packBusy(e);
  if (!b) return String(e);
  return t("ctl.busy", { pack: String(b.address).padStart(2, "0"), action: t(`ctl.action.${b.action}`) });
}
