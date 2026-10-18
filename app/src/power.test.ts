import { describe, expect, it } from "vitest";
import type { MasterView, PackProbeView, ProfileView } from "./api";
import { clockAccess, clockTargets, controlMode, masterBroadcast, powerLocked, powerRtc } from "./power";

const caps = (power_rtc: boolean | null) => ({ status_split: true, uart1_params: true, uart1_identity: true, write_gate: true, start_gate: true, power_rtc });
const master = (m: Partial<MasterView>): MasterView => ({ kind: "patched", version: "16.06.13 MAINT 1.5", patch: "1.5", base: "16.06.13", build_id: "0011223344556677",
  listed: null, caps: caps(true), hard_stop: false, marker: "off", marker_word: "FFFFFFFF", slaves: 3, may_halt: true, may_release: false, ...m });
const profile = (m: Partial<MasterView> = {}, bus: "can" | "pack" = "can", power: boolean | null = null): ProfileView =>
  ({ gen: 4, bus, endpoint: "tcp:gw:4196", master: master(m), gates: power === null ? null : { write: true, start: true, power }, at: 0 });
const probe = (p: Partial<PackProbeView>): PackProbeView => ({ gen: 4, address: 3, state: "proven", version: "16.06.13 MAINT 1.5", listed: null, build_id: null, hard_stop: false, power_rtc: true, ...p });
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
    expect(controlMode({ ...where, bus: "pack", power: false })).toBe("full");
    expect(controlMode({ ...where, power: true })).toBe("power");
    expect(controlMode({ ...where, power: false })).toBeNull();
    // „Nur ansehen“ writes nothing to a pack
    expect(controlMode({ ...where, installation: false, power: true })).toBeNull();
    expect(controlMode({ ...where, bus: "pack", installation: false, power: false })).toBeNull();
    expect(controlMode({ ...where, connected: false, power: true })).toBeNull();
    expect(controlMode({ ...where, demo: true, installation: false, power: false })).toBe("full");
  });

  it("reads the clock freely with the flag and sets it only while not known to be locked", () => {
    expect(clockAccess({ ...where, power: true, locked: false })).toEqual({ read: true, write: true, unsupported: false });
    expect(clockAccess({ ...where, power: true, locked: null })).toEqual({ read: true, write: true, unsupported: false });
    expect(clockAccess({ ...where, power: true, locked: true })).toEqual({ read: true, write: false, unsupported: false });
    // without the flag: the hint that the CAN socket does not know the command, nothing sent
    expect(clockAccess({ ...where, power: false, locked: null })).toEqual({ read: false, write: false, unsupported: true });
    expect(clockAccess({ ...where, installation: false, power: true, locked: false })).toEqual({ read: true, write: false, unsupported: false });
    // RS485-1/2 as before: the lock bit has no meaning there
    expect(clockAccess({ ...where, bus: "pack", power: false, locked: true })).toEqual({ read: true, write: true, unsupported: false });
    expect(clockAccess({ ...where, bus: "pack", installation: false, power: false, locked: null })).toEqual({ read: true, write: false, unsupported: false });
    expect(clockAccess({ ...where, connected: false, power: true, locked: false }).read).toBe(false);
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
