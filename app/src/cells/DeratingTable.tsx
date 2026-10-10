import { useTranslation } from "react-i18next";
import type { Cell, DeratingPoint } from "./types";
import { effectiveChargeRange, toAmps } from "./rules";
import { InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";

const tempLabel = (p: DeratingPoint) =>
  p.temp_c !== undefined ? `${fmt(p.temp_c, 0)} °C`
    : p.temp_min_c === undefined ? `< ${fmt(p.temp_max_c, 0)} °C`
    : p.temp_max_c === undefined ? `> ${fmt(p.temp_min_c, 0)} °C`
    : `${fmt(p.temp_min_c, 0)}–${fmt(p.temp_max_c, 0)} °C`;
const tempKey = (p: DeratingPoint) => p.temp_c ?? p.temp_min_c ?? (p.temp_max_c ?? 0) - 1000;
const socLabel = (p: DeratingPoint) =>
  p.soc_min_pct === undefined && p.soc_max_pct === undefined ? "–"
    : p.soc_min_pct === p.soc_max_pct ? `${p.soc_min_pct} %`
    : `${p.soc_min_pct ?? 0}–${p.soc_max_pct ?? 100} %`;

/** Shade by share of the table maximum: 0 = no charging. */
function tone(v: number, max: number) {
  if (v <= 0) return "bg-alarm/15 text-alarm";
  const r = v / max;
  return r < 0.25 ? "bg-discharge/15" : r < 0.6 ? "bg-ok/15" : "bg-ok/30";
}

/** The datasheet's charge-current-by-temperature table: temperatures as rows, SOC bands as columns. */
export function DeratingTable({ cell }: { cell: Cell }) {
  const { t } = useTranslation();
  const d = cell.charge_derating;
  if (!d) {
    const note = cell.provenance?.charge_derating?.note;
    return <p className="text-sm text-muted">{note ? t("cells.noTable") : t("cells.tableUnknown")}</p>;
  }
  const rows = [...new Map(d.points.map((p) => [tempLabel(p), p])).values()].sort((a, b) => tempKey(a) - tempKey(b));
  const cols = [...new Set(d.points.map(socLabel))];
  const cell2 = (row: DeratingPoint, col: string) => d.points.find((p) => tempLabel(p) === tempLabel(row) && socLabel(p) === col);
  const max = Math.max(...d.points.map((p) => p.value), 0.0001);
  const range = effectiveChargeRange(cell);
  const amps = (v: number) => toAmps(cell, v, d.basis);
  return (
    <div className="space-y-3">
      <p className="text-sm">
        {range ? t("cells.effective", { min: fmt(range.min, Number.isInteger(range.min) ? 0 : 1), max: fmt(range.max, Number.isInteger(range.max) ? 0 : 1) }) : t("cells.neverCharge")}
        {cell.temperature?.charge_min_c !== undefined && (
          <span className="text-muted"> {t("cells.datasheetRange", { min: cell.temperature.charge_min_c, max: cell.temperature.charge_max_c })}</span>
        )}
      </p>
      <div className="overflow-x-auto rounded-md border border-line">
        <table className="w-full border-collapse text-sm tabular-nums">
          <thead>
            <tr className="bg-sunken/60 text-muted">
              <th className="px-3 py-1.5 text-left font-normal">{t("cells.temp")}</th>
              {cols.map((c) => <th key={c} className="px-2 py-1.5 text-right font-normal">{cols.length > 1 ? `SOC ${c}` : t("cells.rate", { basis: d.basis })}</th>)}
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={tempLabel(r)} className="border-t border-line">
                <td className="px-3 py-1 text-muted">{tempLabel(r)}</td>
                {cols.map((c) => {
                  const p = cell2(r, c);
                  const a = p ? amps(p.value) : undefined;
                  return (
                    <td key={c} className={cn("px-2 py-1 text-right", p && tone(p.value, max))} title={a !== undefined ? `${fmt(a, 0, "A")}` : undefined}>
                      {p ? `${fmt(p.value, p.value < 0.1 && p.value > 0 ? 3 : 2)} ${d.basis}` : ""}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="flex items-center gap-1.5 text-xs text-muted">{t("cells.lowerRule")}<InfoIcon id="topic.derating_rule" /></p>
    </div>
  );
}
