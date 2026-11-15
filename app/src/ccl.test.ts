import { describe, expect, it } from "vitest";
import type { FeedPack, FeedState, MasterView, ProfileView } from "./api";
import { FEED_FUTURE_MS, FEED_MAX_AGE_MS, bankStep, feedCcl, feedCclLines, masterKey, masterOffers, parseStep, writeLockedOf } from "./ccl";
import i18n from "./i18n";

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

describe("CCL decrease step over the add-on's feed", () => {
  const de = i18n.getFixedT("de"), en = i18n.getFixedT("en");
  const pack = (address: number, o: Partial<FeedPack> = {}): FeedPack =>
    ({ address, device: null, battery_serial: null, bms_serial: null, parameters: null, parameters_at: null, ...o });
  const feed = (master: Partial<FeedPack> | null, o: Partial<FeedState> = {}): FeedState => ({
    state: "live", boot: "b1", boot_changed: false, skew_ms: 0, version_error: null,
    packs: master ? [pack(0, master), pack(1, { read_caps: 0x1F, ccl: { down: 7, up: 1, min: 1, max: 20 }, ccl_at: 1_000 })] : [], ...o,
  });
  const step = { down: 10, up: 1, min: 1, max: 20 };
  const good = { read_caps: 0x1F, ccl: step, ccl_at: 1_000 };
  it("is offered by the feed master's capability bit 4, never by a slave's or a version", () => {
    expect(feedCcl(feed(good), 2_000)).toEqual({ offered: true, state: "current", step, at: 1_000 });
    expect(feedCcl(feed({ ...good, read_caps: 0x0F }), 2_000)).toMatchObject({ offered: false, step: null });
    expect(feedCcl(feed({ read_caps: null }), 2_000).offered).toBe(false);
    expect(feedCcl(feed(null), 2_000).offered).toBe(false);
    expect(feedCcl(null, 2_000).offered).toBe(false);
  });
  it("no confirmed value, or one past the add-on's own age limit, is no value", () => {
    expect(feedCcl(feed({ ...good, ccl: null, ccl_at: null }), 2_000)).toEqual({ offered: true, state: "none", step: null, at: null });
    expect(feedCcl(feed(good), 1_000 + FEED_MAX_AGE_MS)).toMatchObject({ state: "current", step });
    expect(feedCcl(feed(good), 1_001 + FEED_MAX_AGE_MS)).toMatchObject({ state: "none", step: null });
  });
  it("ages the value on the add-on's clock, with the offset of this boot", () => {
    // the add-on's clock runs an hour ahead: a reading 45 minutes old on its clock is past the limit
    const ahead = 3_600_000, read = 10_000_000;
    const now = read - ahead + 45 * 60_000;
    expect(feedCcl(feed({ ...good, ccl_at: read }, { skew_ms: ahead }), now)).toMatchObject({ state: "none", step: null });
    // a minute behind: a minute younger than the Monitor's clock says
    expect(feedCcl(feed(good, { skew_ms: -60_000 }), 30_000 + FEED_MAX_AGE_MS)).toMatchObject({ state: "current", step });
  });
  it("without a live feed of this boot (offset unknown) nothing is current, whatever the retained meta says", () => {
    // a retained meta right after connecting, before any pack message of its boot: no offset yet
    expect(feedCcl(feed(good, { state: "waiting", skew_ms: null }), 2_000)).toEqual({ offered: true, state: "unknown", step: null, at: null });
    // the case the review names: 45 minutes old on an hour-ahead clock, offset not known: never shown as current
    expect(feedCcl(feed({ ...good, ccl_at: 10_000_000 }, { state: "waiting", skew_ms: null }), 10_000_000 - 3_600_000 + 45 * 60_000).state).toBe("unknown");
    // live but no offset (only system values so far), the add-on offline or the feed switched off
    expect(feedCcl(feed(good, { skew_ms: null }), 2_000).state).toBe("unknown");
    for (const state of ["offline", "off", "broker", "denied", "waiting"] as const) expect(feedCcl(feed(good, { state }), 2_000).state, state).toBe("unknown");
    // after a boot change the backend has dropped the offset until this boot's first pack message
    expect(feedCcl(feed(good, { boot: "b2", boot_changed: true, state: "waiting", skew_ms: null }), 2_000).state).toBe("unknown");
  });
  it("a read time implausibly far ahead of the add-on's clock is not current", () => {
    expect(feedCcl(feed({ ...good, ccl_at: 2_000 + FEED_FUTURE_MS }), 2_000)).toMatchObject({ state: "current" });
    expect(feedCcl(feed({ ...good, ccl_at: 2_001 + FEED_FUTURE_MS }), 2_000)).toMatchObject({ state: "unknown", step: null });
  });
  it("words the lines like the direct section, with the add-on's read time, or says there is no value or it is unknown", () => {
    const at = (ms: number) => `@${ms}`;
    const lines = feedCclLines({ state: "current", step, at: 1_000 }, 4, de, at);
    expect(lines[0]).toEqual({ text: de("ccl.value", { v: 10 }), tone: "ok" });
    expect(lines.map((l) => l.text)).toContain(de("ccl.example", { n: 4, a: 40 }));
    expect(lines.at(-1)?.text).toBe("Gelesen vom Add-on @1000.");
    expect(feedCclLines({ state: "current", step: { ...step, down: 1 }, at: 1 }, 0, en, at).map((l) => l.text)).toEqual([
      `${en("ccl.value", { v: 1 })} ${en("ccl.isDefault")}`, en("ccl.up", { up: 1, min: 1, max: 20 }), "Read by the add-on @1.",
    ]);
    expect(feedCclLines({ state: "none", step: null, at: null }, 4, de, at)).toEqual([{ text: "Das Add-on meldet noch keinen bestätigten Wert.", tone: "warn" }]);
    expect(feedCclLines({ state: "none", step: null, at: null }, 4, en, at)[0].text).toBe("The add-on reports no confirmed value yet.");
    // unknown: neutral (no tone), never the value
    for (const t of [de, en]) {
      const u = feedCclLines({ state: "unknown", step: null, at: null }, 4, t, at);
      expect(u).toEqual([{ text: t("ccl.feedUnknown") }]);
      expect(u[0].text).not.toMatch(/\d+ A/);
    }
  });
});
