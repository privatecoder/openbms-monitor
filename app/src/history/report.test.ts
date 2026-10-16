import { describe, expect, it } from "vitest";
import type { HealthReport, PackDay, PairDay } from "../api";
import { localDate, packRows, pairRows, periodRange, reportCsv, trend } from "./report";

const DAY = 86_400_000;
const day0 = new Date(2030, 0, 1).getTime();
const pd = (i: number, o: Partial<PackDay> = {}): PackDay => ({
  day: day0 + i * DAY, rows: 100, spread_p50: 20, spread_p95: 30, rest_rows: 50, rest_p50: 8, rest_p95: 10, cycles: 100 + i, temp_max: 25, ...o,
});
const pairDay = (i: number, o: Partial<PairDay> = {}): PairDay => ({
  day: day0 + i * DAY, samples: 100, share: 50, equalizing: 0, judged: 100, judged_minutes: 60, judged_share: 50, marked: false, ...o,
});
const report = (o: Partial<HealthReport>): HealthReport => ({ from: day0, to: day0 + 10 * DAY, packs: [], pairs: [], spread_from: day0, raw_from: day0, ...o });
const pack = (n: number, rate: number | null, days: PackDay[], resets = 0) => ({ pack: n, days, cycles_per_30d: rate, cycle_resets: resets });

describe("health report", () => {
  it("periods end now; all starts with the recording", () => {
    expect(periodRange(7, 10 * DAY, 0)).toEqual([3 * DAY, 10 * DAY]);
    expect(periodRange("all", 10 * DAY, 2 * DAY)).toEqual([2 * DAY, 10 * DAY]);
  });

  it("trend over the days, flat below the threshold", () => {
    const up = trend([0, 1, 2, 3].map((i) => ({ day: i * DAY, v: 10 + i * 2 })), 3)!;
    expect(up.dir).toBe("up");
    expect(up.change).toBeCloseTo(6);
    expect(trend([0, 1, 2].map((i) => ({ day: i * DAY, v: 10 + (i % 2) })), 3)!.dir).toBe("flat");
    expect(trend([{ day: 0, v: 1 }, { day: DAY, v: null }, { day: 2 * DAY, v: 3 }], 1)).toBeUndefined();
  });

  it("flags a pack counting cycles faster than the median, a spread outlier, a rising spread and heat", () => {
    const flat = [0, 1, 2, 3].map((i) => pd(i));
    const r = report({
      packs: [
        pack(0, 10, flat), pack(1, 10, flat), pack(2, 13, flat),
        pack(3, 10, [0, 1, 2, 3].map((i) => pd(i, { rest_p95: 10 + i * 5, temp_max: i === 2 ? 33 : 25 }))),
        pack(4, 10, flat, 1),
      ],
    });
    const rows = packRows(r);
    expect(rows[2].flags).toEqual([{ key: "cycles", pct: 30 }]);
    expect(rows[0].flags).toEqual([]);
    // 25 mV at rest against a median of 10 mV, rising by 15 mV, 8 K warmer
    expect(rows[3].flags).toEqual([{ key: "spread", mv: 25, median: 10 }, { key: "spreadRising", mv: 15 }, { key: "warm", c: 33, median: 25 }]);
    expect(rows[4].flags).toEqual([{ key: "counterCleared", count: 1 }]);
    expect(rows[3]).toMatchObject({ restP50: 8, restP95: 25, loadP95: 30, cycles: 103, rate: 10, tempMax: 33 });
  });

  it("no bank comparison with a single pack", () => {
    expect(packRows(report({ packs: [pack(0, 50, [pd(0, { rest_p95: 80 })])] }))[0].flags).toEqual([]);
  });

  it("names the pack that carries more on marked days", () => {
    const r = report({
      pairs: [
        { plus: 0, minus: 1, days: [pairDay(0), pairDay(1, { judged_share: 58, share: 57, marked: true })] },
        { plus: 2, minus: 3, days: [pairDay(0, { judged_share: 42, marked: true })] },
        { plus: 4, minus: 5, days: [pairDay(0, { judged: 0, judged_share: null, share: 60, equalizing: 100 })] },
      ],
    });
    const rows = pairRows(r);
    expect(rows[0]).toMatchObject({ share: 58, samples: 200, markedDays: 1, flag: { pack: 0, share: 58, days: 1 } });
    expect(rows[1].flag).toEqual({ pack: 3, share: 58, days: 1 });
    // all samples explained by equalizing: shown, never flagged
    expect(rows[2]).toMatchObject({ share: 60, markedDays: 0, flag: undefined });
  });

  it("writes one CSV with packs, their period values and pairs", () => {
    const csv = reportCsv(report({ packs: [pack(7, 12.345, [pd(0, { rest_p50: null })])], pairs: [{ plus: 0, minus: 1, days: [pairDay(0, { marked: true })] }] }));
    const lines = csv.trim().split("\n");
    expect(lines[0].startsWith("kind,pack,partner,date,rows,")).toBe(true);
    expect(lines[1]).toBe(`pack,7,,${localDate(day0)},100,20,30,50,,10,100,25,,,,,,,,,`);
    expect(lines[2]).toBe("pack_period,7,,,,,,,,,,,12.35,0,,,,,,,");
    expect(lines[3]).toBe(`pair,0,1,${localDate(day0)},,,,,,,,,,,100,50,0,100,60,50,1`);
    expect(localDate(day0)).toBe("2030-01-01");
  });
});
