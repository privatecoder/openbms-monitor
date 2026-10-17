import { describe, expect, it } from "vitest";
import { axisTicks, covered, endLabel, sharePct, tickLabel } from "./timeAxis";

const at = (y: number, mo: number, d: number, h = 0, mi = 0) => new Date(y, mo - 1, d, h, mi).getTime();

describe("time axis", () => {
  it("ticks every 6 h for a day, every day for a week, every Monday beyond", () => {
    const day = axisTicks(at(2030, 1, 1, 14, 5), at(2030, 1, 2, 14, 5));
    expect(day.map((t) => new Date(t).getHours())).toEqual([18, 0, 6, 12]);
    const week = axisTicks(at(2030, 1, 1, 14), at(2030, 1, 8, 14));
    expect(week).toHaveLength(7);
    expect(week.every((t) => new Date(t).getHours() === 0)).toBe(true);
    const month = axisTicks(at(2030, 1, 1, 14), at(2030, 1, 31, 14));
    expect(month.every((t) => new Date(t).getDay() === 1)).toBe(true);
    expect(month).toHaveLength(4);
    // nothing right at the ends
    expect(axisTicks(at(2030, 1, 1, 17, 50), at(2030, 1, 2, 17, 50))[0]).toBe(at(2030, 1, 2, 0));
  });

  it("labels the ends with the date when the span crosses a day", () => {
    const from = at(2030, 10, 16, 14), to = at(2030, 10, 17, 14);
    expect(endLabel(from, from, to, "de")).toBe("16.10. 14:00");
    expect(endLabel(to, from, to, "de")).toBe("17.10. 14:00");
    expect(endLabel(at(2030, 10, 17, 8), at(2030, 10, 17, 8), at(2030, 10, 17, 14), "de")).toBe("08:00");
    expect(tickLabel(at(2030, 10, 16, 18), from, to, "de")).toBe("18:00");
    expect(tickLabel(at(2030, 10, 17, 0), from, to, "de")).toBe("17.10.");
  });

  it("never rounds a small share away", () => {
    expect(sharePct(0.004, "de")).toBe("< 1 %");
    expect(sharePct(0.996, "en")).toBe("> 99 %");
    expect(sharePct(1, "en")).toBe("100 %");
    expect(sharePct(0, "en")).toBe("0 %");
    expect(sharePct(0.426, "en")).toBe("43 %");
    expect(sharePct(null, "en")).toBe("–");
  });

  it("covers the slices with values", () => {
    expect(covered([1, null, 0, null], 0, 400)).toBe(200);
    expect(covered([], 0, 400)).toBe(0);
  });
});
