import { useTranslation } from "react-i18next";
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
  const n = cells.length || 16, w = 100 / n, mid = height / 2;
  return (
    <svg viewBox={`0 0 100 ${height}`} preserveAspectRatio="none" className={cn("block w-full", className)} style={{ height }} role="img"
      aria-label={cells.map((v, i) => `${i + 1}: ${v.toFixed(3)} V`).join(", ")}>
      <line x1="0" x2="100" y1={mid} y2={mid} className="stroke-line" strokeWidth={1} vectorEffect="non-scaling-stroke" />
      {cells.map((v, i) => {
        const d = Math.max(-1, Math.min(1, (v - center) / SPAN));
        const len = Math.max(1.5, Math.abs(d) * (mid - 1));
        const up = d >= 0;
        const tone = Math.abs(v - center) < QUIET ? "fill-muted/45" : up ? "fill-high" : "fill-low";
        return (
          <g key={i}>
            <rect x={i * w + w * 0.2} width={w * 0.6} y={up ? mid - len : mid} height={len} className={tone} />
            {(balancing >> i) & 1 ? <rect x={i * w + w * 0.2} width={w * 0.6} y={height - 2} height={2} className="fill-charge" /> : null}
          </g>
        );
      })}
    </svg>
  );
}

const stateTone: Record<PackState, string> = {
  charging: "text-charge", discharging: "text-discharge", standby: "text-muted", off: "text-muted", unknown: "text-muted",
};
const stateDot: Record<PackState, string> = {
  charging: "bg-charge", discharging: "bg-discharge", standby: "bg-muted/50", off: "border border-muted bg-transparent", unknown: "bg-line",
};

export function StateMark({ state }: { state: PackState }) {
  const { t } = useTranslation();
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-sm font-medium", stateTone[state])}>
      <span className={cn("h-2 w-2 rounded-full", stateDot[state])} />{t(`state.${state}`)}
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
        <div className={cn("absolute inset-y-0 left-0 transition-[width] duration-700", fill)} style={{ width: `${v}%` }} />
        {[25, 50, 75].map((x) => <div key={x} className="absolute inset-y-0 w-px bg-surface/40" style={{ left: `${x}%` }} />)}
        <div className="absolute inset-y-0 flex items-center pl-4 font-display text-3xl font-semibold text-surface" style={{ left: 0 }}>
          {fmt(v, 1)} %
        </div>
      </div>
      <div className="mt-1.5 flex justify-between text-sm text-muted"><span>0 %</span><span>{caption}</span><span>100 %</span></div>
    </div>
  );
}

export function Stat({ label, help, value, note, tone }: { label: string; help: string; value: string; note?: string; tone?: string }) {
  return (
    <div>
      <div className="flex items-center gap-1.5 text-sm text-muted">{label}<InfoIcon id={help} /></div>
      <div className={cn("mt-0.5 font-display text-xl font-medium", tone)}>{value}</div>
      {note && <div className="text-sm text-muted">{note}</div>}
    </div>
  );
}

const sevOrder: Severity[] = ["fault", "protection", "warning", "info"];
export const worstSeverity = (alarms: ActiveAlarm[]) => sevOrder.find((s) => alarms.some((a) => a.severity === s));
export const sevTone: Record<Severity, string> = { fault: "text-alarm", protection: "text-alarm", warning: "text-discharge", info: "text-low" };
const sevBar: Record<Severity, string> = { fault: "bg-alarm", protection: "bg-alarm", warning: "bg-discharge", info: "bg-low" };

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
      <span className={cn("mt-1 h-4 w-1 shrink-0 rounded-full", sevBar[alarm.severity])} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 font-medium">{e?.title ?? alarm.key}<InfoIcon id={alarm.id} /></div>
        <div className={cn("text-sm", sevTone[alarm.severity])}>{t(`sev.${alarm.severity}`)}</div>
      </div>
    </li>
  );
}

export const kw = (watts: number) => (Math.abs(watts) >= 1000 ? fmt(watts / 1000, 2, "kW") : fmt(watts, 0, "W"));
