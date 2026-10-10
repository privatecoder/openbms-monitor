import { useEffect, useState } from "react";

/**
 * Two packs wired in parallel with diagonal connection: the main + cable at `plus`, the main −
 * cable at `minus`, short bridges between them. The `plus` pack's current also crosses the minus
 * bridge, the `minus` pack's current the plus bridge.
 */
export interface Pair { plus: number; minus: number }

const keyFor = (site: string) => `pairs:${site}`;

function read(site: string): Pair[] {
  // Browser preview: ?pairs=1 pre-fills neighbouring pairs.
  const demo = new URLSearchParams(location.search).get("pairs");
  try {
    const stored = JSON.parse(localStorage.getItem(keyFor(site)) ?? "[]") as Pair[];
    if (stored.length || !demo) return stored;
  } catch { if (!demo) return []; }
  return neighbourPairs(Array.from({ length: 12 }, (_, i) => i));
}

/** Pairs of neighbouring packs: 00/01, 02/03, … (an odd last pack stays single). */
export const neighbourPairs = (packs: number[]): Pair[] => {
  const s = [...packs].sort((a, b) => a - b), out: Pair[] = [];
  for (let i = 0; i + 1 < s.length; i += 2) out.push({ plus: s[i], minus: s[i + 1] });
  return out;
};

/** Only pairs whose packs are both present, and no pack in two pairs. */
export function validPairs(pairs: Pair[], present: number[]): Pair[] {
  const used = new Set<number>();
  return pairs.filter((p) => {
    if (p.plus === p.minus || !present.includes(p.plus) || !present.includes(p.minus) || used.has(p.plus) || used.has(p.minus)) return false;
    used.add(p.plus); used.add(p.minus);
    return true;
  });
}

/** Pack pairs (diagonal wiring), stored per installation like the groups. */
export function usePairs(site: string) {
  const [pairs, setPairs] = useState<Pair[]>(() => read(site));
  useEffect(() => setPairs(read(site)), [site]);
  const save = (p: Pair[]) => {
    setPairs(p);
    try { localStorage.setItem(keyFor(site), JSON.stringify(p)); } catch { /* preview without storage */ }
  };
  return { pairs, save };
}

/**
 * judged: false while the load is too low for a statement (see PAIR_MIN_A).
 * equalizing: the smaller share is explained by the states of charge (lower SOC gives less when discharging,
 * higher SOC takes less when charging), so it is reported as equalizing instead of a weak pack.
 * gap: output (port) voltage of the first minus the second pack in V, see PORT_GAP_V.
 */
export interface PairCheck {
  pair: Pair; total: number; share: number; judged: boolean; lowTotal: boolean; weak?: number;
  equalizing?: { pack: number; soc: [number, number] }; gap?: number; gapHigh?: boolean;
  /** how the gap behaves over the current, once known (see gapTrend) */
  trend?: ReturnType<typeof gapTrend>;
}

/**
 * Both packs of a pair sit on the same bridges, so their output voltages differ only by the drop across
 * bridges and connectors (plus a few 10 mV of sensor offset). Logged 2026-10-10: 0.17–0.39 V with the bad
 * plug on pack 00's plus bridge, 0.01–0.04 V after re-plugging, under 0.05 V in all healthy pairs.
 */
export const PORT_GAP_V = 0.1;
/** SOC difference (points) from which a smaller share counts as equalizing. */
export const EQUALIZE_SOC = 3;

/**
 * The check needs this much current per pack (median over all packs). Packs at different states of charge
 * equalize through their bridges: logged 2026-10-10, pack 00 (70 %) fed pack 01 (61 %) with ~5 A while the
 * bank was idle. That offset is independent of the load and decides the split at low currents (at 10 A per
 * pack, 00 looked like the weak pack), so only a load well above it shows the resistances.
 */
export const PAIR_MIN_A = 20;

/**
 * Pair currents: a low total against the other pairs points at the shared main cables; one pack
 * carrying much less than its partner points at that pack's bridge (or the pack itself).
 * weak: the pack with less than 70 % of its partner's current (cleared again above 75 %).
 * lowTotal: total below 85 % of the median pair total (cleared above 90 %), but only when the stronger
 * pack is itself below normal; otherwise the weak pack alone explains the low total.
 * `prev` (the last result) provides the hysteresis so values near a threshold do not flicker.
 * Only judged when the median pack current is at least PAIR_MIN_A.
 */
