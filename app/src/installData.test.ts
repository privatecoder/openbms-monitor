import { describe, expect, it } from "vitest";
import { collectLegacy, legacyKeys } from "./installData";
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
