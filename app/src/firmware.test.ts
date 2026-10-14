import { describe, expect, it } from "vitest";
import { confirmationPhrase, imageLabel, imageWarning, imageWarningIsAlarm, progressFraction, resultVersions, steps, type FwSetup } from "./firmware";
import i18n from "./i18n";

const image: FwSetup["image"] = { release: "16.06.13", device: "EMU1101_10D_HW16", bytes: 53000, blocks: 209, sha256: "ab", kind: "original", custom: false };
const target = { device_name: "X", firmware_version: "16.04", can_protocol: "CAN" };
const base: FwSetup = { running: false, done: false, port: "/dev/x", address: 0, image, target, checkedFor: { port: "/dev/x", address: 0 }, typed: "", busy: false };

describe("firmware steps", () => {
  it("check pack needs port and image", () => {
    expect(steps({ ...base, port: "" }).inspect).toBe(false);
    expect(steps({ ...base, image: null }).inspect).toBe(false);
    expect(steps(base).inspect).toBe(true);
  });
  it("target_must_match_port_and_address", () => {
    expect(steps(base).confirm).toBe(true);
    expect(steps({ ...base, address: 1 }).confirm).toBe(false);
    expect(steps({ ...base, port: "/dev/y" }).confirm).toBe(false);
    expect(steps({ ...base, target: null }).confirm).toBe(false);
  });
  it("start_needs_exact_phrase", () => {
    expect(confirmationPhrase(3)).toBe("FLASH 3");
    expect(steps({ ...base, typed: "FLASH 0" }).start).toBe(true);
    expect(steps({ ...base, typed: " FLASH 0 " }).start).toBe(true);
    expect(steps({ ...base, typed: "flash 0" }).start).toBe(false);
    expect(steps({ ...base, typed: "FLASH 1" }).start).toBe(false);
  });
  it("custom image needs FLASH CUSTOM", () => {
    const custom: FwSetup = { ...base, image: { ...image, release: "custom", kind: "custom", custom: true } };
    expect(confirmationPhrase(2, true)).toBe("FLASH CUSTOM 2");
    expect(steps({ ...custom, typed: "FLASH CUSTOM 0" }).start).toBe(true);
    expect(steps({ ...custom, typed: "FLASH 0" }).start).toBe(false);
    expect(steps({ ...base, typed: "FLASH CUSTOM 0" }).start).toBe(false);
  });
  it("another action (pack check, measurement, End step) locks the inputs while it runs", () => {
    expect(steps({ ...base, typed: "FLASH 0", busy: true })).toEqual({ image: false, inspect: false, confirm: false, start: false });
  });
  it("nothing is editable while running", () => {
    const s = steps({ ...base, typed: "FLASH 0", running: true });
    expect(s).toEqual({ image: false, inspect: false, confirm: false, start: false });
  });
  it("a shown result blocks a second run with the old confirmation", () => {
    const s = steps({ ...base, typed: "FLASH 0", done: true });
    expect(s).toEqual({ image: false, inspect: false, confirm: false, start: false });
  });
  it("after New update the pack must be checked and confirmed again", () => {
    // what reset() and start() leave behind
    const cleared = { ...base, target: null, checkedFor: null, typed: "" };
    expect(steps(cleared)).toEqual({ image: true, inspect: true, confirm: false, start: false });
    expect(steps({ ...cleared, typed: "FLASH 0" }).start).toBe(false);
  });
});

describe("progress", () => {
  it("counts blocks and treats the stages around them as 0 and 1", () => {
    expect(progressFraction(null)).toBe(0);
    expect(progressFraction({ stage: "erase", block: null, total: null })).toBe(0);
    expect(progressFraction({ stage: "block", block: 0, total: 4 })).toBe(0);
    expect(progressFraction({ stage: "block", block: 2, total: 4 })).toBe(0.5);
    expect(progressFraction({ stage: "finalize", block: null, total: null })).toBe(1);
  });
});

describe("image naming", () => {
  const maint = { ...image!, release: "16.06.13 MAINT 1.1", kind: "maintenance" as const, custom: true };
  const custom = { ...image!, release: "custom", kind: "custom" as const, custom: true };
  const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
  it("names the maintenance build as known, not as custom firmware", () => {
    expect(imageLabel(maint, tr("de"), true)).toBe("Wartungs-Build 16.06.13 MAINT 1.1 (bekannte Prüfsumme)");
    expect(imageLabel(maint, tr("en"), true)).toBe("maintenance build 16.06.13 MAINT 1.1 (known checksum)");
    expect(imageLabel(maint, tr("de"))).toBe("Wartungs-Build 16.06.13 MAINT 1.1");
    expect(imageLabel(custom, tr("de"), true)).toBe("eigene Firmware");
    expect(imageLabel(image!, tr("de"), true)).toBe("16.06.13");
  });
  it("warns per kind: the known maintenance build is not an unknown checksum", () => {
    expect(imageWarning(maint)).toBe("fw.maintWarning");
    expect(imageWarningIsAlarm(maint)).toBe(false);
    expect(imageWarningIsAlarm(custom)).toBe(true);
    expect(imageWarning(custom)).toBe("fw.customWarning");
    expect(imageWarning(image!)).toBeNull();
    expect(i18n.t("fw.maintWarning", { lng: "de" })).not.toMatch(/keiner bekannten/);
    // still behind the custom opt-in and its own phrase
    expect(confirmationPhrase(0, maint.custom)).toBe("FLASH CUSTOM 0");
  });
});

describe("flash result", () => {
  const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
  const info = (v: string) => ({ device_name: "X", firmware_version: v, can_protocol: "CAN" });
  const maint = { ...image!, release: "16.06.13 MAINT 1.1", kind: "maintenance" as const, custom: true };
  const custom = { ...image!, release: "custom", kind: "custom" as const, custom: true };

  it("the version line names the image in full, the maintenance build with MAINT", () => {
    const done = { before: info("16.06"), after: info("16.06") };
    expect(resultVersions(done, "16.06", maint, tr("de"))).toEqual({ from: "16.06", to: "16.06.13 MAINT 1.1" });
    expect(resultVersions(done, "16.06", image, tr("de"))).toEqual({ from: "16.06", to: "16.06.13" });
    expect(resultVersions({ before: info("16.04"), after: info("16.06") }, null, image, tr("de"))).toEqual({ from: "16.04", to: "16.06.13" });
  });
  it("without a 0x51 answer before the flash the pack check's version is used", () => {
    expect(resultVersions({ before: null, after: info("16.06") }, "16.05", image, tr("de")).from).toBe("16.05");
  });
  it("a pack that reports another version than the image says so; a custom image shows what the pack reports", () => {
    expect(resultVersions({ before: info("16.06"), after: info("16.04") }, null, image, tr("de")).to).toBe("16.06.13 (Pack meldet 16.04)");
    expect(resultVersions({ before: info("16.06"), after: info("16.06") }, null, custom, tr("en")).to).toBe("custom firmware (pack reports 16.06)");
    expect(resultVersions({ before: info("16.06"), after: null }, null, custom, tr("de")).to).toBe("eigene Firmware");
  });
});
