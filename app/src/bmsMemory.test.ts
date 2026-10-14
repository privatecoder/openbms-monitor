import { describe, expect, it, vi } from "vitest";
import type { BmsHistoryRead, BmsHistoryRecord } from "./api";
import { clockDiff, csvName, currentRun, lastRead, noteProgress, startHistoryRead, durationText, fmtBmsTime, fromBmsTime, historyCsv, readMark, setHistoryReading, summary, toBmsTime, utcOffset, zoneLabel } from "./bmsMemory";

const T = { year: 2026, month: 3, day: 9, hour: 7, minute: 5, second: 30 };

const rec = (over: Partial<BmsHistoryRecord> = {}): BmsHistoryRecord => ({
  time: T, system_status: 2, events: [0, 3, 0, 0, 0, 0, 0, 0],
  alarms: [{ id: "alarm.ev2.b0", key: "cell_high_voltage_warning", severity: "warning" }, { id: "alarm.ev2.b1", key: "cell_overvoltage_protection", severity: "protection" }],
  cell_voltages: Array.from({ length: 16 }, (_, i) => 3.3 + i / 1000).map((v, i) => (i === 5 ? 3.25 : v)),
  cell_temperatures: [21, 24.5, 22, 23], ambient_temperature: 20, power_temperature: 30, current: -12.34, pack_voltage: 52.81,
  remaining_capacity_ah: 140, full_capacity_ah: 280, soc: 50, rated_capacity_ah: 280, cycles: 123, soh: 100, port_voltage: 52.9, ...over,
});

describe("clock", () => {
  it("converts local PC time to BMS fields and back", () => {
    const d = new Date(2026, 9, 14, 11, 32, 5);
    expect(toBmsTime(d)).toEqual({ year: 2026, month: 10, day: 14, hour: 11, minute: 32, second: 5 });
    expect(fromBmsTime(toBmsTime(d)).getTime()).toBe(d.getTime());
  });

  it("formats every field, DE and EN", () => {
    expect(fmtBmsTime(T, "de")).toBe("09.03.2026 07:05:30");
    expect(fmtBmsTime(T, "en")).toBe("2026-03-09 07:05:30");
  });

  it("difference: + = BMS ahead", () => {
    const pc = fromBmsTime(T);
    expect(clockDiff(T, new Date(pc.getTime() + 192_000))).toBe(-192);
    expect(clockDiff(T, new Date(pc.getTime() - 5_000))).toBe(5);
  });

  it("durations in at most two units", () => {
    expect(durationText(-192)).toBe("3 min 12 s");
    expect(durationText(0)).toBe("0 s");
    expect(durationText(3600 * 2 + 300 + 7)).toBe("2 h 5 min");
    expect(durationText(86_400 * 4 + 3 * 3600)).toBe("4 d 3 h");
    expect(durationText(60)).toBe("1 min");
  });

  it("zone label: short name with offset, or only the offset", () => {
    const summer = new Date(Date.UTC(2026, 6, 1, 12));
    expect(zoneLabel(summer, "de", "Europe/Berlin")).toMatch(/^MESZ, UTC[+−]\d/);
    expect(zoneLabel(summer, "en", "Etc/GMT-3")).toMatch(/^UTC/);
    expect(utcOffset(summer)).toMatch(/^UTC[+−]\d+(:\d\d)?$/);
  });
});

describe("history records", () => {
  it("summary: lowest/highest cell with number, spread, warmest sensor", () => {
    expect(summary(rec())).toEqual({ min: 3.25, minCell: 6, max: 3.315, maxCell: 16, spreadMv: 65, maxTemp: 24.5 });
  });

  it("CSV has every field, one row per record, oldest first", () => {
    const csv = historyCsv(3, [rec(), rec({ time: { ...T, hour: 8 }, alarms: [] })]).trim().split("\n");
    expect(csv).toHaveLength(3);
    const head = csv[0].split(",");
    expect(head.slice(0, 4)).toEqual(["pack", "bms_time", "system_status", "event1"]);
    expect(head).toContain("cell16_v");
    expect(head).toContain("cell_temp4_c");
    expect(head.at(-1)).toBe("port_voltage_v");
    const row = csv[1].split(",");
    expect(row.length).toBe(head.length);
    expect(row.slice(0, 2)).toEqual(["3", "2026-03-09 07:05:30"]);
    expect(row[head.indexOf("alarms")]).toBe("cell_high_voltage_warning cell_overvoltage_protection");
    expect(row[head.indexOf("current_a")]).toBe("-12.34");
    expect(csv[2].split(",")[1]).toBe("2026-03-09 08:05:30");
  });

  it("file name names the pack and the time", () => {
    expect(csvName(4, new Date(2026, 9, 14, 9, 7))).toBe("bms-history-pack04-20261014-0907.csv");
  });

  it("read mark covers the read and its end", () => {
    expect(readMark()).toBe(0);
    setHistoryReading(true);
    expect(Math.abs(readMark() - Date.now())).toBeLessThan(50);
    setHistoryReading(false);
    expect(readMark()).toBeGreaterThan(0);
  });
});

describe("history read runner", () => {
  const done = (address: number, n = 1): BmsHistoryRead => ({ address, records: Array.from({ length: n }, () => rec()), end: "last", error: null });

  it("a second start re-attaches to the running read; only its end clears the run and the read mark", async () => {
    let finish!: (r: BmsHistoryRead) => void;
    const read = vi.fn(() => new Promise<BmsHistoryRead>((res) => { finish = res; }));
    const a = startHistoryRead(2, read);
    noteProgress(5);
    const b = startHistoryRead(3, read);
    expect(b).toBe(a);
    expect(read).toHaveBeenCalledTimes(1);
    expect(currentRun()).toMatchObject({ address: 2, n: 5 });
    expect(Math.abs(readMark() - Date.now())).toBeLessThan(50);
    finish(done(2, 3));
    expect((await a.promise).records).toHaveLength(3);
    expect(currentRun()).toBeNull();
    expect(lastRead()?.records).toHaveLength(3);
  });

  it("an error, also 'already running', becomes the result and ends the run", async () => {
    const failed = startHistoryRead(1, () => Promise.reject("timeout"));
    expect(await failed.promise).toMatchObject({ end: "error", error: "timeout", records: [] });
    const other = startHistoryRead(1, () => Promise.reject("a history read is already running"));
    expect(await other.promise).toMatchObject({ end: "error", records: [] });
    expect(currentRun()).toBeNull();
  });
});
