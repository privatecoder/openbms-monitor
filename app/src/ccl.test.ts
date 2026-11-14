import { describe, expect, it } from "vitest";
import type { MasterView, ProfileView } from "./api";
import { bankStep, masterKey, masterOffers, parseStep, writeLockedOf } from "./ccl";

const master = (read_caps: number | null, o: Partial<MasterView> = {}): MasterView => ({
  kind: "patched", version: "EMU1101-EXT Full 1.6.7", patch: "1.6", base: "16.06.13", build_id: "f445666c205184a8", listed: "EMU1101-EXT Full 1.6.7",
  caps: { status_split: true, uart1_params: true, uart1_identity: true, write_gate: true, start_gate: true, version: [1, 6], flags: 0xFE, read_caps },
  hard_stop: null, marker: "off", marker_word: null, slaves: 3, may_halt: true, may_release: false, ...o,
});
const profile = (m: MasterView, o: Partial<ProfileView> = {}): ProfileView => ({ gen: 4, bus: "can", endpoint: "x", master: m, gates: { write: true, start: true }, at: 100, ...o });

describe("CCL decrease step", () => {
  it("takes a whole number 1..20 only, nothing rounded", () => {
    expect([parseStep("1"), parseStep("10"), parseStep("20"), parseStep(" 7 ")]).toEqual([1, 10, 20, 7]);
    for (const bad of ["", "0", "21", "-1", "10.5", "10,0", "1e1", "+5", "abc", "100", "true", "0x0A"]) expect(parseStep(bad), bad).toBeNull();
  });
  it("is offered by the master's capability bit 4 on this connection, not by its version", () => {
    expect(masterOffers(profile(master(0x001F)))).toBe(true);
    // Light 1.5.6 and Full 1.6.6 report 000F; a 1.6.7 name with 000F is not enough
    expect(masterOffers(profile(master(0x000F)))).toBe(false);
    expect(masterOffers(profile(master(null)))).toBe(false);
    expect(masterOffers(profile(master(0x001F, { kind: "unknown-patch" })))).toBe(false);
    expect(masterOffers(profile(master(0x001F), { bus: "pack" }))).toBe(false);
    expect(masterOffers(null)).toBe(false);
  });
  it("a value belongs to the link, the profile read and the master's build", () => {
    const p = profile(master(0x1F));
    const k = masterKey("L1", p);
    expect(masterKey("L1", p)).toBe(k);
    expect(masterKey("L2", p)).not.toBe(k);
    expect(masterKey("L1", { ...p, gen: 5 })).not.toBe(k);
    expect(masterKey("L1", { ...p, at: 101 })).not.toBe(k);
    expect(masterKey("L1", profile(master(0x1F, { build_id: "0000000000000000" })))).not.toBe(k);
  });
  it("the write release as the profile last read it, open when stale", () => {
    expect(writeLockedOf(profile(master(0x1F)))).toBe(false);
    expect(writeLockedOf(profile(master(0x1F), { gates: { write: false, start: true } }))).toBe(true);
    expect(writeLockedOf(profile(master(0x1F), { gates: { write: false, start: true }, gates_stale: true }))).toBeNull();
    expect(writeLockedOf(profile(master(0x1F), { gates: null }))).toBeNull();
  });
  it("names the bank decrease per update, not per second", () => {
    expect(bankStep(12, 10)).toBe(120);
    expect(bankStep(12, 1)).toBe(12);
  });
});

const SOURCES = import.meta.glob(["./**/*.{ts,tsx}", "!./**/*.test.{ts,tsx}", "!./generated/**"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

describe("CCL decrease step: apart from every bulk path", () => {
  it("only its own section saves it; no parameter, file, multi-pack or restore code names it", () => {
    const savers = Object.entries(SOURCES).filter(([, src]) => src.includes("cclStepWrite(")).map(([p]) => p).sort();
    expect(savers).toEqual(["./components/CclStep.tsx"]);
    for (const p of ["./params.ts", "./paramWrite.ts", "./multiWrite.ts", "./installData.ts"]) {
      expect(SOURCES[p], p).toBeDefined();
      expect(SOURCES[p], p).not.toMatch(/ccl/i);
    }
    // the view only places the section
    expect(SOURCES["./views/Parameters.tsx"]).not.toMatch(/cclStepWrite|cclStepRead/);
  });
});
