import { describe, expect, it } from "vitest";
import { RECONNECT_TRIES, reconnectDelay, waitText } from "./components/live";

describe("reconnect schedule", () => {
  it("doubles from one minute and stays at 30 minutes", () => {
    const min = Array.from({ length: RECONNECT_TRIES }, (_, i) => reconnectDelay(i) / 60_000);
    expect(min).toEqual([1, 2, 4, 8, 16, 30, 30, 30, 30, 30]);
  });
  it("shows seconds up to two minutes, then minutes", () => {
    expect(waitText(-500)).toBe("0 s");
    expect(waitText(44_200)).toBe("45 s");
    expect(waitText(120_000)).toBe("120 s");
    expect(waitText(121_000)).toBe("3 min");
  });
});
