import { describe, expect, it } from "vitest";
import { collectLegacy, legacyKeys, queueCellTypes, serialWriter, settingWriter, writeCellTypes, type CellQueue } from "./installData";
import { parseGroups } from "./groups";
import { parsePairs } from "./pairs";
import { parseAssignments } from "./cells/packCheck";
import { parseDiag } from "./diag/store";

/** A Storage stand-in with the given entries. */
function storage(entries: Record<string, string>) {
  const keys = Object.keys(entries);
  return { length: keys.length, key: (i: number) => keys[i] ?? null, getItem: (k: string) => entries[k] ?? null };
}

describe("the old browser storage", () => {
  it("is collected per site key, other keys untouched", () => {
    const s = storage({
      "groups:gw-a:4196": "[]", "pairs:gw-a:4196": "[1]", "cellTypes:/dev/cu.usb-X": "{}", "diag:gw-a:4196": "{\"offsets\":{}}",
      "maint.installation": "inst-1", "conn.tcp": "gw-a:4196", "groups:": "[2]",
    });
    expect(collectLegacy(s)).toEqual({
      "gw-a:4196": { groups: "[]", pairs: "[1]", diag: "{\"offsets\":{}}" },
      "/dev/cu.usb-X": { cellTypes: "{}" },
      "": { groups: "[2]" },
    });
    expect(legacyKeys("gw-a:4196")).toEqual(["cellTypes:gw-a:4196", "groups:gw-a:4196", "pairs:gw-a:4196", "diag:gw-a:4196"]);
  });
});

describe("stored settings", () => {
  it("keep only well-formed entries", () => {
    expect(parseGroups([{ name: "T1", packs: [0, 1] }, { name: 3, packs: [] }, null, { name: "T2", packs: ["x"] }])).toEqual([{ name: "T1", packs: [0, 1] }]);
    expect(parseGroups(null)).toEqual([]);
    expect(parsePairs([{ plus: 0, minus: 1 }, { plus: "a", minus: 2 }])).toEqual([{ plus: 0, minus: 1 }]);
    expect(parseAssignments({ "3": { cell: "syn", parallel: 2 }, x: { cell: "syn", parallel: 1 }, "4": { cell: "syn", parallel: 0 } })).toEqual({ 3: { cell: "syn", parallel: 2 } });
    const d = parseDiag({ offsets: { 1: { inner: 0.1, outer: 2 } }, snapshots: [{ at: 1 }, { at: 2, point: "bus", refs: {}, rows: {} }] });
    expect(d.offsets[1].outer).toEqual({ bus: 2 });
    expect(d.snapshots.map((x) => x.at)).toEqual([2]);
    expect(parseDiag("nope")).toEqual({ offsets: {}, snapshots: [] });
  });
});

describe("serial writes", () => {
  it("one write at a time, of the values given meanwhile only the latest, then idle", async () => {
    const started: number[] = [], finish: ((ok: boolean) => void)[] = [];
    let idle = 0;
    const write = serialWriter((v: number) => { started.push(v); return new Promise<void>((res, rej) => finish.push((ok) => (ok ? res() : rej(new Error("no"))))); }, () => idle++);
    const tick = () => new Promise((r) => setTimeout(r, 0));
    const a = write(1), b = write(2), c = write(3);
    expect(started).toEqual([1]);
    finish[0](true);
    expect(await a).toBeNull();
    await tick();
    // 2 was replaced by 3 while 1 was in flight: never written, it shares 3's result
    expect(started).toEqual([1, 3]);
    expect(idle).toBe(0);
    finish[1](false);
    const [rb, rc] = await Promise.all([b, c]);
    expect(String(rb)).toContain("no");
    expect(rc).toBe(rb);
    await tick();
    expect(idle).toBe(1);
    const d = write(4);
    expect(started).toEqual([1, 3, 4]);
    finish[2](true);
    expect(await d).toBeNull();
    await tick();
    expect(idle).toBe(2);
  });
});

