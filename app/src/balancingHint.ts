/** Why a pack does not balance now, from its stored parameters (the newest stored block of the place), and what an
 *  assigned external balancer does by its manual settings. Both only explain the live line in the pack details; the
 *  recorded timeline computes the external balancer with its hysteresis in `balancer.rs`. */

import type { FeedState, SnapshotParams, Source } from "./api";
import type { BalancerValues } from "./devices";

/** The BMS's own balancing thresholds (V) and switches of one stored block. */
export interface BmsBalancing {
  /** parameter 8: the highest cell must reach it */
  start: number;
  /** parameter 62: the spread must exceed it */
  spread: number;
  /** function switches group 6 bit 0 (null: no switches in the block) */
  enabled: boolean | null;
  /** group 6 bit 1: also while idle */
  idle: boolean | null;
  /** when the block was stored (ms) */
  at: number;
}

const value = (s: SnapshotParams, i: number): number | null => {
  const v = s.parameters?.parameters.find((x) => x.index === i)?.value;
  return typeof v === "number" && Number.isFinite(v) ? v : null;
};

/** The thresholds of `address` from the stored blocks; none without a readable block of the current board. */
export function bmsBalancing(rows: SnapshotParams[], address: number): BmsBalancing | null {
  const s = rows.filter((r) => r.address === address && r.parameters && r.board !== "previous").sort((a, b) => b.at - a.at)[0];
  if (!s) return null;
  const start = value(s, 8), spread = value(s, 62);
  if (start === null || spread === null) return null;
  const g6 = s.parameters!.function_switches?.[6];
  const bit = (b: number) => (typeof g6 === "number" ? !!(g6 & (1 << b)) : null);
  return { start, spread, enabled: bit(0), idle: bit(1), at: s.at };
}

export type PackMode = "charging" | "discharging" | "idle";

/** The first reason the BMS does not balance; "met" when every threshold it shows is reached. */
export type BmsWhy = "discharging" | "disabled" | "idleOff" | "below" | "met";

export function bmsWhy(b: BmsBalancing, hi: number, lo: number, mode: PackMode): BmsWhy {
  if (mode === "discharging") return "discharging";
  if (b.enabled === false) return "disabled";
  if (mode === "idle" && b.idle === false) return "idleOff";
  return hi >= b.start && hi - lo > b.spread ? "met" : "below";
}

/** An external balancer now by its settings: on, off, or between its stop and start thresholds (then it keeps what it
 *  did before, which the live line does not know). Same rule as `balancer.rs` `single`. */
export type ExtNow = "on" | "off" | "between";

export function extNow(v: BalancerValues, hi: number, lo: number, current: number): ExtNow {
  const hiMv = Math.round(hi * 1000), spread = Math.round((hi - lo) * 1000);
  const when = v.when === "always" || (v.when === "charging" ? current > 1 : current >= -1);
  const cellStop = v.start_mv === null ? null : Math.min(v.stop_cell_mv ?? v.start_mv, v.start_mv);
  if (!when || (cellStop !== null && hiMv < cellStop)) return "off";
  if (spread >= v.trigger_mv && (v.start_mv === null || hiMv >= v.start_mv)) return "on";
  return spread < Math.min(v.stop_mv ?? v.trigger_mv, v.trigger_mv) ? "off" : "between";
}

/** The blocks the line explains with: the stored ones over the monitor's own bus; over the add-on's feed (MQTT) only
 *  the feed's blocks of this connection, since the feed cannot tell which board a stored block came from. */
export function balancingBlocks(source: Source | null, stored: SnapshotParams[], feed: FeedState | null): SnapshotParams[] {
  if (source !== "mqtt") return stored;
  return (feed?.packs ?? []).filter((p) => p.parameters && p.parameters_at !== null).map((p) => ({
    address: p.address, at: p.parameters_at!, kind: "read" as const, board: "current" as const, bms_serial: null, parameters: p.parameters,
  }));
}
