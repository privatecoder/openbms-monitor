import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { bundleName, importChoices, importResult } from "./views/InstallationTransfer";
import type { ImportPreview } from "./api";

const t = i18n.getFixedT("de");
const base: ImportPreview = {
  export_id: "exp-1", installation: "inst-1", name: "Bank", created: 0, master_serial: "SYN-M",
  places: 2, boards: 2, snapshots: 3, logs: 1, recording_bytes: null,
  same: null, master_differs: false, master_matches: [], completed: null, open_into: null,
};

describe("installation transfer", () => {
  it("names the file after the installation and the day, with safe characters only", () => {
    expect(bundleName("Bank Süd/2", new Date(2026, 9, 5))).toBe("openbms-Bank-Sud-2-2026-10-05.zip");
    expect(bundleName("///", new Date(2026, 0, 1))).toBe("openbms-anlage-2026-01-01.zip");
  });

  it("offers only what the backend accepts, the safe choice first", () => {
    expect(importChoices(base).map((c) => c.into)).toEqual([{ kind: "new", copy: false }]);
    expect(importChoices({ ...base, master_matches: [{ id: "x", name: "Hier" }] }).map((c) => c.into))
      .toEqual([{ kind: "new", copy: false }, { kind: "merge", id: "x" }]);
    expect(importChoices({ ...base, same: { id: "inst-1", name: "Bank" } }).map((c) => c.into)).toEqual([{ kind: "same" }]);
    // same id, another master serial: a copy by default, merging only on purpose
    expect(importChoices({ ...base, same: { id: "inst-1", name: "Bank" }, master_differs: true }).map((c) => c.into))
      .toEqual([{ kind: "new", copy: true }, { kind: "merge-anyway" }]);
  });

  it("every choice and result has a text", () => {
    for (const p of [base, { ...base, master_matches: [{ id: "x", name: "Hier" }] }, { ...base, same: { id: "i", name: "B" }, master_differs: true }]) {
      for (const c of importChoices(p)) expect(i18n.exists(c.label, { lng: "en" }) && i18n.exists(c.label, { lng: "de" })).toBe(true);
    }
    const r = importResult({ target: "inst-1", recording_rows: 10, recording_events: 2, logs_copied: 1, already: false,
      report: { boards_added: 2, snapshots_added: 1, not_taken: { groups: "[1]" } } }, "Bank", t);
    expect(r).toBe("Importiert in „Bank“: 2 Platinen, 1 Parameterstand, 1 Protokoll, 10 Werte und 2 Meldungen der Aufzeichnung. Abweichende Einstellungen der Datei wurden nicht übernommen (die vorhandenen gelten).");
  });
});
