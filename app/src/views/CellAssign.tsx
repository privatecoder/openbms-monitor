import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sheet, SheetContent } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { PackPicker, Segmented, packSections } from "../components/PackPicker";
import { useGroups } from "../groups";
import { InfoIcon } from "../help";
import { useCellDb } from "../cells/store";
import { suggestParallel, useAssignments, type Assignment } from "../cells/packCheck";
import { effectiveChargeRange } from "../cells/rules";
import type { Cell } from "../cells/types";
import type { PackEntry } from "../store";
import { cn, fmt } from "../lib/utils";

const no = (a: number) => String(a).padStart(2, "0");
export const cellName = (c: Cell) => [c.manufacturer, c.model, c.variant].filter(Boolean).join(" ");
/** Configuration as on the pack label, e.g. 16S1P. */
export const config = (series: number | undefined, parallel: number) => `${series ?? "?"}S${parallel}P`;

/** Packs listed compactly: 00–05, 08, 10–11. */
export function packRanges(packs: number[]): string {
  const s = [...packs].sort((a, b) => a - b), out: string[] = [];
  for (let i = 0; i < s.length; i++) {
    let j = i;
    while (j + 1 < s.length && s[j + 1] === s[j] + 1) j++;
    out.push(j > i ? `${no(s[i])}–${no(s[j])}` : no(s[i]));
    i = j;
  }
  return out.join(", ");
}

