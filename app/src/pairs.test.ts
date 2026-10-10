import { describe, expect, it } from "vitest";
import { checkPairs, neighbourPairs, validPairs, windowMean } from "./pairs";

describe("pairs", () => {
  it("builds neighbour pairs and drops invalid ones", () => {
    expect(neighbourPairs([0, 1, 2, 3, 4])).toEqual([{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }]);
    expect(validPairs([{ plus: 0, minus: 1 }, { plus: 1, minus: 2 }, { plus: 5, minus: 6 }], [0, 1, 2, 3])).toEqual([{ plus: 0, minus: 1 }]);
  });
  it("does not judge at 16 A per pack (2026-10-10 morning): equalizing currents dominate", () => {
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
  it("does not blame the main cables when the partner is strong (2026-10-10, 13:50)", () => {
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
  it("averages over the window", () => {
    expect(windowMean([{ t: 0, i: 100 }, { t: 50_000, i: 10 }, { t: 60_000, i: 20 }], 70_000, 60_000)).toBe(15);
    expect(windowMean([], 0, 60_000)).toBeUndefined();
  });
});
