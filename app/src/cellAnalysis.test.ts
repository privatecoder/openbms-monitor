import { describe, expect, it } from "vitest";
import type { CellAnalysis, CellResult } from "./api";
import { HEAT_CLIP_MV, heatRgb, hexRgb, maskCells, reasonArgs, reasonCsv, reasonText, reportCells, signedMv } from "./cellAnalysis";

const cell = (n: number, o: Partial<CellResult> = {}): CellResult => ({ cell: n, rest: 0, rest_rows: 100, load: 0, top: 0, bottom: 0, top_corr: 0, bottom_corr: 0,
  r_mohm: 0.5, r_pct: 100, steps: 20, order: null, jumps: 0, class: "ok", reason: { key: "within" }, ...o });
const analysis = (cells: CellResult[]): CellAnalysis => ({ from: 0, to: 1, cell_count: cells.length, rows: 0, incomplete: 0, minute_rows: 0, balancer_active: 0,
  balancer_unknown: 0, balancer: false, rest_rows: 0, top_rows: 0, bottom_rows: 0, load_rows: 0, bms_left_out: 0, rest_no_status: 0, steps: 0, r_median: null,
  charges: 0, cells, heat: [] });

describe("cell analysis display", () => {
  it("colours deviations symmetrically and clips them", () => {
    const n: [number, number, number] = [200, 200, 200], lo: [number, number, number] = [0, 0, 255], hi: [number, number, number] = [255, 0, 0];
    expect(heatRgb(null, n, lo, hi)).toBeNull();
    expect(heatRgb(0, n, lo, hi)).toEqual(n);
    expect(heatRgb(HEAT_CLIP_MV / 2, n, lo, hi)).toEqual([228, 100, 100]);
    expect(heatRgb(-HEAT_CLIP_MV / 2, n, lo, hi)).toEqual([100, 100, 228]);
    expect(heatRgb(-500, n, lo, hi)).toEqual(lo);
    expect(heatRgb(80, n, lo, hi)).toEqual(hi);
    expect(hexRgb("#3f6fb5")).toEqual([63, 111, 181]);
    expect(hexRgb("blue")).toBeNull();
  });

  it("words values with their sign", () => {
    expect(signedMv(12.4, "en")).toBe("+12 mV");
    expect(signedMv(-3.26, "en")).toBe("−3.3 mV");
    expect(signedMv(0, "de")).toBe("±0 mV");
    expect(signedMv(null, "de")).toBe("–");
    expect(reasonArgs({ key: "capacity", top: 31, bottom: -20 }, "en")).toEqual(["cell.reason.capacity", { min: 30, minSteps: 10, top: "+31 mV", bottom: "−20 mV" }]);
    expect(reasonArgs({ key: "resistance", pct: 151.6, steps: 40 }, "en")[1]).toMatchObject({ pct: "152 %", steps: 40 });
    expect(reasonCsv({ key: "ambiguous", top: 30.123, bottom: -30, steps: 4 })).toBe("ambiguous top=30.12 bottom=-30 steps=4");
    expect(reasonCsv({ key: "partial", top: true, bottom: false, r: false, steps: 3 })).toBe("partial top=true bottom=false r=false steps=3");
    // what is missing, worded in the reason
    const t = (k: string, v?: Record<string, unknown>) => `${k}${v?.missing ? `[${v.missing}]` : ""}${k === "cell.missing.r" ? `(${v?.steps})` : ""}`;
    expect(reasonText(t, { key: "partial", top: true, bottom: false, r: false, steps: 3 }, "en")).toBe("cell.reason.partial[cell.missing.bottom, cell.missing.r(3)]");
    expect(maskCells(0b1000_0101)).toBe("1 3 8");
    expect(maskCells(0)).toBe("");
    expect(maskCells(null)).toBe("");
  });

  it("names the cells not matching, one line when no cell has a statement for the same reason", () => {
    const off = cell(3, { class: "resistance", reason: { key: "resistance", pct: 150, steps: 20 } });
    expect(reportCells(analysis([cell(1), off, cell(4)]))).toEqual({ all: null, cells: [off] });
    expect(reportCells(analysis([cell(1), cell(2)]))).toEqual({ all: null, cells: [] });
    // matching as far as the rest values go is not noteworthy either
    expect(reportCells(analysis([cell(1), cell(2, { class: "rest_ok", reason: { key: "rest_only", rest: 1 } })]))).toEqual({ all: null, cells: [] });
    // an offset at rest is named (state of charge or measurement); rest and resistance fine without the ends is quiet
    const rest = cell(2, { class: "rest_offset", reason: { key: "rest", rest: 8 } });
    expect(reportCells(analysis([cell(1), rest])).cells).toEqual([rest]);
    expect(reportCells(analysis([cell(1), cell(2, { class: "rest_ok", reason: { key: "partial", top: false, bottom: false, r: true, steps: 20 } })])).cells).toEqual([]);
    const none = (n: number) => cell(n, { class: "none", reason: { key: "no_rest", rows: 3 } });
    expect(reportCells(analysis([none(1), none(2)]))).toEqual({ all: none(1), cells: [] });
    const mixed = [none(1), cell(2, { class: "none", reason: { key: "top_only", top: 20 } })];
    expect(reportCells(analysis(mixed)).cells).toHaveLength(2);
  });
});
