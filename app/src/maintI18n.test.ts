import { describe, expect, it } from "vitest";
import i18n from "./i18n";

// every refusal key the backend can send (reasons, block reasons used as refusals, refusal_key values)
const REFUSALS = [
  "control-on-pack-bus", "dip-off", "dip-session-open", "end-order", "end-sequential", "endpoint-in-use", "how",
  "installation-mismatch", "inverter", "isolated-or-released", "marker-unexplained", "master-unclear", "no-block",
  "no-control-channel", "no-port", "no-session", "not-patched-off", "pack-not-answering", "port-not-assigned",
  "state-changed", "stop-needs-control-channel", "update-interrupted", "update-unclear", "bootloader-unclear",
  "quiet-first", "state-unknown",
];

describe("maintenance texts", () => {
  it("has a German and an English text for every refusal key", () => {
    expect(REFUSALS).toHaveLength(26);
    for (const lng of ["de", "en"]) {
      for (const k of REFUSALS) {
        const key = `maint.err.refused.${k}`;
        expect(i18n.exists(key, { lng }), `${lng} ${key}`).toBe(true);
        expect(i18n.t(key, { lng })).not.toBe(key);
      }
    }
  });
  it("never says exclusive", () => {
    for (const lng of ["de", "en"]) expect(JSON.stringify(i18n.getResourceBundle(lng, "translation").maint)).not.toMatch(/exklusiv|exclusive/i);
  });
});
