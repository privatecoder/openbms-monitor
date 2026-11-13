import { describe, expect, it } from "vitest";
import type { MasterView, PackProbeView, PatchCaps, ProfileView } from "./api";
import { clockAccess, clockTargets, controlHint, controlMode, masterBroadcast, packCutoff, powerLocked, powerRtc } from "./power";
import { inverseG0 } from "./profile";

const caps = (power_rtc: boolean | null) => ({ status_split: true, uart1_params: true, uart1_identity: true, write_gate: true, start_gate: true, power_rtc });
/** The version and image flags a 64/03 with these capabilities reports (null flags: not known). */
const reported = (power: boolean | null | undefined, cutoff: boolean | null | undefined) => ({
  version: (cutoff ? [1, 6] : [1, 5]) as [number, number],
  flags: power === null || power === undefined ? null : (power ? 2 : 0) | (cutoff ? 4 : 0),
});
const withReport = (c: PatchCaps | null) => (c ? { ...reported(c.power_rtc, c.pack_cutoff), ...c } : c);
const master = (m: Partial<MasterView>): MasterView => {
  const v: MasterView = { kind: "patched", version: "EMU1101-EXT Light 1.5.4", patch: "1.5", base: "16.06.13", build_id: "0011223344556677",
    listed: null, caps: caps(true), hard_stop: false, marker: "off", marker_word: "FFFFFFFF", slaves: 3, may_halt: true, may_release: false, ...m };
  return { ...v, caps: withReport(v.caps) };
};
const profile = (m: Partial<MasterView> = {}, bus: "can" | "pack" = "can", power: boolean | null = null): ProfileView =>
  ({ gen: 4, bus, endpoint: "tcp:gw:4196", master: master(m), gates: power === null ? null : { write: true, start: true, power }, at: 0 });
const probe = (p: Partial<PackProbeView>): PackProbeView => {
  const v: PackProbeView = { gen: 4, address: 3, state: "proven", version: "EMU1101-EXT Light 1.5.4", listed: null, build_id: null, hard_stop: false, power_rtc: true, ...p };
  const r = reported(v.power_rtc, v.pack_cutoff);
  return v.state === "proven" ? { patch_version: r.version, flags: r.flags, ...v } : v;
};
/** The firmware behind a proven (or not) power and clock flag. */
const fw = (power: boolean) => ({ patch: { kind: "version" as const, version: [1, 5] as [number, number] }, flags: power ? 2 : 0, caps: 0 });
const where = { connected: true, bus: "can" as const, demo: false, installation: true };

