import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, CircleAlert, OctagonAlert, FileText } from "lucide-react";
import { effectiveCurrent } from "../api";
import type { PackEntry } from "../store";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { InfoIcon } from "../help";
import { useCellDb } from "../cells/store";
import { checkPack, standardChargeA, useAssignments, type Check, type Level } from "../cells/packCheck";
import { CellAssignSheet, cellName, config } from "./CellAssign";
import { cn, fmt } from "../lib/utils";

const tone: Record<Level, string> = { ok: "text-ok", near: "text-discharge", over: "text-alarm" };
const icon = { ok: CircleCheck, near: CircleAlert, over: OctagonAlert };

/** The pack's live values against the datasheet of its assigned cell type. */
export function DatasheetPanel({ p, site, packs }: { p: PackEntry; site: string; packs: Record<number, PackEntry> }) {
  const { t } = useTranslation();
  const db = useCellDb();
  const { assigned } = useAssignments(site);
  const [editing, setEditing] = useState(false);
  const a = assigned[p.address];
  const cell = a ? db.entries.find((e) => e.cell.id === a.cell)?.cell : undefined;
  const tm = p.telemetry;
  const checks = cell && tm ? checkPack(cell, a!.parallel, tm, effectiveCurrent(tm)) : [];
  const std = cell ? standardChargeA(cell, a!.parallel) : undefined;
  const coldest = tm?.cell_temperatures.length ? Math.min(...tm.cell_temperatures) : undefined;

  const text = (c: Check) => {
    const outside = c.limit2 !== undefined && (c.value < c.limit || c.value > c.limit2);
    switch (c.id) {
      case "cellHigh": return [t("ds.cellHigh"), fmt(c.value, 3, "V"), t("ds.chargeCutoff", { v: fmt(c.limit, 2, "V") })];
      case "cellLow": return [t("ds.cellLow"), fmt(c.value, 3, "V"), t("ds.dischargeCutoff", { v: fmt(c.limit, 2, "V") })];
      case "tempCharge": return [t("ds.tempCharge"), fmt(c.value, 1, "°C"), outside && c.level !== "over" ? t("ds.noChargeNow", { lo: fmt(c.limit, 0), hi: fmt(c.limit2, 0, "°C") }) : t("ds.range", { lo: fmt(c.limit, 0), hi: fmt(c.limit2, 0, "°C") })];
      case "tempDischarge": return [t("ds.tempDischarge"), fmt(c.value, 1, "°C"), t("ds.range", { lo: fmt(c.limit, 0), hi: fmt(c.limit2, 0, "°C") })];
      case "chargeCurrent": return [t("ds.chargeCurrent"), fmt(c.value, 1, "A"),
        [t("ds.allowedAt", { a: fmt(c.limit, 0, "A"), t: fmt(coldest, 1, "°C") }), std !== undefined && t("ds.standard", { a: fmt(std, 0, "A") })].filter(Boolean).join(", ")];
      case "dischargeCurrent": return [t("ds.dischargeCurrent"), fmt(c.value, 1, "A"), t("ds.continuous", { a: fmt(c.limit, 0, "A") })];
    }
  };

  return (
    <Panel className="p-5">
      <div className="mb-3 flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h2 className="flex items-center gap-1.5 font-display text-xl font-semibold"><FileText className="h-5 w-5 text-muted" aria-hidden />{t("ds.title")}<InfoIcon id="value.datasheet" /></h2>
        {cell && <span className="text-muted">{cellName(cell)}, {config(tm?.cell_voltages.length, a!.parallel)}</span>}
        <Button size="sm" variant="ghost" className="ml-auto" onClick={() => setEditing(true)}>{a ? t("ds.change") : t("ds.assign")}</Button>
      </div>
      {!a ? <p className="text-sm text-muted">{t("ds.unassigned")}</p>
        : !cell ? <p className="text-sm text-alarm">{db.loading ? t("ca.loading") : t("ca.missing", { id: a.cell })}</p>
        : (
          <div className="grid grid-cols-[auto_auto_1fr_auto] items-baseline gap-x-6 gap-y-2 text-sm">
            {checks.map((c) => {
              const [label, value, limit] = text(c);
              const I = icon[c.level];
              return (
                <div key={c.id} className="contents">
                  <span className="text-muted">{label}</span>
                  <span className="text-right font-medium tabular-nums">{value}</span>
                  <span className="text-muted">{limit}</span>
                  <span className={cn("inline-flex items-center gap-1.5 justify-self-end", tone[c.level])}><I className="h-4 w-4 self-center" aria-hidden />{t(`ds.level.${c.level}`)}</span>
                </div>
              );
            })}
          </div>
        )}
      <CellAssignSheet open={editing} onOpenChange={setEditing} site={site} packs={packs} preselect={[p.address]} />
    </Panel>
  );
}
