import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { serialText } from "./serial";

const t = (key: string, params?: Record<string, unknown>) => i18n.t(key, { lng: "de", ...params });

describe("serial number display", () => {
  it("shows a set value, marked when only the first 22 characters are known", () => {
    expect(serialText({ kind: "set", value: "SYN-0001", truncated: false }, "battery", "pack", false, t)).toEqual({ text: "SYN-0001" });
    expect(serialText({ kind: "set", value: "SYN-BAT-2000-01-01-000", truncated: true }, "battery", "can", true, t).text)
      .toBe("SYN-BAT-2000-01-01-000 (erste 22 Zeichen)");
  });
  it("names the placeholder and keeps the raw value as tooltip", () => {
    expect(serialText({ kind: "placeholder", value: "EMU110110S-XXXX-20000101-00000" }, "bms", "pack", false, t))
      .toEqual({ text: "nicht gesetzt (Platzhalter)", title: "EMU110110S-XXXX-20000101-00000" });
  });
  it("tells empty, unknown and unavailable apart", () => {
    expect(serialText({ kind: "empty" }, "battery", "pack", false, t).text).toBe("leer");
    expect(serialText(undefined, "battery", "pack", false, t).text).toBe("unbekannt");
    expect(serialText({ kind: "no_answer" }, "bms", "pack", false, t).text).toBe("keine Antwort");
    // CAN socket bus: slaves show neither, the master only its battery serial number
    expect(serialText({ kind: "unavailable" }, "battery", "can", false, t).text).toBe("nur über RS485-1/2 lesbar");
    expect(serialText({ kind: "unavailable" }, "bms", "can", true, t).text).toBe("nur über RS485-1/2 lesbar");
    expect(serialText({ kind: "unavailable" }, "battery", "can", true, t).text).toBe(i18n.t("sn.unsupported", { lng: "de" }));
    expect(serialText({ kind: "unavailable" }, "bms", "pack", false, t).text).toBe(i18n.t("sn.unsupported", { lng: "de" }));
  });
  it("has every text in German and English", () => {
    for (const lng of ["de", "en"])
      for (const k of ["battery", "bms", "unknown", "first22", "placeholder", "empty", "noAnswer", "packBusOnly", "unsupported"])
        expect(i18n.exists(`sn.${k}`, { lng }), `${lng} sn.${k}`).toBe(true);
  });
});
