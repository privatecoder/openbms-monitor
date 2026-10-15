import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import type { IdentityView } from "./api";
import { bankLine, batteryFromBms, noSerialPacks, packList, withIdentity } from "./bank";
import { bankRefusal, bankText, errorText, writeError } from "./control";

const id = (address: number, kind: IdentityView["kind"], why: IdentityView["why"] = null, other: string | null = null, hint = false): IdentityView =>
  ({ kind, address, serial: kind === "unchecked" ? null : `SYN-${address}`, why, installation: other ? { id: other.toLowerCase(), name: other, archived: false } : null, hint });

describe("sidebar line", () => {
  it("asks once for all new and moved packs", () => {
    const l = bankLine([id(0, "unconfirmed", "new"), id(1, "known"), id(3, "unconfirmed", "moved"), id(4, "unchecked")]);
    expect(l).toEqual({ kind: "new", packs: [0, 3], confirm: [0, 3] });
    expect(bankLine([id(1, "known"), id(2, "failed")])).toBeNull();
  });
  it("a pack of another installation turns the line red and names that installation; taking over covers all listed", () => {
    const l = bankLine([id(0, "unconfirmed", "new"), id(2, "unconfirmed", "foreign", "B"), id(5, "unconfirmed", "foreign", "C"), id(6, "unconfirmed", "foreign", "B")]);
    expect(l).toEqual({ kind: "foreign", packs: [2, 6], other: { id: "b", name: "B", archived: false }, hint: false, confirm: [0, 2, 5, 6] });
  });
  it("a CAN socket hint has nothing to take over", () => {
    expect(bankLine([id(0, "unconfirmed", "foreign", "B", true)])).toMatchObject({ kind: "foreign", hint: true, confirm: [] });
  });
  it("a later read replaces an earlier one; packs without serial are listed", () => {
    const all = withIdentity(withIdentity([id(3, "unconfirmed", "new")], id(1, "unchecked")), id(3, "known"));
    expect(all.map((v) => [v.address, v.kind])).toEqual([[1, "unchecked"], [3, "known"]]);
    expect(noSerialPacks(all)).toEqual([1]);
    expect(packList([0, 12])).toBe("00, 12");
  });
});

describe("battery serial from the BMS serial", () => {
  it("fills what fits and says what the CAN socket shows", () => {
    expect(batteryFromBms(" SYN-0001 ")).toEqual({ text: "SYN-0001", cut: false, beyondCan: false });
    expect(batteryFromBms("A".repeat(25))).toEqual({ text: "A".repeat(25), cut: false, beyondCan: true });
    expect(batteryFromBms("B".repeat(34))).toEqual({ text: "B".repeat(30), cut: true, beyondCan: true });
  });
});

describe("bank refusals", () => {
  const t = (k: string, o?: Record<string, unknown>) => (o ? `${k} ${JSON.stringify(o)}` : k);
  it("are recognised and say what to do", () => {
    const foreign = JSON.stringify({ kind: "bank", state: "unconfirmed", address: 2, why: "foreign", serial: "SYN-2", installation: { id: "b", name: "B", archived: false } });
    expect(bankRefusal(foreign)?.address).toBe(2);
    expect(errorText(foreign, t)).toBe('bank.refused.foreign {"pack":"02","name":"B"}');
    expect(errorText('{"kind":"bank","state":"unconfirmed","address":3,"why":"moved"}', t)).toBe('bank.refused.moved {"pack":"03"}');
    expect(errorText('{"kind":"bank","state":"failed","address":0,"error":"no-answer"}', t)).toBe('bank.refused.silent {"pack":"00"}');
    expect(bankRefusal('{"kind":"pack-busy","address":1,"action":"read"}')).toBeNull();
    expect(bankRefusal("timeout")).toBeNull();
    // a bank refusal is not wrapped into „Nicht geschrieben: …“, other errors are
    expect(writeError(foreign, t, (err) => `wrapped ${err}`)).toBe('bank.refused.foreign {"pack":"02","name":"B"}');
    expect(writeError("timeout", t, (err) => `wrapped ${err}`)).toBe("wrapped timeout");
  });
  it("every text exists in German and English", () => {
    const keys = ["bank.new_one", "bank.new_other", "bank.adopt", "bank.foreign_one", "bank.foreign_other", "bank.disconnect", "bank.connectOther", "bank.adoptAnyway",
      "bank.confirmFailed", "bank.noSerial_one", "bank.noSerial_other", "bank.openParams", "bank.unchecked", "bank.refused.new", "bank.refused.moved",
      "bank.refused.foreign", "bank.refused.silent", "bank.refused.failed", "snw.useBms", "snw.useBmsHint", "snw.useBmsCut", "snw.useBmsCan"];
    for (const lng of ["de", "en"]) for (const k of keys) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
    const de = i18n.getFixedT("de");
    expect(bankText({ state: "failed", address: 4, error: "I/O" }, de)).toContain("Nichts geschrieben");
  });
});
