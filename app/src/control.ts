/** Pack control (0x45, 0x49 index 59): pure helpers for the „Steuern“ block of the Parameters view. */
import type { Charge, PackPatch, Status } from "./api";
import { CUTOFF_BIT, LOCK_BIT } from "./profile";
import { parseNumber } from "./calibration";
import { refusalOf } from "./wiring";
import { decode, kindOf } from "./errors";
import { accessHint } from "./access";

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

/** What the fresh read after an acknowledged „off“ shows: off (command bit set and discharging blocked), back on
 *  (charging current lifts „off“ again), or anything else (unclear). Only "off" is reported as done. */
export function offOutcome(s: Status): "off" | "on" | "unclear" {
  if (!commandedOff(s)) return "on";
  return dischargeFree(s) ? "unclear" : "off";
}

/** The confirmation „Aus“ gets, from Group0 read fresh right before it (`g0`, undefined: not readable). CAN socket
 *  (`mode` "power"): only with the pack cutoff proven on this connection (`cutoff`), bit 4 at 0 the stored lock, else the
 *  soft off. RS485-1/2: every pack (any may be a Full build, whatever the app knows of its patch), bit 4 at 0 the stored
 *  lock („falls das Pack EMU1101-EXT Full hat“ unless the proof names it), else the soft off. "unread": no command offered. */
export function offKind(mode: "full" | "power", cutoff: boolean, patch: PackPatch | null | undefined, g0: number | undefined): "cutoff" | "cutoffMaybe" | "soft" | "unread" {
  if (mode === "power" && !cutoff) return "soft";
  if (g0 === undefined) return "unread";
  if ((g0 & CUTOFF_BIT) !== 0) return "soft";
  return mode === "power" || patch?.caps.pack_cutoff === true ? "cutoff" : "cutoffMaybe";
}

/** What the fresh Group0 (`g0`, undefined: the 0x47 after the action failed) and status say about the pack cutoff after
 *  an ein/aus. Off: locked, or bit 4 on but no lock („notLocked“), or null (soft off, the usual outcome). On: still
 *  locked, released, or released but a protection holds the discharge MOSFET off. Never a release from the ACK alone. */
export function cutoffOutcome(on: boolean, g0: number | undefined, s: Status | null): "locked" | "notLocked" | "stillLocked" | "released" | "releasedHeld" | "unread" | null {
  if (g0 === undefined) return "unread";
  const locked = (g0 & LOCK_BIT) === 0;
  if (on) return locked ? "stillLocked" : s && !dischargeFree(s) ? "releasedHeld" : "released";
  return locked ? "locked" : (g0 & CUTOFF_BIT) === 0 ? "notLocked" : null;
}

/** What „Aus“ hands the backend: the extended off (bit 4 at 0) its confirmation was shown with; the backend compares it
 *  with a fresh 0x47 right before sending (`cutoff-changed`). */
export const offExpect = (g0: number | undefined): { expect?: boolean } => (g0 === undefined ? {} : { expect: (g0 & CUTOFF_BIT) === 0 });

/** The result line of „Aus“ where the pack may store a cutoff (`maybe`: RS485-1/2, only if it is a Full build): locked
 *  (never „Laden möglich“), Group0 not readable afterwards (the charge path unclear), or null for the usual soft-off
 *  texts. */
export function offCutoffText(cut: ReturnType<typeof cutoffOutcome>, maybe: boolean): string | null {
  if (cut === "locked") return maybe ? "ctl.cutoff.offDoneMaybe" : "ctl.cutoff.offDone";
  if (cut === "unread") return "ctl.cutoff.offUnread";
  return null;
}

/** A pack already off (soft off) without a lock, whose last Group0 shows the extended off on (bit 4 at 0, bit 5 at 1):
 *  „Ausschalten und sperren“ sends the addressed off again, which stores the lock in a Full build without switching on
 *  first. CAN socket only with the cutoff proven on this connection (`cutoff`); RS485-1/2 any pack (confirmed „falls
 *  EMU1101-EXT Full hat“ unless proven). */