describe("power and clock over the CAN socket", () => {
  it("takes the image flag only as this connection proved it for exactly this pack", () => {
    expect(powerRtc(0, profile(), undefined)).toBe(true);
    expect(powerRtc(0, profile({ caps: caps(false) }), undefined)).toBe(false);
    // a master whose 64/03 says nothing about it, or not recognised
    expect(powerRtc(0, profile({ caps: caps(null) }), undefined)).toBe(false);
    expect(powerRtc(0, profile({ kind: "unknown-patch", caps: null }), undefined)).toBe(false);
    // RS485-1/2 has no such service, and no profile flag
    expect(powerRtc(0, profile({}, "pack"), undefined)).toBe(false);
    expect(powerRtc(0, null, undefined)).toBe(false);
    // a slave by its own 64/03 only, never by the master's
    expect(powerRtc(3, profile(), probe({}))).toBe(true);
    expect(powerRtc(3, profile(), undefined)).toBe(false);
    expect(powerRtc(3, profile(), probe({ power_rtc: false }))).toBe(false);
    expect(powerRtc(3, profile(), probe({ state: "undecided", power_rtc: null }))).toBe(false);
    // a finding of an earlier connection
    expect(powerRtc(3, profile(), probe({ gen: 3 }))).toBe(false);
    expect(powerRtc(null, profile(), probe({}))).toBe(false);
  });

  it("reads the lock from the newest state seen", () => {
    expect(powerLocked(3, 0xff, profile())).toBe(true);
    expect(powerLocked(3, 0xf7, profile())).toBe(false);
    expect(powerLocked(3, undefined, profile({}, "can", true))).toBeNull();
    // the master falls back on the profile's release bits
    expect(powerLocked(0, undefined, profile({}, "can", false))).toBe(true);
    expect(powerLocked(0, undefined, profile({}, "can", true))).toBe(false);
    expect(powerLocked(0, undefined, profile())).toBeNull();
    // a column read on this connection wins over the profile
    expect(powerLocked(0, 0xf7, profile({}, "can", false))).toBe(false);
  });

  it("offers only restart and ein/aus over the CAN socket, everything on RS485-1/2", () => {
    expect(controlMode({ ...where, bus: "pack", fw: fw(false) })).toBe("full");
    expect(controlMode({ ...where, fw: fw(true) })).toBe("power");
    expect(controlMode({ ...where, fw: fw(false) })).toBeNull();
    // „Nur ansehen“ writes nothing to a pack
    expect(controlMode({ ...where, installation: false, fw: fw(true) })).toBeNull();
    expect(controlMode({ ...where, bus: "pack", installation: false, fw: fw(false) })).toBeNull();
    expect(controlMode({ ...where, connected: false, fw: fw(true) })).toBeNull();
    expect(controlMode({ ...where, demo: true, installation: false, fw: fw(false) })).toBe("full");
  });

  it("reads the clock freely with the flag and sets it only while not known to be locked", () => {
    expect(clockAccess({ ...where, fw: fw(true), locked: false })).toEqual({ read: true, write: true, unsupported: false });
    expect(clockAccess({ ...where, fw: fw(true), locked: null })).toEqual({ read: true, write: true, unsupported: false });
    expect(clockAccess({ ...where, fw: fw(true), locked: true })).toEqual({ read: true, write: false, unsupported: false });
    // without the flag: the hint that the CAN socket does not know the command, nothing sent
    expect(clockAccess({ ...where, fw: fw(false), locked: null })).toEqual({ read: false, write: false, unsupported: true });
    expect(clockAccess({ ...where, installation: false, fw: fw(true), locked: false })).toEqual({ read: true, write: false, unsupported: false });
    // RS485-1/2 as before: the lock bit has no meaning there
    expect(clockAccess({ ...where, bus: "pack", fw: fw(false), locked: true })).toEqual({ read: true, write: true, unsupported: false });
    expect(clockAccess({ ...where, bus: "pack", installation: false, fw: fw(false), locked: null })).toEqual({ read: true, write: false, unsupported: false });
    expect(clockAccess({ ...where, connected: false, fw: fw(true), locked: false }).read).toBe(false);
  });

  it("reads the lock of a positive build (image flag bit 3) the other way round", () => {
    // 1.6.1, flags 1E: the lock as the inverse builds read it decides; the raw bit 3 at 1 allows
    const positive = { patch: { kind: "version" as const, version: [1, 6] as [number, number] }, flags: 0x1e };
    expect(clockAccess({ ...where, fw: positive, locked: false })).toEqual({ read: true, write: true, unsupported: false });
    expect(clockAccess({ ...where, fw: positive, locked: true })).toEqual({ read: true, write: false, unsupported: false });
    // Group0 FF: allowed in the positive build, locked in the inverse one
    expect(powerLocked(3, inverseG0(0xff, true), profile())).toBe(false);
    expect(powerLocked(3, inverseG0(0xff, false), profile())).toBe(true);
    // bit 5 (the lock state) never flips
    expect(inverseG0(0xdf, true)).toBe(0xc7);
    expect(inverseG0(undefined, true)).toBeUndefined();
  });

  it("names the way over RS485-1/2 instead of hiding „Steuern“ for a pack without the service", () => {
    expect(controlHint({ ...where, fw: fw(false) })).toBe(true);
    expect(controlHint({ ...where, fw: fw(true) })).toBe(false);
    expect(controlHint({ ...where, bus: "pack", fw: fw(false) })).toBe(false);
    expect(controlHint({ ...where, demo: true, fw: fw(false) })).toBe(false);
    expect(controlHint({ ...where, connected: false, fw: fw(false) })).toBe(false);
  });

  it("skips and names the packs without the flag when setting all clocks", () => {
    expect(clockTargets([0, 1, 2, 3], (a) => a !== 2)).toEqual({ targets: [0, 1, 3], skipped: [2] });
    expect(clockTargets([0, 1], () => true)).toEqual({ targets: [0, 1], skipped: [] });
  });

  it("notes the slaves only after ein/aus at the master over the CAN socket", () => {
    expect(masterBroadcast("power", 0)).toBe(true);
    expect(masterBroadcast("power", 2)).toBe(false);
    expect(masterBroadcast("full", 0)).toBe(false);
  });
});

describe("the stored pack cutoff of a pack (EMU1101-EXT Full)", () => {
  it("counts only as this connection proved it for exactly this pack", () => {
    expect(packCutoff(0, profile({ caps: { ...caps(true), pack_cutoff: true } }), undefined)).toBe(true);
    expect(packCutoff(0, profile({ caps: { ...caps(true), pack_cutoff: false } }), undefined)).toBe(false);
    expect(packCutoff(0, profile({ caps: { ...caps(true), pack_cutoff: true } }, "pack"), undefined)).toBe(false);
    expect(packCutoff(3, profile(), probe({ pack_cutoff: true }))).toBe(true);
    expect(packCutoff(3, profile(), probe({ pack_cutoff: true, gen: 3 }))).toBe(false);
    expect(packCutoff(3, profile(), probe({ pack_cutoff: true, state: "undecided" }))).toBe(false);
    expect(packCutoff(3, profile({ caps: { ...caps(true), pack_cutoff: true } }), undefined)).toBe(false);
    expect(packCutoff(null, profile(), undefined)).toBe(false);
  });
});

describe("a new patch function needs only its catalog row", () => {
  it("offers 49/3B over the CAN socket once the catalog has a way there, for exactly the packs with its flag", async () => {
    const { ROUTES } = await import("./access");
    const { controlOffers } = await import("./power");
    const flag3 = (flags: number) => ({ ...where, fw: { patch: { kind: "version" as const, version: [1, 7] as [number, number] }, flags } });
    expect(controlOffers(flag3(0x08))("set-remaining")).toBe(false);
    // a later build: 49/3B over the CAN socket with image flag bit 3 (a synthetic row, only for this test)
    ROUTES.push({ action: "set-remaining", channel: "uart1", wire: "ASCII 49/3B", firmware: [{ kind: "image-flag", bit: 3 }], needs: [], implemented: true });
    try {
      expect(controlOffers(flag3(0x08))("set-remaining")).toBe(true);
      expect(controlMode(flag3(0x08))).toBe("power");
      // a pack without the flag, or one whose flags this connection did not prove, gets nothing
      expect(controlOffers(flag3(0x02))("set-remaining")).toBe(false);
      expect(controlOffers({ ...where, fw: { patch: { kind: "version", version: [1, 7] }, flags: null } })("set-remaining")).toBe(false);
      expect(controlMode(flag3(0x00))).toBeNull();
    } finally {
      ROUTES.pop();
    }
  });
});
