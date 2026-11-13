import { describe, expect, it } from "vitest";

// Every help id the UI names literally, and every `related` id of a help page, exists in both languages.
const SOURCES = import.meta.glob(["./**/*.{ts,tsx}", "!./**/*.test.{ts,tsx}", "!./generated/**"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const PAGES = import.meta.glob("../../help/*/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

const page = (lang: string, id: string): string | undefined => PAGES[`../../help/${lang}/${id}.md`];
const ids = (lang: string): string[] =>
  Object.keys(PAGES).filter((p) => p.startsWith(`../../help/${lang}/`)).map((p) => p.slice(`../../help/${lang}/`.length, -3)).sort();

describe("help ids", () => {
  it("every literal topic/value/status id in the UI has a page in both languages", () => {
    const used = new Set<string>();
    for (const text of Object.values(SOURCES)) {
      for (const m of text.matchAll(/["'`]((?:topic|value|status)\.[A-Za-z0-9_-]+)["'`]/g)) used.add(m[1]);
    }
    expect(used.size).toBeGreaterThan(20);
    expect([...used].filter((id) => !page("de", id) || !page("en", id))).toEqual([]);
  });

  it("every related id of a help page exists, and both languages have the same pages", () => {
    expect(ids("de")).toEqual(ids("en"));
    const broken: string[] = [];
    for (const lang of ["de", "en"]) {
      for (const id of ids(lang)) {
        const related = /^related:(.*)$/m.exec(page(lang, id) ?? "")?.[1] ?? "";
        for (const r of related.split(",").map((s) => s.trim()).filter(Boolean)) if (!page(lang, r)) broken.push(`${lang}/${id} → ${r}`);
      }
    }
    expect(broken).toEqual([]);
  });
});
