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
  it("keeps the offsets per reference point", () => {
    const sw = zeroOffset(rest, 53.03, "switch");
    expect(outerMilliohm({ current: 10, pack_voltage: 53.10, port_voltage: 53.14 }, sw, 53.17, "switch")).toBeCloseTo(2);
    expect(outerMilliohm({ current: 10, pack_voltage: 53.10, port_voltage: 53.14 }, sw, 53.20, "bus")).toBeUndefined();
  });
  it("refuses small currents and missing busbar offsets", () => {
    expect(innerMilliohm({ current: 1, pack_voltage: 53, port_voltage: 53 }, off)).toBeUndefined();
    expect(outerMilliohm({ current: 10, pack_voltage: 53, port_voltage: 53 }, zeroOffset(rest), 53)).toBeUndefined();
  });
  const at = (a: number) => ({ 0: a, 1: a, 2: a, 3: a, 4: a });
  it("flags packs well above the median of the others", () => {
    expect([...flagHigh({ 0: 2, 1: 6, 2: 2.2, 3: 1.9, 4: undefined }, at(10))]).toEqual([1]);
    expect([...flagHigh({ 0: 0.2, 1: 0.5, 2: 0.2 }, at(10))]).toEqual([]);
    // two packs: against the partner, not a median of 3.5 that includes the candidate
    expect([...flagHigh({ 0: 2, 1: 5 }, at(20))]).toEqual([1]);
    expect([...flagHigh({ 0: 5 }, at(20))]).toEqual([]);
  });
  it("does not read the 10 mV resolution as a resistance", () => {
    // at 3 A one 10 mV step is 3.3 mΩ, two voltages up to 6.7 mΩ: 4 mΩ above the others is within that
    expect([...flagHigh({ 0: 2, 1: 6, 2: 2.2, 3: 1.9 }, at(3))]).toEqual([]);
    expect([...flagHigh({ 0: 2, 1: 9, 2: 2.2, 3: 1.9 }, at(3))]).toEqual([1]);
    expect([...flagHigh({ 0: 2, 1: 6, 2: 2.2, 3: 1.9 }, { 0: 10, 2: 10, 3: 10 })]).toEqual([]);
  });
});
