import { describe, expect, it } from "vitest";
import { KEYS, UNITS } from "./params";
import { fromRaw, isSigned, rawRange, toRaw } from "./paramWrite";

describe("parameter metadata from the catalog", () => {
  it("keeps keys, units and raw conversions of P0..P86", () => {
    expect(KEYS.length).toBe(87);
    expect([KEYS[0], UNITS[0], fromRaw(0, 3450)]).toEqual(["cell_high_voltage_alarm", "V", 3.45]);
    expect([UNITS[10], fromRaw(10, 5760)]).toEqual(["V", 57.6]);
    expect([UNITS[20], fromRaw(20, 2731 + 500)]).toEqual(["°C", 50]);
    expect([UNITS[50], isSigned(50), toRaw(50, -1.5)]).toEqual(["A", true, -150]);
    expect([UNITS[58], fromRaw(58, 28000)]).toEqual(["Ah", 280]);
    expect([rawRange(59), rawRange(60), rawRange(50)]).toEqual([[0, 0xffff], [0, 0xff], [-32768, 32767]]);
    expect([UNITS[82], fromRaw(82, 100)]).toEqual(["mΩ", 10]);
    expect(KEYS[86]).toBe("compensation_2_resistance");
  });
});
