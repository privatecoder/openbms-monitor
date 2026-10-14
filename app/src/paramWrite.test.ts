import { describe, expect, it } from "vitest";
import { RULES, blockers, capacityResets, changes, fromRaw, notTaken, outOfRange, ratedFor, rawsOf, switchDiff, takeOver, toRaw, toWire, violations } from "./paramWrite";
import { KEYS, UNITS, type Parameters } from "./params";

/** A synthetic, plausible configuration (display values, P0..P86). */
const GOOD = [3.45, 3.35, 2.9, 3.1, 3.65, 3.45, 2.7, 3.1, 3.4, 1.5, 55.2, 53.6, 46.4, 48, 56, 53.6, 43.2, 48, 63, 61,
  50, 47, 2, 5, 55, 50, -10, 0, 52, 47, -10, 3, 55, 50, -15, 0, 0, 10, 50, 47, 0, 3, 60, 55, -10, 0, 90, 85, 100, 85,
  150, 145, -155, -153, 160, -160, -300, 2000, 280, 75, 0.5, 0.3, 0.03, 0.02, 10, 16, 10, 10, 30, 60, 5, 5, 1, 10, 10,
  30, 240, 48, 15, 5, 96, 80, 10, 9, 0, 13, 0];
const good = () => GOOD.map((v, i) => toRaw(i, v));
const set = (changes: Record<number, number>) => { const r = good(); for (const [i, v] of Object.entries(changes)) r[Number(i)] = v; return r; };
const broken = (r: number[]) => violations(r).map((v) => `${v.index}${v.op}${"other" in v ? `P${v.other}` : v.bound}`);

