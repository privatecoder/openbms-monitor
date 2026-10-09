import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Columns3, Rows3, Layers } from "lucide-react";
import { effectiveCurrent, packState, type PackState, type SystemUpdate } from "../api";
import type { PackEntry } from "../store";
import { CellStrip, Stat, StateMark, Tank, kw, median, sevTone, worstSeverity } from "../components/widgets";
import { InfoIcon } from "../help";
import { Button } from "../components/ui/button";
import { arrange, useGroups, type Group } from "../groups";
import { GroupEditor } from "./GroupEditor";
import { cn, fmt } from "../lib/utils";

const ROW = "grid grid-cols-[5.5rem_minmax(8rem,1fr)_7.5rem_5.5rem_5.5rem_4.5rem_6.5rem_8rem] items-center gap-x-5";

export function Dashboard({ packs, system, site, onOpen }: { packs: Record<number, PackEntry>; system: SystemUpdate | null; site: string; onOpen: (a: number) => void }) {
  const { t } = useTranslation();
  const { groups, save } = useGroups(site);
  const [editing, setEditing] = useState(false);
  const [layout, setLayout] = useState<"towers" | "list">(() => { try { return (localStorage.getItem("layout") as "towers" | "list") ?? "towers"; } catch { return "towers"; } });
  const pickLayout = (l: "towers" | "list") => { setLayout(l); try { localStorage.setItem("layout", l); } catch { /* ignore */ } };
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
          <h1 className="font-display text-4xl font-semibold tracking-tight">{t(`dash.head.${state}`, { p: kw(Math.abs(voltage * current)) })}</h1>
          <p className="mt-1 text-muted">{t("dash.sub", { v: fmt(voltage, 1, "V"), a: fmt(current, 1, "A"), n: list.length })}</p>
        </div>
        <Tank soc={soc} state={state} caption={t("dash.tank", { remain: fmt(s ? (s.soc / 100) * s.total_capacity_ah : remainAh, 0), total: fmt(s?.total_capacity_ah ?? fullAh, 0) })} />
        <div className="grid grid-cols-2 gap-6 pt-1 md:grid-cols-4">
          {s ? (
            <>
              <Stat label={t("dash.chargeLimit")} help="value.charge_limits" value={s.charge_allowed ? fmt(s.charge_voltage_limit, 1, "V") : t("dash.blocked")}
                tone={s.charge_allowed ? undefined : "text-alarm"} note={fmt(s.charge_current_limit, 0, "A")} />
              <Stat label={t("dash.dischargeLimit")} help="value.charge_limits" value={s.discharge_allowed ? fmt(s.discharge_voltage_limit, 1, "V") : t("dash.blocked")}
                tone={s.discharge_allowed ? undefined : "text-alarm"} note={fmt(s.discharge_current_limit, 0, "A")} />
            </>
          ) : <><div /><div /></>}
          <Stat label={t("dash.cellRange")} help="value.cell_delta"
            value={`${fmt(((s ? s.highest_cell_voltage - s.lowest_cell_voltage : Math.max(...allCells) - Math.min(...allCells))) * 1000, 0, "mV")}`}
            note={t("dash.cellRangeVal", { lo: fmt(s?.lowest_cell_voltage ?? Math.min(...allCells), 3), hi: fmt(s?.highest_cell_voltage ?? Math.max(...allCells), 3, "V") })} />
          <Stat label={t("dash.tempRange")} help="value.temperatures"
            value={t("dash.cellRangeVal", { lo: fmt(s?.lowest_cell_temperature ?? Math.min(...temps), 1), hi: fmt(s?.highest_cell_temperature ?? Math.max(...temps), 1, "°C") })} />
        </div>
      </header>

      <section>
        <div className="mb-3 flex flex-wrap items-center gap-3">
          <h2 className="font-display text-2xl font-semibold">{t("dash.rack")}</h2>
          {Number.isFinite(center) && <span className="text-sm text-muted">{t("dash.median", { v: fmt(center, 3, "V") })}</span>}
          <div className="ml-auto flex items-center gap-2">
            <div className="flex rounded-md bg-sunken p-0.5" role="group">
              {([["towers", Columns3], ["list", Rows3]] as const).map(([k, I]) => (
                <button key={k} onClick={() => pickLayout(k)} aria-pressed={layout === k}
                  className={cn("flex items-center gap-1.5 rounded px-2.5 py-1 text-sm", layout === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>
                  <I className="h-4 w-4" />{t(`groups.${k}`)}
                </button>
              ))}
            </div>
            <Button size="sm" onClick={() => setEditing(true)}><Layers className="h-4 w-4" />{t("groups.edit")}</Button>
          </div>
        </div>
        {layout === "towers"
          ? <Towers groups={arranged} packs={packs} center={center} bankState={state} onOpen={onOpen} />
          : <RackList groups={arranged} packs={packs} center={center} bankState={state} onOpen={onOpen} />}
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

interface RackProps { groups: Group[]; packs: Record<number, PackEntry>; center: number; bankState: PackState; onOpen: (a: number) => void }

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

function Towers({ groups, packs, center, bankState, onOpen }: RackProps) {
  return (
    <div className="grid gap-5" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(19rem, 1fr))" }}>
      {groups.map((g) => {
        const entries = g.packs.map((a) => packs[a]).filter(Boolean);
        const agg = aggregate(entries);
        return (
          <div key={g.name + g.packs.join()} className="flex flex-col">
            <div className="mb-2"><GroupSummary name={g.name} agg={agg} /></div>
            <div className="overflow-hidden rounded-md border border-line bg-surface">
              <div className="h-1 bg-sunken"><div className="h-full bg-ink/50" style={{ width: `${agg.soc || 0}%` }} /></div>
              {entries.map((p) => <Slab key={p.address} p={p} center={center} bankState={bankState} onOpen={() => onOpen(p.address)} />)}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** A pack in a tower: number, cell strip, charge and current. */
function Slab({ p, center, bankState, onOpen }: { p: PackEntry; center: number; bankState: PackState; onOpen: () => void }) {
  const { t } = useTranslation();
  const tm = p.telemetry, st = p.status;
  const worst = st ? worstSeverity(st.alarms) : undefined;
  const state = packState(st?.system_status);
  return (
    <button onClick={onOpen}
      className="grid w-full grid-cols-[3.25rem_1fr_4rem_4.5rem] min-h-14 items-center gap-x-3 border-t border-line px-3 py-2 text-left first-of-type:border-t-0 hover:bg-sunken/50 focus-visible:bg-sunken/50 focus-visible:outline-none">
      <span className="flex flex-col leading-none">
        <span className="font-display text-xl font-semibold">{String(p.address).padStart(2, "0")}</span>
        {(p.address === 0 || worst || state !== bankState) && (
          <span className={cn("pt-0.5 text-xs", worst ? sevTone[worst] : "text-muted")}>
            {worst ? t(`sev.${worst}`) : state !== bankState ? t(`state.${state}`) : t("dash.master")}
          </span>
        )}
      </span>
      {tm ? <CellStrip cells={tm.cell_voltages} center={center} balancing={st?.balancing} height={24} /> : <span className="text-sm text-muted">{p.error ? t("dash.noData") : "…"}</span>}
      <span className="text-right font-medium">{fmt(tm?.soc, 1, "%")}</span>
      <span className="text-right text-muted">{tm ? fmt(effectiveCurrent(tm), 2, "A") : "–"}</span>
    </button>
  );
}

function RackList({ groups, packs, center, bankState, onOpen }: RackProps) {
  const { t } = useTranslation();
  return (
    <div className="overflow-x-auto">
      <div className="min-w-[56rem] overflow-hidden rounded-md border border-line bg-surface">
        <div className={cn(ROW, "border-b border-line bg-sunken/60 px-4 py-2 text-sm text-muted")}>
          <span>{t("dash.pack")}</span>
          <span className="flex items-center gap-1.5">{t("dash.strip")}<InfoIcon id="value.cell_strip" /></span>
          <span>{t("dash.soc")}</span>
          <span className="text-right">{t("dash.voltage")}</span>
          <span className="text-right">{t("dash.current")}</span>
          <span className="text-right">{t("dash.delta")}</span>
          <span className="text-right">{t("dash.temp")}</span>
          <span />
        </div>
        {groups.map((g) => {
          const entries = g.packs.map((a) => packs[a]).filter(Boolean);
          return (
            <div key={g.name + g.packs.join()}>
              {g.name && <div className="border-b border-line bg-sunken/30 px-4 pb-2 pt-4"><GroupSummary name={g.name} agg={aggregate(entries)} /></div>}
              {entries.map((p) => <RackRow key={p.address} p={p} center={center} bankState={bankState} onOpen={() => onOpen(p.address)} />)}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function RackRow({ p, center, bankState, onOpen }: { p: PackEntry; center: number; bankState: PackState; onOpen: () => void }) {
  const { t } = useTranslation();
  const tm = p.telemetry, st = p.status;
  const cells = tm?.cell_voltages ?? [];
  const delta = cells.length ? (Math.max(...cells) - Math.min(...cells)) * 1000 : null;
  const temps = tm?.cell_temperatures ?? [];
  const worst = st ? worstSeverity(st.alarms) : undefined;
  const state = packState(st?.system_status);
  return (
    <button onClick={onOpen}
      className={cn(ROW, "group w-full border-b border-line px-4 py-3 text-left last:border-b-0 hover:bg-sunken/50 focus-visible:bg-sunken/50 focus-visible:outline-none")}>
      <span className="flex items-baseline gap-2">
        <span className="font-display text-2xl font-semibold">{String(p.address).padStart(2, "0")}</span>
        {p.address === 0 && <span className="text-sm text-muted">{t("dash.master")}</span>}
      </span>
      {tm ? <CellStrip cells={cells} center={center} balancing={st?.balancing} /> : <span className="text-sm text-muted">{p.error ? t("dash.noData") : "…"}</span>}
      <span className="flex items-center gap-2">
        <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-sunken"><span className="block h-full bg-ink/60" style={{ width: `${tm?.soc ?? 0}%` }} /></span>
        <span className="w-12 text-right font-medium">{fmt(tm?.soc, 1, "%")}</span>
      </span>
      <span className="text-right">{fmt(tm?.pack_voltage, 2, "V")}</span>
      <span className="text-right">{tm ? fmt(effectiveCurrent(tm), 2, "A") : "–"}</span>
      <span className={cn("text-right", delta !== null && delta >= 30 && "font-medium text-high")}>{fmt(delta, 0, "mV")}</span>
      <span className="text-right">{temps.length ? `${fmt(Math.max(...temps), 1, "°C")}` : "–"}</span>
      <span className="flex justify-end">
        {worst ? <span className={cn("text-sm font-medium", sevTone[worst])}>{t("dash.alarmCount", { count: st!.alarms.length })}</span> : state !== bankState ? <StateMark state={state} /> : null}
      </span>
    </button>
  );
}
