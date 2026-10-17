import { describe, expect, it } from "vitest";
import type { CellAnalysis, CellResult, HealthReport, PackDay, PairDay } from "../api";
import { localDate, packRows, pairRows, periodRange, reportCsv, trend } from "./report";

const DAY = 86_400_000;
const day0 = new Date(2030, 0, 1).getTime();
const pd = (i: number, o: Partial<PackDay> = {}): PackDay => ({
  day: day0 + i * DAY, rows: 100, spread_p50: 20, spread_p95: 30, spread_raw: true, rest_rows: 50, rest_p50: 8, rest_p95: 10, cycles: 100 + i, temp_max: 25, ...o,
});
const pairDay = (i: number, o: Partial<PairDay> = {}): PairDay => ({
  day: day0 + i * DAY, samples: 100, share: 50, equalizing: 0, judged: 100, judged_minutes: 60, judged_share: 50, marked: false, ...o,
});
const report = (o: Partial<HealthReport>): HealthReport => ({ from: day0, to: day0 + 10 * DAY, packs: [], pairs: [], spread_from: day0, raw_from: day0, ...o });
// a rate over 30 days of counts: the increase follows from it
const pack = (n: number, rate: number | null, days: PackDay[], resets = 0, span = 30 * DAY) =>
  ({ pack: n, days, cycles_per_30d: rate, cycle_resets: resets, cycles_gained: rate === null ? null : Math.round(rate * span / (30 * DAY)), cycles_span_ms: rate === null ? null : span });

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
        pack(4, 10, flat),
      ],
    });
    const rows = packRows(r);
    expect(rows[2].flags).toEqual([{ key: "cycles", pct: 30 }]);
    expect(rows[0].flags).toEqual([]);
    // 25 mV at rest against a median of 10 mV, rising by 15 mV, 8 K warmer
    expect(rows[3].flags).toEqual([{ key: "spread", mv: 25, median: 10 }, { key: "spreadRising", mv: 15 }, { key: "warm", c: 33, median: 25 }]);
    expect(rows[3]).toMatchObject({ restP50: 8, restP95: 25, loadP95: 30, cycles: 103, rate: 10, tempMax: 33 });
  });

  it("cycles: no flag from the counter's resolution, a cleared counter or different windows", () => {
    const flat = [0, 1].map((i) => pd(i));
    const flags = (packs: ReturnType<typeof pack>[]) => packRows(report({ packs })).map((x) => x.flags);
    // the median gained 6 cycles in 30 days: 8 against 6 is +33 %, but too few counts to say
    expect(flags([pack(0, 6, flat), pack(1, 6, flat), pack(2, 8, flat)])).toEqual([[], [], []]);
    // the median gained 8, the pack 10: +25 % is not above 25 %; at 11 it is, and 3 whole cycles more
    expect(flags([pack(0, 8, flat), pack(1, 8, flat), pack(2, 10, flat)])[2]).toEqual([]);
    expect(flags([pack(0, 8, flat), pack(1, 8, flat), pack(2, 11, flat)])[2]).toEqual([{ key: "cycles", pct: 38 }]);
    // over 6 days a median of 1 step and a pack of 2 would read as +100 %: the rate is not even shown
    const short = { ...pack(2, null, flat), cycles_gained: 2, cycles_span_ms: 6 * DAY };
    const rows = packRows(report({ packs: [{ ...pack(0, null, flat), cycles_gained: 1, cycles_span_ms: 6 * DAY }, short] }));
    expect(rows.map((x) => [x.rate, x.rateShort, x.flags])).toEqual([[null, true, []], [null, true, []]]);
    // a cleared counter anywhere, or windows more than a day apart: no comparison
    expect(flags([pack(0, 8, flat), pack(1, 8, flat), pack(2, 11, flat, 1)])).toEqual([[], [], [{ key: "counterCleared", count: 1 }]]);
    expect(flags([pack(0, 8, flat), pack(1, 8, flat), pack(2, 11, flat, 0, 20 * DAY)])[2]).toEqual([]);
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
    const tail = ",".repeat(26);
    expect(lines[1]).toBe(`pack,7,,${localDate(day0)},100,20,30,50,,10,100,25,,,,,,,,,,,${tail}`);
    expect(lines[2]).toBe(`pack_period,7,,,,,,,,,,,12.35,0,12,30,,,,,,,${tail}`);
    expect(lines[3]).toBe(`pair,0,1,${localDate(day0)},,,,,,,,,,,,,100,50,0,100,60,50,1${tail}`);
    expect(localDate(day0)).toBe("2030-01-01");
  });

  it("adds the balancing bits per day and the cell analysis per pack", () => {
    const cell = (n: number, o: Partial<CellResult> = {}): CellResult => ({ cell: n, rest: 0.5, rest_rows: 300, load: 1, top: 2, bottom: -2, top_corr: 1, bottom_corr: -1,
      r_mohm: 0.5, r_pct: 100, steps: 12, order: 2.5, jumps: 0, class: "ok", reason: { key: "within" }, ...o });
    const a: CellAnalysis = { from: day0, to: day0 + DAY, cell_count: 2, rows: 900, incomplete: 3, minute_rows: 0, balancer_active: 10, balancer_unknown: 2, balancer: true,
      rest_rows: 300, top_rows: 40, bottom_rows: 0, load_rows: 200, bms_left_out: 7, rest_no_status: 0, steps: 12, r_median: 0.5, charges: 1, heat: [],
      cells: [cell(1), cell(2, { class: "capacity", reason: { key: "capacity", top: 31.25, bottom: -20 } })] };
    const csv = reportCsv(report({ packs: [pack(7, null, [pd(0, { bal_mask: 0b101, bal_rows: 5, bal_n: 20 }), pd(1)])] }), { 7: a });
    const [head, ...rest] = csv.trim().split("\n");
    const cols = head.split(",");
    const rows = rest.map((l) => Object.fromEntries(l.split(",").map((v, i) => [cols[i], v])));
    expect(rows[0]).toMatchObject({ kind: "pack", bal_status_rows: "20", bal_rows: "5", bal_pct: "25", bal_cells: "1 3" });
    expect(rows[1]).toMatchObject({ kind: "pack", bal_status_rows: "", bal_pct: "", bal_cells: "" });
    expect(rows[3]).toMatchObject({ kind: "cells_period", pack: "7", rows: "900", cell_count: "2", incomplete: "3", balancer_left_out: "12", bms_left_out: "7", steps: "12" });
    expect(rows[5]).toMatchObject({ kind: "cell", pack: "7", cell: "2", class: "capacity", reason: "capacity top=31.25 bottom=-20", top_mv: "2", top_corr_mv: "1", r_pct: "100" });
  });
});