describe("raw values", () => {
  it("converts display values to raws and back", () => {
    expect(toRaw(0, 3.65)).toBe(3650);
    expect(toRaw(10, 56.8)).toBe(5680);
    expect(toRaw(26, -10)).toBe(2631);
    expect(toRaw(20, 0)).toBe(2731);
    expect(toRaw(56, -300)).toBe(-30000);
    expect(toRaw(58, 280)).toBe(28000);
    expect(toRaw(62, 0.03)).toBe(30);
    expect(toRaw(82, 1.5)).toBe(15);
    expect(toRaw(65, 16)).toBe(16);
    for (let i = 0; i < KEYS.length; i++) expect(fromRaw(i, toRaw(i, GOOD[i]))).toBeCloseTo(GOOD[i], 9);
  });

  it("rounds to the raw resolution", () => {
    expect(toRaw(0, 3.6504)).toBe(3650);
    expect(toRaw(0, 3.6506)).toBe(3651);
    expect(toRaw(20, 50.04)).toBe(3231);
    expect(toRaw(50, -0.004)).toBe(-0);
  });

  it("reads signed currents from u16 and sends them back as u16", () => {
    const p: Parameters = { address: 1, device_name: "SYN", function_switches: [1, 2, 3, 4, 5, 6, 7, 8],
      parameters: KEYS.map((key, index) => ({ index, key, raw: index === 55 ? 0x10000 - 16000 : index === 50 ? 15000 : 100, value: 0, unit: UNITS[index] })) };
    const raws = rawsOf(p);
    expect(raws[55]).toBe(-16000);
    expect(raws[50]).toBe(15000);
    expect(raws[0]).toBe(100);
    const w = toWire(raws, p.function_switches);
    expect(w.raws[55]).toBe(0x10000 - 16000);
    expect(w.raws[50]).toBe(15000);
    expect(w.switches).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it("finds values outside the stored width", () => {
    expect(outOfRange(good())).toEqual([]);
    expect(outOfRange(set({ 0: 65536, 1: -1, 50: -32769, 51: 32768, 60: 256, 61: 1.5 }))).toEqual([0, 1, 50, 51, 60, 61]);
    expect(outOfRange(set({ 50: -32768, 51: 32767, 60: 255, 0: 65535 }))).toEqual([]);
  });
});

describe("firmware plausibility rules (0x4C3C)", () => {
  it("accepts the synthetic configuration", () => {
    expect(violations(good())).toEqual([]);
  });

  it("allows equality everywhere", () => {
    // cell chain all equal at the lower bound, P8 equal to P4
    expect(broken(set({ 0: 3400, 1: 3400, 3: 3400, 2: 3400, 4: 3400, 5: 3400, 7: 3400, 6: 3400, 8: 3400, 9: 3400 }))).toEqual([]);
    expect(broken(set({ 52: -16000, 53: -16000, 51: -16000, 50: -16000, 54: -16000, 56: -16000 }))).toEqual([]);
    expect(broken(set({ 59: 28000 }))).toEqual([]);
    expect(broken(set({ 79: 15 }))).toEqual([]);
    expect(broken(set({ 36: 2631, 37: 2931 }))).toEqual([]);
  });

  it("cell voltage chains and bounds", () => {
    expect(broken(set({ 1: 3460 }))).toEqual(["1leP0"]);
    expect(broken(set({ 3: 3360 }))).toEqual(["3leP1"]);
    expect(broken(set({ 2: 3200 }))).toEqual(["2leP3"]);
    expect(broken(set({ 2: 1999, 6: 1999, 9: 1000 }))).toEqual(["2ge2000", "6ge2000"]);
    expect(broken(set({ 0: 4301 }))).toEqual(["0le4300"]);
    expect(broken(set({ 4: 4301 }))).toEqual(["4le4300"]);
    expect(broken(set({ 8: 3329 }))).toEqual(["8ge3330"]);
    expect(broken(set({ 8: 3651 }))).toEqual(["8leP4"]);
    expect(broken(set({ 9: 999 }))).toEqual(["9ge1000"]);
    expect(broken(set({ 9: 2701 }))).toEqual(["9leP6"]);
  });

  it("pack voltage and charger", () => {
    expect(broken(set({ 13: 4639 }))).toEqual(["12leP13"]);
    expect(broken(set({ 12: 1599, 13: 1599, 16: 1599, 17: 1599 }))).toEqual(["12ge1600", "16ge1600"]);
    expect(broken(set({ 10: 6001 }))).toEqual(["10le6000"]);
    expect(broken(set({ 19: 5599 }))).toEqual(["14leP19"]);
    expect(broken(set({ 18: 6099 }))).toEqual(["19leP18"]);
    expect(broken(set({ 18: 6501 }))).toEqual(["18le6500"]);
  });

  it("every temperature group: a ≥ b ≥ d ≥ c ≥ −30 °C, a ≤ 80 °C", () => {
    for (const a of [20, 24, 28, 32, 38, 42]) {
      expect(broken(set({ [a + 1]: toRaw(a + 1, 81), [a]: toRaw(a, 80) }))).toEqual([`${a + 1}leP${a}`]);
      expect(broken(set({ [a]: 3532, [a + 1]: 3000 }))).toEqual([`${a}le3531`]);
      expect(broken(set({ [a + 2]: 2430, [a + 3]: 2731 }))).toEqual([`${a + 2}ge2431`]);
      expect(violations(set({ [a + 3]: 2430 })).map((v) => v.index)).toContain(a + 2);
    }
    // heating between the charge under-temperature protection and 20 °C
    expect(broken(set({ 36: 2621 }))).toEqual(["26leP36"]);
    expect(broken(set({ 37: 2721 }))).toEqual(["36leP37"]);
    expect(broken(set({ 37: 2932 }))).toEqual(["37le2931"]);
  });

  it("power temperature", () => {
    expect(broken(set({ 47: 3230, 49: 3231 }))).toEqual(["47ge3231"]);
    expect(broken(set({ 46: 3580 }))).toEqual(["47leP46"]);
    expect(broken(set({ 48: 3600 }))).toEqual(["46leP48"]);
    expect(broken(set({ 48: 3570 }))).toEqual(["46leP48", "49leP48"]);
    expect(broken(set({ 48: 3932 }))).toEqual(["48le3931"]);
    expect(broken(set({ 49: 3230 }))).toEqual(["49ge3231"]);
  });

  it("currents are compared signed", () => {
    // discharge values are negative: P55 ≤ P52 ≤ P53 ≤ P51 ≤ P50 ≤ P54 ≤ 300 A
    expect(broken(set({ 55: -15400 }))).toEqual(["55leP52"]);
    expect(broken(set({ 52: -15200 }))).toEqual(["52leP53"]);
    expect(broken(set({ 53: 14600 }))).toEqual(["53leP51"]);
    expect(broken(set({ 51: 15100 }))).toEqual(["51leP50"]);
    expect(broken(set({ 50: 16100 }))).toEqual(["50leP54"]);
    expect(broken(set({ 54: 30001 }))).toEqual(["54le30000"]);
    expect(broken(set({ 56: -30001 }))).toEqual(["56ge-30000"]);
    expect(broken(set({ 56: -15000 }))).toEqual(["56leP55"]);
    // as unsigned the discharge values would all be "greater" than the charge values
    expect(broken(set({ 55: 0x10000 - 16000 }))).not.toEqual([]);
  });

  it("delay, capacity, cell difference and balancing", () => {
    expect(broken(set({ 57: 499 }))).toEqual(["57ge500"]);
    expect(broken(set({ 57: 5001 }))).toEqual(["57le5000"]);
    expect(broken(set({ 59: 28001 }))).toEqual(["59leP58"]);
    expect(broken(set({ 58: 65001, 59: 100 }))).toEqual(["58le65000"]);
    expect(broken(set({ 61: 9 }))).toEqual(["61ge10"]);
    expect(broken(set({ 61: 51 }))).toEqual(["61leP60"]);
    expect(broken(set({ 60: 101 }))).toEqual(["60le100"]);
    expect(broken(set({ 63: 31 }))).toEqual(["63leP62"]);
    expect(broken(set({ 63: 9 }))).toEqual(["63ge10"]);
    expect(broken(set({ 62: 101 }))).toEqual(["62le100"]);
  });

  it("single-byte ranges", () => {
    const ranges: [number, number, number][] = [[64, 1, 18], [65, 7, 16], [66, 3, 180], [67, 3, 180], [68, 5, 250], [69, 10, 250],
      [70, 1, 30], [71, 1, 30], [72, 1, 30], [73, 1, 24], [74, 1, 100], [75, 1, 250], [76, 30, 250], [77, 1, 250],
      [80, 70, 99], [81, 10, 150], [82, 0, 100], [83, 1, 9], [84, 0, 50], [85, 10, 16], [86, 0, 50]];
    for (const [i, lo, hi] of ranges) {
      expect(broken(set({ [i]: lo })), `P${i}=${lo}`).toEqual([]);
      expect(broken(set({ [i]: hi })), `P${i}=${hi}`).toEqual([]);
      if (lo > 0) expect(broken(set({ [i]: lo - 1 }))).toEqual([`${i}ge${lo}`]);
      expect(broken(set({ [i]: hi + 1 }))).toEqual([`${i}le${hi}`]);
    }
    expect(broken(set({ 79: 0 }))).toEqual(["79ge1"]);
    expect(broken(set({ 79: 16 }))).toEqual(["79leP78"]);
    expect(broken(set({ 78: 31, 79: 5 }))).toEqual(["78le30"]);
  });

  it("does not check the switches or P65 against anything else", () => {
    const named = new Set(RULES.flatMap((r) => ("lo" in r ? [r.lo] : []).concat("hi" in r ? [r.hi] : [])));
    for (let i = 0; i < KEYS.length; i++) expect(named.has(i), `P${i}`).toBe(true);
  });
});

describe("change list and checks", () => {
  it("lists parameters and switch bits", () => {
    const base = good(), raws = set({ 12: 4800, 0: 3500 });
    const c = changes(base, raws, [0xff, 0, 0, 0, 0, 0, 0, 0], [0xfd, 0, 0x80, 0, 0, 0, 0, 0]);
    expect(c).toEqual([
      { kind: "param", index: 0, from: 3450, to: 3500 },
      { kind: "param", index: 12, from: 4640, to: 4800 },
      { kind: "switch", group: 0, bit: 1, on: false },
      { kind: "switch", group: 2, bit: 7, on: true },
    ]);
    expect(changes(base, base, [1, 2, 3, 4, 5, 6, 7, 8], [1, 2, 3, 4, 5, 6, 7, 8])).toEqual([]);
  });

  it("names the switches a refused block left live", () => {
    expect(switchDiff([0x01, 0, 0, 0, 0, 0, 0, 0x80], [0x00, 0, 0, 0, 0, 0, 0, 0x80])).toEqual([{ kind: "switch", group: 0, bit: 0, on: true }]);
    expect(switchDiff([0, 0, 0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0, 0])).toEqual([]);
  });

  it("marks values the read-back does not show", () => {
    const sent = set({ 12: 4800 });
    const r = notTaken(sent, [1, 2, 3, 4, 5, 6, 7, 8], set({ 12: 4640 }), [1, 2, 3, 4, 5, 6, 7, 9]);
    expect([...r.params]).toEqual([12]);
    expect([...r.switches]).toEqual([7]);
  });

  it("takes over another source but keeps P65 unless unlocked", () => {
    const own = set({ 65: 8 });
    const t = takeOver(own, GOOD, true);
    expect(t[65]).toBe(8);
    expect(t[0]).toBe(3450);
    expect(t[26]).toBe(2631);
    expect(t[56]).toBe(-30000);
    expect(takeOver(own, GOOD, false)[65]).toBe(16);
  });

  it("blocks bad input, out-of-range raws, a locked P65 change and broken rules", () => {
    const base = good();
    expect(blockers(base, base, [], false)).toEqual([]);
    expect(blockers(base, set({ 65: 15 }), [], false)).toEqual([{ kind: "series" }]);
    expect(blockers(base, set({ 65: 15 }), [], true)).toEqual([]);
    expect(blockers(base, set({ 64: 300 }), [], false).map((b) => b.kind)).toEqual(["range", "rule"]);
    expect(blockers(base, set({ 64: 300 }), [64], false).map((b) => b.kind)).toEqual(["input", "rule"]);
    expect(blockers(base, set({ 1: 3460 }), [], false)).toEqual([{ kind: "rule", v: { index: 1, op: "le", other: 0 } }]);
  });

  it("warns when the BMS will set the learned full capacity to P58", () => {
    expect(capacityResets(265, 28000)).toBe(true);
    expect(capacityResets(270.01, 28000)).toBe(false);
    expect(capacityResets(270, 28000)).toBe(true);
    expect(capacityResets(290, 28000)).toBe(true);
    expect(capacityResets(280, 28000)).toBe(false);
    expect(capacityResets(null, 28000)).toBe(false);
    expect(capacityResets(undefined, 28000)).toBe(false);
    expect(ratedFor(265.004)).toBe(26500);
    expect(capacityResets(265, ratedFor(265))).toBe(false);
  });
});
