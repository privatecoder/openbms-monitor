import type { TFunction } from "i18next";
import type { Bus, OtherBus } from "./api";

/** what a connect's check heard when no pack answered */
export type NoAnswer = { noise: boolean; intra: boolean };

const BAUD: Record<Bus, number> = { pack: 19200, can: 9600 };
const other = (b: Bus): Bus => (b === "pack" ? "can" : "pack");
const pad = (a: number) => String(a).padStart(2, "0");

/** the connect error `{ code: "no-answer", … }`, else null */
export function noAnswer(e: unknown): NoAnswer | null {
  try {
    const v = JSON.parse(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
    return v?.code === "no-answer" ? { noise: !!v.noise, intra: !!v.intra } : null;
  } catch { return null; }
}

/** the first sentence: what was heard on the chosen bus */
export function firstLine(bus: Bus, n: NoAnswer, t: TFunction): string {
  const p = { bus: t(`conn.busName.${bus}`), baud: BAUD[bus] };
  if (n.intra) return t("conn.check.intra");
  return n.noise ? t("conn.check.noise", p) : t("conn.check.none", p);
}

/** The message after a failed check, and the bus to offer when the other one answered. A USB port has been
 *  asked at the other baud rate (`probe`), a gateway cannot be (its baud rate is set in the gateway). */
export function explain(bus: Bus, n: NoAnswer, probe: OtherBus | null, t: TFunction): { text: string; offer: Bus | null } {
  const first = firstLine(bus, n, t);
  if (!probe) return { text: `${first} ${t("conn.check.hintTcp")}`, offer: null };
  const o = other(bus);
  if (probe.answered != null) {
    const addr = pad(probe.answered);
    // an answer at 9600 baud from a pack that knows 0x47 is RS485-1/2 of an old firmware, not the CAN socket
    if (probe.pack_at_9600) return { text: `${first} ${t("conn.check.pack9600", { addr })}`, offer: null };
    return { text: `${first} ${t(o === "can" ? "conn.check.otherCan" : "conn.check.otherPack", { addr })}`, offer: o };
  }
  const second = probe.noise ? t("conn.check.otherNoise", { baud: BAUD[o] }) : t("conn.check.otherNone", { baud: BAUD[o] });
  return { text: `${first} ${second} ${t("conn.check.hintSerial")}`, offer: null };
}

/** after the device query of a fixed pack count: which packs stayed silent (empty when all answered) */
export function partialLine(addrs: number[], found: number[], t: TFunction): string {
  const missing = addrs.filter((a) => !found.includes(a));
  if (!missing.length || !found.length) return "";
  return t("conn.partial", { n: addrs.length - missing.length, total: addrs.length, list: missing.map(pad).join(", ") });
}

export { BAUD, other as otherBus, pad as padAddress };
