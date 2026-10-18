import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import type { PackPatch, Status } from "./api";
import { bankText, uart1Error, writeError, belowSocProtection, blockingAlarms, chargeFree, commandedOff, cutoffOutcome, dischargeFree, energyText, errorText, heaterOn, linkChanged, noAnswer, lockState, offKind, offOutcome, offReasons, packBusy, remainingAllowed, remainingRaw, socOf } from "./control";

const status = (system_status: number, switch_state: number, alarms: [string, Status["alarms"][number]["severity"]][] = []): Status => ({
  address: 1, cell_limits: [], events: [0, 0, 0, 0, 0, 0, 0, 0], switch_state, balancing: 0, system_status,
  alarms: alarms.map(([id, severity]) => ({ id, key: id, severity })),
});

describe("state", () => {
  it("keeps the off command apart from the real outputs", () => {
    const s = status(0x20, 0x02);
    expect([commandedOff(s), dischargeFree(s), chargeFree(s), heaterOn(s)]).toEqual([true, false, true, false]);
    // not switched off, but the discharge MOSFET closed by a protection
    const p = status(0x00, 0x0a, [["alarm.ev2.b3", "protection"], ["alarm.ev2.b2", "warning"], ["alarm.ev4.b6", "info"]]);
    expect([commandedOff(p), dischargeFree(p), heaterOn(p)]).toEqual([false, false, true]);
    expect(blockingAlarms(p, "discharge")).toEqual(["alarm.ev2.b3"]);
    // a charge-only protection is no reason for a closed discharge MOSFET, and the other way round
    const q = status(0x00, 0x00, [["alarm.ev2.b1", "protection"], ["alarm.ev2.b3", "protection"], ["alarm.ev3.b3", "protection"], ["alarm.ev6.b3", "protection"]]);
    expect(blockingAlarms(q, "discharge")).toEqual(["alarm.ev2.b3", "alarm.ev6.b3"]);
    expect(blockingAlarms(q, "charge")).toEqual(["alarm.ev2.b1", "alarm.ev3.b3"]);
  });

  it("names undervoltage and SOC protection as reasons for switching off again", () => {
    expect(offReasons(status(0x20, 0, [["alarm.ev2.b7", "protection"], ["alarm.ev6.b3", "protection"], ["alarm.ev3.b1", "protection"]])))
      .toEqual(["alarm.ev2.b7", "alarm.ev6.b3"]);
    expect(offReasons(status(0x20, 0))).toEqual([]);
  });
});

describe("remaining capacity", () => {
  it("takes Ah with comma or point in 10 mAh", () => {
    expect(remainingRaw("140,5")).toBe(14050);
    expect(remainingRaw("0")).toBe(0);
    expect(remainingRaw(" 99.994 ")).toBe(9999);
    for (const bad of ["", "-1", "abc", "655.36"]) expect(remainingRaw(bad), bad).toBeNull();
  });

  it("allows 0 up to the full capacity", () => {
    expect(remainingAllowed(28000, 280)).toBe(true);
    expect(remainingAllowed(0, 280)).toBe(true);
    expect(remainingAllowed(28001, 280)).toBe(false);
    expect(remainingAllowed(100, null)).toBe(false);
    expect(remainingAllowed(null, 280)).toBe(false);
  });

  it("shows the SOC it gives and the SOC protection", () => {
    expect(socOf(14000, 280)).toBeCloseTo(50);
    expect(socOf(100, 0)).toBe(0);
    expect(belowSocProtection(4, 5)).toBe(true);
    expect(belowSocProtection(5, 5)).toBe(false);
    expect(belowSocProtection(4, null)).toBeNull();
  });
});

