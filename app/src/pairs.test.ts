import { describe, expect, it } from "vitest";
import { checkPairs, neighbourPairs, validPairs, windowMean } from "./pairs";

describe("pairs", () => {
  it("builds neighbour pairs and drops invalid ones", () => {
    expect(neighbourPairs([0, 1, 2, 3, 4])).toEqual([{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }]);
    expect(validPairs([{ plus: 0, minus: 1 }, { plus: 1, minus: 2 }, { plus: 5, minus: 6 }], [0, 1, 2, 3])).toEqual([{ plus: 0, minus: 1 }]);
  });
  it("flags the screenshot situation of 2026-10-10", () => {
    const I: Record<number, number> = { 0: 16.65, 1: 6.53, 2: 15.49, 3: 16.0, 4: 9.5, 5: 12.96, 6: 13.08, 7: 15.37, 8: 15.68, 9: 16.18, 10: 16.2, 11: 15.93 };
    const res = checkPairs(neighbourPairs(Object.keys(I).map(Number)), (a) => I[a]);
    const byPlus = Object.fromEntries(res.map((r) => [r.pair.plus, r]));
    expect(byPlus[0]).toMatchObject({ lowTotal: false, weak: 1 }); // 00 carries a normal share: only 01 is short
    expect(byPlus[4].lowTotal).toBe(true);
    expect(byPlus[4].weak).toBeUndefined(); // 9.5 vs 12.96 is 73 %, below the 70 % line
    expect(byPlus[2]).toMatchObject({ lowTotal: false, weak: undefined });
    expect(byPlus[8]).toMatchObject({ lowTotal: false, weak: undefined });
  });
  it("does not blame the main cables when the partner is strong (2026-10-10, 13:50)", () => {
    const I: Record<number, number> = { 0: 44.83, 1: 25.03, 2: 36.3, 3: 35.0, 4: 36.5, 5: 38.6, 6: 36.2, 7: 37.1, 8: 37.3, 9: 37.9, 10: 38.0, 11: 39.8 };
    const res = checkPairs(neighbourPairs(Object.keys(I).map(Number)), (a) => I[a]);
    expect(res[0]).toMatchObject({ lowTotal: false, weak: 1 });
  });
  it("keeps a note until the value clearly recovers and skips low currents", () => {
    const pairs = [{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }];
    const at = (x: number) => (a: number) => ({ 0: 20, 1: x, 2: 20, 3: 20 } as Record<number, number>)[a];
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
