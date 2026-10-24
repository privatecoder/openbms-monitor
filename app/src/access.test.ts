import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { accessRows, bestState, cellTitle, decide, group0Plan, reasonText, ROUTES, type Facts, type Group0Plan, type Proof } from "./access";
import VECTORS from "./generated/access.vectors.json";
import type { AccessDecision } from "./api";

const d = (action: string, channel: "uart0" | "uart1" | "uart2", state: AccessDecision["state"], reasons: AccessDecision["reasons"] = []): AccessDecision =>
  ({ route: { action, channel, wire: "W", implemented: state !== "not-implemented" }, state, reasons });

describe("access matrix texts", () => {
  it("groups the ways per action in the backend's order", () => {
    const rows = accessRows([d("read-parameters", "uart0", "ready"), d("read-parameters", "uart1", "unknown"), d("flash", "uart0", "ready")]);
    expect(rows.map((r) => [r.action, Object.keys(r.cells)])).toEqual([["read-parameters", ["uart0", "uart1"]], ["flash", ["uart0"]]]);
    // two ways on one channel: the cell shows the better one
    const two = accessRows([d("read-serials", "uart1", "blocked"), d("read-serials", "uart1", "ready")]);
    expect(bestState(two[0].cells.uart1!)).toBe("ready");
  });
  it("names every reason in both languages", async () => {
    for (const lng of ["de", "en"]) {
      await i18n.changeLanguage(lng);
      const t = i18n.t.bind(i18n);
      for (const r of [
        { id: "firmware", need: { kind: "patch-from", version: [1, 5] }, known: "unknown" }, { id: "firmware", need: { kind: "image-flag", bit: 1 }, known: "no" },
        { id: "firmware", need: { kind: "image-flag", bit: 2 }, known: "unknown" }, { id: "firmware", need: { kind: "image-flag", bit: 5 }, known: "no" },
        { id: "bit", group: 0, bit: 0, want: true, known: false }, { id: "bit", group: 7, bit: 3, want: true, known: null },
        { id: "master-only" }, { id: "quiet-bus" }, { id: "local-board" }, { id: "not-implemented" }, { id: "view-only" },
        { id: "marker", on: false, known: "foreign" }, { id: "marker", on: true, known: null },
        { id: "base", from: [16, 6, 4], known: [16, 6, 3] }, { id: "base", from: [16, 6, 11], known: null },
        { id: "with-slaves", known: 0 }, { id: "with-slaves", known: null },
      ] as AccessDecision["reasons"]) {
        const text = reasonText(t, r);
        expect(text.length, JSON.stringify(r)).toBeGreaterThan(5);
        expect(text, JSON.stringify(r)).not.toMatch(/acc\./);
      }
      expect(cellTitle(t, [d("set-can-protocol", "uart2", "not-implemented", [{ id: "local-board" }])])).toMatch(/^W: .+\n· /);
      // a flag the texts do not name yet (a later build) still reads as a sentence with its bit, the version as „1.7“
      expect(reasonText(t, { id: "firmware", need: { kind: "image-flag", bit: 5 }, known: "no" })).toMatch(/5/);
      expect(reasonText(t, { id: "firmware", need: { kind: "patch-from", version: [1, 7] }, known: "no" })).toMatch(/1\.7/);
    }
  });
});

describe("the catalog evaluated as in Rust", () => {
  it("decides every route like access::decide for every vector", () => {
    for (const v of VECTORS.decide) {
      const got = ROUTES.map((r) => decide(r, v.facts as Facts));
      expect(got).toEqual(v.decisions);
    }
    expect(VECTORS.decide.length).toBeGreaterThan(50);
  });
  it("plans Group0 like access::group0_plan", () => {
    for (const v of VECTORS.group0) {
      expect(group0Plan(v.channel as "uart0" | "uart1" | "uart2", v.proof as Proof | null)).toEqual(v.plan as Group0Plan);
    }
  });
});

describe("the firmware a way needs", () => {
  it("names the first MAINT version, the power/RTC build of 1.5 and the OEM base", async () => {
    const { needText, routeNeed } = await import("./access");
    const de = i18n.getFixedT("de");
    const need = (action: string, channel: string) => needText(de, routeNeed(ROUTES.find((r) => r.action === action && r.channel === channel)!));
    expect(need("read-parameters", "uart1")).toBe("ab MAINT 1.5");
    expect(need("power-on-off", "uart1")).toBe("ab MAINT 1.5 (Power/RTC-Build)");
    expect(need("pack-lock-set", "uart1")).toBe("ab MAINT 1.6");
    expect(need("read-identity", "uart0")).toBe("ab MAINT 1.5.1");
    expect(need("read-system", "uart1")).toBe("ab OEM 16.06.04");
    // the original firmware is enough
    expect(need("read-telemetry", "uart0")).toBe("");
    // every way with a firmware condition gets a text
    for (const r of ROUTES) if (r.firmware.length) expect(needText(de, routeNeed(r))).toMatch(/^ab MAINT /);
  });
});
