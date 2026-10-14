import { describe, expect, it } from "vitest";
import i18n from "./i18n";

// the sources that use the write texts, as text (vite's raw import)
const sources = import.meta.glob(["./components/ParamEdit.tsx", "./components/SerialWrite.tsx", "./views/Parameters.tsx"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

describe("parameter and serial write texts", () => {
  it("every literal key exists in German and English", () => {
    const used = new Set<string>();
    for (const text of Object.values(sources)) for (const m of text.matchAll(/["`]((?:pw|snw)\.[A-Za-z0-9.]+)["`]/g)) used.add(m[1]);
    expect(used.size).toBeGreaterThan(30);
    for (const lng of ["de", "en"]) for (const k of [...used, "snw.battery", "snw.bms"])
      expect(i18n.exists(k, { lng }) || i18n.exists(`${k}_other`, { lng }), `${lng} ${k}`).toBe(true);
  });
});
