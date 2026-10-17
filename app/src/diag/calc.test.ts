import { describe, expect, it } from "vitest";
import { flagHigh, innerMilliohm, outerMilliohm, resolutionMilliohm, smooth, zeroOffset } from "./calc";

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
  const at = (i: number) => { const b = resolutionMilliohm(i); return { 0: b, 1: b, 2: b, 3: b, 4: b }; };
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
    expect([...flagHigh({ 0: 2, 1: 6, 2: 2.2, 3: 1.9 }, { 0: 2, 2: 2, 3: 2 })]).toEqual([]);
  });
  it("checks a smoothed resistance against the bound smoothed the same way", () => {
    // one 10 mV step at 3 A reads 3.3 mΩ, the next poll at 30 A reads 0: smoothed 2.3 mΩ against the others' 0
    const r = smooth(smooth(undefined, 10 / 3), 0), bound = smooth(smooth(undefined, resolutionMilliohm(3)), resolutionMilliohm(30));
    expect(r).toBeCloseTo(2.33, 2);
    // against the 30 A bound alone (0.67 mΩ) it would be flagged; the smoothed bound (4.9 mΩ) keeps it quiet
    expect([...flagHigh({ 0: 0, 1: r, 2: 0, 3: 0 }, { 0: bound, 1: bound, 2: bound, 3: bound })]).toEqual([]);
  });
});
