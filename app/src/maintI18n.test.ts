import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { failingText, packsDerived, packsLine } from "./maint";

// every refusal key the backend can send (reasons, block reasons used as refusals, refusal_key values)
const REFUSALS = [
  "control-on-pack-bus", "dip-off", "dip-session-open", "end-order", "end-sequential", "endpoint-in-use", "how",
  "installation-mismatch", "inverter", "isolated-or-released", "marker-unexplained", "master-unclear", "no-block",
  "no-control-channel", "no-port", "no-session", "not-patched-off", "pack-not-answering", "port-not-assigned",
  "state-changed", "stop-needs-control-channel", "update-interrupted", "update-unclear", "bootloader-unclear",
  "quiet-first", "state-unknown", "slaves-range", "not-marker-unexplained", "slave-present",
];
// every value the backend stores as a failing End check (besides "stop: …", which carries an error)
const FAILING = ["pack-communication-incomplete", "master-not-answering", "system-values", "master-not-zero", "slave-present", "stop", "slave-count-mismatch", "master-unreadable"];

describe("maintenance texts", () => {
  it("has a German and an English text for every refusal key", () => {
    expect(REFUSALS).toHaveLength(29);
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
      for (const key of [...FAILING.map((k) => `maint.failing.${k}`), "maint.panel.changeSlaves", "maint.panel.packsEq", "maint.panel.dipWas", "maint.panel.save", "maint.panel.dipZeroHint", "maint.panel.packs", "maint.panel.endOkZero"]) {
        expect(i18n.exists(key, { lng }), `${lng} ${key}`).toBe(true);
      }
    }
  });
  it("the setup names two ports, CAN socket and RS485-1/2; the endpoint list and its wording are gone", () => {
    for (const lng of ["de", "en"]) {
      for (const k of ["can", "rs485", "networkLater", "notPlugged", "oneAdapter", "err.sameAdapter", "err.packRequired", "err.packSetMeanwhile", "save"]) {
        expect(i18n.exists(`maint.setup.${k}`, { lng }), `${lng} ${k}`).toBe(true);
      }
      for (const k of ["endpoints", "packPort", "add", "remove", "control"]) expect(i18n.exists(`maint.setup.${k}`, { lng }), `${lng} ${k}`).toBe(false);
      expect(i18n.exists("maint.panel.portUnassigned", { lng })).toBe(false);
      const all = JSON.stringify(i18n.getResourceBundle(lng, "translation"));
      expect(all).not.toMatch(/USB-Port am Pack-Bus|USB port on the pack bus|Pack-Port|pack port/i);
    }
    expect(i18n.t("maint.setup.rs485", { lng: "de" })).toBe("RS485-1/2 (Packs messen und flashen, 19200 Baud)");
    expect(i18n.t("maint.flow.packPort", { lng: "de", port: "/dev/x" })).toBe("RS485-1/2: /dev/x");
    expect(i18n.t("maint.flow.packPort", { lng: "en", port: "/dev/x" })).toBe("RS485-1/2: /dev/x");
  });
  it("shows the pack count with the derived slave count and DIP value", () => {
    const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
    expect(packsLine(8, tr("de"))).toBe("8 Packs = 7 Slaves + 1 Master");
    expect(packsLine(8, tr("en"))).toBe("8 packs = 7 slaves + 1 master");
    expect(packsDerived(8, tr("de"))).toBe("8 Packs = 7 Slaves + 1 Master · DIP 5–8 des Masters standen auf 7");
    expect(packsDerived(8, tr("en"))).toBe("8 packs = 7 slaves + 1 master · the master's DIP 5–8 were set to 7");
    expect(packsLine(2, tr("de"))).toBe("2 Packs = 1 Slave + 1 Master");
    expect(packsLine(1, tr("en"))).toBe("1 pack = 0 slaves + 1 master");
  });
  it("failing texts end without a period: they go into „Prüfung fehlgeschlagen: …. Die Wartung bleibt offen.“", () => {
    for (const lng of ["de", "en"]) {
      for (const k of FAILING) expect(i18n.t(`maint.failing.${k}`, { lng, addr: 7, error: "x" }), `${lng} ${k}`).not.toMatch(/\.$/);
    }
    expect(i18n.t("maint.failing.slave-present", { lng: "de", addr: 7 })).toBe("An Adresse 7 antwortet ein Slave – es sind mehr als 1 Pack");
  });
  it("composes End's failure text with clean punctuation, also around a stop timeout", () => {
    const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
    const timeout = 'stop: {"kind":"timeout","marker":true,"slaves":0}';
    for (const lng of ["de", "en"]) {
      const text = tr(lng)("maint.panel.endFailed", { what: failingText(timeout, tr(lng)) });
      expect(text, lng).not.toMatch(/\)\.\)|\.\)|\.\.|\)\)/);
      expect(text, lng).toMatch(/\)\. /);
    }
    expect(tr("de")("maint.panel.endFailed", { what: failingText(timeout, tr("de")) }))
      .toBe("Prüfung fehlgeschlagen: „stop“ ging nicht durch: Der Master hat den neuen Zustand nicht rechtzeitig gemeldet (zuletzt Marker 1, 0 Slaves). Die Wartung bleibt offen.");
    expect(tr("de")("maint.panel.endFailed", { what: failingText("slave-count-mismatch: 0 1", tr("de")) }))
      .toBe("Prüfung fehlgeschlagen: Der Master meldet 0 Slaves, eingetragen sind 2 Packs (1 Slave). DIP 5–8 zurückstellen oder die Packzahl ändern. Die Wartung bleibt offen.");
    expect(tr("en")("maint.panel.endFailed", { what: failingText("slave-count-mismatch: 2 1", tr("en")) }))
      .toBe("Check failed: the master reports 2 slaves, 2 packs are entered (1 slave). Set DIP 5–8 back or change the pack count. Maintenance stays open.");
  });
  it("has progress texts for every panel action and phase, in German and English", () => {
    for (const lng of ["de", "en"]) {
      for (const a of ["check", "activate", "start-dip", "scan", "account-marker", "set-slaves", "measure", "end", "end-step", "clear-block"]) {
        expect(i18n.exists(`maint.progress.action.${a}`, { lng }), `${lng} ${a}`).toBe(true);
      }
      for (const p of ["check", "start", "stop", "end-check", "quiet", "scan", "scan-skipped", "bus", "values", "block"]) {
        expect(i18n.exists(`maint.progress.phase.${p}`, { lng }), `${lng} ${p}`).toBe(true);
      }
    }
    expect(i18n.t("maint.progress.phase.scan", { lng: "de", address: 7, of: 15, pass: 1, passes: 2 })).toBe("Suche Slaves (Adresse 7 von 15, Durchgang 1 von 2) …");
  });
  it("the guided flow has the same texts in German and English, with every reason flowOf can name", () => {
    const keys = (o: unknown, prefix = ""): string[] => Object.entries(o as Record<string, unknown>).flatMap(([k, v]) => (typeof v === "object" && v ? keys(v, `${prefix}${k}.`) : [`${prefix}${k}`]));
    const de = keys(i18n.getResourceBundle("de", "translation").maint.flow).sort();
    expect(keys(i18n.getResourceBundle("en", "translation").maint.flow).sort()).toEqual(de);
    for (const r of ["first", "setup-pack", "pack-unplugged", "confirm-one-adapter", "control-or-none", "check-needed", "marker-unknown", "after-scan", "after-recovery", "patched-on-no-session", "recovery-only-original", "marker-not-on", "running", "no-installation", "end-first"]) {
      expect(de).toContain(`reason.${r}`);
    }
    for (const m of ["paused", "offline", "lost"]) expect(de).toContain(`monitor.${m}`);
    for (const lng of ["de", "en"]) expect(i18n.exists("conn.paused", { lng })).toBe(true);
    // three distinct monitor texts
    for (const lng of ["de", "en"]) expect(new Set(["paused", "offline", "lost"].map((m) => i18n.t(`maint.flow.monitor.${m}`, { lng }))).size).toBe(3);
  });
  it("German quotes are „…“", () => {
    const values = (o: unknown): string[] => Object.values(o as Record<string, unknown>).flatMap((v) => (typeof v === "object" && v ? values(v) : [String(v)]));
    const text = values(i18n.getResourceBundle("de", "translation").maint.flow).join(" ");
    expect(text).not.toMatch(/["”]/);
    expect((text.match(/„/g) ?? []).length).toBe((text.match(/“/g) ?? []).length);
  });
  it("never says exclusive", () => {
    for (const lng of ["de", "en"]) expect(JSON.stringify(i18n.getResourceBundle(lng, "translation").maint)).not.toMatch(/exklusiv|exclusive/i);
  });
});
