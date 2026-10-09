import { useTranslation } from "react-i18next";
import { BatteryCharging, BatteryFull, BatteryLow, BatteryMedium, Info, OctagonX, PowerOff, ShieldAlert, ShieldCheck, TriangleAlert, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Tick } from "./live";
import { cn, fmt } from "../lib/utils";
import { InfoIcon, useHelpEntry } from "../help";
import type { ActiveAlarm, PackState, Severity } from "../api";

/** Window around the bank median that the cell strip spans (± volts). */
const SPAN = 0.02;

/** Deviation below this (volts) counts as "on the median". */
export const QUIET = 0.004;

export const median = (xs: number[]) => {
  if (!xs.length) return NaN;
  const s = [...xs].sort((a, b) => a - b), m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};

/**
 * The cell strip: one tick per cell, drawn up or down from a center line by its deviation from the
 * bank median. A flat line means a balanced pack; a long tick is the cell to look at.
 */
export function CellStrip({ cells, center, balancing = 0, height = 28, className }: {
  cells: number[]; center: number; balancing?: number; height?: number; className?: string;
}) {
  return (
    <div className={cn("relative flex w-full", className)} style={{ height }} role="img"
      aria-label={cells.map((v, i) => `${i + 1}: ${v.toFixed(3)} V`).join(", ")}>
      <div className="absolute inset-x-0 top-1/2 h-px bg-line" />
      {cells.map((v, i) => {
        const d = Math.max(-1, Math.min(1, (v - center) / SPAN));
        const pct = Math.max(3, Math.abs(d) * 48);
        const up = d >= 0;
        const tone = Math.abs(v - center) < QUIET ? "bg-muted/45" : up ? "bg-high" : "bg-low";
        return (
          <div key={i} className="relative flex-1">
            <div className={cn("absolute inset-x-[20%] transition-[height] duration-700 ease-out", tone, up ? "bottom-1/2" : "top-1/2")} style={{ height: `${pct}%` }} />
            {(balancing >> i) & 1 ? <div className="balancing absolute inset-x-[20%] bottom-0 h-0.5 bg-charge" /> : null}
          </div>
        );
      })}
    </div>
  );
}

const stateTone: Record<PackState, string> = {
  charging: "text-charge", discharging: "text-discharge", standby: "text-muted", off: "text-muted", unknown: "text-muted",
};

const stateIcon: Record<PackState, LucideIcon | null> = {
  charging: BatteryCharging, discharging: BatteryMedium, standby: BatteryFull, off: PowerOff, unknown: null,
};

/** Battery icon that follows the state and, when discharging or idle, the charge level. */
export function StateIcon({ state, soc, className, strokeWidth }: { state: PackState; soc?: number; className?: string; strokeWidth?: number }) {
  let I = stateIcon[state];
  if (I && (state === "discharging" || state === "standby") && soc !== undefined) I = soc < 20 ? BatteryLow : soc < 70 ? BatteryMedium : BatteryFull;
  return I ? <I className={cn(stateTone[state], className)} strokeWidth={strokeWidth} aria-hidden /> : null;
}

export function StateMark({ state }: { state: PackState }) {
  const { t } = useTranslation();
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-sm font-medium", stateTone[state])}>
      <StateIcon state={state} className="h-4 w-4" />{t(`state.${state}`)}
    </span>
  );
}

/** State of charge as a full-width tank: the one large shape on the overview. */
export function Tank({ soc, state, caption }: { soc: number; state: PackState; caption: string }) {
  const v = Math.max(0, Math.min(100, soc));
  const fill = v < 15 ? "bg-alarm" : state === "charging" ? "bg-charge" : state === "discharging" ? "bg-discharge" : "bg-muted";
  return (
    <div>
      <div className="relative h-14 overflow-hidden rounded-md bg-sunken" role="meter" aria-valuenow={v} aria-valuemin={0} aria-valuemax={100}>
        <div className={cn("absolute inset-y-0 left-0 transition-[width] duration-700", fill, state === "charging" && "flow-charge", state === "discharging" && "flow-discharge")} style={{ width: `${v}%` }} />
        {[25, 50, 75].map((x) => <div key={x} className="absolute inset-y-0 w-px bg-surface/40" style={{ left: `${x}%` }} />)}
        <div className="absolute inset-y-0 flex items-center pl-4 font-display text-3xl font-semibold text-surface" style={{ left: 0 }}>
          {fmt(v, 1)} %
        </div>
      </div>
      <div className="mt-1.5 flex justify-between text-sm text-muted"><span>0 %</span><span>{caption}</span><span>100 %</span></div>
    </div>
  );
}