describe("errors", () => {
  const t = (k: string, o?: Record<string, unknown>) => (o ? `${k} ${JSON.stringify(o)}` : k);
  it("reads the pack reservation from the backend's JSON", () => {
    const e = JSON.stringify({ kind: "pack-busy", address: 3, action: "history" });
    expect(packBusy(e)).toEqual({ address: 3, action: "history" });
    expect(packBusy(JSON.stringify({ kind: "refused" }))).toBeNull();
    expect(packBusy("timeout")).toBeNull();
    expect(errorText(e, t)).toBe('ctl.busy {"pack":"03","action":"ctl.action.history"}');
    expect(errorText("timeout after 1500 ms", t)).toBe("timeout after 1500 ms");
    // a silent pack and a connection changed meanwhile have their own texts
    expect(noAnswer('{"kind":"no-answer"}')).toBe(true);
    expect(linkChanged('{"kind":"link-changed"}')).toBe(true);
    expect(noAnswer('{"kind":"refused"}')).toBe(false);
    expect(errorText('{"kind":"no-answer"}', t)).toBe("ctl.silent");
    expect(errorText('{"kind":"link-changed"}', t)).toBe("ctl.linkChanged");
  });

  it("writes energy in kWh or MWh", () => {
    expect(energyText(850, "de")).toBe("850 kWh");
    expect(energyText(4500, "de")).toBe("4,5 MWh");
  });
});

describe("CAN socket parameter service errors", () => {
  it("are told apart and named in both languages", () => {
    for (const error of ["no-answer", "protocol", "pending", "other-master", "no-identity", "readback", "unchecked", "changed", "duplicate", "elsewhere"]) {
      const e = JSON.stringify({ kind: "uart1", error, code: 0x87 });
      expect(uart1Error(e)?.error).toBe(error);
      for (const lng of ["de", "en"]) expect(i18n.exists(`uart1.${error}`, { lng }), `${lng} ${error}`).toBe(true);
    }
    const t = i18n.getFixedT("de");
    expect(errorText(JSON.stringify({ kind: "uart1", error: "protocol", code: 0x87 }), t)).toContain("87");
    // said as it is, not wrapped as a plain error
    const noId = JSON.stringify({ kind: "uart1", error: "no-identity" });
    expect(writeError(noId, t, (err) => `X ${err}`)).toBe(t("uart1.no-identity"));
    expect(uart1Error("timeout")).toBeNull();
    expect(uart1Error(JSON.stringify({ kind: "bank" }))).toBeNull();
  });
});

describe("CAN socket identity (MAINT 1.3)", () => {
  it("names the place to sort out and the reasons to take a board over", () => {
    const t = i18n.getFixedT("de");
    const e = JSON.stringify({ kind: "uart1", error: "elsewhere", address: 4, board: 7, at: 5 });
    expect(uart1Error(e)).toEqual({ kind: "uart1", error: "elsewhere", address: 4, board: 7, at: 5 });
    expect(errorText(e, t)).toContain("04");
    for (const lng of ["de", "en"]) for (const k of ["markGone", "markGoneConfirm", "markGoneYes", "markGoneNo", "markedGone", "markGoneChanged"]) expect(i18n.exists(`uart1.${k}`, { lng }), `${lng} ${k}`).toBe(true);
    for (const why of ["legacy", "earlier"] as const) {
      const text = bankText({ state: "unconfirmed", address: 2, why }, t);
      expect(text).toBe(t(`bank.refused.${why}`, { pack: "02" }));
      expect(text).toContain("02");
    }
  });
});

describe("„Aus“ after its acknowledgement", () => {
  it("is done only when the fresh read shows off and discharging blocked", () => {
    expect(offOutcome(status(0x20, 0x02))).toBe("off");
    // charging current lifted „off“ meanwhile
    expect(offOutcome(status(0x00, 0x03))).toBe("on");
    expect(offOutcome(status(0x00, 0x02))).toBe("on");
    // the bit set, but the discharge MOSFET still on
    expect(offOutcome(status(0x20, 0x03))).toBe("unclear");
    for (const lng of ["de", "en"]) for (const k of ["ctl.onOff.offBackOn", "ctl.onOff.offUnclear", "ctl.tip.offMaster", "ctl.canUnsupported"]) expect(i18n.exists(k, { lng })).toBe(true);
    // the master's texts promise no blocked bank and no other packs staying on
    expect(i18n.t("ctl.off.masterText", { lng: "de", pack: "00" })).not.toMatch(/ganze Bank|bleiben an/);
    expect(i18n.t("ctl.tip.offMaster", { lng: "en" })).not.toMatch(/whole bank|stay on/);
  });
});

