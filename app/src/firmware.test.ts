import { describe, expect, it } from "vitest";
import { benchMarkerKey, benchPick, confirmationPhrase, imageLabel, imageWarning, imageWarningIsAlarm, progressFraction, resultVersions } from "./firmware";
import type { BenchPack, FirmwareImage } from "./api";
import i18n from "./i18n";

const image: FirmwareImage = { release: "16.06.13", device: "EMU1101_10D_HW16", bytes: 53000, blocks: 209, sha256: "ab", kind: "original", custom: false };
const target = { device_name: "X", firmware_version: "16.04", can_protocol: "CAN" };

describe("flashing a board", () => {
  const pack = (address: number, marker: BenchPack["marker"] = null): BenchPack => ({ address, info: target, serial: `SYN-${address}`, version: marker ? "16.06.13 MAINT 1.6.1" : null, marker });
  it("preselects only a single pack", () => {
    expect(benchPick([pack(3)])).toBe(3);
    expect(benchPick([pack(0), pack(1)])).toBeNull();
    expect(benchPick([])).toBeNull();
  });
  it("says what it knows about the marker before flashing", () => {
    // shown over RS485-1/2 (1.5.1/1.6.1): set, cleared before an image without the maintenance functions, at any address
    expect(benchMarkerKey(pack(0, "on"), image)).toBe("fw.bench.markerOn");
    expect(benchMarkerKey(pack(3, "on"), image)).toBe("fw.bench.markerOn");
    expect(benchMarkerKey(pack(0, "on"), { ...image, kind: "maintenance" })).toBeNull();
    expect(benchMarkerKey(pack(0, "off"), image)).toBeNull();
    // not shown there: at a master the way over the CAN socket
    expect(benchMarkerKey(pack(0), image)).toBe("fw.bench.markerOlder");
    expect(benchMarkerKey(pack(0), null)).toBe("fw.bench.markerOlder");
    expect(benchMarkerKey(pack(3), image)).toBeNull();
    expect(benchMarkerKey(null, image)).toBeNull();
  });
  it("the phrase of a custom image differs", () => {
    expect(confirmationPhrase(3)).toBe("FLASH 3");
    expect(confirmationPhrase(2, true)).toBe("FLASH CUSTOM 2");
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

describe("inputs while a flash runs", () => {
  it("are locked during a flash and a check, free otherwise", async () => {
    const { firmwareInputsLocked } = await import("./firmware");
    expect(firmwareInputsLocked(true, false)).toBe(true);
    expect(firmwareInputsLocked(false, true)).toBe(true);
    expect(firmwareInputsLocked(false, false)).toBe(false);
  });
  it("the switch is disabled and looks so", async () => {
    const { renderToStaticMarkup } = await import("react-dom/server");
    const { createElement } = await import("react");
    const { Switch } = await import("./components/PackPicker");
    const html = renderToStaticMarkup(createElement(Switch, { checked: false, onChange: () => {}, disabled: true }));
    expect(html).toContain("disabled");
    expect(html).toContain("disabled:opacity-40");
  });
});
