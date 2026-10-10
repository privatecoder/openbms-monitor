import { describe, expect, it } from "vitest";
import { checkPairs, neighbourPairs, validPairs } from "./pairs";

describe("pairs", () => {
  it("builds neighbour pairs and drops invalid ones", () => {
    expect(neighbourPairs([0, 1, 2, 3, 4])).toEqual([{ plus: 0, minus: 1 }, { plus: 2, minus: 3 }]);
    expect(validPairs([{ plus: 0, minus: 1 }, { plus: 1, minus: 2 }, { plus: 5, minus: 6 }], [0, 1, 2, 3])).toEqual([{ plus: 0, minus: 1 }]);
  });
  it("flags the screenshot situation of 2026-10-10", () => {
    const I: Record<number, number> = { 0: 16.65, 1: 6.53, 2: 15.49, 3: 16.0, 4: 9.5, 5: 12.96, 6: 13.08, 7: 15.37, 8: 15.68, 9: 16.18, 10: 16.2, 11: 15.93 };
    const res = checkPairs(neighbourPairs(Object.keys(I).map(Number)), (a) => I[a]);
    const byPlus = Object.fromEntries(res.map((r) => [r.pair.plus, r]));
    expect(byPlus[0]).toMatchObject({ lowTotal: true, weak: 1 });
    expect(byPlus[4].lowTotal).toBe(true);
    expect(byPlus[4].weak).toBeUndefined(); // 9.5 vs 12.96 is 73 %, below the 70 % line
    expect(byPlus[2]).toMatchObject({ lowTotal: false, weak: undefined });
    expect(byPlus[8]).toMatchObject({ lowTotal: false, weak: undefined });
  });
});