describe("the stored pack cutoff in „Steuern“ (MAINT 1.6)", () => {
  const caps = { status_split: true, uart1_params: true, uart1_identity: true, write_gate: true, start_gate: true };
  const patch = (pack_cutoff: boolean | null, source: PackPatch["source"] = "live"): PackPatch => ({ source, release: "16.06.13 MAINT 1.6", caps: { ...caps, power_rtc: true, pack_cutoff }, at: null });
  it("asks for the stored lock before „Aus“ by bit 4 (and on RS485-1/2 bit 5) of the fresh read", () => {
    // CAN socket with the proven cutoff: bit 4 at 0 locks, at 1 the soft off; not readable: the lock to be safe
    expect(offKind("power", true, patch(true), 0xe7)).toBe("cutoff");
    expect(offKind("power", true, patch(true), 0xf7)).toBe("soft");
    expect(offKind("power", true, null, undefined)).toBe("cutoff");
    expect(offKind("power", false, patch(null), 0xe7)).toBe("soft");
    // RS485-1/2: any proof with bit 4 or 5 at 0 „falls MAINT 1.6“; without a proof as before
    expect(offKind("full", false, patch(null, "user"), 0xef)).toBe("cutoffMaybe");
    expect(offKind("full", false, patch(null, "flash"), 0xdf)).toBe("cutoffMaybe");
    expect(offKind("full", false, patch(null, "flash"), 0xff)).toBe("soft");
    expect(offKind("full", false, null, 0xcf)).toBe("soft");
    expect(offKind("full", false, patch(false), 0xcf)).toBe("soft");
  });
  it("reports the result from the fresh bit 5 and status, never from the acknowledgement", () => {
    const free = status(0x00, 0x03), held = status(0x00, 0x02);
    expect(cutoffOutcome(false, 0xc7, status(0x20, 0x00))).toBe("locked");
    expect(cutoffOutcome(false, 0xe7, status(0x20, 0x02))).toBe("notLocked");
    expect(cutoffOutcome(false, 0xf7, status(0x20, 0x02))).toBeNull();
    expect(cutoffOutcome(true, 0xc7, free)).toBe("stillLocked");
    expect(cutoffOutcome(true, 0xe7, free)).toBe("released");
    expect(cutoffOutcome(true, 0xe7, held)).toBe("releasedHeld");
    // released also with bit 4 off meanwhile
    expect(cutoffOutcome(true, 0xf7, free)).toBe("released");
    expect(cutoffOutcome(true, undefined, free)).toBe("unread");
    expect(cutoffOutcome(false, undefined, null)).toBe("unread");
  });
  it("shows the lock in the state line only where it means one", () => {
    expect(lockState(true, false, 0xc7)).toBe("active");
    expect(lockState(true, false, 0xe7)).toBe("none");
    expect(lockState(false, true, 0xc7)).toBe("maybeActive");
    expect(lockState(false, true, 0xe7)).toBeNull();
    expect(lockState(false, false, 0xc7)).toBeNull();
    expect(lockState(true, false, undefined)).toBeNull();
  });
  it("has its texts in both languages and never calls RTN 08 a success", () => {
    const keys = ["offTitle", "offConfirm", "offText", "maybe", "offMore", "master", "ccl", "offDone", "notLocked", "released", "releasedHeld", "stillLocked", "unread", "bit5", "bit5Locked", "bit5Free", "state", "active", "none", "maybeActive", "tipOff", "tipOn"];
    for (const lng of ["de", "en"]) for (const k of [...keys.map((k) => `ctl.cutoff.${k}`), "uart1.cutoff-failed"]) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
    const e = errorText(JSON.stringify({ kind: "uart1", error: "cutoff-failed" }), (k, o) => i18n.t(k, { ...o, lng: "de" }));
    expect(e).toMatch(/RTN 08/);
    expect(e).toMatch(/nicht freigegeben/);
    expect(i18n.t("ctl.cutoff.offText", { lng: "de" })).toBe("Gespeicherte Packsperre: sperrt Laden und Entladen bis zum gezielten Ein, auch über Neustarts.");
    expect(i18n.t("ctl.cutoff.releasedHeld", { lng: "de" })).toBe("Sperre gelöst; Schutz hält die MOSFETs aus (siehe Zustand).");
  });
});
