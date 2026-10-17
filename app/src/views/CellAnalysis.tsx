import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Microscope } from "lucide-react";
import { api, isDemo, type CellAnalysis, type CellResult } from "../api";
import { Panel } from "../components/ui/card";
import { Segmented } from "../components/PackPicker";
import { InfoIcon } from "../help";
import { useSettings } from "../installData";
import { useDark } from "../history/Chart";
import { cn, fmt } from "../lib/utils";
import { HEAT_CLIP_MV, MIN_ROWS, MIN_STEPS, bottomOf, heatRgb, hexRgb, reasonArgs, signedMv, topOf } from "../cellAnalysis";
import { BalancerTimeline } from "./BalancerTimeline";

const DAY = 86_400_000;
const PERIODS = ["1", "7", "30", "90"] as const;
type Period = (typeof PERIODS)[number];
const SLICES = 240;
// kept while the app runs, like the report's period
let keptPeriod: Period = "7";

const CLASS_TONE: Record<CellResult["class"], string> = {
  ok: "text-muted", soc_offset: "text-discharge", capacity: "text-alarm", ambiguous: "text-discharge", resistance: "text-alarm", measurement: "text-alarm", none: "text-muted",
};

/** The cell analysis of a pack (`cell_analysis`) over a period of its recording: a heatmap of each cell's deviation from
 *  the row median, the balancing timeline over the same span right below it, and per cell the deviation at rest, at the
 *  top and at the bottom, the resistance from load steps and the class with its reason. */
