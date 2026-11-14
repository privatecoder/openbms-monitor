import { describe, expect, it } from "vitest";
import type { FirmwareDone, FirmwareImage, FlashComparison } from "./api";
import i18n from "./i18n";
import { judge, runFlashSeries, seriesOrder, seriesRowText, seriesView, startFlashSeries, summarizeSeries, withEnd, type PackEnd, type SeriesView } from "./flashSeries";
import { confirmationPhrase, seriesPhrase } from "./firmware";

const image = { release: "16.06.13", device: "EMU1101", bytes: 1, blocks: 1, sha256: "a", kind: "original", custom: false } as FirmwareImage;
const ext = { ...image, release: "EMU1101-EXT Full 1.6.5", kind: "maintenance", custom: true, base: "16.06.13" } as FirmwareImage;
const custom = { ...image, release: "custom", kind: "custom", custom: true } as FirmwareImage;
const cmp = (verdict: string) => ({ verdict } as FlashComparison);
const info = (v: string) => ({ device_name: "SYN", firmware_version: v, can_protocol: "" });
const done = (o: Partial<FirmwareDone> = {}): FirmwareDone => ({ ok: true, error: null, stage: null, written: true, before: info("16.05"), after: info("16.06"), log: "x.log", pending: false, comparison: cmp("same"), ...o });

describe("flash series: order and phrase", () => {
  it("the slaves by address first, the master last", () => {
    expect(seriesOrder([0, 3, 1])).toEqual([1, 3, 0]);
    expect(seriesOrder([15, 0, 2, 10])).toEqual([2, 10, 15, 0]);
    expect(seriesOrder([2, 1])).toEqual([1, 2]);
    expect(seriesOrder([1, 1, 0])).toEqual([1, 0]);
  });
  it("one phrase for the count, its own for a custom image; the single flash's phrase stays", () => {
    expect(seriesPhrase(3)).toBe("FLASH 3 PACKS");
    expect(seriesPhrase(3, true)).toBe("FLASH CUSTOM 3 PACKS");
    expect(confirmationPhrase(3)).toBe("FLASH 3");
    expect(confirmationPhrase(3, true)).toBe("FLASH CUSTOM 3");
  });
});

describe("flash series: one pack's end", () => {
  it("flashed only with a full after-check and the new version", () => {
    expect(judge(done(), image)).toEqual({ kind: "flashed", to: "16.06" });
    expect(judge(done(), ext)).toEqual({ kind: "flashed", to: "16.06" });
    expect(judge(done({ after: info("16.05") }), image)).toMatchObject({ kind: "deviates", why: "version" });
    expect(judge(done({ after: info("16.05") }), ext)).toMatchObject({ kind: "deviates", why: "version" });
    // an unknown custom image has no version to compare with
    expect(judge(done({ after: info("99.01") }), custom)).toEqual({ kind: "flashed", to: "99.01" });
  });
  it("a missing, incomplete or deviating after-check, a failure or a pending record are no clean end", () => {
    expect(judge(done({ comparison: null }), image)).toMatchObject({ kind: "deviates", why: "missing" });
    expect(judge(done({ after: null }), image)).toMatchObject({ kind: "deviates", why: "missing" });
    expect(judge(done({ comparison: cmp("incomplete") }), image)).toMatchObject({ kind: "deviates", why: "incomplete" });
    expect(judge(done({ comparison: cmp("differs") }), image)).toMatchObject({ kind: "deviates", why: "differs" });
    expect(judge(done({ ok: false, error: "x" }), image).kind).toBe("failed");
    expect(judge(done({ ok: false, pending: true }), image).kind).toBe("pending");
    expect(judge(done({ pending: true }), image).kind).toBe("pending");
  });
});

