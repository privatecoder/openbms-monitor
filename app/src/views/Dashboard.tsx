import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownToLine, ArrowUpToLine, Layers, MoveVertical, Thermometer } from "lucide-react";
import { effectiveCurrent, packState, type PackState, type SystemUpdate } from "../api";
import type { PackEntry } from "../store";
import { CellStrip, SevIcon, Stat, StateIcon, Tank, kw, median, sevTone, worstSeverity } from "../components/widgets";
import { InfoIcon } from "../help";
import { Button } from "../components/ui/button";
import { arrange, useGroups, type Group } from "../groups";
import { GroupEditor } from "./GroupEditor";
import { cn, fmt } from "../lib/utils";

const ROW = "grid grid-cols-[6.5rem_minmax(10rem,1fr)_8rem_5.5rem_5.5rem_5rem_6rem] items-center gap-x-5 whitespace-nowrap";

export function Dashboard({ packs, system, site, onOpen }: { packs: Record<number, PackEntry>; system: SystemUpdate | null; site: string; onOpen: (a: number) => void }) {
  const { t } = useTranslation();
  const { groups, save } = useGroups(site);
  const [editing, setEditing] = useState(false);
  const list = Object.values(packs).sort((a, b) => a.address - b.address);
  const live = list.filter((p) => p.telemetry);
  const allCells = live.flatMap((p) => p.telemetry!.cell_voltages);
  const center = median(allCells);
  const s = system?.values;

  // Bank totals: from the master's system values, or summed from the packs (RS485-1/2 has no system values).
  const voltage = s?.voltage ?? (live.length ? live.reduce((x, p) => x + p.telemetry!.pack_voltage, 0) / live.length : NaN);
  const current = s?.current ?? live.reduce((x, p) => x + effectiveCurrent(p.telemetry!), 0);
  const fullAh = live.reduce((x, p) => x + p.telemetry!.full_capacity_ah, 0);
  const remainAh = live.reduce((x, p) => x + p.telemetry!.remaining_capacity_ah, 0);
  const soc = s?.soc ?? (fullAh ? (remainAh / fullAh) * 100 : NaN);
  const state: PackState = s ? packState(s.state) : current >= 1 ? "charging" : current <= -0.8 ? "discharging" : live.length ? "standby" : "unknown";
  const temps = live.flatMap((p) => p.telemetry!.cell_temperatures);
  const arranged = arrange(groups, list.map((p) => p.address), t("groups.unassigned"));

  return (
    <div className="mx-auto max-w-6xl space-y-10">
      <header className="space-y-5">
        <div>
          <h1 className="flex items-center gap-3 font-display text-4xl font-semibold tracking-tight">
            <StateIcon state={state} soc={soc} className="h-9 w-9 shrink-0" strokeWidth={1.75} />
            {t(`dash.head.${state}`, { p: kw(Math.abs(voltage * current)) })}
          </h1>
          <p className="mt-1 pl-12 text-muted">{t("dash.sub", { v: fmt(voltage, 1, "V"), a: fmt(current, 1, "A"), n: list.length })}</p>
        </div>
        <Tank soc={soc} state={state} caption={t("dash.tank", { remain: fmt(s ? (s.soc / 100) * s.total_capacity_ah : remainAh, 0), total: fmt(s?.total_capacity_ah ?? fullAh, 0) })} />
        <div className="grid grid-cols-2 gap-6 pt-1 md:grid-cols-4">
          {s ? (
            <>
              <Stat icon={ArrowUpToLine} label={t("dash.chargeLimit")} help="value.charge_limits" value={s.charge_allowed ? fmt(s.charge_voltage_limit, 1, "V") : t("dash.blocked")}
                tone={s.charge_allowed ? undefined : "text-alarm"} note={fmt(s.charge_current_limit, 0, "A")} />
              <Stat icon={ArrowDownToLine} label={t("dash.dischargeLimit")} help="value.charge_limits" value={s.discharge_allowed ? fmt(s.discharge_voltage_limit, 1, "V") : t("dash.blocked")}
                tone={s.discharge_allowed ? undefined : "text-alarm"} note={fmt(s.discharge_current_limit, 0, "A")} />
            </>
          ) : <><div /><div /></>}
          <Stat icon={MoveVertical} label={t("dash.cellRange")} help="value.cell_delta"
            value={`${fmt(((s ? s.highest_cell_voltage - s.lowest_cell_voltage : Math.max(...allCells) - Math.min(...allCells))) * 1000, 0, "mV")}`}
            note={t("dash.cellRangeVal", { lo: fmt(s?.lowest_cell_voltage ?? Math.min(...allCells), 3), hi: fmt(s?.highest_cell_voltage ?? Math.max(...allCells), 3, "V") })} />
          <Stat icon={Thermometer} label={t("dash.tempRange")} help="value.temperatures"
            value={t("dash.cellRangeVal", { lo: fmt(s?.lowest_cell_temperature ?? Math.min(...temps), 1), hi: fmt(s?.highest_cell_temperature ?? Math.max(...temps), 1, "°C") })} />
        </div>
      </header>

      <section>
        <div className="mb-3 flex flex-wrap items-center gap-3">
          <h2 className="font-display text-2xl font-semibold">{t("dash.rack")}</h2>
          {Number.isFinite(center) && <span className="text-sm text-muted">{t("dash.median", { v: fmt(center, 3, "V") })}</span>}
          <div className="ml-auto flex items-center gap-2">
            <Button size="sm" onClick={() => setEditing(true)}><Layers className="h-4 w-4" />{t("groups.edit")}</Button>
          </div>
        </div>
        <RackList groups={arranged} packs={packs} center={center} bankState={state} multi={list.length > 1} onOpen={onOpen} />
        <GroupEditor open={editing} onOpenChange={setEditing} groups={groups} packs={list.map((p) => p.address)} onSave={save} />
      </section>
    </div>
  );
}

