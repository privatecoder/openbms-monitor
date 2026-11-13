import { describe, expect, it } from "vitest";
import { benchMarkerKey, benchPick, compareLines, confirmationPhrase, imageLabel, imageWarning, imageWarningIsAlarm, progressFraction, resultVersions } from "./firmware";
import type { BenchPack, FirmwareImage, FlashComparison, FlashSide } from "./api";
import i18n from "./i18n";

const image: FirmwareImage = { release: "16.06.13", device: "EMU1101_10D_HW16", bytes: 53000, blocks: 209, sha256: "ab", kind: "original", custom: false };
const target = { device_name: "X", firmware_version: "16.04", can_protocol: "CAN" };

describe("flashing a board", () => {
  const pack = (address: number, marker: BenchPack["marker"] = null): BenchPack => ({ address, info: target, serial: `SYN-${address}`, version: marker ? "EMU1101-EXT Full 1.6.5" : null, marker });
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
  const maint = { ...image!, release: "EMU1101-EXT Full 1.6.5", kind: "maintenance" as const, custom: true };
  const custom = { ...image!, release: "custom", kind: "custom" as const, custom: true };
  const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
  it("names the maintenance build as known, not as custom firmware", () => {
    expect(imageLabel(maint, tr("de"), true)).toBe("EMU1101-EXT Full 1.6.5 (bekannte Prüfsumme)");
    expect(imageLabel(maint, tr("en"), true)).toBe("EMU1101-EXT Full 1.6.5 (known checksum)");
    expect(imageLabel(maint, tr("de"))).toBe("EMU1101-EXT Full 1.6.5");
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
    expect(i18n.t("fw.maintWarning", { lng: "de" })).toMatch(/^Gepatchte Firmware/);
    expect(i18n.t("fw.maintWarning", { lng: "en" })).toMatch(/patched firmware/);
    // still behind the custom opt-in and its own phrase
    expect(confirmationPhrase(0, maint.custom)).toBe("FLASH CUSTOM 0");
  });
});

