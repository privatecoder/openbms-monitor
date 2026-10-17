import { describe, expect, it } from "vitest";
import { current, freshSystem, omittedStatus, packFresh, staleAfter, STALE_FALLBACK_MS, type Timed } from "./freshness";
import type { Status, SystemValues, Telemetry } from "./api";

const tm = { soc: 50 } as unknown as Telemetry;
const st = { alarms: [] } as unknown as Status;
const entry = (o: Partial<Timed> = {}): Timed => ({ telemetry: tm, status: st, updated: 1000, statusUpdated: 1000, telemetryOk: true, statusOk: true, interval: 4000, ...o });

describe("freshness of a pack", () => {
  it("two rounds, else 30 s", () => {
    expect(staleAfter(4000)).toBe(8000);
    expect(staleAfter(1000)).toBe(5000);
    expect(staleAfter(undefined)).toBe(STALE_FALLBACK_MS);
  });
  it("fresh while the last polls succeeded and are young", () => {
    expect(packFresh(entry(), 9000)).toEqual({ telemetry: true, status: true });
    expect(packFresh(entry(), 9001)).toEqual({ telemetry: false, status: false });
  });
  it("a failed poll makes the kept values old at once, telemetry and status apart", () => {
    expect(packFresh(entry({ telemetryOk: false }), 2000)).toEqual({ telemetry: false, status: true });
    expect(packFresh(entry({ statusOk: false }), 2000)).toEqual({ telemetry: true, status: false });
    // status not answered for long while telemetry goes on
    expect(packFresh(entry({ updated: 20_000, statusUpdated: 1000 }), 20_500)).toEqual({ telemetry: true, status: false });
  });
  it("the counted entry leaves old values out and says which", () => {
    const c = current(entry({ telemetryOk: false }), 2000);
    expect(c.telemetry).toBeNull();
    expect(c.status).toBe(st);
    expect(c.old).toEqual({ telemetry: true, status: false });
    // nothing ever answered: nothing old either
    expect(current(entry({ telemetry: null, status: null, updated: 0, statusUpdated: 0 }), 2000).old).toEqual({ telemetry: false, status: false });
  });
});

describe("the head's note on old statuses", () => {
  it("counts the packs whose status was left out", () => {
    const now = 2000;
    const list = [current(entry(), now), current(entry({ statusOk: false }), now), current(entry({ statusUpdated: 0, status: null }), now)];
    expect(omittedStatus(list)).toBe(1);
  });
});

describe("freshness of the system values", () => {
  const v = { charge_allowed: true } as unknown as SystemValues;
  it("only a successful recent poll counts", () => {
    expect(freshSystem({ values: v, error: null, ok: true, updated: 1000 }, 5000)).toBe(v);
    expect(freshSystem({ values: v, error: "timeout", ok: false, updated: 1000 }, 2000)).toBeNull();
    expect(freshSystem({ values: v, error: null, ok: true, updated: 1000 }, 1000 + STALE_FALLBACK_MS + 1)).toBeNull();
    expect(freshSystem(null, 0)).toBeNull();
  });
});
