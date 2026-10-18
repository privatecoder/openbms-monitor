import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { packProbeLine } from "./packProbes";

const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });

describe("a slave's own 64/03 in its pack view", () => {
  it("is not checked yet, proven, negative or undecided", () => {
    expect(packProbeLine(undefined, tr("de"))).toBe("Firmwarekennung: noch ungeprüft");
    const proven = { address: 1, state: "proven" as const, version: "16.06.13 MAINT 1.5", listed: "16.06.13 MAINT 1.5", build_id: "297f9f2aa8402e9f", hard_stop: false };
    expect(packProbeLine(proven, tr("de"))).toBe("Firmwarekennung: 16.06.13 MAINT 1.5 (Build 297f9f2aa8402e9f)");
    expect(packProbeLine({ ...proven, state: "negative", listed: null, version: "16.06.13 MAINT 1.4" }, tr("en"))).toBe("Firmware identity: 16.06.13 MAINT 1.4, without patch functions");
    expect(packProbeLine({ ...proven, state: "undecided", version: null, listed: null, build_id: null, hard_stop: null }, tr("en"))).toContain("no answer to 64/03");
    for (const lng of ["de", "en"]) for (const k of ["par.gateStartMasterOnly", "par.patch.source.live", "probe.recheck"]) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
  });
});
