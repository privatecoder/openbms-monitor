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
  /** the newest read time and when it arrived here (local ms): the source's clock for the window */
  clock: null as { readAt: number; at: number } | null,
};

/** "Now" for the pair window, on the clock the read times come from: the newest read time plus the local time since it
 *  arrived. For the monitor's own bus that is the local clock; for the add-on's feed (`mqtt_source.rs`) the add-on
 *  host's, so a clock offset between the two machines neither empties nor stretches the window. */
export function windowNow(now = Date.now()): number {
  return live.clock ? live.clock.readAt + Math.max(0, now - live.clock.at) : now;
}

/** One pack answer: its telemetry at the time it was read (the receive time for a backend that sends none). */
export function addAnswer(u: PackUpdate, now = Date.now()) {
  if (!u.telemetry) return;
  const t = u.read_at ?? now;
  if (!live.clock || t >= live.clock.readAt) live.clock = { readAt: t, at: now };
  const list = (live.samples.get(u.address) ?? []).filter((x) => t - x.t <= PAIR_WINDOW_MS);
  list.push({ t, i: effectiveCurrent(u.telemetry), u: u.telemetry.port_voltage });
  live.samples.set(u.address, list);
}

export function resetLivePairs() {
  live.samples.clear();
  live.last = [];
  live.trend.clear();
  live.seen = "";
  live.clock = null;
}
