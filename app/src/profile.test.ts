import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import type { MasterView, PackPatch, ProfileView } from "./api";
import { firmwareText, gateBits, gateRowLabel, hardStopLine, patchName, profileLine, recheckable, selfLock, writeLocked } from "./profile";
import { flashGateKeys } from "./firmware";

const caps = { status_split: true, uart1_params: true, uart1_identity: true, write_gate: true, start_gate: true };
const master = (m: Partial<MasterView>): MasterView => ({ kind: "patched", version: "16.06.13 MAINT 1.5", patch: "1.5", base: "16.06.13", build_id: "909a604e60e9225b",
  listed: "16.06.13 MAINT 1.5", caps, hard_stop: null, marker: "off", marker_word: "FFFFFFFF", slaves: 3, may_halt: true, may_release: false, ...m });
const profile = (m: Partial<MasterView>, bus: "can" | "pack" = "can"): ProfileView => ({ gen: 1, bus, endpoint: "tcp:gw:4196", master: master(m), gates: null, at: 0 });
const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
const proof: PackPatch = { source: "user", release: "MAINT ≥ 1.5", caps, at: 1 };

describe("firmware profile of the connection", () => {
  it("names the class the master reports, by capabilities and not by a version number", () => {
    expect(profileLine(profile({}), tr("de"))).toEqual({ tone: "ok", text: "16.06.13 MAINT 1.5" });
    expect(profileLine(profile({ marker: "on", slaves: 0 }), tr("en"))?.text).toBe("16.06.13 MAINT 1.5");
    // an unlisted build with the same capabilities: same configuration, named by version and base
    const later = profile({ listed: null, version: "16.07.02 MAINT 1.6", patch: "1.6", base: "16.07.02" });
    expect(profileLine(later, tr("de"))).toMatchObject({ text: "16.07.02 MAINT 1.6", title: "MAINT 1.6 (Basis 16.07.02, Build nicht gelistet) · Build 909a604e60e9225b" });
    expect(patchName(later.master, tr("en"))).toBe("MAINT 1.6 (base 16.07.02, build not listed)");
    expect(profileLine(profile({ kind: "unknown-patch", caps: null, version: "16.06.13 MAINT 1.4" }), tr("de"))?.text).toBe("Unbekannte Patch-Firmware 16.06.13 MAINT 1.4: nur Original-Funktionen");
    expect(profileLine(profile({ kind: "no-0x64", caps: null }), tr("de"))?.text).toBe("0x64 nicht unterstützt, Firmwarestand nicht belegt");
    expect(profileLine(profile({ kind: "unknown", caps: null }), tr("en"))?.text).toBe("Firmware not recognised");
    // RS485-1/2 knows no 0x64: nothing to say
    expect(profileLine(profile({}, "pack"), tr("de"))).toBeNull();
    expect(profileLine(null, tr("de"))).toBeNull();
  });
  it("shows the hard stop: the image's flag wins, else what the pack showed, else unknown", () => {
    const seen = { address: 0, at: Date.UTC(2026, 0, 2, 12) };
    expect(hardStopLine(true, seen, tr("de"))).toBe("Hard-Stop: an");
    expect(hardStopLine(false, seen, tr("en"))).toBe("Hard stop: off");
    expect(hardStopLine(null, seen, tr("de"), "de")).toBe(`Hard-Stop: an (beobachtet am ${new Date(seen.at).toLocaleDateString("de")})`);
    expect(hardStopLine(null, seen, tr("en"), "en")).toBe(`Hard stop: on (observed ${new Date(seen.at).toLocaleDateString("en")})`);
    for (const lng of ["de", "en"]) expect(i18n.exists("profile.hardStopNotSeen", { lng })).toBe(false);
    expect(hardStopLine(null, undefined, tr("de"))).toBe("Hard-Stop: unbekannt");
    // an unknown OEM revision shows in the base
    expect(profileLine(profile({ listed: null, base: "16.06.?", patch: "1.5", version: "16.06.? MAINT 1.5" }), tr("de"))?.text).toBe("16.06.? MAINT 1.5");
  });
  it("shows the master's release bits separately from its maintenance state", () => {
    // after a write to the master without a read-back: marked old, and „Erneut prüfen“ also for a recognised patch
    expect(recheckable(profile({}))).toBe(true);
    expect(recheckable(profile({ kind: "unknown", caps: null }))).toBe(true);
    expect(recheckable(profile({}, "pack"))).toBe(false);
  });
});

