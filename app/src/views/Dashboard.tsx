import { useTranslation } from "react-i18next";
import { effectiveCurrent, packState, type PackState, type SystemUpdate } from "../api";
import type { PackEntry } from "../store";
import { CellStrip, Stat, StateMark, Tank, kw, median, sevTone, worstSeverity } from "../components/widgets";
import { InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";

const ROW = "grid grid-cols-[5.5rem_minmax(8rem,1fr)_7.5rem_5.5rem_5.5rem_4.5rem_6.5rem_8rem] items-center gap-x-5";

export function Dashboard({ packs, system, onOpen }: { packs: Record<number, PackEntry>; system: SystemUpdate | null; onOpen: (a: number) => void }) {
  const { t } = useTranslation();
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
        <div className="mb-2 flex items-baseline justify-between">
          <h2 className="font-display text-2xl font-semibold">{t("dash.rack")}</h2>
          {Number.isFinite(center) && <span className="text-sm text-muted">{t("dash.median", { v: fmt(center, 3, "V") })}</span>}
        </div>
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
            {list.map((p) => <RackRow key={p.address} p={p} center={center} bankState={state} onOpen={() => onOpen(p.address)} />)}
          </div>
        </div>
      </section>
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