describe("cell type writes", () => {
  it("a failed write gives its error and puts the stored cell types back", async () => {
    let shown: Record<string, unknown> = { 3: { cell: "new", parallel: 1 } };
    const restore = (s: Record<string, unknown>) => { shown = s; };
    const stored = { 3: { cell: "old", parallel: 1 } };
    const err = await writeCellTypes(() => Promise.reject("disk full"), async () => stored, {}, restore);
    expect(err).toBe("disk full");
    expect(shown).toEqual(stored);
    // the read back fails too: the cell types from before the change, never the unsaved ones
    shown = { 3: { cell: "new", parallel: 1 } };
    expect(await writeCellTypes(() => Promise.reject("disk full"), () => Promise.reject("gone"), { 3: { cell: "before", parallel: 1 } }, restore)).toBe("disk full");
    expect(shown).toEqual({ 3: { cell: "before", parallel: 1 } });
    // a write that goes through changes nothing back
    shown = { 3: { cell: "new", parallel: 1 } };
    expect(await writeCellTypes(async () => undefined, async () => stored, {}, restore)).toBeNull();
    expect(shown).toEqual({ 3: { cell: "new", parallel: 1 } });
  });
});

describe("cell type writes of one installation", () => {
  it("run one after the other, and a late failure never rolls back a newer change", async () => {
    const q: CellQueue = { tail: Promise.resolve(), gen: 0, stale: false };
    const log: string[] = [];
    let shown = "b";
    let failFirst: (e: unknown) => void = () => {};
    const first = queueCellTypes(q, () => { log.push("write a"); return new Promise((_, rej) => { failFirst = rej; }); },
      async () => { log.push("read"); return { v: "stored" }; }, { v: "before-a" }, (b) => { shown = String(b.v); });
    const second = queueCellTypes(q, async () => { log.push("write b"); }, async () => { log.push("read"); return { v: "stored" }; }, { v: "a" }, (b) => { shown = String(b.v); });
    await new Promise((r) => setTimeout(r, 0));
    // the second waits for the first
    expect(log).toEqual(["write a"]);
    failFirst("disk full");
    expect(await first).toBe("disk full");
    // the failed first did not put back its state over the newer change shown
    expect(shown).toBe("b");
    expect(await second).toBeNull();
    // the latest went through and the queue was stale: the stored state is read back
    expect(log).toEqual(["write a", "read", "write b", "read"]);
    expect(shown).toBe("stored");
  });
});

describe("an unresolved cell type cache", () => {
  it("stays marked until a read back succeeds, and the next write tries again", async () => {
    const q: CellQueue = { tail: Promise.resolve(), gen: 0, stale: true };
    let shown = "both", reads = 0;
    const restore = (b: Record<string, unknown>) => { shown = String(b.v); };
    // the write goes through, the read back fails: still unresolved
    const failing = async () => { reads++; throw new Error("locked"); };
    expect(await queueCellTypes(q, async () => undefined, failing, { v: "x" }, restore)).toBeNull();
    expect(shown).toBe("both");
    expect(q.stale).toBe(true);
    // the next write reads again; now it works and the cache is the stored state
    expect(await queueCellTypes(q, async () => undefined, async () => { reads++; return { v: "stored" }; }, { v: "x" }, restore)).toBeNull();
    expect(q.stale).toBe(false);
    expect(shown).toBe("stored");
    expect(reads).toBe(2);
  });
});

describe("quick changes of one setting (packs: count, then its number)", () => {
  // a store whose writes finish when told: in order, or the first one failing
  function harness(failFirst: boolean) {
    let db: unknown = null;
    let shown: unknown = null;
    const waits: (() => void)[] = [];
    let n = 0;
    const w = settingWriter(`t${Math.random()}`, "packs", {
      write: (v) => { const i = n++; return new Promise((ok, no) => waits.push(() => { if (failFirst && i === 0) no(new Error("store")); else { db = v; ok(undefined); } })); },
      read: async () => db,
      put: (v) => { shown = v; },
    });
    return { w, waits, db: () => db, shown: () => shown };
  }
  const flush = () => new Promise((r) => setTimeout(r, 0));
  for (const failFirst of [false, true]) {
    it(`the last choice is stored and shown${failFirst ? " also when the first write fails" : ""}`, async () => {
      const h = harness(failFirst);
      const first = h.w({ mode: "count", n: 1 });
      const second = h.w({ mode: "count", n: 12 });
      // only one write in flight: the second waits for the first
      expect(h.waits.length).toBe(1);
      h.waits[0](); await flush();
      expect(h.waits.length).toBe(2);
      h.waits[1](); await flush(); await flush();
      expect(await second).toBeNull();
      expect(await first).toEqual(failFirst ? expect.any(Error) : null);
      expect(h.db()).toEqual({ mode: "count", n: 12 });
      expect(h.shown()).toEqual({ mode: "count", n: 12 });
    });
  }
});
