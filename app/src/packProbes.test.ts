import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { packProbeLine, probeForLink } from "./packProbes";

const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });

describe("a slave's own 64/03 in its pack view", () => {
  it("is not checked yet, proven, negative or undecided", () => {
    expect(packProbeLine(undefined, tr("de"))).toBe("Firmwarekennung: noch ungeprüft");
    const proven = { gen: 3, address: 1, state: "proven" as const, version: "EMU1101-EXT Light 1.5.4", listed: "EMU1101-EXT Light 1.5.4", build_id: "297f9f2aa8402e9f", hard_stop: false };
    expect(packProbeLine(proven, tr("de"))).toBe("Firmwarekennung: EMU1101-EXT Light 1.5.4 (Build 297f9f2aa8402e9f)");
    expect(packProbeLine({ ...proven, state: "negative", listed: null, version: "EMU1101-EXT 1.4.0" }, tr("en"))).toBe("Firmware identity: EMU1101-EXT 1.4.0, without patch functions");
    expect(packProbeLine({ ...proven, state: "negative", listed: null, version: null, build_id: null, hard_stop: null }, tr("de"))).toBe("Firmwarekennung: Patch-Funktionen von dieser App nicht belegt (Format unbekannt)");
    expect(packProbeLine({ ...proven, state: "undecided", version: null, listed: null, build_id: null, hard_stop: null }, tr("en"))).toContain("no answer to 64/03");
    for (const lng of ["de", "en"]) for (const k of ["par.gateStartMasterOnly", "par.patch.source.live", "probe.recheck"]) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
  });
});

describe("a finding of another connection", () => {
  it("is never shown on the current one", () => {
    const p = { gen: 3, address: 1, state: "proven" as const, version: "EMU1101-EXT Light 1.5.4", listed: null, build_id: "297f9f2aa8402e9f", hard_stop: false };
    expect(probeForLink(p, 3)).toBe(true);
    expect(probeForLink(p, 4)).toBe(false);
    expect(probeForLink(p, undefined)).toBe(false);
  });
});
