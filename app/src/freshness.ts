/** Freshness of live values per pack and of the system values: the store keeps the last values, but sums, extremes
 *  and the pack rows count only fresh ones. Fresh: the last poll for it succeeded and is at most two polling rounds
 *  old (the round as learned per pack), else at most STALE_FALLBACK_MS. Telemetry (0x42) and status (0x44) apart. */

import type { Status, SystemValues, Telemetry } from "./api";

export const STALE_FALLBACK_MS = 30_000;
/** Never below this, so a quick round does not flicker. */
const STALE_MIN_MS = 5_000;

/** The fields of a pack entry the freshness needs (store.ts `PackEntry`). */
export interface Timed {
  telemetry: Telemetry | null; status: Status | null;
  /** last successful 0x42 / 0x44 (ms) and whether the last poll of each succeeded */
  updated: number; statusUpdated: number; telemetryOk: boolean; statusOk: boolean;
  /** smoothed time between two answers: one polling round (ms) */
  interval?: number;
}

export function staleAfter(interval?: number): number {
  return interval ? Math.max(STALE_MIN_MS, 2 * interval) : STALE_FALLBACK_MS;
}

export function packFresh(e: Timed, now: number): { telemetry: boolean; status: boolean } {
  const limit = staleAfter(e.interval);
  return {
    telemetry: !!e.telemetry && e.telemetryOk && e.updated > 0 && now - e.updated <= limit,
    status: !!e.status && e.statusOk && e.statusUpdated > 0 && now - e.statusUpdated <= limit,
  };
}

/** The entry as the dashboard counts it: stale telemetry or status left out (null), with `old` saying which was. */
export function current<T extends Timed>(e: T, now: number): T & { old: { telemetry: boolean; status: boolean } } {
  const f = packFresh(e, now);
  return {
    ...e,
    telemetry: f.telemetry ? e.telemetry : null,
    status: f.status ? e.status : null,
    old: { telemetry: !!e.telemetry && !f.telemetry, status: !!e.status && !f.status },
  };
}

/** How many counted entries had their status left out as old (the dashboard's head says so beside the messages). */
export function omittedStatus(entries: { old: { status: boolean } }[]): number {
  return entries.filter((e) => e.old.status).length;
}

/** The system values (CAN socket, Modbus) as kept by the store: the last ones, whether the last poll succeeded, when. */
export interface SystemEntry { values: SystemValues | null; error: string | null; ok: boolean; updated: number }

/** The system values only while fresh; otherwise null (limits and permissions are unknown then). */
export function freshSystem(s: SystemEntry | null, now: number): SystemValues | null {
  return s?.values && s.ok && s.updated > 0 && now - s.updated <= STALE_FALLBACK_MS ? s.values : null;
}