interface Agg { soc: number; current: number; power: number; spread: number | null; alarms: number }

function aggregate(entries: PackEntry[]): Agg {
  const live = entries.filter((p) => p.telemetry).map((p) => p.telemetry!);
  const full = live.reduce((x, tm) => x + tm.full_capacity_ah, 0);
  const cells = live.flatMap((tm) => tm.cell_voltages);
  return {
    soc: full ? (live.reduce((x, tm) => x + tm.remaining_capacity_ah, 0) / full) * 100 : NaN,
    current: live.reduce((x, tm) => x + effectiveCurrent(tm), 0),
    power: live.reduce((x, tm) => x + effectiveCurrent(tm) * tm.pack_voltage, 0),
    spread: cells.length ? (Math.max(...cells) - Math.min(...cells)) * 1000 : null,
    alarms: entries.reduce((x, p) => x + (p.status?.alarms.length ?? 0), 0),
  };
}

/** multi: more than one pack found, so address 0 is the master (a lone pack at 0 is standalone). */
interface RackProps { groups: Group[]; packs: Record<number, PackEntry>; center: number; bankState: PackState; multi: boolean; onOpen: (a: number) => void }

function GroupSummary({ name, agg }: { name: string; agg: Agg }) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
      {name && <h3 className="font-display text-xl font-semibold">{name}</h3>}
      <span className="text-sm text-muted">
        {t("groups.summary", { soc: fmt(agg.soc, 1, "%"), a: fmt(agg.current, 1, "A"), p: kw(agg.power), d: fmt(agg.spread, 0, "mV") })}
      </span>
      {agg.alarms > 0 && <span className="text-sm font-medium text-alarm">{t("dash.alarmCount", { count: agg.alarms })}</span>}
    </div>
  );
}

