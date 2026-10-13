import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { packsDerived, packsLine } from "./maint";

// every refusal key the backend can send (reasons, block reasons used as refusals, refusal_key values)
const REFUSALS = [
  "control-on-pack-bus", "dip-off", "dip-session-open", "end-order", "end-sequential", "endpoint-in-use", "how",
  "installation-mismatch", "inverter", "isolated-or-released", "marker-unexplained", "master-unclear", "no-block",
  "no-control-channel", "no-port", "no-session", "not-patched-off", "pack-not-answering", "port-not-assigned",
  "state-changed", "stop-needs-control-channel", "update-interrupted", "update-unclear", "bootloader-unclear",
  "quiet-first", "state-unknown", "slaves-range", "not-marker-unexplained",
];
// every value the backend stores as a failing End check (besides "stop: …", which carries an error)
const FAILING = ["pack-communication-incomplete", "master-not-answering", "system-values", "master-not-zero", "stop"];

describe("maintenance texts", () => {
  it("has a German and an English text for every refusal key", () => {
    expect(REFUSALS).toHaveLength(28);
    for (const lng of ["de", "en"]) {
      for (const k of REFUSALS) {
        const key = `maint.err.refused.${k}`;
        expect(i18n.exists(key, { lng }), `${lng} ${key}`).toBe(true);
        expect(i18n.t(key, { lng })).not.toBe(key);
      }
    }
  });
  it("has a German and an English text for every failing End check and the new panel texts", () => {
    for (const lng of ["de", "en"]) {
      for (const key of [...FAILING.map((k) => `maint.failing.${k}`), "maint.panel.changeSlaves", "maint.panel.packsEq", "maint.panel.dipWas", "maint.panel.save", "maint.panel.dipZeroHint", "maint.panel.packs", "maint.panel.endOkZero"]) {
        expect(i18n.exists(key, { lng }), `${lng} ${key}`).toBe(true);
      }
    }
  });
  it("shows the pack count with the derived slave count and DIP value", () => {
    const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
    expect(packsLine(12, tr("de"))).toBe("12 Packs = 11 Slaves + 1 Master");
    expect(packsLine(12, tr("en"))).toBe("12 packs = 11 slaves + 1 master");
    expect(packsDerived(12, tr("de"))).toBe("12 Packs = 11 Slaves + 1 Master · DIP 5–8 des Masters standen auf 11");
    expect(packsDerived(12, tr("en"))).toBe("12 packs = 11 slaves + 1 master · the master's DIP 5–8 were set to 11");
    expect(packsLine(2, tr("de"))).toBe("2 Packs = 1 Slave + 1 Master");
    expect(packsLine(1, tr("en"))).toBe("1 pack = 0 slaves + 1 master");
  });
  it("never says exclusive", () => {
    for (const lng of ["de", "en"]) expect(JSON.stringify(i18n.getResourceBundle(lng, "translation").maint)).not.toMatch(/exklusiv|exclusive/i);
  });
});
