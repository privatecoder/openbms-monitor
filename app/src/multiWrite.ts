/** Writing one parameter change to several packs: what each pack's write came to, and the series that writes them
 *  one after another. Pure; the Parameters view supplies the writes. */

import type { ParamWrite } from "./api";
import { bankRefusal, linkChanged, packBusy } from "./control";
import { notTaken, rawsOf, switchDiff, type Blocker, type Raws } from "./paramWrite";

/** One pack's line in the result. `refused.live`: switch bits sent that are live but not stored after RTN 06. */
export type Outcome =
  | { kind: "written"; unanswered?: boolean }
  | { kind: "unclear"; error: string }
  | { kind: "notTaken"; params: number[]; switches: number[] }
  | { kind: "unread"; error: string }
  | { kind: "refused"; refusal: "refused" | "invalid" | "locked"; live: number }
  | { kind: "skipped"; why: "blocked"; blockers: Blocker[] }
  | { kind: "skipped"; why: "same" | "pending" | "stale" }
  | { kind: "skipped"; why: "bank" | "busy"; error: unknown }
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

/** A failed write: refused before sending (bank not confirmed, pack held by another action) is skipped; a pack that
 *  does not answer, also to the bank check (a polling master), is not reached; a changed connection ends the run. */
export function failure(e: unknown): Outcome {
  const bank = bankRefusal(e);
  if (bank) return bank.state === "failed" && bank.error === "no-answer" ? { kind: "unreached", error: e } : { kind: "skipped", why: "bank", error: e };
  if (packBusy(e)) return { kind: "skipped", why: "busy", error: e };
  if (linkChanged(e)) return { kind: "cancelled" };
  return { kind: "unreached", error: e };
}

/** A refusal that left switches live but not stored: the series waits at that pack until the user goes on. */
export const needsPause = (o: Outcome) => o.kind === "refused" && o.refusal === "refused" && o.live > 0;

/** How a series ended: all done, cancelled, the link changed, or stopped at a pack whose result is unclear. */
export type SeriesEnd = "done" | "cancelled" | "link" | "unclear";

/** Writes `addresses` one after another (never in parallel). Before each pack it stops when the link (installation
 *  and connection) is no longer the one it began on or the user cancelled; the packs left are reported as cancelled.
 *  After a refusal with live switches it waits for `pause` (true: go on); at a pack whose result is unclear it stops. */
export async function runSeries(addresses: number[], o: {
  link: () => string; cancelled: () => boolean;
  write: (address: number) => Promise<Outcome>;
  pause: (address: number) => Promise<boolean>;
  report: (address: number, outcome: Outcome) => void;
}): Promise<SeriesEnd> {
  const begun = o.link();
  for (const [n, a] of addresses.entries()) {
    const stop = o.link() !== begun ? "link" : o.cancelled() ? "cancelled" : null;
    if (stop) { for (const rest of addresses.slice(n)) o.report(rest, { kind: "cancelled" }); return stop; }
    const out = await o.write(a);
    o.report(a, out);
    if (out.kind === "unclear") {
      for (const rest of addresses.slice(n + 1)) o.report(rest, { kind: "cancelled" });
      return "unclear";
    }
    if (needsPause(out) && !(await o.pause(a))) {
      for (const rest of addresses.slice(n + 1)) o.report(rest, { kind: "cancelled" });
      return o.link() !== begun ? "link" : "cancelled";
    }
  }
  return "done";
}
