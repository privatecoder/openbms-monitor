import { describe, expect, it } from "vitest";
import { checkPairs, equalizes, gapTrend, neighbourPairs, offsetMaxMilliohm, pairedMeans, validPairs } from "./pairs";
import vectors from "./equalize.vectors.json";

describe("pairs", () => {
  it("builds neighbour pairs and drops invalid ones", () => {
    expect(neighbourPairs([0, 1, 2, 3, 4])).toEqual([{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }]);
    expect(validPairs([{ plus: 0, minus: 1 }, { plus: 1, minus: 2 }, { plus: 5, minus: 6 }], [0, 1, 2, 3])).toEqual([{ plus: 0, minus: 1 }]);
  });
  it("does not judge at 16 A per pack: equalizing currents dominate", () => {
    const I: Record<number, number> = { 0: 16.65, 1: 6.53, 2: 15.49, 3: 16.0, 4: 9.5, 5: 12.96, 6: 13.08, 7: 15.37, 8: 15.68, 9: 16.18, 10: 16.2, 11: 15.93 };
    const res = checkPairs(neighbourPairs(Object.keys(I).map(Number)), (a) => I[a]);
    expect(res.every((r) => !r.lowTotal && r.weak === undefined)).toBe(true);
    expect(res[0].total).toBeCloseTo(23.18);
  });
  it("flags a low total when both packs of a pair are short", () => {
    const I: Record<number, number> = { 0: 30, 1: 30, 2: 40, 3: 40, 4: 40, 5: 40 };
    const res = checkPairs(neighbourPairs(Object.keys(I).map(Number)), (a) => I[a]);
    expect(res[0]).toMatchObject({ lowTotal: true, weak: undefined });
  });
  it("does not blame the main cables when the partner is strong", () => {
    const I: Record<number, number> = { 0: 44.83, 1: 25.03, 2: 36.3, 3: 35.0, 4: 36.5, 5: 38.6, 6: 36.2, 7: 37.1, 8: 37.3, 9: 37.9, 10: 38.0, 11: 39.8 };
    const res = checkPairs(neighbourPairs(Object.keys(I).map(Number)), (a) => I[a]);
    expect(res[0]).toMatchObject({ lowTotal: false, weak: 1 });
  });
  it("keeps a note until the value clearly recovers and skips low currents", () => {
    const pairs = [{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }];
    const at = (x: number) => (a: number) => ({ 0: 22, 1: x * 1.1, 2: 22, 3: 22 } as Record<number, number>)[a];
    const first = checkPairs(pairs, at(13)); // 65 %
    expect(first[0].weak).toBe(1);
    expect(checkPairs(pairs, at(14.6), first)[0].weak).toBe(1); // 73 %: still flagged
    expect(checkPairs(pairs, at(14.6))[0].weak).toBeUndefined(); // without history: not flagged
    expect(checkPairs(pairs, at(15.2), first)[0].weak).toBeUndefined(); // 76 %: cleared
    expect(checkPairs(pairs, (a) => ({ 0: 6, 1: 2, 2: 6, 3: 6 } as Record<number, number>)[a])[0].weak).toBeUndefined();
  });
  it("live figures only from time-paired answers in the window", () => {
    const s = (t: number, i: number, u = 53.5) => ({ t, i, u });
    // every 10 s, the partner 1 s later: six pairs in the minute
    const a = [0, 1, 2, 3, 4, 5].map((k) => s(k * 10_000, 30)), b = [0, 1, 2, 3, 4, 5].map((k) => s(k * 10_000 + 1_000, 20, 53.45));
    expect(pairedMeans(a, b, 55_000, 60_000)).toMatchObject({ a: 30, b: 20, n: 6 });
    expect(pairedMeans(a, b, 55_000, 60_000)!.ua - pairedMeans(a, b, 55_000, 60_000)!.ub).toBeCloseTo(0.05);
    // rounds of 12 s, the partner 11 s later in the polling order: within one round, paired
    const r12 = [0, 1, 2, 3, 4].map((k) => s(k * 12_000, 30)), late = [0, 1, 2, 3, 4].map((k) => s(k * 12_000 + 11_000, 20));
    expect(pairedMeans(r12, late, 59_000, 60_000)).toMatchObject({ a: 30, b: 20, n: 4 }); // one to one, closest first: 11/12 s, 23/24 s, …
    // one pack's last answer 55 s ago at 40 A, the other answering every 10 s at 20 A: not the same moment
    const fresh = [1, 2, 3, 4, 5, 6].map((k) => s(k * 10_000, 20));
    expect(pairedMeans([s(5_000, 40)], fresh, 60_000, 60_000)).toBeUndefined();
    // one answer of the minus pack is used once: three plus answers and one partner are one pair, not three
    const one = pairedMeans([s(0, 40), s(8_000, 40), s(16_000, 0)], [s(8_000, 40)], 20_000, 60_000);
    expect(one).toBeUndefined();
    // two pairs are too few
    expect(pairedMeans(a.slice(0, 2), b.slice(0, 2), 20_000, 60_000)).toBeUndefined();
  });
  it("takes the true median of an even count of pack currents", () => {
    // one pair at 35 A and 1 A: the median is 18 A, below the 20 A the check needs
    expect(checkPairs([{ plus: 0, minus: 1 }], (a) => (a === 0 ? 35 : 1))[0]).toMatchObject({ judged: false });
    expect(checkPairs([{ plus: 0, minus: 1 }], (a) => (a === 0 ? 35 : 10))[0]).toMatchObject({ judged: true, weak: 1 });
  });
  it("keeps the weak pack and names the states of charge beside it as a possible explanation", () => {
    const I: Record<number, number> = { 0: -19.7, 1: -6.3, 2: -21, 3: -20, 4: -21, 5: -22, 6: -21, 7: -21, 8: -21, 9: -22, 10: -22, 11: -23 };
    const soc: Record<number, number> = { 0: 74.4, 1: 67.6 };
    const res = checkPairs(neighbourPairs(Object.keys(I).map(Number)), (a) => I[a], undefined, (a) => soc[a] ?? 79);
    expect(res[0]).toMatchObject({ weak: 1, lowTotal: false, equalizing: { pack: 1, soc: [74.4, 67.6] } });
    // charging, the emptier pack taking less does not fit the states of charge
    const C = Object.fromEntries(Object.entries(I).map(([k, v]) => [k, -v]));
    expect(checkPairs(neighbourPairs(Object.keys(C).map(Number)), (a) => C[Number(a)], undefined, (a) => soc[a] ?? 79)[0]).toMatchObject({ weak: 1, equalizing: undefined });
  });
  it("does not judge currents against each other as load sharing", () => {
    // +100 A against −40 A: the median of the magnitudes would be 70 A, but the packs do not share a load
    const res = checkPairs([{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }], (a) => [100, -40, 30, 30][a]);
    expect(res[0]).toMatchObject({ opposite: true, judged: false, lowTotal: false });
    expect(res[0].weak).toBeUndefined();
    expect(res[1]).toMatchObject({ judged: true, weak: undefined });
    // a disconnected pack at 0 A next to a loaded partner is no counter-current: it is the weak one
    expect(checkPairs([{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }], (a) => [40, 0, 40, 40][a])[0]).toMatchObject({ judged: true, weak: 1 });
    expect(checkPairs([{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }], (a) => [40, 0, 40, 40][a])[0].opposite).toBeUndefined();
  });
  it("flags a large output voltage gap inside a pair, at any load", () => {
    const pairs = [{ plus: 0, minus: 1 }];
    const port = (gap: number) => (a: number) => (a === 0 ? 53.5 : 53.5 - gap);
    expect(checkPairs(pairs, () => 2, undefined, undefined, port(0.27))[0]).toMatchObject({ judged: false, gapHigh: true });
    expect(checkPairs(pairs, () => 2, undefined, undefined, port(0.02))[0].gapHigh).toBe(false);
  });
  it("no cable note for a pair whose low total is equalizing", () => {
    const I: Record<number, number> = { 0: -21.91, 1: -7.57, 2: -22.64, 3: -20.78, 4: -21.5, 5: -22, 6: -21.8, 7: -21.6, 8: -21.7, 9: -22.1, 10: -22, 11: -22.3 };
    const soc: Record<number, number> = { 0: 71.7, 1: 67.9 };
    const res = checkPairs(neighbourPairs(Object.keys(I).map(Number)), (a) => I[a], undefined, (a) => soc[a] ?? 74);
    expect(res[0]).toMatchObject({ lowTotal: false, weak: 1, equalizing: { pack: 1 } });
  });
});

