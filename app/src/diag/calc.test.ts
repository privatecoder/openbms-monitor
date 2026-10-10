import { describe, expect, it } from "vitest";
import { flagHigh, innerMilliohm, outerMilliohm, zeroOffset } from "./calc";

describe("resistance diagnosis", () => {
  const rest = { current: 0.1, pack_voltage: 53.00, port_voltage: 53.02 };
  const off = zeroOffset(rest, 53.05);
  it("removes the zero offset and gives the same R for charging and discharging", () => {
    // 10 A charge: 20 mV inside, 30 mV outside on top of the offsets
    expect(innerMilliohm({ current: 10, pack_voltage: 53.10, port_voltage: 53.14 }, off)).toBeCloseTo(2);
    expect(outerMilliohm({ current: 10, pack_voltage: 53.10, port_voltage: 53.14 }, off, 53.20)).toBeCloseTo(3);
    // 10 A discharge: drops reverse
    expect(innerMilliohm({ current: -10, pack_voltage: 52.90, port_voltage: 52.90 }, off)).toBeCloseTo(2);
    expect(outerMilliohm({ current: -10, pack_voltage: 52.90, port_voltage: 52.90 }, off, 52.90)).toBeCloseTo(3);
  });
  it("refuses small currents and missing busbar offsets", () => {
    expect(innerMilliohm({ current: 1, pack_voltage: 53, port_voltage: 53 }, off)).toBeUndefined();
    expect(outerMilliohm({ current: 10, pack_voltage: 53, port_voltage: 53 }, zeroOffset(rest), 53)).toBeUndefined();
  });
  it("flags packs well above the median", () => {
    expect([...flagHigh({ 0: 2, 1: 6, 2: 2.2, 3: 1.9, 4: undefined })]).toEqual([1]);
    expect([...flagHigh({ 0: 0.2, 1: 0.5, 2: 0.2 })]).toEqual([]);
  });
});
