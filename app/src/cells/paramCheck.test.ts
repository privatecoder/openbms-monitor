import { describe, expect, it } from "vitest";
import { checkParam, paramFindings, paramLimits } from "./paramCheck";
import type { Cell } from "./types";

const cell = {
  schema_version: 1, id: "t", entry_version: 1, updated: "2026-01-01", manufacturer: "T", model: "280", chemistry: "LFP",
  datasheet: { title: "t", source_type: "manufacturer_pdf", url: "https://example.org" },
  capacity: { nominal_ah: 280 },
  voltage: { nominal_v: 3.2, charge_cutoff_v: 3.65, discharge_cutoff_v: 2.5 },
  current: { max_continuous_charge_a: 280, max_continuous_discharge_a: 280 },
  temperature: { charge_min_c: 0, charge_max_c: 55, discharge_min_c: -20, discharge_max_c: 55 },
} satisfies Cell;

// a Seplos 16S configuration: everything inside the datasheet except the charge under-temperature protection
const values = Array.from({ length: 87 }, () => 0);
Object.assign(values, {
  0: 3.45, 2: 2.9, 4: 3.65, 6: 2.7, 10: 55.2, 12: 46.4, 14: 56, 16: 43.2, 20: 50, 22: 2, 24: 55, 26: -10,
  28: 52, 30: -10, 32: 55, 34: -15, 50: 150, 52: -155, 54: 160, 55: -160, 58: 280, 65: 16,
});

describe("paramLimits", () => {
  it("scales pack voltages by the series count and currents by the parallel count", () => {
    const l = paramLimits(cell, 2, 16);
    expect(l.get(14)).toEqual({ kind: "max", limit: 58.4 });
    expect(l.get(16)).toEqual({ kind: "min", limit: 40 });
    expect(l.get(54)).toEqual({ kind: "max", limit: 560 });
    expect(l.get(58)).toEqual({ kind: "rated", limit: 560 });
  });
  it("leaves out what the datasheet does not give", () => {
    const l = paramLimits({ ...cell, current: undefined }, 1, 16);
    expect(l.has(54)).toBe(false);
    expect(l.has(4)).toBe(true);
  });
});

describe("checkParam", () => {
  it("compares discharge currents by magnitude", () => {
    expect(checkParam({ kind: "maxAbs", limit: 280 }, -160)).toBe("ok");
    expect(checkParam({ kind: "maxAbs", limit: 100 }, -160)).toBe("over");
  });
  it("accepts a limit hit exactly and a rated capacity within 2 %", () => {
    expect(checkParam({ kind: "max", limit: 3.65 }, 3.65)).toBe("ok");
    expect(checkParam({ kind: "rated", limit: 280 }, 275)).toBe("ok");
    expect(checkParam({ kind: "rated", limit: 280 }, 230)).toBe("note");
  });
});

describe("paramFindings", () => {
  it("finds the charge under-temperature protection below 0 °C", () => {
    expect([...paramFindings(cell, 1, values)]).toEqual([[26, "over"]]);
  });
  it("flags a cell overvoltage protection above the charge cut-off", () => {
    const v = [...values]; v[4] = 3.75;
    expect(paramFindings(cell, 1, v).get(4)).toBe("over");
  });
});
