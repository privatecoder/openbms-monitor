import { describe, expect, it } from "vitest";
import { cellMv, cellStats, centiAmps, currentAllowed, notCalibrated, parseNumber, protocolOf, refusal } from "./calibration";

describe("current button", () => {
  it("needs both values at 10 A or more in the same direction", () => {
    expect(currentAllowed(25.3, 24.9)).toBe(true);
    expect(currentAllowed(-12, -10)).toBe(true);
    expect(currentAllowed(10, 10)).toBe(true);
    expect(currentAllowed(9.99, 12)).toBe(false);
    expect(currentAllowed(12, 9.99)).toBe(false);
    expect(currentAllowed(0.01, 2)).toBe(false);
    expect(currentAllowed(15, -15)).toBe(false);
    expect(currentAllowed(-15, 15)).toBe(false);
    expect(currentAllowed(null, 15)).toBe(false);
    expect(currentAllowed(15, undefined)).toBe(false);
  });

  it("sends 0.01 A", () => {
    expect(centiAmps(-12.345)).toBe(-1234);
    expect(centiAmps(25.3)).toBe(2530);
  });
});

describe("voltage per cell", () => {
  it("divides by the cell count and rounds to mV", () => {
    expect(cellMv(52.81, 16)).toBe(3301);
    expect(cellMv(52.808, 16)).toBe(3301);
    expect(cellMv(52.8, 16)).toBe(3300);
    expect(cellMv(26.41, 8)).toBe(3301);
    expect(cellMv(52.81, 0)).toBeNull();
    expect(cellMv(null, 16)).toBeNull();
    expect(cellMv(0, 16)).toBeNull();
  });

  it("mean and spread of the cells", () => {
    const s = cellStats([3.3, 3.31, 3.305, 3.295])!;
    expect(s.mean).toBeCloseTo(3.3025, 6);
    expect(s.spread).toBeCloseTo(0.015, 6);
    expect(cellStats([])).toBeNull();
  });
});

describe("input and answers", () => {
  it("parses decimal comma and point", () => {
    expect(parseNumber("52,81")).toBe(52.81);
    expect(parseNumber(" -12.5 ")).toBe(-12.5);
    expect(parseNumber("")).toBeNull();
    expect(parseNumber("12 A")).toBeNull();
    expect(parseNumber("1,2,3")).toBeNull();
  });

  it("recognises refusals", () => {
    expect(refusal('{"kind":"refused"}')).toBe("refused");
    expect(refusal('{"kind":"invalid"}')).toBe("invalid");
    expect(refusal('{"kind":"busy","action":null}')).toBeNull();
    expect(refusal("timeout after 1500 ms")).toBeNull();
  });

  it("maps the 0x51 text to an offered protocol", () => {
    expect(protocolOf("CAN:PNG_DYE_Luxp_TBB")?.n).toBe(1);
    expect(protocolOf("CAN:Studer")?.n).toBe(6);
    expect(protocolOf("CAN:MUST")).toBeUndefined();
    expect(protocolOf(undefined)).toBeUndefined();
  });

  it("reads the not-calibrated flags of event 8", () => {
    const ev = [0, 0, 0, 0, 0, 0, 0, 0b11100];
    expect(notCalibrated(ev, "zero_point")).toBe(true);
    expect(notCalibrated(ev, "current")).toBe(true);
    expect(notCalibrated(ev, "voltage")).toBe(true);
    expect(notCalibrated([0, 0, 0, 0, 0, 0, 0, 0b01000], "voltage")).toBe(false);
    expect(notCalibrated(undefined, "current")).toBe(false);
  });
});

describe("texts", () => {
  it("every key of the calibration and CAN protocol UI exists in German and English", async () => {
    const i18n = (await import("./i18n")).default;
    const src = (await import("./components/Calibration.tsx?raw")).default as string;
    const used = new Set([...src.matchAll(/["`]((?:cal|can)\.[A-Za-z0-9_.]+)["`]/g)].map((m) => m[1]));
    for (const k of ["zero_point", "current", "voltage"]) used.add(`cal.cond.${k}`);
    expect(used.size).toBeGreaterThan(30);
    for (const lng of ["de", "en"]) for (const k of used) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
  });
});
