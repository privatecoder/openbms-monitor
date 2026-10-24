import type { TFunction } from "i18next";
import type { Bus, Installation, OtherBus } from "./api";
import { storedPort } from "./maint";

/** what a connect's check heard when no pack answered */
export type NoAnswer = { noise: boolean; intra: boolean; busy?: boolean };

const BAUD: Record<Bus, number> = { pack: 19200, can: 9600 };
const other = (b: Bus): Bus => (b === "pack" ? "can" : "pack");
const pad = (a: number) => String(a).padStart(2, "0");

/** the connect error `{ code: "no-answer", … }`, else null */
export function noAnswer(e: unknown): NoAnswer | null {
  try {
    const v = JSON.parse(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
    return v?.code === "no-answer" ? { noise: !!v.noise, intra: !!v.intra, busy: !!v.busy } : null;
  } catch { return null; }
}

/** the backend stopped connecting on „Abbrechen“ */
export function connectCancelled(e: unknown): boolean {
  try {
    return JSON.parse(typeof e === "string" ? e : e instanceof Error ? e.message : String(e))?.code === "cancelled";
  } catch { return false; }
}

/** the first sentence: what was heard on the chosen bus */
export function firstLine(bus: Bus, n: NoAnswer, t: TFunction): string {
  const p = { bus: t(`conn.busName.${bus}`), baud: BAUD[bus] };
  if (n.intra) return t("conn.check.intra");
  // traffic without a pause: requests were not even sent
  if (n.busy) return t("conn.check.busy", p);
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
  const second = probe.busy ? t("conn.check.otherBusy", { baud: BAUD[o] }) : probe.noise ? t("conn.check.otherNoise", { baud: BAUD[o] }) : t("conn.check.otherNone", { baud: BAUD[o] });
  return { text: `${first} ${second} ${t("conn.check.hintSerial")}`, offer: null };
}

/** after the device query of a fixed pack count: which packs stayed silent (empty when all answered) */
export function partialLine(addrs: number[], found: number[], t: TFunction): string {
  const missing = addrs.filter((a) => !found.includes(a));
  if (!missing.length || !found.length) return "";
  return t("conn.partial", { n: addrs.length - missing.length, total: addrs.length, list: missing.map(pad).join(", ") });
}

/** The packs the connect's device query missed: `expected` were asked, `missing` did not answer, `searched` what the
 *  search of 0..15 found afterwards (null: no search). */
export interface Gap { expected: number[]; missing: number[]; searched: number[] | null }

/** The connect's gap note against the packs answering now: only the addresses still silent are named, and the note is
 *  gone once all have answered (a pack slow at connect is polled on). */
export function gapLine(g: Gap, answering: number[], t: TFunction): string {
  const still = g.missing.filter((a) => !answering.includes(a));
  if (!still.length) return "";
  if (g.searched) {
    const silent = still.filter((a) => !g.searched!.includes(a));
    const silentNote = silent.length ? t("conn.expectedSilent", { count: silent.length, list: silent.map(pad).join(", ") }) : "";
    return [fixedSearchedLine(still, g.searched, t), silentNote].filter(Boolean).join(" ");
  }
  return still.length === g.expected.length ? t("conn.noAnswer") : partialLine(g.expected, g.expected.filter((a) => !still.includes(a)), t);
}

/** The gap without the packs that have answered by now: an answer ends a pack's gap for good, so a monitor stopped
 *  later (no fresh values at all) does not bring the note back; null once none is left. */
export function pruneGap(g: Gap, answering: number[]): Gap | null {
  const missing = g.missing.filter((a) => !answering.includes(a));
  if (!missing.length) return null;
  return missing.length === g.missing.length ? g : { ...g, missing };
}

/** A fixed pack count (0..n-1) or a fixed address (k) is polled without a search. Fall back to the normal search (0..15)
 *  when the connect check answered at an address outside them (decided before polling, `found` null); after the device
 *  query, a count searches as soon as any expected address is missing (a pack may sit outside 0..n-1), a fixed address
 *  only when it does not answer. Otherwise keep the fast path. */
export function fixedFallback(fixed: number[], answered: number, found: number[] | null, mode: "count" | "address" = "count"): "keep" | "search" {
  if (!fixed.includes(answered)) return "search";
  if (found === null) return "keep";
  const missing = fixed.filter((a) => !found.includes(a));
  if (mode === "count" ? missing.length > 0 : missing.length === fixed.length) return "search";
  return "keep";
}

/** After a fallback search: the expected addresses stay (a pack silent at connect is polled on, so its return shows),
 *  plus the packs found outside them; sorted, without doubles. */
export function mergeAfterSearch(expected: number[], found: number[]): number[] {
  return [...new Set([...expected, ...found])].sort((a, b) => a - b);
}

/** „Keine Antwort an Adresse 00–02; gesucht: Pack 01 gefunden“ (`missing`: the expected addresses that did not answer;
 *  `found`: what the search found). */
export function fixedSearchedLine(missing: number[], found: number[], t: TFunction): string {
  const contiguous = missing.every((a, i) => i === 0 || a === missing[i - 1] + 1);
  const range = missing.length > 1 && contiguous ? `${pad(missing[0])}–${pad(missing[missing.length - 1])}` : missing.length ? missing.map(pad).join(", ") : pad(0);
  const result = found.length ? t("conn.searchedFound", { count: found.length, list: found.map(pad).join(", ") }) : t("conn.searchedNone");
  return t("conn.fixedMissed", { range, result });
}

/** Which packs a connection polls: the normal search of 0..15, a count n (addresses 0..n-1), or exactly one pack at
 *  address k (no search). */
export type PackSel = { mode: "search" } | { mode: "count"; n: number } | { mode: "address"; k: number };

/** The addresses polled at once (null: search). */
export const selAddresses = (sel: PackSel): number[] | null =>
  sel.mode === "count" ? Array.from({ length: sel.n }, (_, i) => i) : sel.mode === "address" ? [sel.k] : null;

/** As stored: "search", "count:n", "address:k". */
export const selText = (sel: PackSel): string => (sel.mode === "count" ? `count:${sel.n}` : sel.mode === "address" ? `address:${sel.k}` : "search");

/** A stored selection, or the old stored pack count (0 → search, n → count n); anything else: search. */
export function parseSel(stored: string | null, legacyCount?: string | number | null): PackSel {
  const m = stored?.match(/^(count|address):(\d+)$/);
  if (m) {
    const v = Number(m[2]);
    if (m[1] === "count" && v >= 1 && v <= 16) return { mode: "count", n: v };
    if (m[1] === "address" && v >= 0 && v <= 15) return { mode: "address", k: v };
  }
  if (stored === "search") return { mode: "search" };
  const n = Number(legacyCount ?? 0);
  return Number.isInteger(n) && n >= 1 && n <= 16 ? { mode: "count", n } : { mode: "search" };
}

/** The last connection that worked for an installation, kept in this browser's storage (`packs`: the old pack count,
 *  read when `sel` is missing). */
export interface LastConn { kind: "tcp" | "serial"; target: string; bus: Bus; sel?: string; packs?: number }

const lastKey = (installation: string) => `conn.last.${installation}`;

export function saveLastConn(installation: string, c: LastConn) {
  try { localStorage.setItem(lastKey(installation), JSON.stringify(c)); } catch { /* storage full or off */ }
}

/** What `saveLastConn` stored, if it is still well-formed. */
export function lastConn(installation: string): LastConn | null {
  try {
    const v = JSON.parse(localStorage.getItem(lastKey(installation)) ?? "null");
    if (v && (v.kind === "tcp" || v.kind === "serial") && typeof v.target === "string" && v.target
      && (v.bus === "pack" || v.bus === "can") && (typeof v.sel === "string" || (Number.isInteger(v.packs) && v.packs >= 0 && v.packs <= 16))) return v;
  } catch { /* not JSON */ }
  return null;
}

/** The sidebar's connection fields after the installation (`on: "installation"`) or the bus changed. The installation's
 *  stored port for the chosen bus wins; the remembered last connection gives the bus and the pack count, and its
 *  address/port only where no port is stored for that bus. A new installation without a remembered connection whose
 *  current bus has no port takes its CAN socket. Without either: an empty USB field, so no port of another
 *  installation stays. null for „Nur ansehen“: the fields stay as they are. */
export function fillFields(inst: Pick<Installation, "control" | "endpoints"> | null, bus: Bus, last: LastConn | null, on: "installation" | "bus"): { kind: "tcp" | "serial"; target: string; bus: Bus; packs: PackSel | null } | null {
  if (!inst) return null;
  const remembered = on === "installation" ? last : null;
  let b = remembered?.bus ?? bus;
  let stored = storedPort(inst, b);
  if (on === "installation" && !last && !stored && inst.control) { b = "can"; stored = storedPort(inst, b); }
  const packs = remembered ? parseSel(remembered.sel ?? null, remembered.packs) : null;
  if (stored) return { ...stored, bus: b, packs };
  if (last && last.bus === b) return { kind: last.kind, target: last.target, bus: b, packs };
  return { kind: "serial", target: "", bus: b, packs };
}

export { BAUD, other as otherBus, pad as padAddress };

/** Before the chosen installation changes: a connect still running is cancelled and awaited (its own cancel path
 *  closes what it opened), then a live connection ends. Only then does the choice change. */
export async function endBeforeSwitch(o: { running: () => Promise<void> | null; cancel: () => void; live: () => boolean; disconnect: () => Promise<void> }): Promise<void> {
  const r = o.running();
  if (r) { o.cancel(); await r.catch(() => {}); }
  if (o.live()) await o.disconnect().catch(() => {});
}

/** A connect that returns for another installation than the one chosen now (the choice changed meanwhile) must not
 *  stay open. */
export const connectedForOther = (connInst: string | null, chosen: string | null) => connInst !== chosen;
