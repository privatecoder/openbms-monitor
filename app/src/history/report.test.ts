import { describe, expect, it } from "vitest";
import type { CellAnalysis, CellResult, HealthReport, PackDay, PairDay } from "../api";
import { localDate, packRows, pairRows, periodRange, reportCsv, trend } from "./report";

const DAY = 86_400_000;
const day0 = new Date(2030, 0, 1).getTime();
const pd = (i: number, o: Partial<PackDay> = {}): PackDay => ({
  day: day0 + i * DAY, rows: 100, t_first: day0 + i * DAY + 3_600_000, t_last: day0 + i * DAY + 82_800_000, spread_p50: 20, spread_p95: 30, spread_raw: true, rest_rows: 200, rest_hours: 10, rest_p50: 8, rest_p95: 10, cycles: 100 + i, temp_max: 25, ...o,
});
const pairDay = (i: number, o: Partial<PairDay> = {}): PairDay => ({
  day: day0 + i * DAY, samples: 100, minutes: 60, share: 50, equalizing: 0, marked: false, ...o,
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

  it("compares a pack with the others' median: with two packs the partner", () => {
    const rows = (a: Partial<PackDay>, b: Partial<PackDay>) => packRows(report({ packs: [pack(0, null, [pd(0, a)]), pack(1, null, [pd(0, b)])] }));
    // [10, 25] mV: against a median of 17.5 including itself 25 would pass, against the partner's 10 it does not
    expect(rows({ rest_p95: 10 }, { rest_p95: 25 }).map((x) => x.flags)).toEqual([[], [{ key: "spread", mv: 25, median: 10 }]]);
    // [25, 32] °C: 7 K above the partner
    expect(rows({ temp_max: 25 }, { temp_max: 32 })[1].flags).toEqual([{ key: "warm", c: 32, median: 25 }]);
    // cycles: two packs at 8, one at 11 against the others' 8
    const flat = [pd(0)];
    expect(packRows(report({ packs: [pack(0, 8, flat), pack(1, 11, flat)] }))[1].flags).toEqual([{ key: "cycles", pct: 38 }]);
  });

  it("compares rest spreads only from the latest day with rest values and from enough rest rows", () => {
    const flags = (packs: ReturnType<typeof pack>[]) => packRows(report({ packs })).map((x) => x.flags);
    // the other pack's rest values are a week old: not the same moment, no comparison
    const stale = [pack(0, null, [pd(7, { rest_p95: 55 })]), pack(1, null, [pd(0, { rest_p95: 10 }), pd(7, { rest_p95: null, rest_p50: null, rest_rows: 0 })])];
    expect(flags(stale)).toEqual([[], []]);
    expect(packRows(report({ packs: stale }))[1].restP95).toBe(10);
    // two rest values that day are too few: shown, not flagged, and not in the others' median
    const thin = [pack(0, null, [pd(0, { rest_p95: 55, rest_rows: 2 })]), pack(1, null, [pd(0)]), pack(2, null, [pd(0, { rest_p95: 40, rest_rows: 2 })])];
    expect(flags(thin)).toEqual([[], [], []]);
    expect(flags([...thin, pack(3, null, [pd(0, { rest_p95: 30 })])])).toEqual([[], [], [], [{ key: "spread", mv: 30, median: 10 }]]);
    // minute maxima and raw rows of the same day are different measures: raw ones are compared
    const kinds = [pack(0, null, [pd(0, { rest_p95: 50, spread_raw: false })]), pack(1, null, [pd(0)]), pack(2, null, [pd(0, { rest_p95: 30 })])];
    expect(flags(kinds)).toEqual([[], [], [{ key: "spread", mv: 30, median: 10 }]]);
  });

  it("compares on the latest day two packs have enough rest values, else says not rated", () => {
    // yesterday 50 / 10 mV with enough rest values; today one rest value of pack 1 and none of pack 0
    const packs = [pack(0, null, [pd(0, { rest_p95: 50 }), pd(1, { rest_p95: null, rest_p50: null, rest_rows: 0 })]), pack(1, null, [pd(0), pd(1, { rest_rows: 1 })])];
    expect(packRows(report({ packs })).map((x) => [x.flags, x.unrated])).toEqual([[[{ key: "spread", mv: 50, median: 10 }], []], [[], []]]);
    // a pack missing on that day is not rated, not unremarkable
    const three = [...packs, pack(2, null, [pd(1, { rest_rows: 5 })])];
    expect(packRows(report({ packs: three }))[2].unrated).toEqual(["spreadPack"]);
    // a single pack: nothing to compare with
    expect(packRows(report({ packs: [pack(0, null, [pd(0)])] }))[0].unrated).toEqual(["spread", "warm"]);
  });

  it("compares the warmest cell only on days every pack recorded at least half the usual rows", () => {
    // pack 1 recorded only a short while that day: its maximum is no comparison for pack 0's whole day
    const rows = packRows(report({ packs: [pack(0, null, [pd(0, { rows: 100, temp_max: 35 })]), pack(1, null, [pd(0, { rows: 10, temp_max: 25 })])] }));
    expect(rows.map((x) => [x.flags, x.unrated])).toEqual([[[], ["warm"]], [[], ["warm"]]]);
    const ok = packRows(report({ packs: [pack(0, null, [pd(0, { rows: 100, temp_max: 35 })]), pack(1, null, [pd(0, { rows: 60, temp_max: 25 })])] }));
    expect(ok[0].flags).toEqual([{ key: "warm", c: 35, median: 25 }]);
  });

  it("compares the warmest cell only where the packs' recordings overlap, and never calls a pack without temperatures unremarkable", () => {
    const h = 3_600_000;
    // pack 0 recorded 06–07 h at 25 °C, pack 1 17–18 h at 35 °C: no common time, no comparison
    const apart = [pack(0, null, [pd(0, { t_first: day0 + 6 * h, t_last: day0 + 7 * h })]), pack(1, null, [pd(0, { temp_max: 35, t_first: day0 + 17 * h, t_last: day0 + 18 * h })])];
    expect(packRows(report({ packs: apart })).map((x) => [x.flags, x.unrated])).toEqual([[[], ["warm"]], [[], ["warm"]]]);
    // overlapping by half the shorter span: compared
    const half = [pack(0, null, [pd(0, { t_first: day0 + 6 * h, t_last: day0 + 8 * h })]), pack(1, null, [pd(0, { temp_max: 35, t_first: day0 + 7 * h, t_last: day0 + 18 * h })])];
    expect(packRows(report({ packs: half }))[1].flags).toEqual([{ key: "warm", c: 35, median: 25 }]);
    // a pack without any temperature: not rated, not unremarkable
    const none = [pack(0, null, [pd(0, { temp_max: null })]), pack(1, null, [pd(0)]), pack(2, null, [pd(0)])];
    expect(packRows(report({ packs: none }))[0].unrated).toEqual(["warm"]);
    expect(packRows(report({ packs: none }))[1].unrated).toEqual([]);
  });

  it("compares the warmest cell only over the days every pack has temperatures", () => {
    // pack 0 has a hot day nobody else recorded; over the shared days it is as warm as the other
    const a = [pd(0, { temp_max: 33 }), pd(1), pd(2)], b = [pd(1), pd(2)];
    const rows = packRows(report({ packs: [pack(0, null, a), pack(1, null, b)] }));
    expect(rows.map((x) => [x.tempMax, x.flags])).toEqual([[33, []], [25, []]]);
  });

  it("a rest trend only from well-covered days of one kind", () => {
    const rising = (o: Partial<PackDay>) => [0, 1, 2].map((i) => pd(i, { rest_p95: 10 + i * 5, ...o }));
    expect(packRows(report({ packs: [pack(0, null, rising({}))] }))[0].restTrend).toMatchObject({ dir: "up", change: 10 });
    // two rest values a day are no trend; single answers need 180 rest rows a day, minute maxima 30
    expect(packRows(report({ packs: [pack(0, null, rising({ rest_rows: 2 }))] }))[0].restTrend).toBeUndefined();
    expect(packRows(report({ packs: [pack(0, null, rising({ rest_rows: 50 }))] }))[0].restTrend).toBeUndefined();
    // enough rest rows, but all within one clock hour: a short excerpt, no trend
    expect(packRows(report({ packs: [pack(0, null, rising({ rest_hours: 1 }))] }))[0].restTrend).toBeUndefined();
    expect(packRows(report({ packs: [pack(0, null, rising({ rest_rows: 50, spread_raw: false }))] }))[0].restTrend).toMatchObject({ dir: "up" });
    // minute maxima before, raw rows after the change: only the raw days, never across
    const change = [pd(0, { rest_p95: 40, spread_raw: false }), pd(1, { rest_p95: 40, spread_raw: false }), pd(2), pd(3), pd(4)];
    expect(packRows(report({ packs: [pack(0, null, change)] }))[0].restTrend).toMatchObject({ dir: "flat", change: 0 });
    expect(packRows(report({ packs: [pack(0, null, change.slice(0, 4))] }))[0].restTrend).toBeUndefined();
  });

  it("no bank comparison with a single pack", () => {
    expect(packRows(report({ packs: [pack(0, 50, [pd(0, { rest_p95: 80 })])] }))[0].flags).toEqual([]);
  });

  it("names the pack that carries more on marked days", () => {
    const r = report({
      pairs: [
        { plus: 0, minus: 1, days: [pairDay(0), pairDay(1, { share: 58, marked: true })] },
        { plus: 2, minus: 3, days: [pairDay(0, { share: 42, marked: true })] },
        { plus: 4, minus: 5, days: [pairDay(0, { share: 60, equalizing: 100, marked: true })] },
      ],
    });
    const rows = pairRows(r);
    expect(rows[0]).toMatchObject({ share: 58, samples: 200, markedDays: 1, flag: { pack: 0, share: 58, days: 1 } });
    expect(rows[1].flag).toEqual({ pack: 3, share: 58, days: 1 });
    // samples the states of charge may explain still count; their number is kept as information
    expect(rows[2]).toMatchObject({ share: 60, markedDays: 1, equalizing: 100, flag: { pack: 4, share: 60, days: 1 } });
  });

  it("writes one CSV with packs, their period values and pairs", () => {
    const csv = reportCsv(report({ packs: [pack(7, 12.345, [pd(0, { rest_p50: null })])], pairs: [{ plus: 0, minus: 1, days: [pairDay(0, { marked: true })] }] }));
    const lines = csv.trim().split("\n");
    expect(lines[0].startsWith("kind,pack,partner,date,rows,")).toBe(true);
    const tail = ",".repeat(26);
    expect(lines[1]).toBe(`pack,7,,${localDate(day0)},100,20,30,1,200,,10,100,25,,,,,,,,,${tail}`);
    expect(lines[2]).toBe(`pack_period,7,,,,,,,,,,,,12.35,0,12,30,,,,,${tail}`);
    expect(lines[3]).toBe(`pair,0,1,${localDate(day0)},,,,,,,,,,,,,,100,60,50,0,1${tail}`);
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
    expect(rows[0]).toMatchObject({ kind: "pack", spread_raw: "1", bal_status_rows: "20", bal_rows: "5", bal_pct: "25", bal_cells: "1 3" });
    expect(rows[1]).toMatchObject({ kind: "pack", bal_status_rows: "", bal_pct: "", bal_cells: "" });
    expect(rows[3]).toMatchObject({ kind: "cells_period", pack: "7", rows: "900", cell_count: "2", incomplete: "3", balancer_left_out: "12", bms_left_out: "7", steps: "12" });
    expect(rows[5]).toMatchObject({ kind: "cell", pack: "7", cell: "2", class: "capacity", reason: "capacity top=31.25 bottom=-20", top_mv: "2", top_corr_mv: "1", r_pct: "100" });
  });
});
