import { useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownToLine, ArrowUpToLine, Cpu, Layers, MoveVertical, Scale, Thermometer } from "lucide-react";
import { effectiveCurrent, packState, type PackState, type SystemUpdate } from "../api";
import type { PackEntry } from "../store";
import { BalancingMark, CellStrip, MessagesMark, MessagesText, Stat, StateIcon, Tank, kw, median, socTone, stateTone } from "../components/widgets";
import { LiveIndicator } from "../components/live";
import { InfoIcon } from "../help";
import { Button } from "../components/ui/button";
import { arrange, useGroups, type Group } from "../groups";
import { PAIR_MIN_A, checkPairs, usePairs, validPairs, windowMean, type Pair, type PairCheck } from "../pairs";
import { GroupEditor } from "./GroupEditor";
import { cn, fmt } from "../lib/utils";

// Pack (with messages), SOC, voltage, current, temperature, cycles, spread, deviation strip.
const ROW = "grid grid-cols-[8rem_7.5rem_5.5rem_5.5rem_5.5rem_4rem_4.5rem_minmax(9rem,1fr)] items-center gap-x-6 whitespace-nowrap";

export function Dashboard({ packs, system, site, onOpen }: { packs: Record<number, PackEntry>; system: SystemUpdate | null; site: string; onOpen: (a: number) => void }) {
  const { t } = useTranslation();
  const { groups, save } = useGroups(site);
  const { pairs, save: savePairs } = usePairs(site);
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
  // Where the bank's extremes sit: from the master's system values, else from the packs themselves.
  const extreme = (pick: (p: PackEntry) => number[], hi: boolean) => {
    let best: { v: number; a: number } | undefined;
    for (const p of live) for (const v of pick(p)) if (!best || (hi ? v > best.v : v < best.v)) best = { v, a: p.address };
    return best;
  };
  const cellLo = s?.lowest_cell_voltage_pack != null ? { v: s.lowest_cell_voltage, a: s.lowest_cell_voltage_pack } : extreme((p) => p.telemetry!.cell_voltages, false);
  const cellHi = s?.highest_cell_voltage_pack != null ? { v: s.highest_cell_voltage, a: s.highest_cell_voltage_pack } : extreme((p) => p.telemetry!.cell_voltages, true);
  const tempLo = s?.lowest_cell_temperature_pack != null ? { v: s.lowest_cell_temperature, a: s.lowest_cell_temperature_pack } : extreme((p) => p.telemetry!.cell_temperatures, false);
  const tempHi = s?.highest_cell_temperature_pack != null ? { v: s.highest_cell_temperature, a: s.highest_cell_temperature_pack } : extreme((p) => p.telemetry!.cell_temperatures, true);
  const ambHi = extreme((p) => [p.telemetry!.ambient_temperature], true);
  const mosHi = extreme((p) => [p.telemetry!.power_temperature], true);
  const packNo = (a: number) => String(a).padStart(2, "0");
  // cells in series per pack, to show the voltage limits per cell
  const series = live[0]?.telemetry?.cell_voltages.length ?? 0;
  const balancingPacks = list.filter((p) => (p.status?.balancing ?? 0) !== 0).length;

  return (
    <div className="mx-auto max-w-6xl space-y-10">
      <header className="space-y-5">
        <div>
          <h1 className="flex items-center gap-3 font-display text-4xl font-semibold tracking-tight">
            <StateIcon state={state} soc={soc} className="h-9 w-9 shrink-0" strokeWidth={1.75} />
            {t(`dash.head.${state}`, { p: kw(Math.abs(voltage * current)) })}
          </h1>
          <div className="mt-1 flex flex-wrap items-center gap-x-5 gap-y-1 pl-12">
            <span className="text-muted">{t("dash.sub", { v: fmt(voltage, 1, "V"), a: fmt(current, 1, "A"), n: list.length })}</span>
            {live.length > 0 && <MessagesMark alarms={list.flatMap((p) => p.status?.alarms ?? [])} />}
            {balancingPacks > 0 && <span className="inline-flex items-center gap-1.5 text-sm text-charge"><Scale className="h-4 w-4" aria-hidden />{t("bal.packs", { count: balancingPacks })}</span>}
            <LiveIndicator updated={Math.max(0, ...list.map((p) => p.updated))} />
          </div>
        </div>
        <Tank soc={soc} state={state} caption={t("dash.tank", { remain: fmt(s ? (s.soc / 100) * s.total_capacity_ah : remainAh, 0), total: fmt(s?.total_capacity_ah ?? fullAh, 0) })} />
        <div className="grid grid-cols-2 gap-6 pt-1 md:grid-cols-3 xl:grid-cols-5">
          {s ? (
            <>
              <Stat icon={ArrowUpToLine} label={t("dash.chargeLimit")} help="value.charge_limits" value={s.charge_allowed ? fmt(s.charge_current_limit, 0, "A") : t("dash.blocked")}
                tone={s.charge_allowed ? undefined : "text-alarm"} note={<>
                  <span className="block">{t("dash.atMaxV", { v: fmt(s.charge_voltage_limit, 1, "V") })}</span>
                  {series > 0 && <span className="block">{t("dash.perCellMax", { v: fmt(s.charge_voltage_limit / series, 3, "V") })}</span>}
                </>} />
              <Stat icon={ArrowDownToLine} label={t("dash.dischargeLimit")} help="value.charge_limits" value={s.discharge_allowed ? fmt(s.discharge_current_limit, 0, "A") : t("dash.blocked")}
                tone={s.discharge_allowed ? undefined : "text-alarm"} note={<>
                  <span className="block">{t("dash.downToMinV", { v: fmt(s.discharge_voltage_limit, 1, "V") })}</span>
                  {series > 0 && <span className="block">{t("dash.perCellMin", { v: fmt(s.discharge_voltage_limit / series, 3, "V") })}</span>}
                </>} />
            </>
          ) : <><div /><div /></>}
          <Stat icon={MoveVertical} label={t("dash.cellRange")} help="value.bank_cell_delta"
            value={cellLo && cellHi ? fmt((cellHi.v - cellLo.v) * 1000, 0, "mV") : "–"}
            note={cellLo && cellHi ? <>
              <span className="block">{t("dash.minIn", { v: fmt(cellLo.v, 3, "V"), p: packNo(cellLo.a) })}</span>
              <span className="block">{t("dash.maxIn", { v: fmt(cellHi.v, 3, "V"), p: packNo(cellHi.a) })}</span>
            </> : undefined} />
          <Stat icon={Thermometer} label={t("dash.tempRange")} help="value.temperatures"
            value={tempLo && tempHi ? t("dash.tempRangeVal", { lo: fmt(tempLo.v, 1), hi: fmt(tempHi.v, 1, "°C") }) : "–"}
            note={tempLo && tempHi ? <>
              <span className="block">{t("dash.minIn", { v: fmt(tempLo.v, 1, "°C"), p: packNo(tempLo.a) })}</span>
              <span className="block">{t("dash.maxIn", { v: fmt(tempHi.v, 1, "°C"), p: packNo(tempHi.a) })}</span>
            </> : undefined} />
          <Stat icon={Cpu} label={t("dash.boardTemp")} help="value.temperatures"
            value={ambHi && mosHi ? t("dash.boardTempVal", { amb: fmt(ambHi.v, 1), mos: fmt(mosHi.v, 1, "°C") }) : "–"}
            note={ambHi && mosHi ? <>
              <span className="block">{t("dash.otherTemps", { pa: packNo(ambHi.a) })}</span>
              <span className="block">{t("dash.ambMosWhere", { pm: packNo(mosHi.a) })}</span>
            </> : undefined} />
        </div>
      </header>

      <section>
        <div className="mb-3 flex flex-wrap items-center gap-3">
          <h2 className="font-display text-2xl font-semibold">{t("dash.rack")}</h2>
          <div className="ml-auto flex items-center gap-2">
            <Button size="sm" onClick={() => setEditing(true)}><Layers className="h-4 w-4" />{t("groups.edit")}</Button>
          </div>
        </div>
        <RackList groups={arranged} packs={packs} center={center} multi={list.length > 1} onOpen={onOpen} pairs={validPairs(pairs, list.map((p) => p.address))} />
        <GroupEditor open={editing} onOpenChange={setEditing} groups={groups} packs={list.map((p) => p.address)} onSave={save} pairs={pairs} onSavePairs={savePairs} />
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
interface RackProps { groups: Group[]; packs: Record<number, PackEntry>; center: number; multi: boolean; onOpen: (a: number) => void; pairs: Pair[] }

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
function RackList({ groups, packs, center, multi, onOpen, pairs }: RackProps) {
  const { t } = useTranslation();
  const checks = new Map(usePairChecks(pairs, packs).map((c) => [c.pair.plus, c]));
  const partner = new Map(pairs.flatMap((p) => [[p.plus, p], [p.minus, p]] as [number, Pair][]));
  return (
    <div className="overflow-x-auto">
      <div className="min-w-[56rem]">
        <div className={cn(ROW, "mb-5 border-b border-line px-4 pb-2 text-sm text-muted")}>
          <span className="flex items-center gap-1.5">{t("dash.pack")}<InfoIcon id="topic.alarms" /></span>
          <span>{t("dash.soc")}</span>
          <span className="text-right">{t("dash.voltage")}</span>
          <span className="text-right">{t("dash.current")}</span>
          <span className="flex items-center justify-end gap-1.5">{t("dash.cellTempMax")}<InfoIcon id="value.cell_temp_max" /></span>
          <span className="flex items-center justify-end gap-1.5">{t("dash.cycles")}<InfoIcon id="value.cycles" /></span>
          <span className="flex items-center justify-end gap-1.5">{t("dash.delta")}<InfoIcon id="value.cell_delta" /></span>
          <span className="flex flex-col leading-tight">
            <span className="flex items-center gap-1.5">{t("dash.strip")}<InfoIcon id="value.cell_strip" /></span>
            {Number.isFinite(center) && <span className="text-xs">{t("dash.medianShort", { v: fmt(center, 3, "V") })}</span>}
          </span>
        </div>
        <div className="space-y-6">
          {groups.map((g, gi) => {
            const offset = groups.slice(0, gi).reduce((n, x) => n + x.packs.length, 0);
            const entries = g.packs.map((a) => packs[a]).filter(Boolean);
            return (
              <div key={g.name + g.packs.join()}>
                {g.name && <div className="mb-2 px-1"><GroupSummary name={g.name} agg={aggregate(entries)} /></div>}
                <div className="overflow-hidden rounded-md border border-line bg-surface">
                  {blocks(entries, partner).map((b, bi) => b.pair ? (
                    <PairBlock key={b.pair.plus} pair={b.pair} check={checks.get(b.pair.plus)}>
                      {b.rows.map((p, i) => <RackRow key={p.address} p={p} index={offset + bi + i} center={center} multi={multi} onOpen={() => onOpen(p.address)} />)}
                    </PairBlock>
                  ) : <RackRow key={b.rows[0].address} p={b.rows[0]} index={offset + bi} center={center} multi={multi} onOpen={() => onOpen(b.rows[0].address)} />)}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

/** Rows of a group, with the two packs of a pair (both in this group) kept together: plus pack first. */
function blocks(entries: PackEntry[], partner: Map<number, Pair>): { pair?: Pair; rows: PackEntry[] }[] {
  const byAddr = new Map(entries.map((p) => [p.address, p]));
  const done = new Set<number>();
  const out: { pair?: Pair; rows: PackEntry[] }[] = [];
  for (const p of entries) {
    if (done.has(p.address)) continue;
    const pair = partner.get(p.address);
    if (pair && byAddr.has(pair.plus) && byAddr.has(pair.minus)) {
      out.push({ pair, rows: [byAddr.get(pair.plus)!, byAddr.get(pair.minus)!] });
      done.add(pair.plus); done.add(pair.minus);
    } else {
      out.push({ rows: [p] });
      done.add(p.address);
    }
  }
  return out;
}

const PAIR_WINDOW_MS = 60_000;

/**
 * Pair check on each pack's mean current over the last minute, with hysteresis against the last result.
 * A polling round over many packs takes ~10 s and the inverter's current swings by a few amperes between
 * rounds, so single readings made the notes flicker. Waits until every pack has answered once.
 */
function usePairChecks(pairs: Pair[], packs: Record<number, PackEntry>): PairCheck[] {
  const samples = useRef(new Map<number, { t: number; i: number }[]>());
  const last = useRef<PairCheck[]>([]);
  const seen = useRef("");
  const now = Date.now();
  for (const p of Object.values(packs)) {
    if (!p.telemetry || !p.updated) continue;
    const s = samples.current.get(p.address) ?? [];
    if (s[s.length - 1]?.t !== p.updated) s.push({ t: p.updated, i: effectiveCurrent(p.telemetry) });
    samples.current.set(p.address, s.filter((x) => now - x.t <= PAIR_WINDOW_MS));
  }
  if (!Object.values(packs).every((p) => p.telemetry)) return [];
  // recompute only when new data arrived, so re-renders do not step the hysteresis
  const key = Object.values(packs).map((p) => p.updated).join();
  if (key !== seen.current) {
    seen.current = key;
    last.current = checkPairs(pairs, (a) => windowMean(samples.current.get(a) ?? [], now, PAIR_WINDOW_MS), last.current);
  }
  return last.current;
}

/** Two rows joined by a bar on the left, with the pair's total current below. */
function PairBlock({ pair, check, children }: { pair: Pair; check?: PairCheck; children: ReactNode }) {
  const { t } = useTranslation();
  const warn = !!check && (check.lowTotal || check.weak !== undefined);
  const no = (a: number) => String(a).padStart(2, "0");
  return (
    <div className="relative border-b border-line last:border-b-0 [&>button:last-of-type]:border-b-0">
      <span className={cn("absolute bottom-7 left-1.5 top-3 w-1 rounded-full", warn ? "bg-alarm/60" : "bg-charge/40")} aria-hidden />
      {children}
      <div className={cn("border-t border-line/60 bg-sunken/30 px-4 py-1 pl-9 text-xs", warn ? "text-alarm" : "text-muted")}>
        {t("pairs.footer", { a: no(pair.plus), b: no(pair.minus), sum: check ? fmt(check.total, 1, "A") : "–" })}
        {check?.lowTotal && <>; {t("diag.pairLow")}</>}
        {check?.weak !== undefined && <>; {t("diag.pairWeak", { p: no(check.weak), bridge: check.weak === pair.minus ? "+" : "−" })}</>}
        {check && !check.judged && <span className="text-muted/70">; {t("pairs.notJudged", { a: PAIR_MIN_A })}</span>}
      </div>
    </div>
  );
}

function RackRow({ p, index, center, multi, onOpen }: { p: PackEntry; index: number; center: number; multi: boolean; onOpen: () => void }) {
  const { t } = useTranslation();
  const tm = p.telemetry, st = p.status;
  const cells = tm?.cell_voltages ?? [];
  const delta = cells.length ? (Math.max(...cells) - Math.min(...cells)) * 1000 : null;
  const temps = tm?.cell_temperatures ?? [];
  const state = packState(st?.system_status);
  const alarms = st?.alarms;
  return (
    <button onClick={onOpen} style={{ animationDelay: `${index * 35}ms` }}
      className={cn(ROW, "row-in group w-full border-b border-line px-4 py-3.5 text-left transition-colors last:border-b-0 hover:bg-sunken/50 focus-visible:bg-sunken/50 focus-visible:outline-none")}>
      <span className="flex flex-col gap-1">
        <span className="flex items-center gap-2">
          <span className="font-display text-2xl font-semibold leading-none">{String(p.address).padStart(2, "0")}</span>
          <MessagesMark alarms={alarms} compact />
          <BalancingMark mask={st?.balancing} count={cells.length} />
        </span>
        {(alarms?.length || (multi && p.address === 0)) ? (
          <span className="text-xs text-muted">
            {alarms?.length ? <MessagesText alarms={alarms} /> : t("dash.master")}
          </span>
        ) : null}
      </span>
      <span className="flex flex-col gap-1.5">
        <span className="font-medium">{fmt(tm?.soc, 1, "%")}</span>
        <span className="h-1.5 overflow-hidden rounded-full bg-sunken"><span className={cn("block h-full transition-[width,background-color] duration-700", socTone(tm?.soc))} style={{ width: `${tm?.soc ?? 0}%` }} /></span>
      </span>
      <span className="text-right">{fmt(tm?.pack_voltage, 2, "V")}</span>
      <span className="flex flex-col items-end gap-0.5">
        <span className={cn("font-medium", stateTone[state])}>{tm ? fmt(effectiveCurrent(tm), 2, "A") : "–"}</span>
        {st && <span className="text-xs text-muted">{t(`stateShort.${state}`)}</span>}
      </span>
      <span className="text-right">{temps.length ? fmt(Math.max(...temps), 1, "°C") : "–"}</span>
      <span className="text-right">{tm ? tm.cycles : "–"}</span>
      <span className={cn("text-right", delta !== null && delta >= 30 && "font-medium text-high")}>{fmt(delta, 0, "mV")}</span>
      {tm ? <CellStrip cells={cells} center={center} balancing={st?.balancing} /> : <span className="text-sm text-muted">{p.error ? t("dash.noData") : "…"}</span>}
    </button>
  );
}