/** Assign a cell model from the database to packs (all, by group or single), stored per installation. */
export function CellAssignSheet({ open, onOpenChange, site, packs, preselect }: {
  open: boolean; onOpenChange: (o: boolean) => void; site: string; packs: Record<number, PackEntry>; preselect?: number[];
}) {
  const { t } = useTranslation();
  const db = useCellDb();
  const { groups } = useGroups(site);
  const { assigned, assign } = useAssignments(site);
  const present = Object.keys(packs).map(Number).sort((a, b) => a - b);
  const [cellId, setCellId] = useState("");
  const [parallel, setParallel] = useState(1);
  const [chosen, setChosen] = useState<number[]>([]);
  const [done, setDone] = useState<string | null>(null);

  // on opening: the given packs (or all), and the cell type they already have
  useEffect(() => {
    if (!open) return;
    const sel = preselect?.length ? preselect : present;
    setChosen(sel);
    const first = sel.map((a) => assigned[a]).find(Boolean);
    setCellId(first?.cell ?? "");
    setParallel(first?.parallel ?? 1);
    setDone(null);
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const byId = useMemo(() => new Map(db.entries.map((e) => [e.cell.id, e.cell])), [db.entries]);
  const cell = byId.get(cellId);
  const makers = [...new Set(db.entries.map((e) => e.cell.manufacturer))];
  const rated = [...new Set(chosen.map((a) => packs[a]?.telemetry?.rated_capacity_ah).filter((x): x is number => x !== undefined))];
  const suggested = cell && rated.length === 1 ? suggestParallel(cell, rated[0]) : undefined;
  const series = packs[chosen[0]]?.telemetry?.cell_voltages.length;
  const sections = packSections(groups, present, t("rec.ungrouped"), t("rec.packs"));
  const toggle = (list: number[], on: boolean) =>
    setChosen((c) => { const s = new Set(c); list.forEach((a) => (on ? s.add(a) : s.delete(a))); return [...s].sort((x, y) => x - y); });

  // current assignments, one line per cell type and configuration
  const lines = new Map<string, { a: Assignment; packs: number[] }>();
  for (const p of present) {
    const a = assigned[p];
    if (!a) continue;
    const k = `${a.cell}|${a.parallel}`;
    lines.set(k, { a, packs: [...(lines.get(k)?.packs ?? []), p] });
  }
  const unassigned = present.filter((p) => !assigned[p]);

  const apply = () => {
    if (!cell) return;
    assign(chosen, { cell: cell.id, parallel });
    setDone(t("ca.assigned", { cell: cellName(cell), count: chosen.length }));
  };
  const clear = () => { assign(chosen, null); setDone(t("ca.removed", { count: chosen.length })); };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent title={t("ca.title")}>
        <div className="space-y-7">
          <p className="flex items-center gap-1.5 text-sm text-muted">{t("ca.intro")}<InfoIcon id="topic.cellAssign" /></p>

          <section className="space-y-2 rounded-lg border border-line bg-sunken/40 p-4 text-sm">
            <h3 className="font-medium">{t("ca.current")}</h3>
            {[...lines.values()].map(({ a, packs: ps }) => {
              const c = byId.get(a.cell);
              return (
                <div key={`${a.cell}|${a.parallel}`} className="flex flex-wrap items-baseline gap-x-2">
                  <span className={cn("font-medium", !c && "text-alarm")}>{c ? cellName(c) : t("ca.missing", { id: a.cell })}</span>
                  <span className="text-muted">{config(packs[ps[0]]?.telemetry?.cell_voltages.length, a.parallel)}</span>
                  <span className="ml-auto tabular-nums text-muted">{t("ca.packsList", { list: packRanges(ps) })}</span>
                </div>
              );
            })}
            {unassigned.length > 0 && <p className="text-muted">{t("ca.none", { list: packRanges(unassigned) })}</p>}
          </section>

          <section className="space-y-3">
            <h3 className="font-medium">{t("ca.cell")}</h3>
            {db.loading ? <p className="text-sm text-muted">{t("ca.loading")}</p> : (
              <select value={cellId} onChange={(e) => { setCellId(e.target.value); setDone(null); }}
                className="h-9 w-full rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge">
                <option value="">{t("ca.choose")}</option>
                {makers.map((m) => (
                  <optgroup key={m} label={m}>
                    {db.entries.filter((e) => e.cell.manufacturer === m).map((e) => (
                      <option key={e.cell.id} value={e.cell.id}>{[e.cell.model, e.cell.variant].filter(Boolean).join(" ")}{e.origin === "own" ? ` (${t("ca.own")})` : ""}</option>
                    ))}
                  </optgroup>
                ))}
              </select>
            )}
            {cell && <CellFacts cell={cell} />}
          </section>

          <section className="space-y-3">
            <div className="flex items-center justify-between">
              <h3 className="font-medium">{t("ca.parallel")}</h3>
              <Segmented value={String(parallel)} onChange={(v) => setParallel(Number(v))} items={[1, 2, 3, 4].map((n) => [String(n), `${n}P`] as [string, string])} />
            </div>
            {cell && rated.length === 1 && (
              <p className={cn("text-sm", suggested !== undefined && suggested !== parallel ? "text-discharge" : "text-muted")}>
                {suggested === undefined ? t("ca.ratedOdd", { ah: fmt(rated[0], 0, "Ah"), cell: fmt(cell.capacity?.nominal_ah, 0, "Ah") })
                  : suggested === parallel ? t("ca.ratedFits", { ah: fmt(rated[0], 0, "Ah"), c: config(series, parallel) })
                  : t("ca.ratedSuggest", { ah: fmt(rated[0], 0, "Ah"), p: suggested })}
              </p>
            )}
          </section>

          <section className="space-y-3">
            <h3 className="font-medium">{t("ca.packs")}</h3>
            <PackPicker sections={sections} chosen={chosen} onToggle={toggle} count={(a, b) => t("rec.ofCount", { a, b })}
              note={(a) => { const c = byId.get(assigned[a]?.cell ?? ""); return c ? c.model : "–"; }} />
          </section>

          <div className="space-y-2">
            <div className="flex flex-wrap gap-2">
              <Button variant="primary" disabled={!cell || !chosen.length} onClick={apply}>{t("ca.assign", { count: chosen.length })}</Button>
              <Button variant="ghost" disabled={!chosen.some((a) => assigned[a])} onClick={clear}>{t("ca.remove")}</Button>
            </div>
            {done && <p className="text-sm text-ok">{done}</p>}
          </div>
        </div>
      </SheetContent>
    </Sheet>
  );
}

/** The datasheet values the checks use, in one short block. */
function CellFacts({ cell }: { cell: Cell }) {
  const { t } = useTranslation();
  const v = cell.voltage, c = cell.current, tc = cell.temperature;
  const range = effectiveChargeRange(cell) ?? (tc?.charge_min_c !== undefined && tc.charge_max_c !== undefined ? { min: tc.charge_min_c, max: tc.charge_max_c } : undefined);
  const rows: [string, string][] = [
    [t("ca.f.capacity"), fmt(cell.capacity?.nominal_ah, 0, "Ah")],
    [t("ca.f.voltage"), v?.charge_cutoff_v !== undefined && v.discharge_cutoff_v !== undefined ? `${fmt(v.discharge_cutoff_v, 2)} – ${fmt(v.charge_cutoff_v, 2, "V")}` : "–"],
    [t("ca.f.current"), c?.max_continuous_charge_a !== undefined || c?.max_continuous_discharge_a !== undefined
      ? t("ca.f.currentVal", { c: fmt(c?.max_continuous_charge_a, 0, "A"), d: fmt(c?.max_continuous_discharge_a, 0, "A") }) : "–"],
    [t("ca.f.chargeTemp"), range ? `${fmt(range.min, 0)} – ${fmt(range.max, 0, "°C")}${cell.charge_derating ? ` (${t("ca.f.derated")})` : ""}` : "–"],
    [t("ca.f.dischargeTemp"), tc?.discharge_min_c !== undefined && tc.discharge_max_c !== undefined ? `${fmt(tc.discharge_min_c, 0)} – ${fmt(tc.discharge_max_c, 0, "°C")}` : "–"],
  ];
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 rounded-md border border-line p-3 text-sm">
      {rows.map(([k, val]) => <div key={k} className="contents"><dt className="text-muted">{k}</dt><dd className="tabular-nums">{val}</dd></div>)}
      <dt className="text-muted">{t("ca.f.source")}</dt>
      <dd className="truncate" title={cell.datasheet.title}>{cell.datasheet.title}</dd>
    </dl>
  );
}
