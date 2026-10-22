import { describe, expect, it } from "vitest";
import type { MarkerView } from "./api";
import i18n from "./i18n";
import { markerError, markerLine } from "./marker";

const t = (key: string, params?: Record<string, unknown>) => i18n.t(key, { lng: "de", ...params });
const v = (o: Partial<MarkerView>): MarkerView => ({ gen: 1, address: 0, bus: "can", kind: "patched", version: "16.06.13 MAINT 1.6.1", marker: "off", slaves: 0, may_set: true, may_clear: false, ...o });

describe("maintenance marker", () => {
  it("shows the build, the marker and the slaves polled", () => {
    expect(markerLine(v({ slaves: 3 }), t).text).toBe("16.06.13 MAINT 1.6.1 · Marker gelöscht · fragt 3 Slaves ab");
    expect(markerLine(v({}), t).text).toBe("16.06.13 MAINT 1.6.1 · Marker gelöscht");
    expect(markerLine(v({ marker: "on" }), t).text).toContain("Marker gesetzt");
    expect(markerLine(v({ marker: "foreign" }), t).tone).toBe("warn");
  });
  it("points older builds and the original on RS485-1/2 to the CAN socket", () => {
    expect(markerLine(v({ bus: "pack", kind: "no-uart0", version: "16.06.13 MAINT 1.6" }), t).text).toContain("nur über die CAN-Buchse");
    expect(markerLine(v({ bus: "pack", kind: "unknown", version: null }), t).text).toContain("über die CAN-Buchse verbinden");
    // RTN 04 proves only that this image ignores it: a marker set earlier may stay
    expect(markerLine(v({ kind: "no-0x64", version: null }), t).text).toContain("kann im Speicher bleiben");
  });
  it("names the refusals and a start the master refused", () => {
    expect(markerError("marker-set-can-only", t)).toBe("Setzen geht nur über die CAN-Buchse.");
    expect(markerError("start-refused:auto-address", t)).toContain("automatische Adress");
    expect(markerError("timeout after 1500 ms", t)).toMatch(/^Nicht geändert oder nicht bestätigt: /);
  });
});
