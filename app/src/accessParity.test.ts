import { describe, expect, it } from "vitest";
import type { PackPatch, PatchCaps } from "./api";
import { cutoffBits, gateBits, keepMask, writeLocked } from "./profile";
import { clockAccess, controlMode } from "./power";

// The rules as they stood before the catalog (`access.json`), kept here only as the reference the catalog's evaluation
// must match for every input.
const old = {
  gateBits(patch: PackPatch | null, uart1: boolean) {
    if (!patch) return { named: false, writeEditable: false, startEditable: false, powerEditable: false, power: null };
    const power = patch.caps.power_rtc ?? null;
    return { named: true, writeEditable: patch.caps.write_gate, startEditable: !uart1 && patch.caps.start_gate, powerEditable: !uart1 && power !== false, power };
  },
  cutoffBits(patch: PackPatch | null, uart1: boolean, group0: number | undefined) {
    if (!patch) return { cutoff: null, cutoffEditable: false };
    const cutoff = patch.caps.pack_cutoff ?? null;
    return { cutoff, cutoffEditable: uart1 ? cutoff === true && group0 !== undefined && (group0 & 1) === 1 : cutoff !== false };
  },
  keepMask(patch: PackPatch | null, uart1: boolean, group0: number | undefined) {
    const gate = old.gateBits(patch, uart1), cut = old.cutoffBits(patch, uart1, group0);
    const fixed = (gate.writeEditable ? 0 : 1) | (gate.startEditable ? 0 : 4) | (gate.powerEditable ? 0 : 8) | (cut.cutoffEditable ? 0 : 0x10);
    return 0x20 | (uart1 || gate.named ? fixed : 0);
  },
  writeLocked: (patch: PackPatch | null, uart1: boolean, group0: number | undefined) => uart1 && !!patch?.caps.write_gate && group0 !== undefined && (group0 & 1) === 0,
  controlMode(w: { connected: boolean; bus: "pack" | "can" | null; demo: boolean; installation: boolean; power: boolean }) {
    if (!w.connected) return null;
    if (w.demo || (w.bus === "pack" && w.installation)) return "full";
    return w.bus === "can" && w.installation && w.power ? "power" : null;
  },
  clockAccess(w: { connected: boolean; bus: "pack" | "can" | null; demo: boolean; installation: boolean; power: boolean; locked: boolean | null }) {
    const can = w.connected && w.bus === "can" && !w.demo;
    const read = w.connected && (w.bus === "pack" || w.demo || (can && w.power));
    const write = read && (w.demo || w.installation) && !(can && w.locked === true);
    return { read, write, unsupported: can && !w.power };
  },
};

const tris = [true, false, null] as const;
const patches: (PackPatch | null)[] = [null];
for (const write_gate of [true, false]) for (const start_gate of [true, false]) for (const power_rtc of tris) for (const pack_cutoff of tris) {
  const caps = { status_split: true, uart1_params: true, uart1_identity: true, write_gate, start_gate, power_rtc, pack_cutoff } as PatchCaps;
  patches.push({ source: "live", release: "16.06.13 MAINT 1.6", caps, at: null });
}
const groups = [undefined, ...Array.from({ length: 256 }, (_, i) => i)];

describe("the catalog's Group0 and button rules match the rules before it", () => {
  it("Group0 bits of a column, for every proof, bus and Group0", () => {
    for (const p of patches) for (const uart1 of [false, true]) {
      expect(gateBits(p, uart1)).toEqual(old.gateBits(p, uart1));
      for (const g of groups) {
        const at = JSON.stringify({ caps: p?.caps ?? null, uart1, g });
        expect(cutoffBits(p, uart1, g), at).toEqual(old.cutoffBits(p, uart1, g));
        expect(keepMask(p, uart1, g), at).toBe(old.keepMask(p, uart1, g));
        expect(writeLocked(p, uart1, g), at).toBe(old.writeLocked(p, uart1, g));
      }
    }
  });
  it("„Steuern“ and the clock line, for every connection state", () => {
    const bools = [true, false];
    for (const connected of bools) for (const bus of ["pack", "can", null] as const) for (const demo of bools) for (const installation of bools) for (const power of bools) {
      const w = { connected, bus, demo, installation, power };
      const now = { ...w, fw: { patch: { kind: "version" as const, version: [1, 5] as [number, number] }, flags: power ? 2 : 0 } };
      expect(controlMode(now), JSON.stringify(w)).toBe(old.controlMode(w));
      for (const locked of tris) expect(clockAccess({ ...now, locked }), JSON.stringify({ ...w, locked })).toEqual(old.clockAccess({ ...w, locked }));
    }
  });
});
