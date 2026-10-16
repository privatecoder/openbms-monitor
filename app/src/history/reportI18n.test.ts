import { describe, expect, it } from "vitest";
import i18n from "../i18n";

const source = Object.values(import.meta.glob("../views/HealthReport.tsx", { query: "?raw", import: "default", eager: true }) as Record<string, string>)[0];

describe("health report texts", () => {
  it("every literal and built key exists in German and English", () => {
    const used = new Set([...source.matchAll(/["`](hist\.rep\.[A-Za-z0-9.]+)["`]/g)].map((m) => m[1]));
    for (const k of ["up", "down", "flat"]) used.add(`hist.rep.trend.${k}`);
    used.add("hist.tab.report");
    expect(used.size).toBeGreaterThan(25);
    for (const lng of ["de", "en"]) for (const k of used) expect(i18n.exists(k, { lng }) || i18n.exists(`${k}_other`, { lng }), `${lng} ${k}`).toBe(true);
  });
});
