import { useEffect, useMemo, useState } from "react";
import { decideUi, lockProps } from "../locks";
import { useTranslation } from "react-i18next";
import { ArrowDownRight, ArrowRight, ArrowUpRight, Download } from "lucide-react";
import { api, isDemo, type CellAnalysis, type HealthReport as Report, type PackDay } from "../api";
import { reasonText, reportCells } from "../cellAnalysis";
import { forKey, type Keyed } from "../keyed";
import { useSettings } from "../installData";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { Segmented } from "../components/PackPicker";
import { HintNote, InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";
import { Chart, useDark, type Line } from "../history/Chart";
import { exportName } from "../history/data";
import { PERIODS, packRows, pairRows, periodRange, reportCsv, type PackFlag, type Period, type Trend } from "../history/report";
import type { Pair } from "../pairs";

const no = (a: number) => String(a).padStart(2, "0");

// kept while the app runs, like the chart span
let keptPeriod: Period = 30;

/** The health report of an installation: per pack spread, cycles and temperature, per pair the current share, over
 *  7 / 30 / 90 days or everything. From the recording only (health.rs). */
export function HealthReport({ site, name, siteFrom, pairs }: { site: string; name: string; siteFrom: number | null; pairs: Pair[] }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const dark = useDark();
  const [period, setPeriodState] = useState<Period>(keptPeriod);
  const setPeriod = (p: Period) => { keptPeriod = p; setPeriodState(p); };
  // the report and the cell analyses with the query they answer: another installation, period or pair set shows none
  const [reportState, setReport] = useState<Keyed<Report> | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [sel, setSel] = useState<{ pack: number } | { pair: number } | null>(null);
  const pairKey = pairs.map((p) => `${p.plus}/${p.minus}`).join(",");
  const { settings, loaded } = useSettings(site);
  const devicesKey = JSON.stringify(settings.devices ?? null);
  // the cell analysis per pack, one after the other once the report is there
  const [cellsState, setCells] = useState<Keyed<Record<number, CellAnalysis | string>> | null>(null);
  const reportKey = `${site}|${period}|${siteFrom}|${pairKey}`;
  const report = forKey(reportState, reportKey);
  const cellsKey = `${reportKey}|${devicesKey}`;
  const cells = forKey(cellsState, cellsKey) ?? {};

  useEffect(() => {
    let alive = true;
    const [from, to] = periodRange(period, Date.now(), siteFrom);
    setBusy(true); setErr(null);
    api.healthReport(site, from, to, pairs)
      .then((r) => { if (alive) setReport({ key: reportKey, value: r }); })
      .catch((e) => { if (alive) { setReport(null); setErr(String(e)); } })
      .finally(() => alive && setBusy(false));
    return () => { alive = false; };
  }, [site, period, siteFrom, pairKey]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    setCells({ key: cellsKey, value: {} });
    if (!report || !loaded) return;
    let alive = true;
    (async () => {
      for (const p of report.packs) {
        if (!alive) return;
        const r = await api.cellAnalysis(site, p.pack, report.from, report.to, settings.devices ?? null, 0).catch((e) => String(e));
        if (alive) setCells((c) => (c?.key === cellsKey ? { key: cellsKey, value: { ...c.value, [p.pack]: r } } : c));
      }
    })();
    return () => { alive = false; };
  }, [report, loaded, devicesKey, site]); // eslint-disable-line react-hooks/exhaustive-deps
  const cellsByPack = Object.fromEntries(Object.entries(cells).filter((e): e is [string, CellAnalysis] => typeof e[1] !== "string"));

  const packs = useMemo(() => (report ? packRows(report) : []), [report]);
  const pairList = useMemo(() => (report ? pairRows(report) : []), [report]);
  const date = (ms: number) => new Intl.DateTimeFormat(lang, { day: "2-digit", month: "2-digit", year: "numeric" }).format(ms);

  const save = async () => {
    if (!report) return;
    setSaved(null); setErr(null);
    const content = reportCsv(report, cellsByPack), file = exportName(`${name}_report`, "csv");
    try {
      if (isDemo) {
        const a = document.createElement("a");
        a.href = URL.createObjectURL(new Blob([content], { type: "text/csv" }));
        a.download = file; a.click(); URL.revokeObjectURL(a.href);
        setSaved(file);
        return;
      }
      const { save: pick } = await import("@tauri-apps/plugin-dialog");
      const [, exports] = await api.historyDirs();
      const path = await pick({ defaultPath: `${exports}/${file}`, filters: [{ name: "CSV", extensions: ["csv"] }] });
      if (!path) return;
      await api.saveBmsHistory(path, content);
      setSaved(path.split(/[\\/]/).pop() ?? path);
    } catch (e) { setErr(String(e)); }
  };

  const flagText = (f: PackFlag) => {
    switch (f.key) {
      case "cycles": return t("hist.rep.flag.cycles", { pct: f.pct });
      case "spread": return t("hist.rep.flag.spread", { mv: fmt(f.mv, 0), median: fmt(f.median, 0) });
      case "spreadRising": return t("hist.rep.flag.spreadRising", { mv: fmt(f.mv, 0) });
      case "warm": return t("hist.rep.flag.warm", { c: fmt(f.c, 1), median: fmt(f.median, 1) });
      case "counterCleared": return t("hist.rep.flag.counterCleared", { count: f.count });
    }
  };

  // the chart of the selected row: daily values
  let chart: { title: string; unit: string; dec: number; x: number[]; lines: Line[] } | null = null;
  if (report && sel && "pack" in sel) {
    const p = report.packs.find((x) => x.pack === sel.pack);
    if (p) {
      // single answers and minute maxima are different measures: their own curves, never joined across the change
      const of = (raw: boolean, f: (d: PackDay) => number | null) => p.days.map((d) => (d.spread_raw === raw ? f(d) : null));
      const base: [string, string, (d: PackDay) => number | null, number[] | undefined][] = [
        [t("hist.rep.l.restP50"), dark ? "#3fb6bf" : "#0e7c86", (d) => d.rest_p50, undefined],
        [t("hist.rep.l.restP95"), dark ? "#7fa6e3" : "#3f6fb5", (d) => d.rest_p95, undefined],
        [t("hist.rep.l.loadP95"), dark ? "#e0a548" : "#b7791f", (d) => d.spread_p95, [6, 4]]];
      const minute = p.days.some((d) => !d.spread_raw && d.spread_p95 !== null);
      chart = { title: t("hist.rep.chartPack", { p: no(p.pack) }), unit: "mV", dec: 0, x: p.days.map((d) => d.day), lines: [
        ...base.map(([label, color, f, dash]) => ({ label, color, values: of(true, f), dash })),
        ...(minute ? base.map(([label, color, f]) => ({ label: `${label} ${t("hist.rep.l.minute")}`, color, values: of(false, f), dash: [2, 3], width: 1 })) : [])] };
    }
  } else if (report && sel && "pair" in sel) {
    const p = report.pairs.find((x) => x.plus === sel.pair);
    if (p) chart = { title: t("hist.rep.chartPair", { p: `${no(p.plus)}/${no(p.minus)}` }), unit: "%", dec: 1, x: p.days.map((d) => d.day), lines: [
      { label: t("hist.rep.l.share"), color: dark ? "#3fb6bf" : "#0e7c86", values: p.days.map((d) => d.share) },
      { label: "50 %", color: dark ? "#8a96a3" : "#7a8591", values: p.days.map(() => 50), width: 1 }] };
  }

  const spreadLate = report?.spread_from != null && report.spread_from > report.from + 86_400_000;
  const noSpread = !!report?.packs.length && report.spread_from === null;
  const rawLate = report && pairs.length > 0 && (report.raw_from === null || report.raw_from > report.from + 86_400_000);

  return (
    <div className="space-y-6">
      <Panel className="space-y-3 p-5">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <h2 className="flex items-center gap-1.5 font-display text-xl font-semibold">{t("hist.rep.title")}<InfoIcon id="topic.healthReport" /></h2>
          <Segmented value={String(period)} onChange={(v) => setPeriod(v === "all" ? "all" : (Number(v) as Period))}
            items={PERIODS.map((p) => [String(p), p === "all" ? t("hist.all") : t("hist.daysShort", { count: p })] as [string, string])} />
          {busy && <span className="text-sm text-muted">{t("hist.loading")}</span>}
          <Button className="ml-auto" variant="secondary" {...lockProps(decideUi("report.export", { source: null, chosen: null, entries: !!report?.packs.length }), t)} onClick={save}><Download className="h-4 w-4" />{t("hist.rep.csv")}</Button>
        </div>
        <p className="max-w-3xl text-sm text-muted">{t("hist.rep.intro")}</p>
        {report && <p className="text-sm text-muted">{date(report.from)} – {date(Math.max(report.from, report.to - 1)) /* to: the midnight after the last day */}</p>}
        {spreadLate && <p className="text-sm text-discharge">{t("hist.rep.spreadFrom", { date: date(report!.spread_from!) })}</p>}
        {noSpread && <p className="text-sm text-discharge">{t("hist.rep.noSpread")}</p>}
        {rawLate && <p className="text-sm text-discharge">{report!.raw_from === null ? t("hist.rep.noRaw") : t("hist.rep.rawFrom", { date: date(report!.raw_from) })}</p>}
        {saved && <p className="text-sm text-ok">{t("hist.rep.saved", { name: saved })}</p>}
        {err && <p className="text-sm text-alarm">{err}</p>}
      </Panel>

      {report && !report.packs.length && !busy && <p className="text-muted">{t("hist.rep.empty")}</p>}

      {packs.length > 0 && (
        <Panel className="overflow-x-auto p-5">
          <h3 className="mb-2 font-display text-lg font-semibold">{t("hist.rep.packs")}</h3>
          <table className="w-full text-sm tabular-nums">
            <thead className="text-left text-muted">
              <tr className="border-b border-line">
                <th className="py-2 pr-4 font-normal">{t("dash.pack")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("hist.rep.col.rest")}<span className="block text-xs">{t("hist.rep.col.restSub")}</span></th>
                <th className="py-2 pr-4 text-right font-normal">{t("hist.rep.col.load")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("hist.rep.col.cycles")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("hist.rep.col.rate")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("hist.rep.col.temp")}</th>
                <th className="py-2 font-normal">{t("hist.rep.col.note")}</th>
              </tr>
            </thead>
            <tbody>
              {packs.map((r) => {
                const on = !!sel && "pack" in sel && sel.pack === r.pack;
                const warn = r.flags.some((f) => f.key !== "counterCleared");
                return (
                  <tr key={r.pack} onClick={() => setSel({ pack: r.pack })} className={cn("cursor-pointer border-b border-line last:border-b-0 hover:bg-sunken", on && "bg-sunken")}>
                    <td className="py-2 pr-4 font-display text-lg font-semibold">{no(r.pack)}</td>
                    <td className="py-2 pr-4 text-right">
                      <span className="inline-flex items-center gap-1.5">{fmt(r.restP50, 0)} / {fmt(r.restP95, 0, "mV")}<TrendIcon trend={r.restTrend} /></span>
                    </td>
                    <td className="py-2 pr-4 text-right">{fmt(r.loadP95, 0, "mV")}</td>
                    <td className="py-2 pr-4 text-right">{fmt(r.cycles, 0)}</td>
                    <td className={cn("py-2 pr-4 text-right", r.flags.some((f) => f.key === "cycles") && "font-medium text-discharge")}>
                      {r.rateShort
                        ? <HintNote help="topic.healthReport" items={[{ title: t("hist.rep.col.rate"), text: t("hist.rep.tip.rateShort") }]}>–</HintNote>
                        : fmt(r.rate, 1)}
                    </td>
                    <td className="py-2 pr-4 text-right">{fmt(r.tempMax, 1, "°C")}</td>
                    <td className={cn("py-2", warn ? "text-discharge" : "text-muted")}>
                      <HintNote help="topic.healthReport" items={[
                        ...(r.flags.length ? r.flags.map((f) => ({ title: flagText(f), text: t(`hist.rep.flagTip.${f.key}`) })) : r.unrated.length ? [] : [{ title: t("hist.rep.ok"), text: t("hist.rep.tip.packOk") }]),
                        ...r.unrated.map((k) => ({ title: t("hist.rep.unrated.title"), text: t(`hist.rep.unrated.${k}`) }))]}>
                        {r.flags.length ? r.flags.map(flagText).join("; ") : r.unrated.length ? t("hist.rep.unrated.title") : t("hist.rep.ok")}
                      </HintNote>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Panel>
      )}

      {report && report.packs.length > 0 && (
        <Panel className="overflow-x-auto p-5">
          <h3 className="mb-1 flex items-center gap-1.5 font-display text-lg font-semibold">{t("hist.rep.cells.title")}<InfoIcon id="topic.cellAnalysis" /></h3>
          <p className="mb-2 max-w-3xl text-sm text-muted">{t("hist.rep.cells.intro")}</p>
          {Object.keys(cells).length < report.packs.length && (
            <p className="text-sm text-muted">{t("hist.rep.cells.loading", { done: Object.keys(cells).length, total: report.packs.length })}</p>
          )}
          <table className="w-full text-sm">
            <thead className="text-left text-muted">
              <tr className="border-b border-line">
                <th className="py-2 pr-4 font-normal">{t("dash.pack")}</th>
                <th className="py-2 font-normal">{t("hist.rep.cells.col.cells")}</th>
              </tr>
            </thead>
            <tbody>
              {report.packs.filter((p) => cells[p.pack] !== undefined).map((p) => {
                const a = cells[p.pack];
                const off = typeof a === "string" || !a.cell_count ? null : reportCells(a);
                return (
                  <tr key={p.pack} className="border-b border-line align-top last:border-b-0">
                    <td className="py-2 pr-4 font-display text-lg font-semibold">{no(p.pack)}</td>
                    <td className="py-2">
                      {typeof a === "string" ? <span className="text-alarm">{t("hist.rep.cells.failed", { err: a })}</span>
                        : !off ? <span className="text-muted">{t("hist.rep.cells.noRaw")}</span>
                        : off.all ? <HintNote className="text-muted" help="topic.cellAnalysis" items={[{ title: t("hist.rep.cells.allNone"), text: t("hist.rep.tip.cellsAllNone") }]}>{t("hist.rep.cells.allNone")}: {reasonText(t, off.all.reason, lang)}</HintNote>
                        : !off.cells.length ? (
                          <HintNote className="text-muted" help="topic.cellAnalysis" items={[{ title: t("cell.class.rest_ok"), text: t("hist.rep.tip.cellsQuiet") }]}>{a.cells.some((c) => c.class === "rest_ok")
                            ? t("hist.rep.cells.quiet", { ok: a.cells.filter((c) => c.class === "ok").length, rest: a.cells.filter((c) => c.class === "rest_ok").length })
                            : t("hist.rep.cells.allOk", { count: a.cells.length })}</HintNote>
                        )
                        : (
                          <ul className="space-y-0.5">
                            {off.cells.map((c) => (
                              <li key={c.cell}>
                                <HintNote className={cn("font-medium", c.class === "none" ? "text-muted" : "text-discharge")} help="topic.cellAnalysis" items={[{ title: t(`cell.class.${c.class}`), text: t(`cell.classTip.${c.class}`) }]}>{t("hist.rep.cells.cell", { n: c.cell })}: {t(`cell.class.${c.class}`)}</HintNote>
                                <span className="text-muted"> ({reasonText(t, c.reason, lang)})</span>
                              </li>
                            ))}
                          </ul>
                        )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Panel>
      )}

      {report && report.packs.length > 0 && (
        <Panel className="overflow-x-auto p-5">
          <h3 className="mb-2 flex items-center gap-1.5 font-display text-lg font-semibold">{t("hist.rep.pairs")}<InfoIcon id="topic.pairs" /></h3>
          {!pairs.length ? <p className="text-sm text-muted">{t("hist.rep.noPairs")}</p> : (
            <table className="w-full text-sm tabular-nums">
              <thead className="text-left text-muted">
                <tr className="border-b border-line">
                  <th className="py-2 pr-4 font-normal">{t("diag.pair")}</th>
                  <th className="py-2 pr-4 text-right font-normal">{t("hist.rep.col.share")}</th>
                  <th className="py-2 pr-4 text-right font-normal">{t("hist.rep.col.samples")}</th>
                  <th className="py-2 pr-4 text-right font-normal">{t("hist.rep.col.marked")}</th>
                  <th className="py-2 font-normal">{t("hist.rep.col.note")}</th>
                </tr>
              </thead>
              <tbody>
                {pairList.map((r) => {
                  const on = !!sel && "pair" in sel && sel.pair === r.plus;
                  return (
                    <tr key={r.plus} onClick={() => setSel({ pair: r.plus })} className={cn("cursor-pointer border-b border-line last:border-b-0 hover:bg-sunken", on && "bg-sunken")}>
                      <td className="py-2 pr-4 font-medium">{no(r.plus)} / {no(r.minus)}</td>
                      <td className="py-2 pr-4 text-right"><span className="inline-flex items-center gap-1.5">{fmt(r.share, 1, "%")}<TrendIcon trend={r.trend} /></span></td>
                      <td className="py-2 pr-4 text-right">{fmt(r.samples, 0)}</td>
                      <td className={cn("py-2 pr-4 text-right", r.markedDays > 0 && "font-medium text-discharge")}>{fmt(r.markedDays, 0)}</td>
                      <td className={cn("py-2", r.flag ? "text-discharge" : "text-muted")}>
                        {(() => {
                          const text = r.flag ? t("hist.rep.pairFlag", { pack: no(r.flag.pack), share: fmt(r.flag.share, 0), count: r.flag.days })
                            : r.samples ? t("hist.rep.ok") : t("hist.rep.noSamples");
                          const tip = t(r.flag ? "hist.rep.tip.pairFlag" : r.samples ? "hist.rep.tip.pairOk" : "hist.rep.tip.pairNoSamples")
                            + (r.equalizing ? ` ${t("hist.rep.tip.pairEqualizing", { count: r.equalizing, total: r.samples })}` : "");
                          return <HintNote help="topic.healthReport" items={[{ title: text, text: tip }]}>{text}</HintNote>;
                        })()}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </Panel>
      )}

      {report && report.packs.length > 0 && (
        chart && chart.x.length ? (
          <Panel className="space-y-2 p-4">
            <h3 className="flex items-center gap-1.5 font-display text-lg font-semibold">{chart.title}<span className="text-sm font-normal text-muted">{chart.unit}</span></h3>
            <Chart x={chart.x} lines={chart.lines} unit={chart.unit} decimals={chart.dec} lang={lang} timeLabel={t("hist.rep.day")} syncKey="report"
              onZoom={() => {}} onReset={() => {}} height={180} />
          </Panel>
        ) : <p className="text-sm text-muted">{t("hist.rep.choose")}</p>
      )}
    </div>
  );
}

function TrendIcon({ trend }: { trend?: Trend }) {
  const { t } = useTranslation();
  if (!trend) return null;
  const Icon = trend.dir === "up" ? ArrowUpRight : trend.dir === "down" ? ArrowDownRight : ArrowRight;
  const label = t(`hist.rep.trend.${trend.dir}`);
  return <span title={label} aria-label={label}><Icon className="h-3.5 w-3.5 text-muted" aria-hidden /></span>;
}
