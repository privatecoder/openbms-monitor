import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { busLine, masterLine } from "./maint";
import type { MasterView } from "./api";

// the sources that use maintenance texts, as text (vite's raw import)
const sources = import.meta.glob(["./views/PackBusPanel.tsx", "./views/MaintBanner.tsx", "./views/MaintSetup.tsx", "./views/Firmware.tsx", "./views/NameDialog.tsx", "./views/Installations.tsx", "./views/InstallationTransfer.tsx", "./views/Sidebar.tsx", "./maint.ts", "./maintStore.ts"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

// every refusal key the backend can send
const REFUSALS = ["no-control-channel", "control-on-pack-bus", "halt-not-offered", "release-not-offered", "release-master-first", "release-master-first-unread", "no-port", "no-serial", "name-required", "name-taken", "unknown-installation", "installation-busy", "installation-connected", "merge-self", "history-preparing", "bundle-unreadable", "bundle-entry", "bundle-too-large", "bundle-no-manifest", "bundle-manifest", "bundle-format", "bundle-checksum", "bundle-missing", "bundle-symlink", "import-conflict", "import-master-differs", "import-id-exists"];
const keys = (o: unknown, prefix = ""): string[] => Object.entries(o as Record<string, unknown>).flatMap(([k, v]) => (typeof v === "object" && v ? keys(v, `${prefix}${k}.`) : [`${prefix}${k}`]));

describe("maintenance texts", () => {
  it("every literal key the panel, banner, setup and firmware view use exists in German and English", () => {
    const used = new Set<string>();
    for (const text of Object.values(sources)) for (const m of text.matchAll(/["`]((?:maint|fw|conn|inst)\.[A-Za-z0-9.-]+)["`]/g)) used.add(m[1]);
    // localStorage keys, not texts
    for (const k of ["maint.installation", "maint.enabled", "conn.kind", "conn.target", "conn.bus", "conn.packs"]) used.delete(k);
    expect(used.size).toBeGreaterThan(40);
    for (const lng of ["de", "en"]) for (const k of used) expect(i18n.exists(k, { lng }) || i18n.exists(`${k}_other`, { lng }), `${lng} ${k}`).toBe(true);
  });
  it("has the keys built at run time", () => {
    const dynamic = [
      ...REFUSALS.map((r) => `maint.err.refused.${r}`),
      ...["match", "stored", "unchecked"].flatMap((b) => [`maint.bank.${b}`, `maint.bank.flash.${b}`]),
      ...["connection", "unsupported", "no-answer", "rejected", "timeout", "busy", "store", "text", "other-bank", "bootloader-pending"].map((k) => `maint.err.${k}`),
      ...["live", "scan", "halt", "release", "flash", "other"].map((a) => `conn.busyAction.${a}`),
      ...["ok", "failed", "pending", "open"].map((r) => `inst.flash.${r}`),
      ...["read", "written", "refused"].map((k) => `par.kind.${k}`),
    ];
    for (const lng of ["de", "en"]) for (const k of dynamic) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
  });
  it("German and English have the same maintenance keys", () => {
    const de = keys(i18n.getResourceBundle("de", "translation").maint).sort();
    expect(keys(i18n.getResourceBundle("en", "translation").maint).sort()).toEqual(de);
  });
  it("the guided panel's texts are gone", () => {
    for (const lng of ["de", "en"]) {
      for (const k of ["maint.flow", "maint.openWork", "maint.failing", "maint.plan", "maint.progress", "maint.note", "maint.state", "maint.master", "fw.next", "fw.endMaint"]) {
        expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(false);
      }
    }
  });
  it("names the spec's texts", () => {
    const de = (k: string, p?: Record<string, unknown>) => i18n.t(k, { ...p, lng: "de" });
    expect(de("maint.halt")).toBe("Master anhalten");
    expect(de("maint.release")).toBe("Master freigeben");
    expect(de("maint.live.quiet")).toBe("Bus ruhig");
    expect(de("maint.live.polling", { count: 3 })).toBe("Master fragt 3 Slaves ab");
    expect(de("maint.packs.inverter")).toBe("Während der Wartung meldet der Master dem Wechselrichter nur sein eigenes Pack.");
    expect(de("maint.banner.marker", { name: "A", since: "14:02" })).toContain("Wartungsmodus aktiv: Master fragt keine Slaves ab");
    expect(de("maint.err.other-bank", { serial: "X" })).toBe("An diesem Anschluss hängt eine andere Anlage (BMS-SN X).");
    expect(de("maint.bank.adopt")).toBe("Diese Anlage hat einen neuen Master: Seriennummer übernehmen");
    expect(de("maint.bank.unchecked")).toContain("Anlage nicht geprüft");
    expect(de("maint.banner.pendingConfirm", { pack: 3 })).toBe("Pack 3 ist abgeklemmt oder repariert");
  });
  it("status lines in both languages", () => {
    const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
    const m: MasterView = { kind: "maint", version: "16.06.13 MAINT 1.1", build_id: "fe381985bfff8ad0", marker: "off", marker_word: "FFFFFFFF", slaves: 3, may_halt: true, may_release: false };
    expect(masterLine(m, tr("de")).text).toBe("16.06.13 MAINT 1.1 · Marker aus · fragt 3 Slaves ab");
    expect(masterLine({ ...m, kind: "legacy", version: null, marker: "on", slaves: 0 }, tr("de")).text).toBe("16.06 MAINT-1.0-kompatibel · Marker an (Wartung) · fragt 0 Slaves ab");
    expect(masterLine({ ...m, marker: "foreign", marker_word: "12345678" }, tr("en"))).toEqual({ tone: "alarm", text: "16.06.13 MAINT 1.1 · marker sector with unknown content (12345678) · polls 3 slaves" });
    expect(masterLine({ ...m, kind: "unknown-build" }, tr("de")).text).toContain("Unbekannter Wartungs-Build 16.06.13 MAINT 1.1 (Build-ID fe381985bfff8ad0)");
    expect(masterLine({ ...m, kind: "no-0x64", marker: null, slaves: null }, tr("de")).text).toBe("Original 16.06 (0x64 nicht unterstützt)");
    expect(busLine({ bus: "polling", polled: [1, 2], bus_error: null, master: null, master_error: null }, false, tr("en")).text).toBe("Master polls 2 slaves");
    expect(busLine(null, false, tr("de")).text).toBe("Prüfe den Pack-Bus …");
  });
  it("German quotes are „…“", () => {
    const values = (o: unknown): string[] => Object.values(o as Record<string, unknown>).flatMap((v) => (typeof v === "object" && v ? values(v) : [String(v)]));
    const text = values(i18n.getResourceBundle("de", "translation").maint).join(" ");
    expect(text).not.toMatch(/["”]/);
    expect((text.match(/„/g) ?? []).length).toBe((text.match(/“/g) ?? []).length);
  });
});