export function Stat({ label, help, value, note, tone, icon: I }: { label: string; help: string; value: string; note?: ReactNode; tone?: string; icon?: LucideIcon }) {
  return (
    <div>
      <div className="flex items-center gap-1.5 text-sm text-muted">{I && <I className="h-4 w-4" aria-hidden />}{label}<InfoIcon id={help} /></div>
      <div className={cn("mt-0.5 font-display text-xl font-medium", tone)}><Tick value={value}>{value}</Tick></div>
      {note && <div className="text-sm text-muted">{note}</div>}
    </div>
  );
}

const sevOrder: Severity[] = ["fault", "protection", "warning", "info"];
export const worstSeverity = (alarms: ActiveAlarm[]) => sevOrder.find((s) => alarms.some((a) => a.severity === s));
export const sevTone: Record<Severity, string> = { fault: "text-alarm", protection: "text-alarm", warning: "text-discharge", info: "text-low" };
export const sevIcon: Record<Severity, LucideIcon> = { fault: OctagonX, protection: ShieldAlert, warning: TriangleAlert, info: Info };

export function AlarmList({ alarms }: { alarms: ActiveAlarm[] }) {
  const { t } = useTranslation();
  if (!alarms.length) return <p className="text-sm text-muted">{t("dash.noAlarms")}</p>;
  const sorted = [...alarms].sort((a, b) => sevOrder.indexOf(a.severity) - sevOrder.indexOf(b.severity));
  return <ul className="divide-y divide-line">{sorted.map((a) => <AlarmItem key={a.id} alarm={a} />)}</ul>;
}

function AlarmItem({ alarm }: { alarm: ActiveAlarm }) {
  const { t } = useTranslation();
  const e = useHelpEntry(alarm.id);
  return (
    <li className="flex items-start gap-3 py-2.5">
      <SevIcon severity={alarm.severity} className="mt-0.5 h-4 w-4 shrink-0" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 font-medium">{e?.title ?? alarm.key}<InfoIcon id={alarm.id} /></div>
        <div className={cn("text-sm", sevTone[alarm.severity])}>{t(`sev.${alarm.severity}`)}</div>
      </div>
    </li>
  );
}

export function SevIcon({ severity, className }: { severity: Severity; className?: string }) {
  const I = sevIcon[severity];
  return <I className={cn(sevTone[severity], className)} aria-hidden />;
}

/** Colour for a state of charge: red when nearly empty, amber when low, green otherwise. */
export const socTone = (soc: number | null | undefined) =>
  soc === null || soc === undefined || Number.isNaN(soc) ? "bg-line" : soc < 15 ? "bg-alarm" : soc < 30 ? "bg-discharge" : "bg-ok";

/** Messages of one pack (or a group) as icon + short text; green shield when there are none. */
export function MessagesMark({ alarms, compact = false }: { alarms: ActiveAlarm[] | undefined; compact?: boolean }) {
  const { t } = useTranslation();
  if (!alarms) return <span className="text-sm text-muted">–</span>;
  const worst = worstSeverity(alarms);
  if (!worst)
    return (
      <span className="inline-flex items-center gap-1.5 text-sm text-ok" title={t("dash.noAlarms")}>
        <ShieldCheck className="h-4 w-4" aria-hidden />{!compact && t("dash.noAlarms")}
      </span>
    );
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-sm font-medium", sevTone[worst])} title={alarms.map((a) => a.key).join(", ")}>
      <SevIcon severity={worst} className="h-4 w-4" />{!compact && t("dash.alarmCount", { count: alarms.length })}
    </span>
  );
}

/** Short text for a pack's messages in its worst severity colour, e.g. "2 Meldungen". */
export function MessagesText({ alarms }: { alarms: ActiveAlarm[] }) {
  const { t } = useTranslation();
  const worst = worstSeverity(alarms);
  return <span className={cn("font-medium", worst && sevTone[worst])}>{t("dash.alarmCount", { count: alarms.length })}</span>;
}

export const kw = (watts: number) => (Math.abs(watts) >= 1000 ? fmt(watts / 1000, 2, "kW") : fmt(watts, 0, "W"));
