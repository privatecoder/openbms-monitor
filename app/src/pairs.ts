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

export interface PairCheck { pair: Pair; total: number; share: number; lowTotal: boolean; weak?: number }

/** Below this current per pack, state-of-charge differences dominate the split and the check says nothing. */
export const PAIR_MIN_A = 8;

/**
 * Pair currents: a low total against the other pairs points at the shared main cables; one pack
 * carrying much less than its partner points at that pack's bridge (or the pack itself).
 * weak: the pack with less than 70 % of its partner's current (cleared again above 75 %).
 * lowTotal: total below 85 % of the median pair total (cleared above 90 %), but only when the stronger
 * pack is itself below normal; otherwise the weak pack alone explains the low total.
 * `prev` (the last result) provides the hysteresis so values near a threshold do not flicker.
 * Only judged when the stronger pack carries at least PAIR_MIN_A.
 */
export function checkPairs(pairs: Pair[], current: (a: number) => number | undefined, prev?: PairCheck[]): PairCheck[] {
  const last = new Map((prev ?? []).map((c) => [c.pair.plus, c]));
  const rows = pairs.flatMap((pair) => {
    const a = current(pair.plus), b = current(pair.minus);
    if (a === undefined || b === undefined) return [];
    return [{ pair, a, b, total: a + b }];
  });
  const totals = rows.map((r) => Math.abs(r.total)).sort((x, y) => x - y);
  const m = totals.length ? (totals.length % 2 ? totals[totals.length >> 1] : (totals[totals.length / 2 - 1] + totals[totals.length / 2]) / 2) : NaN;
  return rows.map(({ pair, a, b, total }) => {
    const lo = Math.min(Math.abs(a), Math.abs(b)), hi = Math.max(Math.abs(a), Math.abs(b));
    const share = hi ? lo / hi : 1;
    const was = last.get(pair.plus);
    if (hi < PAIR_MIN_A) return { pair, total, share, lowTotal: false };
    const weak = share < (was?.weak !== undefined ? 0.75 : 0.7) ? (Math.abs(a) < Math.abs(b) ? pair.plus : pair.minus) : undefined;
    const below = totals.length > 1 && Math.abs(total) < m * (was?.lowTotal ? 0.9 : 0.85);
    // a strong partner at or above an average pack's share means the missing current is the weak pack's alone
    const lowTotal = below && !(weak !== undefined && hi >= (m / 2) * 0.95);
    return { pair, total, share, lowTotal, weak };
  });
}

/** Mean of each pack's current over the last `windowMs`, so the pair check follows the trend, not single polls. */
export function windowMean(samples: { t: number; i: number }[], now: number, windowMs: number): number | undefined {
  const s = samples.filter((x) => now - x.t <= windowMs);
  return s.length ? s.reduce((sum, x) => sum + x.i, 0) / s.length : undefined;
}
