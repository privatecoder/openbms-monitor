import { describe, expect, it } from "vitest";
import { GUIDE_PLACES, guideChoice, guidesAt, type GuideId, type GuidePlace } from "./guides";

// The one mapping guide → places: every mapped guide has its page in both languages, every guide page is placed, and
// every place is used by a view.
const PAGES = import.meta.glob("../../help/*/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const SOURCES = import.meta.glob(["./**/*.tsx", "!./**/*.test.{ts,tsx}", "!./guides.tsx"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const ids = Object.keys(GUIDE_PLACES) as GuideId[];
const places = [...new Set(Object.values(GUIDE_PLACES).flat())] as GuidePlace[];

describe("guides", () => {
  it("every mapped guide exists in both languages, and every guide page is mapped", () => {
    for (const id of ids) for (const lang of ["de", "en"]) expect(PAGES[`../../help/${lang}/${id}.md`], `${lang}/${id}`).toBeTruthy();
    const pages = Object.keys(PAGES).map((p) => /\/(guide\.[^/]+)\.md$/.exec(p)?.[1]).filter(Boolean);
    expect(pages.length).toBeGreaterThan(0);
    for (const p of pages) expect(ids).toContain(p);
  });

  it("every guide is placed somewhere, and every place is used by a view", () => {
    for (const id of ids) expect(GUIDE_PLACES[id].length).toBeGreaterThan(0);
    const used = new Set<string>();
    for (const text of Object.values(SOURCES)) for (const m of text.matchAll(/<GuideButton place="([^"]+)"/g)) used.add(m[1]);
    expect([...used].sort()).toEqual([...places].sort());
  });

  it("one guide at a place opens directly, several are offered to choose", () => {
    expect(guideChoice("parameters")).toEqual({ kind: "open", id: "guide.paramMulti" });
    expect(guidesAt("packBus")).toEqual(["guide.firmware"]);
    for (const p of places) {
      const c = guideChoice(p);
      const n = guidesAt(p).length;
      expect(c.kind).toBe(n === 1 ? "open" : "pick");
      if (c.kind === "pick") expect(c.ids).toEqual(guidesAt(p));
    }
  });
});