describe("flash series: the run", () => {
  const harness = (ends: Record<number, () => Promise<FirmwareDone>>, o: { link?: () => string; cancelled?: () => boolean; pending?: () => Promise<boolean> } = {}) => {
    const flashed: number[] = [], rows: [number, PackEnd][] = [];
    const run = (order: number[]) => runFlashSeries(order, {
      begun: "inst|port", link: o.link ?? (() => "inst|port"), cancelled: o.cancelled ?? (() => false), pending: o.pending ?? (async () => false),
      flash: (a) => { flashed.push(a); return (ends[a] ?? (async () => done()))(); },
      judge: (d) => judge(d, image), report: (a, e) => rows.push([a, e]),
    });
    return { flashed, rows, run };
  };

  it("flashes every pack in order, one after another", async () => {
    const h = harness({});
    expect(await h.run(seriesOrder([0, 2, 1]))).toBe("done");
    expect(h.flashed).toEqual([1, 2, 0]);
    expect(h.rows.map(([a, e]) => [a, e.kind])).toEqual([[1, "flashed"], [2, "flashed"], [0, "flashed"]]);
  });
  it("a failure at pack k stops; the rest are not flashed", async () => {
    for (const bad of [async () => done({ ok: false, error: "timeout" }), async () => done({ comparison: cmp("differs") }), async () => done({ after: info("16.05") }), async () => done({ comparison: null })]) {
      const h = harness({ 2: bad });
      expect(await h.run([1, 2, 3, 0])).toBe("stopped");
      expect(h.flashed).toEqual([1, 2]);
      expect(h.rows.map(([a, e]) => [a, e.kind])).toEqual([[1, "flashed"], [2, h.rows[1][1].kind], [3, "notReached"], [0, "notReached"]]);
      expect(h.rows[1][1].kind).not.toBe("flashed");
    }
  });
  it("a refused check stops with its reason", async () => {
    const h = harness({ 1: async () => { throw "bus busy"; } });
    expect(await h.run([1, 2, 0])).toBe("stopped");
    expect(h.rows).toEqual([[1, { kind: "refused", error: "bus busy" }], [2, { kind: "notReached" }], [0, { kind: "notReached" }]]);
  });
  it("cancel waits for the running flash and stops before the next pack", async () => {
    let cancel = false;
    const h = harness({ 1: async () => { cancel = true; return done(); } }, { cancelled: () => cancel });
    expect(await h.run([1, 2, 0])).toBe("cancelled");
    expect(h.flashed).toEqual([1]);
    expect(h.rows.map(([a, e]) => [a, e.kind])).toEqual([[1, "flashed"], [2, "notReached"], [0, "notReached"]]);
  });
  it("a changed link (installation or port) ends it between packs", async () => {
    let link = "inst|port";
    const h = harness({ 1: async () => { link = "other|port"; return done(); } }, { link: () => link });
    expect(await h.run([1, 2])).toBe("link");
    expect(h.flashed).toEqual([1]);
  });
  it("a pending bootloader record blocks the start and stops the series", async () => {
    const before = harness({}, { pending: async () => true });
    expect(await before.run([1, 2])).toBe("pending");
    expect(before.flashed).toEqual([]);
    expect(before.rows.map(([, e]) => e.kind)).toEqual(["notReached", "notReached"]);
    // the flash of pack 1 leaves a record: stopped at it
    const after = harness({ 1: async () => done({ ok: false, pending: true }) });
    expect(await after.run([1, 2, 0])).toBe("stopped");
    expect(after.rows.map(([, e]) => e.kind)).toEqual(["pending", "notReached", "notReached"]);
  });
  it("a cancel, an unmount or a link change while the records are read stops before that pack's flash", async () => {
    for (const change of ["cancel", "unmount", "link"] as const) {
      let cancel = false, link = "inst|port";
      const h = harness({}, {
        cancelled: () => cancel, link: () => link,
        // the look at the records answers „none open“ only after the user acted
        pending: async () => { await Promise.resolve(); if (change === "link") link = "other|port"; else cancel = true; return false; },
      });
      expect(await h.run([1, 2, 0])).toBe(change === "link" ? "link" : "cancelled");
      expect(h.flashed).toEqual([]);
      expect(h.rows.map(([a, e]) => [a, e.kind])).toEqual([[1, "notReached"], [2, "notReached"], [0, "notReached"]]);
    }
  });
  it("a link change while the backend confirms ends the series before its first pack", async () => {
    let link = "A|port-a";
    const flashed: number[] = [], rows: [number, string][] = [];
    let ended = 0, order: number[] = [];
    const end = await startFlashSeries({
      link: () => link, cancelled: () => false, pending: async () => false,
      begin: async () => { await Promise.resolve(); link = "B|port-b"; return [1, 2, 0]; },
      end: () => { ended++; }, onOrder: (o) => { order = o; },
      flash: async (a) => { flashed.push(a); return done(); }, judge: (d) => judge(d, image), report: (a, e) => rows.push([a, e.kind]),
    });
    expect(end).toBe("link");
    expect(order).toEqual([1, 2, 0]);
    expect(flashed).toEqual([]);
    expect(rows).toEqual([[1, "notReached"], [2, "notReached"], [0, "notReached"]]);
    // the grant is released
    expect(ended).toBe(1);
  });
  it("a refused confirmation still releases the grant and flashes nothing", async () => {
    let ended = 0;
    const flashed: number[] = [];
    await expect(startFlashSeries({
      link: () => "A", cancelled: () => false, pending: async () => false,
      begin: async () => { throw "refused"; }, end: () => { ended++; }, onOrder: () => {},
      flash: async (a) => { flashed.push(a); return done(); }, judge: (d) => judge(d, image), report: () => {},
    })).rejects.toBe("refused");
    expect([ended, flashed.length]).toEqual([1, 0]);
  });
  it("the result rows name this run's image and show the version reported after a flash", () => {
    const t = (k: string, p?: Record<string, unknown>) => i18n.t(k, { ...p, lng: "en" });
    const err = (e: unknown) => String(e);
    expect(seriesRowText(1, { kind: "flashed", to: "16.06" }, null, "16.06.13", t, err)).toBe("flashed to 16.06.13 (pack reports 16.06)");
    for (const why of ["missing", "incomplete", "differs", "version"] as const) {
      expect(seriesRowText(1, { kind: "deviates", why, done: done({ after: info("16.06") }) }, null, "16.06.13", t, err)).toContain("16.06");
    }
    expect(seriesRowText(2, undefined, 2, "x", t, err)).toBe(t("maint.series.flashing"));
    expect(seriesRowText(2, undefined, 1, "x", t, err)).toBe(t("maint.series.waiting"));
  });
  it("a refused new series keeps the shown result with its own image; a confirmed one replaces rows and label together", async () => {
    const t = (k: string, p?: Record<string, unknown>) => i18n.t(k, { ...p, lng: "en" });
    let view: SeriesView | null = withEnd(seriesView([1], new Map([[1, "16.05"]]), "16.06.13"), 1, { kind: "flashed", to: "16.06" });
    const text = () => seriesRowText(1, view!.rows[0].end, null, view!.label, t, String);
    expect(text()).toBe("flashed to 16.06.13 (pack reports 16.06)");
    const run = (begin: () => Promise<number[]>) => startFlashSeries({
      link: () => "A", cancelled: () => false, pending: async () => false, begin, end: () => {},
      // what the panel does: rows and label of the new image only once the backend confirmed
      onOrder: (o) => { view = seriesView(o, new Map(), "EMU1101-EXT Light 1.5.5"); },
      flash: async () => { throw "stop"; }, judge: (d) => judge(d, image), report: (a, e) => { view = withEnd(view, a, e); },
    });
    await expect(run(async () => { throw "bootloader-pending"; })).rejects.toBe("bootloader-pending");
    expect(text()).toBe("flashed to 16.06.13 (pack reports 16.06)");
    await run(async () => [1, 2]);
    expect(view!.label).toBe("EMU1101-EXT Light 1.5.5");
    expect(view!.rows.map((r) => [r.address, r.end?.kind])).toEqual([[1, "refused"], [2, "notReached"]]);
  });
  it("the summary counts flashed, stopped and not reached", () => {
    expect(summarizeSeries([{ end: { kind: "flashed", to: "16.06" } }, { end: { kind: "refused", error: "x" } }, { end: { kind: "notReached" } }, {}]))
      .toEqual({ flashed: 1, failed: 1, notReached: 1 });
  });
});