export const relockable = (off: boolean, lock: ReturnType<typeof lockState>, can: boolean, cutoff: boolean, g0: number | undefined) =>
  off && lock !== "active" && (!can || cutoff) && g0 !== undefined && (g0 & CUTOFF_BIT) === 0 && (g0 & LOCK_BIT) !== 0;

/** „Ausschalten und sperren“ at a pack already off, from Group0 read fresh: the lock confirmation, or why nothing is
 *  sent (already locked, or bit 5 at 0 without a proof; the extended off is no longer on, a repeated soft off would only repeat the master's broadcast). */
export function relockAsk(kind: ReturnType<typeof offKind>, g0: number | undefined): "ask" | "locked" | "lockedMaybe" | "soft" | "unread" {
  if (kind === "unread" || g0 === undefined) return "unread";
  // bit 5 at 0 is a lock only with the proven cutoff; without (RS485-1/2) the raw finding
  if ((g0 & LOCK_BIT) === 0) return kind === "cutoff" ? "locked" : "lockedMaybe";
  return kind === "soft" ? "soft" : "ask";
}

/** Whether „Steuern“ shows the pack as off (the button offers „Ein“): the fresh status, or a lock this connection proved
 *  („active“); a possible lock (`maybeActive`) never. */
export const shownOff = (s: Status | null, lock: ReturnType<typeof lockState>) => (s ? commandedOff(s) : false) || lock === "active";

/** The lock line of the state: with the proven cutoff „aktiv“ or „keine“; on RS485-1/2 (`maybe`) only a set bit 5, as
 *  „aktiv, falls EMU1101-EXT Full“, an uncertainty that never drives the buttons; else nothing. */
export function lockState(cutoff: boolean, maybe: boolean, g0: number | undefined): "active" | "none" | "maybeActive" | null {
  if (g0 === undefined) return null;
  const locked = (g0 & LOCK_BIT) === 0;
  if (cutoff) return locked ? "active" : "none";
  return maybe && locked ? "maybeActive" : null;
}

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

/** Learned full capacity typed in Ah → 10 mAh for 0x49/3A; null when empty, not above 0 or too large. 0 is never
 *  offered: the firmware would take it (with no charge left) and show 0 Ah full at 100 %. */
export function learnedFullRaw(text: string): number | null {
  const ah = parseNumber(text);
  if (ah === null || ah <= 0) return null;
  const raw = Math.round(ah * 100);
  return raw > 0 && raw <= 0xffff ? raw : null;
}

/** The firmware takes a learned full capacity from the remaining capacity up (equal included); below it RTN 06. */
export const learnedFullAllowed = (raw: number | null, remainingAh: number | null | undefined) =>
  raw !== null && remainingAh !== null && remainingAh !== undefined && raw >= Math.round(remainingAh * 100);

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

/** Outcome of a restart (backend `RestartDone`): `charge` before and once back where the link reads it from the pack
 *  itself (RS485-1/2, or the master on the CAN socket); absent or null: not compared. */
export type RestartResult = ({ kind: "not_reachable" } | { kind: "back"; after_ms: number } | { kind: "no_interruption" } | { kind: "no_answer" }
  | { kind: "sent_link_lost"; error: string }) & { charge?: RestartCharge | null };
export interface RestartCharge { before: Charge | null; after: Charge | null; differs: boolean | null }

/** SOC, remaining and full capacity, cycles before → after (`ctl.charge.line`; a side not read: "–"). `fmt(v, digits)`
 *  formats a number in the UI language. */
export function chargeLine(before: Charge | null, after: Charge | null, fmt: (v: number, digits: number) => string, t: (k: string, o?: Record<string, unknown>) => string): string {
  const v = (c: Charge | null, f: (c: Charge) => string) => (c ? f(c) : "–");
  return t("ctl.charge.line", {
    soc: `${v(before, (c) => fmt(c.soc, 1))} → ${v(after, (c) => fmt(c.soc, 1))}`,
    rest: `${v(before, (c) => fmt(c.remaining_ah, 1))} → ${v(after, (c) => fmt(c.remaining_ah, 1))}`,
    full: `${v(before, (c) => fmt(c.full_ah, 1))} → ${v(after, (c) => fmt(c.full_ah, 1))}`,
    cycles: `${v(before, (c) => String(c.cycles))} → ${v(after, (c) => String(c.cycles))}`,
  });
}


