import { useTranslation } from "react-i18next";
import { ArrowLeft } from "lucide-react";
import { effectiveCurrent, packState } from "../api";
import type { PackEntry } from "../store";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { AlarmList, CellStrip, QUIET, Stat, StateMark, kw, median } from "../components/widgets";
import { InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";

export function PackDetail({ p, onBack }: { p: PackEntry; onBack: () => void }) {
  const { t } = useTranslation();
  const tm = p.telemetry, st = p.status;
  const cells = tm?.cell_voltages ?? [];
  const center = median(cells);
  const hi = Math.max(...cells), lo = Math.min(...cells);
  const sw = st?.switch_state ?? 0;
  const switches: [string, boolean][] = [["discharge", !!(sw & 1)], ["charge", !!(sw & 2)], ["limiter", !!(sw & 4)], ["heater", !!(sw & 8)]];
  const current = tm ? effectiveCurrent(tm) : NaN;

  return (
    <div className="mx-auto max-w-6xl space-y-8">
      <Button variant="ghost" size="sm" className="-ml-3" onClick={onBack}><ArrowLeft className="h-4 w-4" />{t("dash.back")}</Button>
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="font-display text-4xl font-semibold tracking-tight">
            {t("dash.pack")} {String(p.address).padStart(2, "0")}
            {p.address === 0 && <span className="ml-3 text-2xl font-medium text-muted">{t("dash.master")}</span>}
          </h1>
          <p className="mt-1 text-muted">{[p.device?.device_name, p.device?.firmware_version && `Firmware ${p.device.firmware_version}`, p.device?.can_protocol].filter(Boolean).join(", ")}</p>
        </div>
        <StateMark state={packState(st?.system_status)} />
      </header>

      {!tm ? <p className="text-muted">{t("dash.noData")}</p> : (
        <>
          <div className="grid grid-cols-2 gap-6 md:grid-cols-3 lg:grid-cols-6">
            <Stat label={t("dash.soc")} help="value.soc" value={fmt(tm.soc, 1, "%")} note={`${fmt(tm.remaining_capacity_ah, 1)} / ${fmt(tm.full_capacity_ah, 1, "Ah")}`} />
            <Stat label={t("dash.voltage")} help="value.pack_voltage" value={fmt(tm.pack_voltage, 2, "V")} note={t("dash.port", { v: fmt(tm.port_voltage, 2, "V") })} />
            <Stat label={t("dash.current")} help="value.current" value={fmt(current, 3, "A")} />
            <Stat label={t("dash.power")} help="value.power" value={kw(current * tm.pack_voltage)} />
            <Stat label={t("dash.cycles")} help="value.cycles" value={String(tm.cycles)} />
            <Stat label={t("dash.charged")} help="value.energy" value={fmt(tm.energy_charged_kwh, 1, "kWh")}
              note={t("dash.dischargedNote", { v: fmt(tm.energy_discharged_kwh, 1, "kWh") })} />
          </div>

          <Panel className="p-5">
            <div className="mb-4 flex flex-wrap items-baseline justify-between gap-2">
              <h2 className="flex items-center gap-1.5 font-display text-2xl font-semibold">{t("dash.cells")}<InfoIcon id="value.cell_voltages" /></h2>
              <span className="text-sm text-muted">{t("dash.delta")} {fmt((hi - lo) * 1000, 0, "mV")}, {t("dash.stripRefPack", { v: fmt(center, 3, "V") })}</span>
            </div>
            <CellStrip cells={cells} center={center} balancing={st?.balancing} height={64} />
            <div className="mt-3 grid" style={{ gridTemplateColumns: `repeat(${cells.length}, minmax(0, 1fr))` }}>
              {cells.map((v, i) => (
                <div key={i} className="text-center leading-tight">
                  <div className={cn("text-sm font-medium", v - center >= QUIET && "text-high", center - v >= QUIET && "text-low")}>{(v * 1000).toFixed(0)}</div>
                  <div className="text-xs text-muted">{i + 1}</div>
                </div>
              ))}
            </div>
            <p className="mt-3 text-sm text-muted">mV</p>
          </Panel>

          <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
            <Panel className="p-5">
              <h2 className="mb-3 flex items-center gap-1.5 font-display text-xl font-semibold">{t("dash.temp")}<InfoIcon id="value.temperatures" /></h2>
              <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5">
                {tm.cell_temperatures.map((v, i) => <Row key={i} k={`T${i + 1}`} v={fmt(v, 1, "°C")} />)}
                <Row k={t("dash.ambient")} v={fmt(tm.ambient_temperature, 1, "°C")} />
                <Row k="MOSFET" v={fmt(tm.power_temperature, 1, "°C")} />
              </dl>
            </Panel>
            <Panel className="p-5">
              <h2 className="mb-3 flex items-center gap-1.5 font-display text-xl font-semibold">{t("dash.switches")}<InfoIcon id="status.mosfets" /></h2>
              <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5">
                {switches.map(([k, on]) => (
                  <Row key={k} k={t(`sw.${k}`)} v={
                    <span className={cn("inline-flex items-center gap-1.5", on ? "text-charge" : "text-muted")}>
                      <span className={cn("h-2 w-2 rounded-full", on ? "bg-charge" : "border border-muted")} />{on ? t("sw.on") : t("sw.off")}
                    </span>} />
                ))}
              </dl>
            </Panel>
            <Panel className="p-5">
              <h2 className="mb-1 flex items-center gap-1.5 font-display text-xl font-semibold">{t("dash.alarms")}<InfoIcon id="topic.alarms" /></h2>
              <AlarmList alarms={st?.alarms ?? []} />
            </Panel>
          </div>
        </>
      )}
    </div>
  );
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return <><dt className="text-muted">{k}</dt><dd className="text-right font-medium">{v}</dd></>;
}
