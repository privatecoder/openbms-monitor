import { describe, expect, it } from "vitest";
import type { FeedState } from "./api";
import { applyFeed, type PackEntry } from "./store";

const entry = (address: number, serial: string): PackEntry => ({ address, telemetry: null, status: null, error: null, read_at: null, updated: 1, statusUpdated: 1,
  telemetryOk: true, statusOk: true, serials: { battery: { kind: "set", value: serial, truncated: false }, bms: { kind: "set", value: serial, truncated: false } } } as PackEntry);
const feed = (over: Partial<FeedState>): FeedState => ({ state: "live", boot: "b2", boot_changed: false, skew_ms: 0, version_error: null, packs: [], ...over });

describe("the add-on's feed in the live store", () => {
  it("a restart of the add-on drops every pack and identity, the new meta builds them again", () => {
    const prev = { 0: entry(0, "A-0"), 1: entry(1, "A-1"), 5: entry(5, "A-5") };
    const next = applyFeed(prev, feed({ boot_changed: true, packs: [
      { address: 0, device: null, battery_serial: null, bms_serial: null, parameters: null, parameters_at: null },
      { address: 1, device: null, battery_serial: null, bms_serial: null, parameters: null, parameters_at: null }] }));
    expect(Object.keys(next).map(Number)).toEqual([0, 1]);
    expect(next[0].serials).toBeUndefined();
    expect(next[0].updated).toBe(0);
  });

  it("within one boot keeps the values and adds what the meta knows", () => {
    const prev = { 0: entry(0, "A-0") };
    const next = applyFeed(prev, feed({ packs: [{ address: 0, device: null, battery_serial: { kind: "set", value: "B-0", truncated: false }, bms_serial: null, parameters: null, parameters_at: null }] }));
    expect(next[0].serials?.battery).toEqual({ kind: "set", value: "B-0", truncated: false });
    expect(next[0].updated).toBe(1);
  });
});
