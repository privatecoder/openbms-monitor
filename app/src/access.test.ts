import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { accessRows, bestState, cellTitle, decide, group0Plan, reasonText, ROUTES, type Facts, type Group0Plan, type Proof } from "./access";
import VECTORS from "./generated/access.vectors.json";
import type { AccessDecision } from "./api";

const d = (action: string, channel: "uart0" | "uart1" | "uart2", state: AccessDecision["state"], reasons: AccessDecision["reasons"] = []): AccessDecision =>
  ({ route: { action, channel, wire: "W", firmware: "any", implemented: state !== "not-implemented" }, state, reasons });

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
        { id: "firmware", need: "maint", known: "unknown" }, { id: "firmware", need: "power-flag", known: "no" }, { id: "firmware", need: "cutoff", known: "unknown" },
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
