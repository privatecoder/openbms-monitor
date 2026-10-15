import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { explain, noAnswer, partialLine } from "./connCheck";

const t = i18n.getFixedT("de");
const none = { answered: null, noise: false, intra: false, pack_at_9600: false };

describe("connection check", () => {
  it("reads the backend's no-answer error and nothing else", () => {
    expect(noAnswer('{"code":"no-answer","noise":true,"intra":false}')).toEqual({ noise: true, intra: false });
    expect(noAnswer(new Error('{"code":"no-answer","noise":false,"intra":true}'))).toEqual({ noise: false, intra: true });
    expect(noAnswer("serial port: busy")).toBeNull();
    expect(noAnswer('{"code":"installation-busy"}')).toBeNull();
  });

  it("offers the CAN socket when a pack answers there", () => {
    const r = explain("pack", { noise: false, intra: false }, { ...none, answered: 0 }, t);
    expect(r.offer).toBe("can");
    expect(r.text).toContain("RS485-1/2 (19200 Baud)");
    expect(r.text).toContain("CAN-Buchse (9600 Baud) antwortet Pack 00");
  });

  it("offers RS485-1/2 when a pack answers at 19200 baud", () => {
    const r = explain("can", { noise: true, intra: false }, { ...none, answered: 3 }, t);
    expect(r.offer).toBe("pack");
    expect(r.text).toContain("nicht lesbar");
    expect(r.text).toContain("Pack 03");
  });

  it("an answer at 9600 baud from RS485-1/2 (old firmware) is no CAN socket and offers nothing", () => {
    const r = explain("pack", { noise: false, intra: false }, { ...none, answered: 1, pack_at_9600: true }, t);
    expect(r.offer).toBeNull();
    expect(r.text).toContain("16.04.06");
  });

  it("nothing anywhere: names the states, not only the port", () => {
    const r = explain("pack", { noise: false, intra: false }, none, t);
    expect(r.offer).toBeNull();
    expect(r.text).toContain("Auch mit 9600 Baud");
    expect(r.text).toContain("Bootloader");
  });

  it("a polling master is said as such", () => {
    expect(explain("pack", { noise: false, intra: true }, none, t).text).toContain("Master fragt hier seine Slaves ab");
  });

  it("a gateway gets the gateway hint, no probe and no offer", () => {
    const r = explain("can", { noise: false, intra: false }, null, t);
    expect(r.offer).toBeNull();
    expect(r.text).toContain("Port des Gateways");
  });

  it("names the silent packs of a fixed count", () => {
    expect(partialLine([0, 1, 2], [0, 1], t)).toBe("2 von 3 Packs antworten, keine Antwort von 02.");
    expect(partialLine([0, 1], [0, 1], t)).toBe("");
  });
});
