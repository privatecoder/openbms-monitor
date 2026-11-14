/** Writing one parameter change to several packs: what each pack's write came to, and the series that writes them
 *  one after another. Pure; the Parameters view supplies the writes. */

import type { ParamWrite } from "./api";
import { bankRefusal, linkChanged, packBusy, uart1Error } from "./control";
import { notTaken, planTarget, rawsOf, switchDiff, type Blocker, type ParamChange, type Raws, type TargetPlan } from "./paramWrite";
import type { Parameters } from "./params";

/** One pack's line in the result. `refused.live`: switch bits sent that are live but not stored after RTN 06.
 *  `written.byRead`: a retry's fresh read already showed the change, nothing sent; `unread.fresh`: that read failed. */
export type Outcome =
  | { kind: "written"; unanswered?: boolean; byRead?: boolean }
  | { kind: "unclear"; error: string }
  | { kind: "notTaken"; params: number[]; switches: number[] }
  | { kind: "unread"; error: string; fresh?: boolean }
  | { kind: "refused"; refusal: "refused" | "invalid" | "locked"; live: number }
  | { kind: "skipped"; why: "blocked"; blockers: Blocker[] }
  | { kind: "skipped"; why: "same" | "pending" | "stale" }
  | { kind: "skipped"; why: "bank" | "busy" | "uart1"; error: unknown }
  | { kind: "unreached"; error: unknown }
  | { kind: "cancelled" };

/** A write's answer as a result line; `sent` is the block as sent (raws with signed currents, switch bytes). */
export function outcomeOf(r: ParamWrite, sent: Raws, sentSwitches: number[]): Outcome {
  if (r.kind === "stale") return { kind: "skipped", why: "stale" };
  if (r.kind === "unclear") return { kind: "unclear", error: r.error };
  // the CAN socket's E2 takes nothing, not even the switches: nothing is live, the series does not wait
  if (r.kind === "refused") return { kind: "refused", refusal: r.refusal, live: r.refusal === "refused" && r.switches_live !== false ? switchDiff(sentSwitches, r.stored.function_switches).length : 0 };
  if (!r.read_back) return { kind: "unread", error: r.read_error ?? "" };
  const missed = notTaken(sent, sentSwitches, rawsOf(r.read_back), r.read_back.function_switches);
  return missed.params.size + missed.switches.size ? { kind: "notTaken", params: [...missed.params], switches: [...missed.switches] }
    : r.unanswered ? { kind: "written", unanswered: true } : { kind: "written" };
}

/** A failed write: refused before sending (bank not confirmed, pack held by another action, a refusal of the CAN
 *  socket's service: an open interrupted update, the write lock, an unsupported build, another board, the pack's
 *  protocol error) is skipped; a pack that does not answer, also to the bank check (a polling master) or over the CAN
 *  socket, is not reached; an acknowledged CAN socket write whose read-back differs is unclear; a changed connection
 *  ends the run. */
export function failure(e: unknown): Outcome {
  const bank = bankRefusal(e);
  if (bank) return bank.state === "failed" && bank.error === "no-answer" ? { kind: "unreached", error: e } : { kind: "skipped", why: "bank", error: e };
  if (packBusy(e)) return { kind: "skipped", why: "busy", error: e };
  const u = uart1Error(e);
  if (u) return u.error === "no-answer" ? { kind: "unreached", error: e } : u.error === "readback" ? { kind: "unclear", error: String(e) } : { kind: "skipped", why: "uart1", error: e };
  if (linkChanged(e)) return { kind: "cancelled" };
  return { kind: "unreached", error: e };
}

/** A refusal that left switches live but not stored: the series waits at that pack until the user goes on. */
export const needsPause = (o: Outcome) => o.kind === "refused" && o.refusal === "refused" && o.live > 0;

/** Retries per pack: chosen in the run's confirmation, starting at the default every time. */
export const RETRIES = { initial: 2, max: 5 };
/** The pause before a retry, so a pack that just missed a frame has settled. */
export const RETRY_PAUSE_MS = 1000;

/** Worth another attempt: the write may not have landed or not been checked. Not retried: the pack's own refusal
 *  (resending changes nothing), a pack skipped for its bank, a held pack, a CAN socket refusal, blockers, the same
 *  values, a pending refusal, and a cancel. */
export const retried = (o: Outcome) =>
  o.kind === "notTaken" || o.kind === "unread" || o.kind === "unreached" || o.kind === "unclear" || (o.kind === "skipped" && o.why === "stale");

/** One pack's final line: its last outcome, how many attempts it took, and whether it was given up after the last
 *  allowed one (a pack stopped between attempts by a cancel or link change is not given up). */
export interface PackResult { out: Outcome; attempts: number; gaveUp: boolean }

/** A retry: a fresh read of the pack, the run's change planned on it again, and only what still differs written.
 *  The read already showing the change counts as written (`confirmed` gets that read); new blockers skip the pack; a
 *  failed read is a failed attempt, classified like a failed write. `stop`: cancelled or the link changed during the
 *  read, so nothing is written (the read may belong to another connection). */
