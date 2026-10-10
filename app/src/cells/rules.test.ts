import { describe, expect, it } from "vitest";
import cells from "../../../data/cells/cells.json";
import { allowedChargeRate, effectiveChargeRange, missingRequired, toAmps } from "./rules";
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
    expect(effectiveChargeRange(byId("eve-lf105-pbri-rev-d"))).toEqual({ min: 10, max: 55 });
    expect(effectiveChargeRange(byId("eve-lf304-pbri-rev-e-2023"))).toEqual({ min: -5, max: 60 });
    expect(effectiveChargeRange(byId("eve-lf280k-pbri-rev-c-v3"))).toEqual({ min: 0, max: 55 });
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
