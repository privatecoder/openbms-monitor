import { describe, expect, it } from "vitest";
import i18n from "./i18n";

// every refusal key the backend can send (reasons, block reasons used as refusals, refusal_key values)
const REFUSALS = [
  "control-on-pack-bus", "dip-off", "dip-session-open", "end-order", "end-sequential", "endpoint-in-use", "how",
  "installation-mismatch", "inverter", "isolated-or-released", "marker-unexplained", "master-unclear", "no-block",
  "no-control-channel", "no-port", "no-session", "not-patched-off", "pack-not-answering", "port-not-assigned",
  "state-changed", "stop-needs-control-channel", "update-interrupted", "update-unclear", "bootloader-unclear",
  "quiet-first", "state-unknown", "slaves-range", "not-marker-unexplained",
];
// every value the backend stores as a failing End check (besides "stop: …", which carries an error)
const FAILING = ["pack-communication-incomplete", "master-not-answering", "system-values", "stop"];

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
      for (const key of [...FAILING.map((k) => `maint.failing.${k}`), "maint.panel.changeSlaves", "maint.panel.slavesNow", "maint.panel.save", "maint.panel.dipZeroHint"]) {
        expect(i18n.exists(key, { lng }), `${lng} ${key}`).toBe(true);
      }
    }
  });
  it("never says exclusive", () => {
    for (const lng of ["de", "en"]) expect(JSON.stringify(i18n.getResourceBundle(lng, "translation").maint)).not.toMatch(/exklusiv|exclusive/i);
  });
});
