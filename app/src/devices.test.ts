import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { BALANCER_DEFAULTS, addBalancer, assignPacks, assignedNow, editBalancer, fromForm, liveDefs, packsOf, parseDevices, removeBalancer, summary, toForm, valuesOf } from "./devices";

const PACKS = [0, 1, 2, 3];

describe("device settings", () => {
  it("read leniently: nothing or junk is no devices, a broken entry is left out", () => {
    expect(parseDevices(null)).toEqual({ defs: [], assign: {} });
    expect(parseDevices("x")).toEqual({ defs: [], assign: {} });
    const d = parseDevices({
      defs: [{ id: "a", name: "A", kind: "active-balancer", revs: [{ from: 1, trigger_mv: 8, when: "sometimes" }, { from: "x" }] }, { id: "b", kind: "other" }, 5],
      assign: { "2": [{ from: 3, device: "a" }, { from: "y" }], pack: [] },
    });
    expect(d.defs).toHaveLength(1);
    expect(d.defs[0].revs).toEqual([{ from: 1, start_mv: null, trigger_mv: 8, stop_mv: null, when: "always", max_a: null }]);
    expect(d.assign).toEqual({ "2": [{ from: 3, device: "a" }] });
  });

  it("keep revisions: settings and assignments in time, deleting ends the assignments", () => {
    let [d, id] = addBalancer(parseDevices(null), " Balancer 4 A ", BALANCER_DEFAULTS, 100);
    expect(d.defs[0]).toMatchObject({ id, name: "Balancer 4 A", kind: "active-balancer", profile: null, to: null });
    d = assignPacks(d, PACKS, id, 200);
    expect(PACKS.map((p) => assignedNow(d, p)?.id)).toEqual([id, id, id, id]);
    // assigning again adds nothing
    expect(assignPacks(d, PACKS, id, 250)).toEqual(d);
    d = assignPacks(d, [3], null, 300);
    expect(packsOf(d, id, PACKS)).toEqual([0, 1, 2]);
    expect(d.assign[3]).toEqual([{ from: 200, device: id }, { from: 300, device: null }]);
    // a settings change appends, a name change alone does not
    d = editBalancer(d, id, "Renamed", BALANCER_DEFAULTS, 400);
    expect(d.defs[0].name).toBe("Renamed");
    expect(d.defs[0].revs).toHaveLength(1);
    d = editBalancer(d, id, "Renamed", { ...BALANCER_DEFAULTS, trigger_mv: 12, when: "charging" }, 500);
    expect(d.defs[0].revs.map((r) => [r.from, r.trigger_mv])).toEqual([[100, 5], [500, 12]]);
    expect(valuesOf(d.defs[0]).when).toBe("charging");
    // two changes in the same millisecond: one revision
    d = editBalancer(d, id, "Renamed", { ...BALANCER_DEFAULTS, trigger_mv: 15 }, 500);
    expect(d.defs[0].revs.map((r) => [r.from, r.trigger_mv])).toEqual([[100, 5], [500, 15]]);
    d = removeBalancer(d, id, PACKS, 600);
    expect(d.defs[0].to).toBe(600);
    expect(liveDefs(d)).toEqual([]);
    expect(PACKS.map((p) => assignedNow(d, p))).toEqual([null, null, null, null]);
    expect(d.assign[0]).toEqual([{ from: 200, device: id }, { from: 600, device: null }]);
    // a pack without it gets no revision
    expect(d.assign[3]).toEqual([{ from: 200, device: id }, { from: 300, device: null }]);
    // the stored value reads back the same
    expect(parseDevices(JSON.parse(JSON.stringify(d)))).toEqual(d);
  });

  it("one definition, another assigned over it", () => {
    let [d, a] = addBalancer(parseDevices(null), "A", BALANCER_DEFAULTS, 1);
    let b: string;
    [d, b] = addBalancer(d, "B", BALANCER_DEFAULTS, 1);
    expect(a).not.toBe(b);
    d = assignPacks(assignPacks(d, [0, 1], a, 10), [1], b, 20);
    expect([assignedNow(d, 0)?.name, assignedNow(d, 1)?.name]).toEqual(["A", "B"]);
  });

  it("say their settings in words", () => {
    const de = i18n.getFixedT("de");
    expect(summary(BALANCER_DEFAULTS, de)).toBe("ab 5 mV Spreizung, immer");
    const en = i18n.getFixedT("en");
    // numbers in the system's format (fmt)
    expect(summary({ start_mv: 3400, trigger_mv: 10, stop_mv: 3, when: "charging", max_a: 4 }, en)).toMatch(/^from 10 mV spread, until 3 mV, highest cell from 3[.,]400 V, while charging, 4 A$/);
    // a stop at or above the trigger is no hysteresis and is not shown
    expect(summary({ ...BALANCER_DEFAULTS, stop_mv: 9 }, en)).toBe("from 5 mV spread, always");
  });

  it("take the editor's fields only in range", () => {
    const f = toForm("A", BALANCER_DEFAULTS);
    expect(f).toEqual({ name: "A", trigger_mv: "5", stop_mv: "", start_mv: "", when: "always", max_a: "" });
    expect(fromForm(f)).toEqual(BALANCER_DEFAULTS);
    expect(fromForm({ ...f, stop_mv: "3", start_mv: "3400", max_a: "4,5", when: "charging" })).toEqual({ trigger_mv: 5, stop_mv: 3, start_mv: 3400, when: "charging", max_a: 4.5 });
    for (const bad of [{ trigger_mv: "" }, { trigger_mv: "0" }, { trigger_mv: "2.5" }, { stop_mv: "x" }, { start_mv: "9000" }, { max_a: "-1" }]) expect(fromForm({ ...f, ...bad })).toBeNull();
  });
});
