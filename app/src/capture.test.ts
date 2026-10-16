import { describe, expect, it } from "vitest";
import { captureName } from "./capture";

describe("captureName", () => {
  it("uses local date and time with leading zeros", () => {
    expect(captureName(new Date(2026, 0, 2, 3, 4, 5))).toBe("openbms-mitschnitt-20260102-030405.txt");
  });
});
