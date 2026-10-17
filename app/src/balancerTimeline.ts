import type { BalancerSlice } from "./api";

/** How a slice of the balancer timeline is drawn: the state shown and how strong (share of its recorded time). Activity
 *  wins over a longer quiet time in the same slice, so a short run stays visible. */
export type LaneMark = { state: "active" | "inactive" | "unknown" | "none" | "bal" | "idle" | "noStatus" | "gap"; share: number };

export function extMark(s: BalancerSlice): LaneMark {
  const covered = s.active + s.inactive + s.unknown + s.unassigned;
  if (!covered) return { state: "gap", share: 0 };
  if (s.active) return { state: "active", share: s.active / covered };
  const [state, ms] = ([["unknown", s.unknown], ["inactive", s.inactive], ["none", s.unassigned]] as const).reduce((a, b) => (b[1] > a[1] ? b : a));
  return { state, share: ms / covered };
}

export function bmsMark(s: BalancerSlice): LaneMark {
  const covered = s.bal + s.idle + s.no_status;
  if (!covered) return { state: "gap", share: 0 };
  if (s.bal) return { state: "bal", share: s.bal / covered };
  return s.idle >= s.no_status ? { state: "idle", share: s.idle / covered } : { state: "noStatus", share: s.no_status / covered };
}

/** The shares of the whole span (0..1, null where nothing counts): active and undecidable of the recorded time with a
 *  balancer assigned, the BMS balancing of the time with a status, and the time without one. */
export function shares(t: BalancerSlice) {
  const withBalancer = t.active + t.inactive + t.unknown, status = t.bal + t.idle, all = status + t.no_status;
  return {
    active: withBalancer ? t.active / withBalancer : null,
    unknown: withBalancer ? t.unknown / withBalancer : null,
    bal: status ? t.bal / status : null,
    noStatus: all ? t.no_status / all : null,
  };
}