export async function attemptAgain(address: number, change: ParamChange, seriesUnlocked: boolean, o: {
  read: (address: number) => Promise<Parameters>; send: (plan: TargetPlan) => Promise<Outcome>; stop: () => boolean;
  confirmed?: (read: Parameters) => void;
}): Promise<Outcome> {
  let read: Parameters;
  try { read = await o.read(address); } catch (e) {
    if (o.stop()) return { kind: "cancelled" };
    const f = failure(e);
    return f.kind === "unreached" ? { kind: "unread", error: typeof e === "string" ? e : String(e), fresh: true } : f;
  }
  if (o.stop()) return { kind: "cancelled" };
  const plan = planTarget(change, address, rawsOf(read), read.function_switches, seriesUnlocked);
  if (plan.blockers.length) return { kind: "skipped", why: "blocked", blockers: plan.blockers };
  if (!plan.changes.length) { o.confirmed?.(read); return { kind: "written", byRead: true }; }
  return o.send(plan);
}

/** The block before a CAN socket write whose answer stayed unclear, per pack: a later fresh read that shows the change
 *  confirms that write (its „Vorherigen Stand“ is this block). Any other attempt that sent or failed to send clears it,
 *  so a change found after an attempt that never left is not booked as the app's write. */
export function unclearWrites() {
  const before = new Map<number, Parameters>();
  return {
    sent: (address: number, r: ParamWrite | undefined) => { if (r?.kind === "unclear" && r.before) before.set(address, r.before); else before.delete(address); },
    take: (address: number) => { const b = before.get(address); before.delete(address); return b; },
  };
}

/** One pack: the first write, then up to `retries` more attempts while the outcome is worth one, each after `wait`.
 *  Before every retry (before and after the pause) `stop` ends it with the outcome so far. */
export async function writePack(o: {
  retries: number; first: () => Promise<Outcome>; again: () => Promise<Outcome>;
  stop: () => boolean; wait: () => Promise<void>; attempt?: (n: number) => void;
}): Promise<PackResult> {
  o.attempt?.(1);
  let out = await o.first(), n = 1;
  while (retried(out) && n <= o.retries) {
    if (o.stop()) return { out, attempts: n, gaveUp: false };
    await o.wait();
    if (o.stop()) return { out, attempts: n, gaveUp: false };
    n++;
    o.attempt?.(n);
    out = await o.again();
  }
  return { out, attempts: n, gaveUp: retried(out) };
}

/** How a series ended: all done, cancelled, or the link changed. */
export type SeriesEnd = "done" | "cancelled" | "link";

/** Writes `addresses` one after another (never in parallel), each with its retries (`writePack`). Before each pack
 *  and each retry it stops when the link (installation and connection) is no longer the one it began on or the user
 *  cancelled; the packs left are reported as cancelled. A pack given up after its retries is skipped and the series
 *  goes on. After a refusal with live switches it waits for `pause` (true: go on). */
export async function runSeries(addresses: number[], o: {
  link: () => string; cancelled: () => boolean; retries: number;
  first: (address: number) => Promise<Outcome>;
  again: (address: number) => Promise<Outcome>;
  wait: () => Promise<void>;
  pause: (address: number) => Promise<boolean>;
  report: (address: number, result: PackResult) => void;
  attempt?: (address: number, n: number) => void;
}): Promise<SeriesEnd> {
  const begun = o.link();
  const stopNow = () => (o.link() !== begun ? "link" : o.cancelled() ? "cancelled" : null);
  const rest = (from: number) => { for (const a of addresses.slice(from)) o.report(a, { out: { kind: "cancelled" }, attempts: 0, gaveUp: false }); };
  for (const [n, a] of addresses.entries()) {
    const stop = stopNow();
    if (stop) { rest(n); return stop; }
    const res = await writePack({
      retries: o.retries, first: () => o.first(a), again: () => o.again(a), stop: () => stopNow() !== null, wait: o.wait,
      attempt: (k) => o.attempt?.(a, k),
    });
    o.report(a, res);
    if (needsPause(res.out) && !(await o.pause(a))) {
      rest(n + 1);
      return o.link() !== begun ? "link" : "cancelled";
    }
  }
  // the last pack may have ended on a link change or cancel, too
  return stopNow() ?? "done";
}

/** The counts above the result table. `gaveUp`: skipped after all attempts (`tries` of them); `stopped`: a failed
 *  attempt the run did not retry because it ended; `skipped`: left out (before or during the run) for a reason. */
export interface Summary { written: number; gaveUp: number; tries: number; refused: number; skipped: number; stopped: number; cancelled: number }

export function summarize(rows: { out?: Outcome; attempts?: number; gaveUp?: boolean }[]): Summary {
  const s: Summary = { written: 0, gaveUp: 0, tries: 0, refused: 0, skipped: 0, stopped: 0, cancelled: 0 };
  for (const r of rows) {
    if (!r.out) continue;
    if (r.gaveUp) { s.gaveUp++; s.tries = Math.max(s.tries, r.attempts ?? 0); }
    else if (r.out.kind === "written") s.written++;
    else if (r.out.kind === "refused") s.refused++;
    else if (r.out.kind === "skipped" && r.out.why !== "stale") s.skipped++;
    else if (r.out.kind === "cancelled") s.cancelled++;
    else s.stopped++;
  }
  return s;
}
