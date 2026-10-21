import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { accessRows, cellTitle, reasonText } from "./access";
import type { AccessDecision } from "./api";

const d = (action: string, channel: "uart0" | "uart1" | "uart2", state: AccessDecision["state"], reasons: AccessDecision["reasons"] = []): AccessDecision =>
  ({ route: { action, channel, wire: "W", firmware: "any", implemented: state !== "not-implemented" }, state, reasons });

describe("access matrix texts", () => {
  it("groups the ways per action in the backend's order", () => {
    const rows = accessRows([d("read-parameters", "uart0", "ready"), d("read-parameters", "uart1", "unknown"), d("flash", "uart0", "ready")]);
    expect(rows.map((r) => [r.action, Object.keys(r.cells)])).toEqual([["read-parameters", ["uart0", "uart1"]], ["flash", ["uart0"]]]);
  });
  it("names every reason in both languages", async () => {
    for (const lng of ["de", "en"]) {
      await i18n.changeLanguage(lng);
      const t = i18n.t.bind(i18n);
      for (const r of [
        { id: "firmware", need: "maint", known: "unknown" }, { id: "firmware", need: "power-flag", known: "no" }, { id: "firmware", need: "cutoff", known: "unknown" },
        { id: "bit", group: 0, bit: 0, want: true, known: false }, { id: "bit", group: 7, bit: 3, want: true, known: null },
        { id: "master-only" }, { id: "quiet-bus" }, { id: "local-board" }, { id: "not-implemented" },
      ] as AccessDecision["reasons"]) {
        const text = reasonText(t, r);
        expect(text.length, JSON.stringify(r)).toBeGreaterThan(5);
        expect(text, JSON.stringify(r)).not.toMatch(/acc\./);
      }
      expect(cellTitle(t, d("set-can-protocol", "uart2", "not-implemented", [{ id: "local-board" }]))).toContain("W\n");
    }
  });
});
