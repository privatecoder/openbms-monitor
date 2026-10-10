import { describe, expect, it } from "vitest";
import { BM_NAMES, KEYS, UNITS, deviating, exportName, parseExport, toExport, type Parameters } from "./params";

// a typical configuration (16S, 280 Ah)
const VALUES = [3.45, 3.35, 2.9, 3.1, 3.65, 3.45, 2.7, 3.1, 3.4, 1.5, 55.2, 53.6, 46.4, 48, 56, 53.6, 43.2, 48, 63, 61,
  50, 47, 2, 5, 55, 50, -10, 0, 52, 47, -10, 3, 55, 50, -15, 0, 0, 10, 50, 47, 0, 3, 60, 55, -10, 0, 90, 85, 100, 85,
  150, 145, -155, -153, 160, -160, -300, 2000, 280, 75, 0.5, 0.3, 0.03, 0.02, 10, 16, 10, 10, 30, 60, 5, 5, 1, 10, 10,
  30, 240, 48, 15, 5, 96, 80, 10, 9, 0, 13, 0];
const SWITCHES = [0xff, 0xdf, 0xff, 0x3f, 0xbf, 0x9f, 0xbf, 0x1f];
const pack: Parameters = {
  address: 3, device_name: "1101-SP75", function_switches: SWITCHES,
  parameters: VALUES.map((value, index) => ({ index, key: KEYS[index], raw: 0, value, unit: UNITS[index] })),
};

describe("BatteryMonitor export", () => {
  it("writes the format BatteryMonitor reads and parses it back", () => {
    const xml = toExport(pack);
    expect(xml.startsWith("﻿<?xml")).toBe(true);
    expect(xml).toContain("\r\n");
    expect(xml).toContain("<Name>Monomer high voltage alarm</Name>\r\n    <Value>3.450</Value>\r\n    <Unit>V</Unit>");
    expect(xml).toContain("<Value>-10.0</Value>\r\n    <Unit>℃</Unit>");
    expect(xml).toContain("<Value>-155.00</Value>");
    expect(xml).toContain("<Value>2000</Value>\r\n    <Unit>mS</Unit>");
    expect(xml).toContain("<Name>BitGroup1</Name>\r\n    <Value>DF</Value>");
    expect(xml).toContain("<modualName>1101-SP75 </modualName>");
    const s = parseExport(xml, "x.xml");
    expect(s.error).toBeUndefined();
    expect(s.values).toEqual(VALUES);
    expect(s.switches).toEqual(SWITCHES);
    expect(s.suspectFrom).toBeNull();
    expect(s.device).toBe("1101-SP75");
  });

  it("flags an export whose values are shifted against the known order", () => {
    // like the 2024 export: from parameter 2 on every value sits one place off
    const shifted = { ...pack, parameters: pack.parameters.map((p, i) => ({ ...p, value: i < 2 ? p.value : VALUES[(i + 8) % VALUES.length] })) };
    expect(parseExport(toExport(shifted), "old.xml").suspectFrom).toBe(2);
    // other names (another BatteryMonitor version) are flagged by name as well
    const renamed = toExport(pack).replace(BM_NAMES[40], "Something else");
    expect(parseExport(renamed, "v2.xml").suspectFrom).toBe(40);
  });

  it("refuses files that are not a complete export", () => {
    expect(parseExport("<foo/>", "a.xml").error).toBeDefined();
    expect(parseExport(toExport(pack).replace(/<param_bit_group>[\s\S]*?<\/param_bit_group>/, ""), "b.xml").error).toBeDefined();
  });

  it("names saved files by pack and local time", () => {
    expect(exportName(3, new Date(2026, 9, 10, 18, 5))).toBe("Parameter_pack03_2026-10-10_18-05.xml");
  });
});

describe("deviating", () => {
  it("marks only the outliers when there is a clear majority", () => {
    expect([...deviating(["3.450", "3.450", "3.500", undefined, "3.450"])]).toEqual([2]);
  });
  it("marks every source when there is no majority", () => {
    expect([...deviating(["3.450", "3.500"])]).toEqual([0, 1]);
    expect([...deviating(["1", "1", "2", "2"])]).toEqual([0, 1, 2, 3]);
  });
  it("marks nothing when all agree or only one has a value", () => {
    expect(deviating(["5", "5"]).size).toBe(0);
    expect(deviating(["5", undefined]).size).toBe(0);
  });
});