describe("release bits of Group0 in a parameter column", () => {
  it("are named only with a proof; over the CAN socket bit 2 never becomes editable", () => {
    const none = { named: false, writeEditable: false, startEditable: false, powerEditable: false, power: null };
    expect(gateBits(null, true)).toEqual(none);
    expect(gateBits(null, false)).toEqual(none);
    expect(gateBits(proof, true)).toEqual({ named: true, writeEditable: true, startEditable: false, powerEditable: false, power: null });
    expect(gateBits(proof, false)).toEqual({ named: true, writeEditable: true, startEditable: true, powerEditable: true, power: null });
  });
  it("label the shared row neutrally in a mixed table", () => {
    expect(gateRowLabel([{ patch: proof, switches: true }, { patch: proof, switches: true }])).toBe("named");
    // a proven and an original or unproven column: „Bit 0.0“, the meaning only in the proven column
    expect(gateRowLabel([{ patch: proof, switches: true }, { patch: null, switches: true }])).toBe("neutral");
    expect(gateRowLabel([{ patch: null, switches: true }])).toBe("unnamed");
    // a column without values (still reading, unreadable) does not count
    expect(gateRowLabel([{ patch: proof, switches: true }, { patch: null, switches: false }])).toBe("named");
    expect(i18n.t("par.gateNeutral", { b: 2, lng: "de" })).toBe("Bit 0.2");
  });
  it("warn before a self-lock and show a locked pack", () => {
    expect(selfLock(proof, 0xff, 0xfe)).toBe(true);
    expect(selfLock(proof, 0xfe, 0xfe)).toBe(false);
    expect(selfLock(null, 0xff, 0xfe)).toBe(false);
    expect(writeLocked(proof, true, 0xfe)).toBe(true);
    expect(writeLocked(proof, false, 0xfe)).toBe(false);
    expect(writeLocked(null, true, 0xfe)).toBe(false);
  });
  it("say what a patched flash makes of the bits it takes over", () => {
    expect(flashGateKeys(0xff)).toEqual(["fw.patch.ok"]);
    expect(flashGateKeys(0xfe)).toEqual(["fw.patch.writeLocked"]);
    expect(flashGateKeys(0xfb)).toEqual(["fw.patch.startLocked"]);
    expect(flashGateKeys(0xfa)).toEqual(["fw.patch.bothLocked"]);
    // an image with power and clock: FF keeps them locked (bit 3 on)
    expect(flashGateKeys(0xff, true)).toEqual(["fw.patch.ok", "fw.patch.powerLocked"]);
    expect(flashGateKeys(0xf7, true)).toEqual(["fw.patch.ok", "fw.patch.powerOpen"]);
    for (const lng of ["de", "en"]) for (const k of ["fw.patch.ok", "fw.patch.powerLocked", "fw.patch.powerOpen", "par.gatePowerUnknown", "par.gatePowerAbsent", "par.gatePowerFixed", "profile.gate.power", "fw.patch.writeLocked", "fw.patch.startLocked", "fw.patch.bothLocked", "uart1.write-locked", "uart1.start-bit", "pw.locked", "pw.multi.res.locked",
      "maint.err.start.start-not-allowed", "maint.err.start.auto-address", "maint.err.start.token-or-marker", "maint.err.start.start-refused-unread", "maint.err.refused.expected-slaves-needed",
      "maint.done.polling.unverified", "maint.done.polling.no-slaves", "maint.banner.discard", "maint.banner.discardExplain", "maint.banner.discardYes", "maint.err.refused.not-latent"]) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
  });
});

describe("the power and clock lock (Group0 bit 3)", () => {
  const live = (power_rtc: boolean | null): PackPatch => ({ source: "live", release: "16.06.13 MAINT 1.5", caps: { ...caps, power_rtc }, at: null });
  it("changes only via RS485-1/2, and not where the build has no such service", () => {
    expect(gateBits(live(true), false)).toMatchObject({ powerEditable: true, power: true });
    expect(gateBits(live(true), true)).toMatchObject({ powerEditable: false, power: true });
    expect(gateBits(live(null), false)).toMatchObject({ powerEditable: true, power: null });
    expect(gateBits(live(false), false)).toMatchObject({ powerEditable: false, power: false });
  });
});

describe("the firmware a pack header names", () => {
  it("takes the version 64/03 proves when 0x51's short one fits it", () => {
    expect(firmwareText("16.06", "16.06.13 MAINT 1.5")).toBe("16.06.13 MAINT 1.5");
    expect(firmwareText("16.06", null)).toBe("16.06");
    expect(firmwareText(undefined, "16.06.13 MAINT 1.5")).toBe("16.06.13 MAINT 1.5");
    expect(firmwareText("16.05", "16.06.13 MAINT 1.5")).toBe("16.05 (16.06.13 MAINT 1.5)");
    expect(firmwareText("16.06", "16.06.? MAINT 1.5")).toBe("16.06.? MAINT 1.5");
  });
});
