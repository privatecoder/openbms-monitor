/** The live pair check's memory, kept outside the views: the time-stamped answers of each pack (pairs.ts pairedMeans),
 *  the last result (hysteresis) and the gap trend per pair. Fed by every pack answer (store.ts), so opening a pack and
 *  coming back loses nothing; emptied when the connection ends. */

import { effectiveCurrent, type PackUpdate } from "./api";
import type { PackSample, PairCheck } from "./pairs";

/** answers kept per pack (ms): the window of the pair check */
export const PAIR_WINDOW_MS = 60_000;

export const live = {
  samples: new Map<number, PackSample[]>(),
  last: [] as PairCheck[],
  /** per pair (`plus-minus`): mean current per pack and output gap over time */
  trend: new Map<string, { t: number; i: number; g: number }[]>(),
  /** the answers the last result was computed from */
  seen: "",
};

/** One pack answer: its telemetry at the time it was read (the receive time for a backend that sends none). */
export function addAnswer(u: PackUpdate, now = Date.now()) {
  if (!u.telemetry) return;
  const t = u.read_at ?? now;
  const list = (live.samples.get(u.address) ?? []).filter((x) => t - x.t <= PAIR_WINDOW_MS);
  list.push({ t, i: effectiveCurrent(u.telemetry), u: u.telemetry.port_voltage });
  live.samples.set(u.address, list);
}

export function resetLivePairs() {
  live.samples.clear();
  live.last = [];
  live.trend.clear();
  live.seen = "";
}
