import { describe, expect, it } from "vitest";
import type { Status } from "./api";
import { belowSocProtection, blockingAlarms, chargeFree, commandedOff, dischargeFree, energyText, errorText, heaterOn, offReasons, packBusy, remainingAllowed, remainingRaw, socOf } from "./control";

const status = (system_status: number, switch_state: number, alarms: [string, Status["alarms"][number]["severity"]][] = []): Status => ({
  address: 1, cell_limits: [], events: [0, 0, 0, 0, 0, 0, 0, 0], switch_state, balancing: 0, system_status,
  alarms: alarms.map(([id, severity]) => ({ id, key: id, severity })),
});

describe("state", () => {
  it("keeps the off command apart from the real outputs", () => {
    const s = status(0x20, 0x02);
    expect([commandedOff(s), dischargeFree(s), chargeFree(s), heaterOn(s)]).toEqual([true, false, true, false]);
    // not switched off, but the discharge MOSFET closed by a protection
    const p = status(0x00, 0x0a, [["alarm.ev2.b3", "protection"], ["alarm.ev2.b2", "warning"], ["alarm.ev4.b6", "info"]]);
    expect([commandedOff(p), dischargeFree(p), heaterOn(p)]).toEqual([false, false, true]);
    expect(blockingAlarms(p)).toEqual(["alarm.ev2.b3"]);
  });

  it("names undervoltage and SOC protection as reasons for switching off again", () => {
    expect(offReasons(status(0x20, 0, [["alarm.ev2.b7", "protection"], ["alarm.ev6.b3", "protection"], ["alarm.ev3.b1", "protection"]])))
      .toEqual(["alarm.ev2.b7", "alarm.ev6.b3"]);
    expect(offReasons(status(0x20, 0))).toEqual([]);
  });
});

describe("remaining capacity", () => {
  it("takes Ah with comma or point in 10 mAh", () => {
    expect(remainingRaw("140,5")).toBe(14050);
    expect(remainingRaw("0")).toBe(0);
    expect(remainingRaw(" 99.994 ")).toBe(9999);
    for (const bad of ["", "-1", "abc", "655.36"]) expect(remainingRaw(bad), bad).toBeNull();
  });

  it("allows 0 up to the full capacity", () => {
    expect(remainingAllowed(28000, 280)).toBe(true);
    expect(remainingAllowed(0, 280)).toBe(true);
    expect(remainingAllowed(28001, 280)).toBe(false);
    expect(remainingAllowed(100, null)).toBe(false);
    expect(remainingAllowed(null, 280)).toBe(false);
  });

  it("shows the SOC it gives and the SOC protection", () => {
    expect(socOf(14000, 280)).toBeCloseTo(50);
    expect(socOf(100, 0)).toBe(0);
    expect(belowSocProtection(4, 5)).toBe(true);
    expect(belowSocProtection(5, 5)).toBe(false);
    expect(belowSocProtection(4, null)).toBeNull();
  });
});

describe("errors", () => {
  const t = (k: string, o?: Record<string, unknown>) => (o ? `${k} ${JSON.stringify(o)}` : k);
  it("reads the pack reservation from the backend's JSON", () => {
    const e = JSON.stringify({ kind: "pack-busy", address: 3, action: "history" });
    expect(packBusy(e)).toEqual({ address: 3, action: "history" });
    expect(packBusy(JSON.stringify({ kind: "refused" }))).toBeNull();
    expect(packBusy("timeout")).toBeNull();
    expect(errorText(e, t)).toBe('ctl.busy {"pack":"03","action":"ctl.action.history"}');
    expect(errorText("timeout after 1500 ms", t)).toBe("timeout after 1500 ms");
  });

  it("writes energy in kWh or MWh", () => {
    expect(energyText(850, "de")).toBe("850 kWh");
    expect(energyText(4500, "de")).toBe("4,5 MWh");
  });
});
