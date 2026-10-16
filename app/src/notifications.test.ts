import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { NOTIFY_DEFAULTS, notifySettings, notifyTexts } from "./notifications";

const source = Object.values(import.meta.glob("./views/Recording.tsx", { query: "?raw", import: "default", eager: true }) as Record<string, string>)[0];

describe("notification settings", () => {
  it("are off by default and keep the backend's ranges", () => {
    expect(notifySettings(null)).toEqual(NOTIFY_DEFAULTS);
    expect(notifySettings("x")).toEqual(NOTIFY_DEFAULTS);
    expect(notifySettings({ enabled: true })).toEqual({ ...NOTIFY_DEFAULTS, enabled: true });
    expect(notifySettings({ enabled: "yes", spread_mv: 0, lost_minutes: 99999 })).toEqual({ enabled: false, spread_mv: 10, lost_minutes: 1440 });
    expect(notifySettings({ enabled: true, spread_mv: 42.4, lost_minutes: Number.NaN })).toEqual({ enabled: true, spread_mv: 42, lost_minutes: 5 });
  });
});

describe("notification texts", () => {
  const names: Record<string, string> = { "alarm.ev2.b1": "Zell-Überspannungsschutz" };
  const name = (id: string) => names[id] ?? id;

  it("keep the placeholders the backend fills, one pack and several", () => {
    const de = notifyTexts(i18n.getFixedT("de"), name);
    expect(de.alarm_one).toBe("Pack {{packs}}: {{alarm}}");
    expect(de.alarm_other).toBe("{{alarm}} an Pack {{packs}}");
    expect(de.silent_other).toBe("Packs {{packs}} antworten nicht mehr");
    expect(de.title).toContain("{{name}}");
    expect(de.lost).toContain("{{min}}");
    // only alarms with a name of their own
    expect(de.alarms).toEqual({ "alarm.ev2.b1": "Zell-Überspannungsschutz" });
    const en = notifyTexts(i18n.getFixedT("en"), name);
    expect(en.silent_one).toBe("Pack {{packs}} stopped answering");
    expect(en.spread_one).toContain("{{mv}}");
    expect(en.spread_other).toContain("{{limit}}");
  });

  it("every key of the settings block exists in German and English", () => {
    const used = new Set([...source.matchAll(/["`](notify\.[A-Za-z0-9.]+)["`]/g)].map((m) => m[1]));
    expect(used.size).toBeGreaterThan(8);
    for (const lng of ["de", "en"]) for (const k of used) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
  });
});