describe("gapTrend", () => {
  const at = (f: (i: number) => number) => Array.from({ length: 30 }, (_, k) => { const i = -25 + k * 2; return { i, g: f(i) }; });
  it("calls a gap that stays the same over the current a measurement offset", () => {
    const t = gapTrend(at((i) => 0.042 + (i % 3) * 0.001))!;
    expect(t.kind).toBe("offset");
    expect(t.offset).toBeCloseTo(0.042, 2);
    // over −25 … +33 A a resistance below 20 mV / 58 A ≈ 0.34 mΩ stays hidden in the offset
    expect(t.range).toBe(58);
    expect(offsetMaxMilliohm(t.range)).toBeCloseTo(0.345, 3);
  });
  it("calls a gap that grows with the current a resistance", () => {
    const t = gapTrend(at((i) => 0.01 + i * 0.004))!;
    expect(t.kind).toBe("resistance");
    expect(t.milliohm).toBeCloseTo(4);
  });
  it("says nothing before the current has varied enough", () => {
    expect(gapTrend(Array.from({ length: 30 }, (_, k) => ({ i: 5 + (k % 10), g: 0.04 })))).toBeUndefined();
  });
});

describe("equalizes", () => {
  // the same vectors test the backend's copy of the rule (health.rs)
  it.each(vectors)("$case", ({ total, mine, other, equalizes: want }) => {
    expect(equalizes(total, mine, other)).toBe(want);
  });
});