/** The pack did not answer a control command (typically a master polling its slaves). */
export const noAnswer = (e: unknown) => kindOf(e) === "no-answer";
/** An ein/aus whose request left without a definite answer, on either port: "silent" (no answer) or "link" (the gateway
 *  closed or an I/O error after sending); open whether the pack switched. null: refused or failed before sending. */
export function switchUnclear(e: unknown): "silent" | "link" | null {
  const v: unknown = decode(e);
  if (!v || typeof v !== "object") return null;
  const o = v as { kind?: unknown; error?: unknown; link?: unknown };
  if (o.kind === "no-answer") return typeof o.link === "string" ? "link" : "silent";
  if (o.kind === "uart1") return o.error === "power-no-answer" ? "silent" : o.error === "power-lost" ? "link" : null;
  return null;
}

/** The connection changed during the action; nothing more was sent. */
export const linkChanged = (e: unknown) => kindOf(e) === "link-changed";

/** Another action holds the pack, from the backend's JSON error; null for any other error. */
export function packBusy(e: unknown): { address: number; action: string } | null {
  const v: unknown = decode(e);
  if (!v || typeof v !== "object") return null;
  const o = v as { kind?: unknown; address?: unknown; action?: unknown };
  return o.kind === "pack-busy" && typeof o.address === "number" && typeof o.action === "string" ? { address: o.address, action: o.action } : null;
}

/** A write refused before anything was sent because the pack's bank is not confirmed or could not be checked
 *  (backend `bank.rs`); null for any other error. */
export type BankRefusal = { state: "unconfirmed" | "failed"; address: number; why?: "new" | "moved" | "foreign" | "legacy" | "earlier"; installation?: { id: string; name: string } | null; error?: string };
export function bankRefusal(e: unknown): BankRefusal | null {
  const v: unknown = decode(e);
  if (!v || typeof v !== "object") return null;
  const o = v as { kind?: unknown; state?: unknown; address?: unknown };
  return o.kind === "bank" && (o.state === "unconfirmed" || o.state === "failed") && typeof o.address === "number" ? (v as BankRefusal) : null;
}

/** The text of a bank refusal: what the pack is and where to confirm it; always says that nothing was written. */
export function bankText(b: BankRefusal, t: (k: string, o?: Record<string, unknown>) => string): string {
  const pack = String(b.address).padStart(2, "0");
  if (b.state === "failed") {
    if (b.error === "no-answer") return t("bank.refused.silent", { pack });
    const reason = ["connection", "store", "no-installation"].includes(b.error ?? "") ? b.error! : "connection";
    return t("bank.refused.failed", { pack, err: t(`bank.reason.${reason}`) });
  }
  if (b.why === "foreign") return t("bank.refused.foreign", { pack, name: b.installation?.name ?? "" });
  if (b.why === "legacy" || b.why === "earlier") return t(`bank.refused.${b.why}`, { pack });
  return t(b.why === "moved" ? "bank.refused.moved" : "bank.refused.new", { pack });
}

/** An error of the CAN socket's parameter service (backend `uart1_refusal`); null for any other error. */
export type Uart1Error = { error: "no-answer" | "protocol" | "pending" | "other-master" | "no-identity" | "readback" | "unchecked" | "changed" | "duplicate" | "elsewhere"
  | "board-changed" | "no-board" | "power-unsupported" | "power-locked" | "rtc-failed" | "restart-not-saved" | "power-no-answer" | "power-lost" | "cutoff-off-failed" | "cutoff-on-failed" | "cutoff-changed"
  | "unsupported" | "bit-blocked" | "not-ready"
  | "ccl-bad-value" | "ccl-no-cap" | "ccl-no-answer" | "ccl-silent" | "ccl-malformed" | "ccl-read-refused" | "ccl-invalid" | "ccl-denied" | "ccl-persist" | "ccl-unknown"
  | "ccl-readback-differs" | "ccl-readback-failed";
  code?: number; address?: number; board?: number; at?: number | null; gen?: number; group?: number; bit?: number;
  /** the CCL step's save: what a read after the attempt found (null: that read failed) */
  now?: number | null };