/** Column header once on top, then one panel per group with its summary above it. */
function RackList({ groups, packs, center, bankState, multi, onOpen }: RackProps) {
  const { t } = useTranslation();
  return (
    <div className="overflow-x-auto">
      <div className="min-w-[56rem]">
        <div className={cn(ROW, "mb-5 border-b border-line px-4 pb-2 text-sm text-muted")}>
          <span>{t("dash.pack")}</span>
          <span className="flex items-center gap-1.5">{t("dash.strip")}<InfoIcon id="value.cell_strip" /></span>
          <span>{t("dash.soc")}</span>
          <span className="text-right">{t("dash.voltage")}</span>
          <span className="text-right">{t("dash.current")}</span>
          <span className="text-right">{t("dash.delta")}</span>
          <span className="text-right">{t("dash.temp")}</span>
        </div>
        <div className="space-y-6">
          {groups.map((g, gi) => {
            const offset = groups.slice(0, gi).reduce((n, x) => n + x.packs.length, 0);
            const entries = g.packs.map((a) => packs[a]).filter(Boolean);
            return (
              <div key={g.name + g.packs.join()}>
                {g.name && <div className="mb-2 px-1"><GroupSummary name={g.name} agg={aggregate(entries)} /></div>}
                <div className="overflow-hidden rounded-md border border-line bg-surface">
                  {entries.map((p, i) => <RackRow key={p.address} p={p} index={offset + i} center={center} bankState={bankState} multi={multi} onOpen={() => onOpen(p.address)} />)}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function RackRow({ p, index, center, bankState, multi, onOpen }: { p: PackEntry; index: number; center: number; bankState: PackState; multi: boolean; onOpen: () => void }) {
  const { t } = useTranslation();
  const tm = p.telemetry, st = p.status;
  const cells = tm?.cell_voltages ?? [];
  const delta = cells.length ? (Math.max(...cells) - Math.min(...cells)) * 1000 : null;
  const temps = tm?.cell_temperatures ?? [];
  const worst = st ? worstSeverity(st.alarms) : undefined;
  const state = packState(st?.system_status);
  const differs = state !== bankState && state !== "unknown";
  return (
    <button onClick={onOpen} style={{ animationDelay: `${index * 35}ms` }}
      className={cn(ROW, "row-in group w-full border-b border-line px-4 py-3 text-left transition-colors last:border-b-0 hover:bg-sunken/50 focus-visible:bg-sunken/50 focus-visible:outline-none")}>
      <span className="flex items-center gap-2">
        <span className="font-display text-2xl font-semibold leading-none">{String(p.address).padStart(2, "0")}</span>
        {multi && p.address === 0 && <span className="text-sm text-muted">{t("dash.master")}</span>}
        {worst
          ? <span title={t("dash.alarmCount", { count: st!.alarms.length })}><SevIcon severity={worst} className="h-4 w-4" /></span>
          : differs && <span title={t(`state.${state}`)}><StateIcon state={state} soc={tm?.soc} className="h-4 w-4" /></span>}
      </span>
      {tm ? <CellStrip cells={cells} center={center} balancing={st?.balancing} /> : <span className="text-sm text-muted">{p.error ? t("dash.noData") : "…"}</span>}
      <span className="flex items-center gap-2">
        <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-sunken"><span className="block h-full bg-ink/60 transition-[width] duration-700" style={{ width: `${tm?.soc ?? 0}%` }} /></span>
        <span className="w-12 text-right font-medium">{fmt(tm?.soc, 1, "%")}</span>
      </span>
      <span className="text-right">{fmt(tm?.pack_voltage, 2, "V")}</span>
      <span className="text-right">{tm ? fmt(effectiveCurrent(tm), 2, "A") : "–"}</span>
      <span className={cn("text-right", delta !== null && delta >= 30 && "font-medium text-high")}>{fmt(delta, 0, "mV")}</span>
      <span className="text-right">{temps.length ? fmt(Math.max(...temps), 1, "°C") : "–"}</span>
    </button>
  );
}
