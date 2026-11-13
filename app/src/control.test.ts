import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import type { PackPatch, Status } from "./api";
import { bankText, chargeLine, uart1Error, writeError, belowSocProtection, blockingAlarms, chargeFree, commandedOff, cutoffOutcome, dischargeFree, energyText, errorText, heaterOn, linkChanged, noAnswer, lockState, offCutoffText, offExpect, offKind, offOutcome, offReasons, packBusy, learnedFullAllowed, learnedFullRaw, remainingAllowed, remainingRaw, relockAsk, relockable, shownOff, socOf, switchUnclear } from "./control";

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

describe("CAN socket identity (EMU1101-EXT)", () => {
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

describe("the stored pack cutoff in „Steuern“ (EMU1101-EXT Full)", () => {
  const caps = { status_split: true, uart1_params: true, uart1_identity: true, write_gate: true, start_gate: true };
  const patch = (pack_cutoff: boolean | null, source: PackPatch["source"] = "live"): PackPatch => ({ source, release: "16.06.13 MAINT 1.6", caps: { ...caps, power_rtc: true, pack_cutoff }, at: null });
  it("asks for the stored lock before „Aus“ by bit 4 of the fresh read, and offers nothing without it", () => {
    // CAN socket with the proven cutoff: bit 4 at 0 locks, at 1 the soft off; not readable: no command
    expect(offKind("power", true, patch(true), 0xe7)).toBe("cutoff");
    expect(offKind("power", true, patch(true), 0xf7)).toBe("soft");
    expect(offKind("power", true, null, undefined)).toBe("unread");
    // without the cutoff the soft off as before (no read)
    expect(offKind("power", false, patch(null), 0xe7)).toBe("soft");
    expect(offKind("power", false, null, undefined)).toBe("soft");
    // RS485-1/2: every pack, with or without a proof; bit 4 at 0 „falls EMU1101-EXT Full“, bit 4 at 1 the soft off
    expect(offKind("full", false, patch(null, "user"), 0xef)).toBe("cutoffMaybe");
    expect(offKind("full", false, null, 0xef)).toBe("cutoffMaybe");
    expect(offKind("full", false, patch(false), 0xcf)).toBe("cutoffMaybe");
    expect(offKind("full", false, patch(true), 0xef)).toBe("cutoff");
    expect(offKind("full", false, patch(null, "flash"), 0xdf)).toBe("soft");
    expect(offKind("full", false, null, 0xff)).toBe("soft");
    // a failed read: no command, whatever proof is cached
    expect(offKind("full", false, patch(null, "flash"), undefined)).toBe("unread");
    expect(offKind("full", false, null, undefined)).toBe("unread");
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
    for (const lng of ["de", "en"]) for (const k of [...keys.map((k) => `ctl.cutoff.${k}`), "uart1.cutoff-off-failed", "uart1.cutoff-on-failed"]) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
    // RTN 08, the same text on both ports: off may hold in RAM only, on is not released
    const de = (k: string, o?: Record<string, unknown>) => i18n.t(k, { ...o, lng: "de" });
    for (const kind of ["uart1", "switch"]) {
      const off = errorText(JSON.stringify({ kind, error: "cutoff-off-failed" }), de), on = errorText(JSON.stringify({ kind, error: "cutoff-on-failed" }), de);
      expect(off).toMatch(/RTN 08/);
      expect(off).toMatch(/nur im RAM/);
      expect(off).toMatch(/Neustart/);
      expect(on).toMatch(/nicht freigegeben/);
    }
    expect(i18n.t("ctl.cutoff.offText", { lng: "de" })).toBe("Gespeicherte Packsperre: sperrt Laden und Entladen bis zum gezielten Ein, auch über Neustarts.");
    expect(i18n.t("ctl.cutoff.releasedHeld", { lng: "de" })).toBe("Sperre gelöst; Schutz hält die MOSFETs aus (siehe Zustand).");
  });
});

describe("„Aus“ of a pack that may store a cutoff, round 1", () => {
  it("hands the backend the extended off the confirmation showed", () => {
    expect(offExpect(0xe7)).toEqual({ expect: true });
    expect(offExpect(0xf7)).toEqual({ expect: false });
    expect(offExpect(undefined)).toEqual({});
  });
  it("never reports „Laden möglich“ once bit 5 is 0 or Group0 is unreadable, on either way", () => {
    expect(offCutoffText("locked", false)).toBe("ctl.cutoff.offDone");
    expect(offCutoffText("locked", true)).toBe("ctl.cutoff.offDoneMaybe");
    expect(offCutoffText("unread", true)).toBe("ctl.cutoff.offUnread");
    expect(offCutoffText(null, true)).toBeNull();
    expect(offCutoffText("notLocked", false)).toBeNull();
    for (const lng of ["de", "en"]) for (const k of ["ctl.cutoff.offDoneMaybe", "ctl.cutoff.offUnread", "uart1.cutoff-changed", "uart1.cutoff-unread", "ctl.cutoff.readFirst", "uart1.cutoff-off-failed"]) {
      expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
      expect(i18n.t(k, { lng, pack: "03" })).not.toMatch(/Laden möglich|charging possible/);
    }
    expect(errorText(JSON.stringify({ kind: "uart1", error: "cutoff-changed" }), (k, o) => i18n.t(k, { ...o, lng: "de" }))).toMatch(/^Erweitertes Aus hat sich geändert: bitte neu bestätigen/);
  });
});

describe("ein/aus answers per the confirmed contract, round 2", () => {
  const de = (k: string, o?: Record<string, unknown>) => i18n.t(k, { ...o, lng: "de" });
  it("reads an RS485-1/2 refusal as its own kind, never as the CAN socket's", () => {
    const e = JSON.stringify({ kind: "switch", error: "cutoff-unread" });
    expect(uart1Error(e)).toBeNull();
    expect(errorText(e, de)).toMatch(/nichts gesendet/);
    expect(writeError(e, de, (x) => `wrapped ${x}`)).not.toMatch(/^wrapped/);
  });
  it("calls silence after ein/aus unclear, with the polling master as the second sentence", () => {
    for (const lng of ["de", "en"]) expect(i18n.exists("ctl.onOff.noAnswer", { lng })).toBe(true);
    expect(de("ctl.onOff.noAnswer")).toMatch(/^Keine Antwort: ob das Pack geschaltet hat, ist ungeklärt; der neu gelesene Zustand zeigt es\. Ein Master mit Slaves/);
  });
  it("keeps a possible lock on RS485-1/2 as an uncertainty line", () => {
    // bit 5 at 0 of a pack whose capability is not proven: shown, but „maybeActive“ never counts as „Aus“
    expect(lockState(false, true, 0xdf)).toBe("maybeActive");
    expect(shownOff(status(0x00, 0x03), lockState(false, true, 0xdf))).toBe(false);
    expect(shownOff(status(0x00, 0x03), lockState(true, false, 0xdf))).toBe(true);
    expect(shownOff(status(0x20, 0x02), null)).toBe(true);
  });
});

describe("ein/aus after the request left, round 3", () => {
  const de = (k: string, o?: Record<string, unknown>) => i18n.t(k, { ...o, lng: "de" });
  it("is open on a broken link as on silence, on both ports, and not executed only before sending", () => {
    expect(switchUnclear(JSON.stringify({ kind: "no-answer" }))).toBe("silent");
    expect(switchUnclear(JSON.stringify({ kind: "no-answer", link: "connection closed by the gateway" }))).toBe("link");
    expect(switchUnclear(JSON.stringify({ kind: "uart1", error: "power-no-answer" }))).toBe("silent");
    expect(switchUnclear(JSON.stringify({ kind: "uart1", error: "power-lost" }))).toBe("link");
    // a refusal or a failure before sending (plain text) is no open result
    expect(switchUnclear(JSON.stringify({ kind: "switch", error: "cutoff-off-failed" }))).toBeNull();
    expect(switchUnclear("connection closed by the gateway")).toBeNull();
    for (const lng of ["de", "en"]) for (const k of ["ctl.onOff.lostAnswer", "uart1.power-lost"]) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
    expect(de("ctl.onOff.lostAnswer")).toMatch(/Ergebnis ungeklärt/);
    expect(de("ctl.onOff.lostAnswer")).toMatch(/Bit 5 nach dem Wiederverbinden/);
    expect(errorText(JSON.stringify({ kind: "uart1", error: "power-lost" }), de)).not.toMatch(/Nicht ausgeführt/);
  });
});

describe("off and lock at a pack already off", () => {
  it("offered only off, unlocked and with the extended off on; CAN socket only with the proven cutoff", () => {
    expect(relockable(true, "none", true, true, 0xef)).toBe(true);
    expect(relockable(true, null, false, false, 0xef)).toBe(true);
    expect(relockable(false, "none", true, true, 0xef)).toBe(false);
    expect(relockable(true, "active", true, true, 0xcf)).toBe(false);
    expect(relockable(true, "maybeActive", false, false, 0xcf)).toBe(false);
    expect(relockable(true, "none", true, true, 0xff)).toBe(false);
    expect(relockable(true, null, true, false, 0xef)).toBe(false);
    expect(relockable(true, null, false, false, undefined)).toBe(false);
  });
  it("the fresh read decides: confirm, already locked, extended off gone, unread", () => {
    expect(relockAsk("cutoff", 0xef)).toBe("ask");
    expect(relockAsk("cutoffMaybe", 0xef)).toBe("ask");
    expect(relockAsk("cutoff", 0xcf)).toBe("locked");
    // without a proof bit 5 at 0 is only the raw finding
    expect(relockAsk("cutoffMaybe", 0xcf)).toBe("lockedMaybe");
    expect(relockAsk("soft", 0xff)).toBe("soft");
    expect(relockAsk("unread", undefined)).toBe("unread");
  });
});

describe("learned full capacity", () => {
  it("takes Ah above 0 up to 655.35 and never 0", () => {
    expect(learnedFullRaw("280,00")).toBe(28000);
    expect(learnedFullRaw("655.35")).toBe(65535);
    expect(learnedFullRaw("655.36")).toBeNull();
    expect(learnedFullRaw("0")).toBeNull();
    expect(learnedFullRaw("0,004")).toBeNull();
    expect(learnedFullRaw("-1")).toBeNull();
    expect(learnedFullRaw("")).toBeNull();
  });
  it("is allowed from the remaining capacity up, equal included", () => {
    expect(learnedFullAllowed(10000, 100)).toBe(true);
    expect(learnedFullAllowed(9999, 100)).toBe(false);
    expect(learnedFullAllowed(28000, 100)).toBe(true);
    expect(learnedFullAllowed(28000, undefined)).toBe(false);
    expect(learnedFullAllowed(null, 100)).toBe(false);
  });
});

describe("the charge before/after a restart", () => {
  it("shows both sides and a missing one as a dash", () => {
    const t = (_k: string, o?: Record<string, unknown>) => JSON.stringify(o);
    const fmt = (v: number, d: number) => v.toFixed(d);
    const c = { soc: 79.1, remaining_ah: 221.5, full_ah: 280, cycles: 7 };
    expect(JSON.parse(chargeLine(c, { ...c, soc: 26.8, remaining_ah: 75 }, fmt, t))).toEqual({ soc: "79.1 → 26.8", rest: "221.5 → 75.0", full: "280.0 → 280.0", cycles: "7 → 7" });
    expect(JSON.parse(chargeLine(c, null, fmt, t)).soc).toBe("79.1 → –");
  });
});