export function checkPairs(
  pairs: Pair[], current: (a: number) => number | undefined, prev?: PairCheck[],
  soc?: (a: number) => number | undefined, port?: (a: number) => number | undefined,
): PairCheck[] {
  const last = new Map((prev ?? []).map((c) => [c.pair.plus, c]));
  const rows = pairs.flatMap((pair) => {
    const a = current(pair.plus), b = current(pair.minus);
    if (a === undefined || b === undefined) return [];
    return [{ pair, a, b, total: a + b }];
  });
  const totals = rows.map((r) => Math.abs(r.total)).sort((x, y) => x - y);
  const each = rows.flatMap((r) => [Math.abs(r.a), Math.abs(r.b)]).sort((x, y) => x - y);
  const loaded = each.length > 0 && each[each.length >> 1] >= PAIR_MIN_A;
  const m = totals.length ? (totals.length % 2 ? totals[totals.length >> 1] : (totals[totals.length / 2 - 1] + totals[totals.length / 2]) / 2) : NaN;
  return rows.map(({ pair, a, b, total }) => {
    const lo = Math.min(Math.abs(a), Math.abs(b)), hi = Math.max(Math.abs(a), Math.abs(b));
    const share = hi ? lo / hi : 1;
    const was = last.get(pair.plus);
    const pa = port?.(pair.plus), pb = port?.(pair.minus);
    const gap = pa !== undefined && pb !== undefined ? pa - pb : undefined;
    const gapHigh = gap !== undefined && Math.abs(gap) >= (was?.gapHigh ? PORT_GAP_V * 0.8 : PORT_GAP_V);
    if (!loaded) return { pair, total, share, judged: false, lowTotal: false, gap, gapHigh };
    const small = share < (was?.weak !== undefined || was?.equalizing ? 0.75 : 0.7) ? (Math.abs(a) < Math.abs(b) ? pair.plus : pair.minus) : undefined;
    const sa = soc?.(pair.plus), sb = soc?.(pair.minus);
    let weak = small, equalizing: PairCheck["equalizing"];
    if (small !== undefined && sa !== undefined && sb !== undefined && Math.abs(sa - sb) >= EQUALIZE_SOC) {
      const mine = small === pair.plus ? sa : sb, other = small === pair.plus ? sb : sa;
      // discharging: the emptier pack gives less; charging: the fuller pack takes less
      if ((total < 0 && mine < other) || (total > 0 && mine > other)) {
        weak = undefined;
        equalizing = { pack: small, soc: [sa, sb] };
      }
    }
    const below = totals.length > 1 && Math.abs(total) < m * (was?.lowTotal ? 0.9 : 0.85);
    // a strong partner at or above an average pack's share means the missing current is the weak pack's alone
    // also when the smaller share is only equalizing: the partner then still carries a normal share
    const lowTotal = below && !(small !== undefined && hi >= (m / 2) * 0.85);
    return { pair, total, share, judged: true, lowTotal, weak, equalizing, gap, gapHigh };
  });
}

/** Current range (A, mean per pack of the pair) the trend needs before it says anything. */
export const TREND_MIN_RANGE_A = 20;
/** Change of the gap over that range (V) below which the gap counts as constant. */
export const TREND_FLAT_V = 0.02;

/**
 * How a pair's output gap behaves over the current: a resistance in bridges or plugs makes it grow with
 * the current (and turn round between charging and discharging), a voltage measurement offset of one BMS
 * keeps it constant. Least-squares line gap = offset + slope × current over the samples (60 s means).
 * Logged 2026-10-10, pair 04/05: 40–47 mV from −24 A to +35 A, i.e. an offset.
 * undefined until the samples span TREND_MIN_RANGE_A.
 */
export function gapTrend(samples: { i: number; g: number }[]): { kind: "offset" | "resistance"; offset: number; milliohm: number } | undefined {
  if (samples.length < 10) return undefined;
  const is = samples.map((s) => s.i);
  const range = Math.max(...is) - Math.min(...is);
  if (range < TREND_MIN_RANGE_A) return undefined;
  const n = samples.length;
  const mi = is.reduce((a, b) => a + b, 0) / n, mg = samples.reduce((a, s) => a + s.g, 0) / n;
  let sxy = 0, sxx = 0;
  for (const s of samples) { sxy += (s.i - mi) * (s.g - mg); sxx += (s.i - mi) ** 2; }
  const slope = sxx ? sxy / sxx : 0;
  return { kind: Math.abs(slope) * range < TREND_FLAT_V ? "offset" : "resistance", offset: mg - slope * mi, milliohm: slope * 1000 };
}

/** Mean of each pack's current over the last `windowMs`, so the pair check follows the trend, not single polls. */
export function windowMean(samples: { t: number; i: number }[], now: number, windowMs: number): number | undefined {
  const s = samples.filter((x) => now - x.t <= windowMs);
  return s.length ? s.reduce((sum, x) => sum + x.i, 0) / s.length : undefined;
}
