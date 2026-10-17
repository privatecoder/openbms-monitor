import { describe, expect, it } from "vitest";
import type { BalancerSlice } from "./api";
import { bmsMark, extMark, shares } from "./balancerTimeline";

const slice = (o: Partial<BalancerSlice>): BalancerSlice =>
  ({ from: 0, to: 60_000, active: 0, inactive: 0, unknown: 0, unassigned: 0, bal: 0, idle: 0, no_status: 0, cells: 0, minutes: false, ...o });

describe("balancer timeline", () => {
  it("shows a short run over a longer quiet time, and a gap where nothing was recorded", () => {
    expect(extMark(slice({ active: 10_000, inactive: 30_000 }))).toEqual({ state: "active", share: 0.25 });
    expect(extMark(slice({ unknown: 20_000, inactive: 10_000 }))).toEqual({ state: "unknown", share: 2 / 3 });
    expect(extMark(slice({ unassigned: 60_000 }))).toEqual({ state: "none", share: 1 });
    expect(extMark(slice({}))).toEqual({ state: "gap", share: 0 });
    expect(bmsMark(slice({ bal: 6_000, idle: 54_000 }))).toEqual({ state: "bal", share: 0.1 });
    expect(bmsMark(slice({ no_status: 40_000, idle: 20_000 }))).toEqual({ state: "noStatus", share: 2 / 3 });
    expect(bmsMark(slice({}))).toEqual({ state: "gap", share: 0 });
  });

  it("counts shares only over the time that can say something", () => {
    expect(shares(slice({ active: 10, inactive: 30, unknown: 10, unassigned: 50, bal: 5, idle: 15, no_status: 20 }))).toEqual({ active: 0.2, unknown: 0.2, bal: 0.25, noStatus: 0.5 });
    expect(shares(slice({ unassigned: 50 }))).toEqual({ active: null, unknown: null, bal: null, noStatus: null });
  });
});
