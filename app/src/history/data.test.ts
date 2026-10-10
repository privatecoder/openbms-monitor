import { describe, expect, it } from "vitest";
import { RecordingParser, makeGrid, onGrid, parseRecording, shares, typicalInterval } from "./data";

const pack = (t: number, address: number, current: number, cells = [3.3, 3.31, 3.29]) =>
  JSON.stringify({ t, pack: { address, telemetry: { address, cell_voltages: cells, cell_temperatures: [20, 22], current, soc: 50, pack_voltage: 52.8, idle_current_ma: 120 }, status: null, error: null } });
const system = (t: number) =>
  JSON.stringify({ t, system: { values: { current: 40, soc: 50, voltage: 52.8, charge_current_limit: 150, discharge_current_limit: 200, charge_voltage_limit: 56, highest_cell_voltage: 3.31, lowest_cell_voltage: 3.29 }, error: null } });

const text = [
  JSON.stringify({ t: 0, start: { packs: [], system: true, interval_s: 0, bus: "can" } }),
  pack(1000, 0, 10), pack(1500, 1, 30), system(1800),
  pack(4000, 0, 0), JSON.stringify({ t: 4500, pack: { address: 1, telemetry: null, status: null, error: "timeout" } }),
  '{"t":5000,"pack":{"addr', // cut off by a crash
].join("\n");

describe("parseRecording", () => {
  it("reads packs, system values, errors and a cut-off last line", () => {
    const r = parseRecording(text);
    expect(r.start?.bus).toBe("can");
    expect([...r.packs.keys()]).toEqual([0, 1]);
    expect(r.packs.get(0)!.current).toEqual([10, 0.12]); // idle current where the regular value is 0
    expect(r.packs.get(0)!.cellMax).toEqual([3.31, 3.31]);
    expect(r.packs.get(0)!.cells[2]).toEqual([3.29, 3.29]);
    expect(r.system.ccl).toEqual([150]);
    expect([r.from, r.to, r.errors, r.skipped]).toEqual([1000, 4000, 1, 1]);
  });
  it("gives the same result in pieces split anywhere", () => {
    const p = new RecordingParser();
    for (let i = 0; i < text.length; i += 7) p.push(text.slice(i, i + 7));
    expect(p.finish()).toEqual(parseRecording(text));
  });
});

describe("grid", () => {
  it("averages per bucket and bridges only short holes", () => {
    const t = [0, 1000, 2000, 6000, 7000, 20000], v = [1, 3, 5, 7, 9, 11];
    const g = makeGrid(0, 20000, 1000, 10); // step 2000
    expect(g.step).toBe(2000);
    expect(onGrid(t, v, g)).toEqual([2, 5, null, 8, null, null, null, null, null, 11]);
    expect(onGrid(t, v, g, "mean", 2000)).toEqual([2, 5, 5, 8, null, null, null, null, null, 11]);
    expect(onGrid(t, v, g, "max")[0]).toBe(3);
  });
  it("finds the typical interval", () => {
    expect(typicalInterval(parseRecording([pack(0, 0, 1), pack(3000, 0, 1), pack(6000, 0, 1), pack(9500, 0, 1)].join("\n")))).toBe(3000);
  });
});

describe("shares", () => {
  it("relates each pack to the mean, only under real load", () => {
    const s = shares(new Map([[0, [10, 1, null]], [1, [30, 3, 5]]]));
    expect(s.get(0)).toEqual([0.5, null, null]);
    expect(s.get(1)).toEqual([1.5, null, null]);
  });
  it("uses the packs that answered when most did", () => {
    const s = shares(new Map([[0, [10]], [1, [20]], [2, [30]], [3, [null]]]));
    expect(s.get(0)).toEqual([0.5]);
    expect(s.get(3)).toEqual([null]);
  });
});
