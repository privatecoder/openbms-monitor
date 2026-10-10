import { describe, expect, it } from "vitest";
import { exportName, shares } from "./data";

describe("shares", () => {
  it("relates each pack to the mean, only under real load", () => {
    const s = shares(new Map([[0, [10, 1, null]], [1, [30, 3, 5]]]));
    expect(s.get(0)).toEqual([0.5, null, null]);
    expect(s.get(1)).toEqual([1.5, null, null]);
  });
  it("uses the packs with a value when most have one", () => {
    const s = shares(new Map([[0, [10]], [1, [20]], [2, [30]], [3, [null]]]));
    expect(s.get(0)).toEqual([0.5]);
    expect(s.get(3)).toEqual([null]);
  });
});

describe("exportName", () => {
  it("keeps the installation readable and the name safe", () => {
    const d = new Date(2026, 9, 10, 18, 5);
    expect(exportName("192.168.1.10:4196", "csv", d)).toBe("openbms_192.168.1.10-4196_2026-10-10_18-05.csv");
    expect(exportName("/dev/cu.usbserial-0001", "jsonl", d)).toBe("openbms_cu.usbserial-0001_2026-10-10_18-05.jsonl");
  });
});