export function uart1Error(e: unknown): Uart1Error | null {
  const v: unknown = decode(e);
  if (!v || typeof v !== "object") return null;
  const o = v as { kind?: unknown; error?: unknown; code?: unknown };
  return o.kind === "uart1" && typeof o.error === "string" ? (v as Uart1Error) : null;
}

/** An ein/aus of RS485-1/2 refused before sending or failed at the cutoff store (backend `switch_refusal`). */
export function switchError(e: unknown): { error: "cutoff-changed" | "cutoff-unread" | "cutoff-off-failed" | "cutoff-on-failed" } | null {
  const v: unknown = decode(e);
  if (!v || typeof v !== "object") return null;
  const o = v as { kind?: unknown; error?: unknown };
  return o.kind === "switch" && typeof o.error === "string" ? (v as { error: "cutoff-changed" }) : null;
}

/** The error name of an ein/aus refusal on either port (CAN socket `uart1`, RS485-1/2 `switch`). */
export const cutoffError = (e: unknown): string | undefined => uart1Error(e)?.error ?? switchError(e)?.error;

/** The text of a failed write: a bank refusal as it is (it says that nothing was written), any other error through
 *  `wrap` (e.g. „Nicht geschrieben: …“). */
export const writeError = (e: unknown, t: (k: string, o?: Record<string, unknown>) => string, wrap: (err: string) => string) =>
  bankRefusal(e) || uart1Error(e) || switchError(e) ? errorText(e, t) : wrap(errorText(e, t));

/** An error as text: „Pack 00 ist gerade mit … beschäftigt“ for a reserved pack, the texts for a silent pack, a
 *  changed connection and a bank refusal, else as it came. */
export function errorText(e: unknown, t: (k: string, o?: Record<string, unknown>) => string): string {
  // the bus rule sent nothing (`wiring.rs`), whatever action it stopped
  const r = refusalOf(e);
  if (r) return t(`wiring.refused.${r}`);
  const bank = bankRefusal(e);
  if (bank) return bankText(bank, t);
  const u = uart1Error(e);
  // RTN 05 from the pack itself (locked meanwhile, or data it does not take) reads apart from the app's own refusal
  if (u) {
    const text = t(`uart1.${u.error === "power-locked" && u.code !== undefined ? "power-refused" : u.error}`, { code: (u.code ?? 0).toString(16).toUpperCase().padStart(2, "0"), pack: String(u.address ?? 0).padStart(2, "0"), group: u.group, bit: u.bit });
    // a save of the CCL step names what the master holds after it (read once, never a second save)
    return u.now === undefined ? text : `${text} ${u.now === null ? t("uart1.ccl-now-unknown") : t("uart1.ccl-now", { now: u.now })}`;
  }
  // the same texts on both ports
  const sw = switchError(e);
  if (sw) return t(`uart1.${sw.error}`);
  if (noAnswer(e)) return t("ctl.silent");
  if (linkChanged(e)) return t("ctl.linkChanged");
  // the reads of the read-capability bits on a pack whose own 64/03 does not announce them (`diagnosis.rs`, `bms_memory.rs`)
  if (typeof e === "string" && e.startsWith("history-no-cap:")) return accessHint(t, "bmsh.canBus");
  if (typeof e === "string" && e.startsWith("no-cap:")) return accessHint(t, "rdiag.noCap");
  const b = packBusy(e);
  if (!b) return String(e);
  return t("ctl.busy", { pack: String(b.address).padStart(2, "0"), action: t(`ctl.action.${b.action}`) });
}