export function CellAnalysisPanel({ pack, scope }: { pack: number; scope: string | null }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const { settings, loaded } = useSettings(scope);
  const [period, setPeriodState] = useState<Period>(keptPeriod);
  const setPeriod = (p: Period) => { keptPeriod = p; setPeriodState(p); };
  const [span, setSpan] = useState(() => { const now = Date.now(); return { from: now - Number(keptPeriod) * DAY, to: now }; });
  const [data, setData] = useState<CellAnalysis | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const site = scope ?? (isDemo ? "demo" : null);
  const devicesKey = JSON.stringify(settings.devices ?? null);

  // one span for the heatmap and the timeline below it; the 24 h view moves with the recording
  useEffect(() => {
    const set = () => { const now = Date.now(); setSpan({ from: now - Number(period) * DAY, to: now }); };
    set();
    const timer = period === "1" ? window.setInterval(set, 60_000) : undefined;
    return () => clearInterval(timer);
  }, [period]);

  useEffect(() => {
    if (!site || !loaded) return;
    let alive = true;
    setBusy(true);
    api.cellAnalysis(site, pack, span.from, span.to, settings.devices ?? null, SLICES)
      .then((d) => { if (alive) { setData(d); setErr(null); } })
      .catch((e) => alive && setErr(String(e)))
      .finally(() => alive && setBusy(false));
    return () => { alive = false; };
  }, [site, pack, span, loaded, devicesKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const when = (ms: number) => new Date(ms).toLocaleString(lang, period === "1" ? { timeStyle: "short" } : { dateStyle: "short", timeStyle: "short" });
  const n = (v: number) => v.toLocaleString(lang);

  return (
    <Panel className="space-y-5 p-5">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h2 className="flex items-center gap-1.5 font-display text-2xl font-semibold"><Microscope className="h-5 w-5 text-muted" aria-hidden />{t("cell.title")}<InfoIcon id="topic.cellAnalysis" /></h2>
        <div className="flex items-center gap-3">
          {busy && <span className="text-sm text-muted">{t("hist.loading")}</span>}
          <Segmented value={period} onChange={(v) => setPeriod(v as Period)} items={PERIODS.map((p) => [p, t(`cell.period.${p}`)])} />
        </div>
      </div>
      {!site ? <p className="text-sm text-muted">{t("dev.tl.needInst")}</p>
        : err ? <p className="text-sm text-alarm">{err}</p>
        : !data ? <p className="text-sm text-muted">{t("hist.loading")}</p>
        : !data.cell_count ? <p className="text-sm text-muted">{t("cell.noRows")}</p> : (
          <section className="space-y-2">
            <p className="max-w-3xl text-sm text-muted">{t("cell.intro")}</p>
            <Heatmap data={data} when={when} />
            <div className="flex justify-between text-xs text-muted tabular-nums"><span>{when(data.from)}</span><HeatLegend /><span>{when(data.to)}</span></div>
          </section>
        )}

      <BalancerTimeline pack={pack} scope={scope} from={span.from} to={span.to} />

      {data && data.cell_count > 0 && (
        <section className="space-y-3">
          <h3 className="font-display text-lg font-semibold">{t("cell.tableTitle")}</h3>
          <div className="overflow-x-auto">
            <table className="w-full text-sm tabular-nums">
              <thead className="text-left text-muted">
                <tr className="border-b border-line">
                  <th className="py-2 pr-4 font-normal">{t("cell.col.cell")}</th>
                  <th className="py-2 pr-4 text-right font-normal">{t("cell.col.rest")}</th>
                  <th className="py-2 pr-4 text-right font-normal">{t("cell.col.top")}</th>
                  <th className="py-2 pr-4 text-right font-normal">{t("cell.col.bottom")}</th>
                  <th className="py-2 pr-4 text-right font-normal">{t("cell.col.r")}</th>
                  <th className="py-2 pr-4 text-right font-normal">{t("cell.col.order")}</th>
                  <th className="py-2 font-normal">{t("cell.col.class")}</th>
                </tr>
              </thead>
              <tbody>
                {data.cells.map((c) => {
                  const [rk, rv] = reasonArgs(c.reason, lang);
                  return (
                    <tr key={c.cell} className="border-b border-line align-top last:border-b-0">
                      <td className="py-2 pr-4 font-medium">{c.cell}</td>
                      <td className="py-2 pr-4 text-right">
                        {c.rest === null ? <NoStatement title={t("cell.none.rest", { rows: c.rest_rows, min: MIN_ROWS })} /> : signedMv(c.rest, lang)}
                      </td>
                      <EndCell value={topOf(c)} raw={c.top} corrected={c.top_corr !== null} missing={t("cell.none.top")} />
                      <EndCell value={bottomOf(c)} raw={c.bottom} corrected={c.bottom_corr !== null} missing={t("cell.none.bottom")} />
                      <td className="py-2 pr-4 text-right">
                        {c.r_mohm === null ? <NoStatement title={t("cell.none.steps", { steps: data.steps, min: MIN_STEPS })} />
                          : <>{fmt(c.r_mohm, 2, "mΩ")}<span className={cn("block text-xs", (c.r_pct ?? 100) >= 130 ? "text-alarm" : "text-muted")}>{fmt(c.r_pct, 0, "%")}</span></>}
                      </td>
                      <td className="py-2 pr-4 text-right">{fmt(c.order, 1)}</td>
                      <td className="py-2">
                        <span className={cn("font-medium", CLASS_TONE[c.class], c.class === "ok" && "text-ink")}>{t(`cell.class.${c.class}`)}</span>
                        <span className="block text-xs text-muted">{t(rk, rv)}{c.jumps > 0 && c.reason.key !== "jumps" ? `; ${t("cell.jumpsNote", { count: c.jumps })}` : ""}</span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <ul className="space-y-0.5 text-sm text-muted">
            <li>{t("cell.sum.rows", { rows: n(data.rows), cells: data.cell_count, rest: n(data.rest_rows), top: n(data.top_rows), bottom: n(data.bottom_rows), load: n(data.load_rows), steps: n(data.steps) })}</li>
            {data.top_rows < MIN_ROWS && <li>{t("cell.none.top")}</li>}
            {data.bottom_rows < MIN_ROWS && <li>{t("cell.none.bottom")}</li>}
            {data.steps < MIN_STEPS && <li>{t("cell.none.stepsPack", { steps: data.steps, min: MIN_STEPS })}</li>}
            {data.balancer_active + data.balancer_unknown > 0 && <li>{t("cell.sum.balancer", { count: data.balancer_active + data.balancer_unknown, unknown: n(data.balancer_unknown) })}</li>}
            {data.bms_left_out > 0 && <li>{t("cell.sum.bms", { count: data.bms_left_out })}</li>}
            {data.rest_no_status > 0 && <li>{t("cell.sum.noStatus", { count: data.rest_no_status })}</li>}
            {data.incomplete > 0 && <li>{t("cell.sum.incomplete", { count: data.incomplete, cells: data.cell_count })}</li>}
            {data.minute_rows > 0 && <li>{t("cell.sum.minutes", { count: data.minute_rows })}</li>}
            {data.charges > 0 && <li>{t("cell.sum.order", { count: data.charges })}</li>}
            {data.r_median !== null && <li>{t("cell.sum.rMedian", { r: fmt(data.r_median, 2, "mΩ") })}</li>}
          </ul>
        </section>
      )}
    </Panel>
  );
}

function NoStatement({ title }: { title: string }) {
  const { t } = useTranslation();
  return <span className="cursor-help text-muted underline decoration-dotted underline-offset-2" title={title}>{t("cell.none.short")}</span>;
}

/** Top or bottom: the corrected value where the resistance is known, the raw one in the tooltip. */
function EndCell({ value, raw, corrected, missing }: { value: number | null; raw: number | null; corrected: boolean; missing: string }) {
  const { t, i18n } = useTranslation();
  return (
    <td className="py-2 pr-4 text-right">
      {value === null ? <NoStatement title={missing} /> : (
        <span title={corrected ? t("cell.corrected", { raw: signedMv(raw, i18n.language) }) : t("cell.uncorrected")}>
          {signedMv(value, i18n.language)}{!corrected && <span className="text-muted">*</span>}
        </span>
      )}
    </td>
  );
}

/** Cells × time, one pixel per cell and slice scaled to the width, so it lines up with the timeline below. */
function Heatmap({ data, when }: { data: CellAnalysis; when: (ms: number) => string }) {
  const { t, i18n } = useTranslation();
  const dark = useDark();
  const ref = useRef<HTMLCanvasElement>(null);
  const [tip, setTip] = useState("");
  const rows = data.heat.length, cols = data.heat[0]?.length ?? 0;
  const rowPx = rows > 20 ? 7 : 10;

  useEffect(() => {
    const c = ref.current, ctx = c?.getContext("2d");
    if (!c || !ctx || !cols) return;
    const css = getComputedStyle(document.documentElement);
    const color = (name: string, fallback: string) => hexRgb(css.getPropertyValue(name)) ?? hexRgb(fallback)!;
    const neutral = color("--surface", dark ? "#222b35" : "#fafbfc"), low = color("--low", "#3f6fb5"), high = color("--high", "#8b4fb0");
    const img = ctx.createImageData(cols, rows);
    data.heat.forEach((line, y) => line.forEach((v, x) => {
      const rgb = heatRgb(v, neutral, low, high), i = (y * cols + x) * 4;
      if (!rgb) return;
      img.data.set([...rgb, 255], i);
    }));
    ctx.putImageData(img, 0, 0);
  }, [data, dark, rows, cols]);

  const move = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = Math.min(cols - 1, Math.max(0, Math.floor(((e.clientX - r.left) / r.width) * cols)));
    const y = Math.min(rows - 1, Math.max(0, Math.floor(((e.clientY - r.top) / r.height) * rows)));
    const width = (data.to - data.from) / cols, v = data.heat[y]?.[x] ?? null;
    setTip(`${t("cell.col.cell")} ${y + 1}, ${when(data.from + x * width)}–${when(data.from + (x + 1) * width)}: ${v === null ? t("dev.tl.legend.gap") : signedMv(v, i18n.language)}`);
  };

  return (
    <div className="relative">
      <canvas ref={ref} width={cols} height={rows} onMouseMove={move} title={tip} aria-label={t("cell.heatLabel")}
        className="block w-full rounded border border-line" style={{ height: rows * rowPx, imageRendering: "pixelated" }} />
      <div className="pointer-events-none absolute inset-y-px left-px flex flex-col" aria-hidden>
        {Array.from({ length: rows }, (_, i) => (
          <span key={i} className="bg-surface/70 px-0.5 text-[8px] leading-none text-muted" style={{ height: rowPx, lineHeight: `${rowPx}px` }}>{rows <= 20 || i % 2 === 0 ? i + 1 : ""}</span>
        ))}
      </div>
    </div>
  );
}

function HeatLegend() {
  const { t } = useTranslation();
  return (
    <span className="inline-flex items-center gap-1.5">
      <span>−{HEAT_CLIP_MV} mV</span>
      <span className="h-2 w-24 rounded-sm border border-line" style={{ background: "linear-gradient(to right, var(--low), var(--surface), var(--high))" }} />
      <span>+{HEAT_CLIP_MV} mV</span>
      <span>{t("cell.heatRef")}</span>
    </span>
  );
}
