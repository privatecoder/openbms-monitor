import { describe, expect, it } from "vitest";
import { bmsBalancing, bmsWhy, extNow } from "./balancingHint";
import { BALANCER_DEFAULTS } from "./devices";
import { KEYS, UNITS } from "./params";
import type { SnapshotParams } from "./api";

/** A synthetic stored block: parameter 8 and 62 as given, switches group 6 as given. */
const block = (address: number, at: number, p8: number, p62: number, g6: number, board: SnapshotParams["board"] = "current"): SnapshotParams => ({
  address, at, kind: "read", board, bms_serial: null,
  parameters: { address, device_name: "SYN", function_switches: [0, 0, 0, 0, 0, 0, g6, 0],
    parameters: KEYS.map((key, index) => ({ index, key, raw: 0, value: index === 8 ? p8 : index === 62 ? p62 : 0, unit: UNITS[index] })) },
});

describe("the BMS's own balancing from the stored parameters", () => {
  it("takes the newest block of the pack and its current board", () => {
    const rows = [block(3, 100, 3.4, 0.03, 0b01), block(3, 300, 3.5, 0.02, 0b11, "previous"), block(3, 200, 3.45, 0.025, 0b11), block(4, 400, 3.6, 0.05, 0)];
    expect(bmsBalancing(rows, 3)).toEqual({ start: 3.45, spread: 0.025, enabled: true, idle: true, at: 200 });
    expect(bmsBalancing(rows, 5)).toBeNull();
  });

  it("names the first reason", () => {
    const b = bmsBalancing([block(1, 1, 3.45, 0.03, 0b01)], 1)!;
    expect(bmsWhy(b, 3.5, 3.44, "discharging")).toBe("discharging");
    expect(bmsWhy(b, 3.5, 3.44, "idle")).toBe("idleOff");
    expect(bmsWhy(b, 3.44, 3.4, "charging")).toBe("below");
    expect(bmsWhy(b, 3.5, 3.47, "charging")).toBe("below");
    expect(bmsWhy(b, 3.498, 3.439, "charging")).toBe("met");
    expect(bmsWhy({ ...b, enabled: false }, 3.498, 3.439, "charging")).toBe("disabled");
  });
});

describe("an external balancer now", () => {
  const v = { ...BALANCER_DEFAULTS, trigger_mv: 10, stop_mv: 5, start_mv: 3400, when: "charging" as const };
  it("works above its thresholds, is off below them and undecided between", () => {
    expect(extNow(v, 3.45, 3.43, 10)).toBe("on");
    expect(extNow(v, 3.45, 3.447, 10)).toBe("off");
    expect(extNow(v, 3.45, 3.443, 10)).toBe("between");
    expect(extNow(v, 3.39, 3.3, 10)).toBe("off");
    expect(extNow(v, 3.45, 3.43, -5)).toBe("off");
  });
});

describe("the blocks the balancing line explains with", () => {
  it("over MQTT only the feed's block, never a stored one", async () => {
    const { balancingBlocks, bmsBalancing } = await import("./balancingHint");
    const stored = [{ address: 0, at: 9_000, kind: "read", board: "current", bms_serial: null,
      parameters: { parameters: [{ index: 8, value: 3.4 }, { index: 62, value: 0.03 }], function_switches: [0, 0, 0, 0, 0, 0, 1] } }] as never[];
    const feed = { state: "live", boot: "b", boot_changed: false, skew_ms: 0, version_error: null,
      packs: [{ address: 1, device: null, battery_serial: null, bms_serial: null, parameters_at: 5,
        parameters: { parameters: [{ index: 8, value: 3.45 }, { index: 62, value: 0.02 }], function_switches: [0, 0, 0, 0, 0, 0, 3] } }] } as never;
    expect(balancingBlocks("bus", stored, feed)).toBe(stored);
    expect(bmsBalancing(balancingBlocks("mqtt", stored, feed), 0)).toBeNull();
    expect(bmsBalancing(balancingBlocks("mqtt", stored, feed), 1)).toMatchObject({ start: 3.45, spread: 0.02, enabled: true, idle: true, at: 5 });
    expect(balancingBlocks("mqtt", stored, null)).toEqual([]);
  });
});
