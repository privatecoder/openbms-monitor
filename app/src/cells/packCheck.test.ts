import { describe, expect, it } from "vitest";
import type { Telemetry } from "../api";
import { allowedChargeA, allowedDischargeA, checkPack, suggestParallel } from "./packCheck";
import type { Cell } from "./types";

const base = {
  schema_version: 1, entry_version: 1, updated: "2026-10-10", chemistry: "LFP",
  datasheet: { title: "t", source_type: "manufacturer_pdf", url: "https://example.org" },
} as const;
// like the EVE LF280K Rev B: no derating table, 1C continuous, charging 0–55 °C
const plain: Cell = {
  ...base, id: "plain", manufacturer: "X", model: "280",
  capacity: { nominal_ah: 280 }, voltage: { nominal_v: 3.2, charge_cutoff_v: 3.65, discharge_cutoff_v: 2.5 },
  current: { standard_charge: { value: 0.5, basis: "C" }, max_continuous_charge_a: 280, max_continuous_discharge_a: 280 },
  temperature: { charge_min_c: 0, charge_max_c: 55, discharge_min_c: -20, discharge_max_c: 55 },
};
// with a derating table: 0.1C from 0 to 10 °C, 0.5C from 10 to 45 °C
const derated: Cell = {
  ...plain, id: "derated",
  charge_derating: { basis: "C", points: [{ temp_min_c: 0, temp_max_c: 10, value: 0.1 }, { temp_min_c: 10, temp_max_c: 45, value: 0.5 }] },
};
const tm = (cells: number[], temps: number[], soc = 60) => ({ cell_voltages: cells, cell_temperatures: temps, soc } as unknown as Telemetry);

describe("allowedDischargeA", () => {
  it("uses the continuous discharge current inside the discharge range", () => {
    expect(allowedDischargeA(plain, 2, [-5, 30])).toBe(560);
    expect(allowedDischargeA(plain, 1, [-25, 20])).toBe(0);
    expect(allowedDischargeA({ ...plain, current: undefined }, 1, [20])).toBeUndefined();
  });
});

describe("allowedChargeA", () => {
  it("uses the continuous current inside the charge range when there is no table", () => {
    expect(allowedChargeA(plain, 1, [20, 25])).toBe(280);
    expect(allowedChargeA(plain, 2, [20, 25])).toBe(560);
    expect(allowedChargeA(plain, 1, [-2, 20])).toBe(0);
  });
  it("takes the lower value of the coldest and warmest cell from the table", () => {
    expect(allowedChargeA(derated, 1, [8, 20])).toBeCloseTo(28);
    expect(allowedChargeA(derated, 1, [15, 20])).toBeCloseTo(140);
    expect(allowedChargeA(derated, 1, [20, 50])).toBe(0);
  });
});

describe("checkPack", () => {
  const ids = (cs: ReturnType<typeof checkPack>) => Object.fromEntries(cs.map((c) => [c.id, c.level]));
  it("is quiet in normal operation", () => {
    expect(ids(checkPack(plain, 1, tm([3.30, 3.31], [22, 24]), -40))).toEqual({
      cellHigh: "ok", cellLow: "ok", tempCharge: "ok", tempDischarge: "ok", chargeCurrent: "ok", dischargeCurrent: "ok" });
  });
  it("flags cells near and beyond the cut-off voltages", () => {
    expect(ids(checkPack(plain, 1, tm([3.61, 3.4], [22]), 10)).cellHigh).toBe("near");
    expect(ids(checkPack(plain, 1, tm([3.66, 3.4], [22]), 10)).cellHigh).toBe("over");
    expect(ids(checkPack(plain, 1, tm([3.2, 2.6], [22]), -10)).cellLow).toBe("near");
  });
  it("treats a cold pack as a violation only while it charges", () => {
    expect(ids(checkPack(plain, 1, tm([3.3], [-3]), 20)).tempCharge).toBe("over");
    expect(ids(checkPack(plain, 1, tm([3.3], [-3]), -20)).tempCharge).toBe("near");
    expect(ids(checkPack(plain, 1, tm([3.3], [-3]), 20)).chargeCurrent).toBe("over");
  });
  it("compares the charge current with the derated limit", () => {
    const c = checkPack(derated, 1, tm([3.3], [8, 9]), 40).find((x) => x.id === "chargeCurrent")!;
    expect(c.level).toBe("over");
    expect(c.limit).toBeCloseTo(28);
  });
});

describe("suggestParallel", () => {
  it("matches the pack's rated capacity", () => {
    expect(suggestParallel(plain, 280)).toBe(1);
    expect(suggestParallel(plain, 560)).toBe(2);
    expect(suggestParallel(plain, 300)).toBeUndefined();
  });
});
