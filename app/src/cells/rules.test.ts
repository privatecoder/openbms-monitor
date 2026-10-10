import { describe, expect, it } from "vitest";
import cells from "../../../data/cells/cells.json";
import { allowedChargeRate, chargeIntervals, effectiveChargeRange, missingRequired, toAmps } from "./rules";
import { REQUIRED_FOR_CHECKS, type Cell } from "./types";

const db = cells as unknown as Cell[];
const byId = (id: string) => db.find((c) => c.id === id)!;

describe("allowedChargeRate (lower value between points)", () => {
  const lf280kC = byId("eve-lf280k-pbri-rev-c-v3");
  it("uses the printed value on a point", () => {
    expect(allowedChargeRate(lf280kC, 0)).toBe(0.03);
    expect(allowedChargeRate(lf280kC, 5)).toBe(0.12);
  });
  it("uses the lower neighbour between points", () => {
    expect(allowedChargeRate(lf280kC, 3)).toBe(0.03);
    expect(allowedChargeRate(lf280kC, 47)).toBe(0.5);
  });
  it("does not allow charging outside the table", () => {
    expect(allowedChargeRate(lf280kC, -1)).toBe(0);
    expect(allowedChargeRate(lf280kC, 60)).toBe(0);
    expect(allowedChargeRate(lf280kC, 61)).toBe(0);
  });
  it("takes the lower value on shared band bounds", () => {
    const env = byId("envision-aesc-hc-l315a-rev-1-0");
    expect(allowedChargeRate(env, 15)).toBe(0.1);
    expect(allowedChargeRate(env, -5)).toBe(0);
    expect(allowedChargeRate(env, 70)).toBe(0);
  });
  it("applies the rule along SOC columns and uses the worst SOC when SOC is unknown", () => {
    const lf304 = byId("eve-lf304-pbri-rev-e-2023");
    const atKnown = allowedChargeRate(lf304, 25, 15)!;
    const at10 = allowedChargeRate(lf304, 25, 10)!, at20 = allowedChargeRate(lf304, 25, 20)!;
    expect(atKnown).toBe(Math.min(at10, at20));
    expect(allowedChargeRate(lf304, 25)).toBeLessThanOrEqual(atKnown);
  });
  it("returns undefined without a table", () => {
    expect(allowedChargeRate(byId("eve-lf280k-rev-b-2021"), 25)).toBeUndefined();
  });
});

describe("effectiveChargeRange", () => {
  it("matches the documented ranges", () => {
    expect(effectiveChargeRange(byId("eve-lf105-pbri-rev-d"))).toMatchObject({ min: 10, max: 55 });
    expect(effectiveChargeRange(byId("eve-lf304-pbri-rev-e-2023"))).toMatchObject({ min: -5, max: 60 });
    expect(effectiveChargeRange(byId("eve-lf280k-pbri-rev-c-v3"))).toMatchObject({ min: 0, max: 55 });
  });
});

describe("toAmps and required values", () => {
  it("converts C and P", () => {
    const c = byId("eve-lf280k-rev-b-2021");
    expect(toAmps(c, 0.5, "C")).toBe(140);
    expect(toAmps(byId("eve-lf280k-pbri-rev-c-v3"), 1, "P")).toBeCloseTo(280, 0);
  });
  it("finds no missing required values in the bundled set", () => {
    for (const c of db) expect([c.id, missingRequired(c, REQUIRED_FOR_CHECKS)]).toEqual([c.id, []]);
  });
});

const mk = (points: Cell["charge_derating"] extends infer D ? D extends { points: infer P } ? P : never : never, linear = false): Cell =>
  ({ ...byId("eve-lf280k-pbri-rev-c-v3"), charge_derating: { basis: "C", points, ...(linear ? { interpolation: "linear" as const } : {}) } });

describe("review edge cases", () => {
  it("keeps rows without SOC bounds for every SOC (lower value wins)", () => {
    const c = mk([{ temp_c: 25, value: 0.1 }, { temp_c: 25, soc_min_pct: 0, soc_max_pct: 50, value: 0.5 }]);
    expect(allowedChargeRate(c, 25, 25)).toBe(0.1);
    expect(allowedChargeRate(c, 25, 75)).toBe(0.1);
  });
  it("interpolates linear tables without SOC columns when SOC is known", () => {
    const c = mk([{ temp_c: 0, value: 0.2 }, { temp_c: 10, value: 0.8 }], true);
    expect(allowedChargeRate(c, 5, 50)).toBeCloseTo(0.5);
    expect(effectiveChargeRange(c)).toMatchObject({ max: 10 });
  });
  it("honours temperature bands in linear tables", () => {
    expect(allowedChargeRate(mk([{ temp_min_c: 0, temp_max_c: 10, value: 0.4 }], true), 5, 50)).toBe(0.4);
  });
  it("finds narrow bands and open ends", () => {
    expect(effectiveChargeRange(mk([{ temp_min_c: 0.1, temp_max_c: 0.2, value: 0.3 }]))).toEqual({ min: 0.1, max: 0.2 });
    expect(effectiveChargeRange(mk([{ temp_c: 0, soc_min_pct: 10.2, soc_max_pct: 10.8, value: 0.3 }]))).toMatchObject({ min: 0, max: 0 });
    expect(chargeIntervals(mk([{ temp_max_c: 0, value: 0 }, { temp_min_c: 0, value: 0.5 }]))).toEqual([{ min: 0, max: 0, minExcl: true, maxExcl: false, openLow: false, openHigh: true }]);
  });
  it("reports the exact start of a linear segment and gaps between bands", () => {
    const lin = chargeIntervals(mk([{ temp_c: 0, value: 0 }, { temp_c: 10, value: 1 }], true))!;
    expect(lin).toHaveLength(1);
    expect(lin[0]).toMatchObject({ min: 0, minExcl: true, max: 10, maxExcl: false });
    expect(chargeIntervals(mk([{ temp_min_c: 0, temp_max_c: 10, value: 0.5 }, { temp_min_c: 30, temp_max_c: 40, value: 0.5 }]))!.map((i) => [i.min, i.max]))
      .toEqual([[0, 10], [30, 40]]);
  });
  it("marks a shared bound with 0 on one side as excluded (Envision: above 0 °C)", () => {
    expect(chargeIntervals(byId("envision-aesc-hc-l315a-rev-1-0"))![0]).toMatchObject({ min: 0, minExcl: true });
  });
});