describe("flash result", () => {
  const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
  const info = (v: string) => ({ device_name: "X", firmware_version: v, can_protocol: "CAN" });
  const maint = { ...image!, release: "EMU1101-EXT Full 1.6.5", kind: "maintenance" as const, custom: true };
  const custom = { ...image!, release: "custom", kind: "custom" as const, custom: true };

  it("the version line names the image in full, an EMU1101-EXT build by its name", () => {
    const done = { before: info("16.06"), after: info("16.06") };
    expect(resultVersions(done, "16.06", maint, tr("de"))).toEqual({ from: "16.06", to: "EMU1101-EXT Full 1.6.5 (Pack meldet 16.06)" });
    // with the listed build's OEM base: a fitting answer is not repeated, another one is named
    const light = { ...maint, release: "EMU1101-EXT Light 1.5.5", base: "16.06.13" };
    const full = { ...maint, base: "16.06.13" };
    for (const [img, lng] of [[light, "de"], [full, "en"]] as const) {
      expect(resultVersions(done, "16.06", img, tr(lng)).to).toBe(img.release);
      expect(resultVersions({ before: info("16.06"), after: info("16.04") }, null, img, tr(lng)).to).toMatch(/\(Pack meldet 16\.04\)|\(pack reports 16\.04\)/);
      expect(resultVersions({ before: info("16.06"), after: info("17.01") }, null, img, tr(lng)).to).toContain("17.01");
      expect(resultVersions({ before: info("16.06"), after: null }, null, img, tr(lng)).to).toBe(img.release);
    }
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
  it("the switch is disabled and looks so", async () => {
    const { renderToStaticMarkup } = await import("react-dom/server");
    const { createElement } = await import("react");
    const { Switch } = await import("./components/PackPicker");
    const html = renderToStaticMarkup(createElement(Switch, { checked: false, onChange: () => {}, disabled: true }));
    expect(html).toContain("disabled");
    expect(html).toContain("disabled:opacity-40");
  });
});

describe("the comparison before/after a flash", () => {
  const t = (k: string, o?: Record<string, unknown>) => (o ? `${k} ${JSON.stringify(o)}` : k);
  const side = (over: Partial<FlashSide> = {}): FlashSide => ({ serial: "SYN-1", build: null, positive: null, switches: [0x1d, 0, 0, 0, 0, 0, 0, 0], charge: { soc: 79.1, remaining_ah: 221.5, full_ah: 280, cycles: 7 }, ...over });
  const cmp = (over: Partial<FlashComparison> = {}): FlashComparison => ({ before: side(), after: side(), identity: "same", params: [], switches: [], charge: false, verdict: "same", ...over });
  const keys = (c: FlashComparison, img: FirmwareImage | null = image) => compareLines(c, img, t).map((l) => l.key);
  it("says the same pack is unchanged", () => {
    expect(keys(cmp())).toEqual(["fw.cmp.verdict.same", "fw.cmp.board.same", "fw.cmp.build", "fw.cmp.params.same", "fw.cmp.switches.same"]);
  });
  it("lists changed parameters with their index and changed switch bytes raw", () => {
    const lines = compareLines(cmp({ verdict: "differs", params: [{ index: 10, key: "x", unit: "V", before: 3.5, after: 3.4 }], switches: [{ group: 0, before: 0x1d, after: 0x3d }] }), image, t);
    expect(lines.find((l) => l.param === 10)?.vars).toEqual({ a: 3.5, b: 3.4, unit: "V" });
    expect(lines.find((l) => l.key === "fw.cmp.switches.line")?.vars).toEqual({ g: 0, a: "1D", b: "3D" });
    expect(lines[0]).toMatchObject({ key: "fw.cmp.verdict.differs", tone: "warn" });
  });
  it("keeps what was not read apart from unchanged", () => {
    const k = keys(cmp({ identity: "unchecked", params: null, switches: null, verdict: "incomplete" }));
    expect(k).toContain("fw.cmp.board.unchecked");
    expect(k).toContain("fw.cmp.params.unchecked");
    expect(k).not.toContain("fw.cmp.params.same");
    // no 64/03 over RS485-1/2: not supported, on each side
    expect(compareLines(cmp(), image, t)[2].vars).toEqual({ a: "fw.cmp.unsupported", b: "fw.cmp.unsupported" });
  });
  it("another board is an alarm", () => {
    expect(compareLines(cmp({ identity: "changed", verdict: "differs", after: side({ serial: "SYN-2" }) }), image, t)[1]).toMatchObject({ key: "fw.cmp.board.changed", tone: "alarm", vars: { a: "SYN-1", b: "SYN-2" } });
  });
  it("reads the release bits under the image flashed and keeps a stored pack lock visible", () => {
    const img: FirmwareImage = { ...image, kind: "maintenance", power_rtc: true, pack_cutoff: true };
    // 1.6.3: bit 3 set allows power/clock, bit 4 extended off on, bit 5 at 0 a stored lock
    const k = keys(cmp({ after: side({ positive: true, switches: [0x1d, 0, 0, 0, 0, 0, 0, 0] }) }), img);
    expect(k).toEqual(expect.arrayContaining(["fw.cmp.meaning", "fw.patch.powerOpen", "fw.patch.cutoffOn", "fw.patch.lockKept"]));
    // TEMP(maint-1.6): the first 1.6 reports bit 3 the other way round: FF locks power/clock
    expect(keys(cmp({ after: side({ positive: false, switches: [0xff, 0, 0, 0, 0, 0, 0, 0] }) }), img)).toContain("fw.patch.powerLocked");
    // 1.5.3 (power, no cutoff): FF allows
    expect(keys(cmp({ after: side({ positive: true, switches: [0xff, 0, 0, 0, 0, 0, 0, 0] }) }), { ...img, pack_cutoff: false })).toContain("fw.patch.powerOpen");
    // the original image: no meaning shown
    expect(keys(cmp({ after: side({ positive: null }) }), image)).not.toContain("fw.cmp.meaning");
  });
  it("says when the same raw bit 3 means the opposite after the flash", () => {
    expect(keys(cmp({ before: side({ positive: false }), after: side({ positive: true }) }))).toContain("fw.cmp.polarity");
    expect(keys(cmp({ before: side({ positive: true }), after: side({ positive: true }) }))).not.toContain("fw.cmp.polarity");
    expect(keys(cmp({ before: side({ positive: null }), after: side({ positive: true }) }))).not.toContain("fw.cmp.polarity");
  });
  it("has every key in both languages", () => {
    const all = ["fw.cmp.title", "fw.cmp.verdict.same", "fw.cmp.verdict.differs", "fw.cmp.verdict.incomplete", "fw.cmp.board.same", "fw.cmp.board.changed", "fw.cmp.board.unchecked", "fw.cmp.build", "fw.cmp.unsupported",
      "fw.cmp.params.unchecked", "fw.cmp.params.same", "fw.cmp.params.changed", "fw.cmp.params.line", "fw.cmp.switches.same", "fw.cmp.switches.line", "fw.cmp.meaning", "fw.cmp.polarity", "fw.cmp.chargeUnchecked",
      "ctl.charge.line", "ctl.charge.hint", "ctl.charge.unchecked"];
    for (const lng of ["de", "en"]) for (const k of all) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
  });
});

describe("recovery phrase", () => {
  it("matches the backend: no address, custom named", async () => {
    const { recoveryPhrase } = await import("./firmware");
    expect(recoveryPhrase(false)).toBe("RECOVER");
    expect(recoveryPhrase(true)).toBe("RECOVER CUSTOM");
  });
});
