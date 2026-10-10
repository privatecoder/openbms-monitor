import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChartLine, Download, FileUp, RotateCcw } from "lucide-react";
import { api, isDemo, type Series, type SiteSpan } from "../api";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { Segmented, Tick } from "../components/PackPicker";
import { SevIcon } from "../components/widgets";
import { InfoIcon, helpTitle } from "../help";
import { cn, fmt } from "../lib/utils";
import { Chart, useDark, type Line } from "../history/Chart";
import { exportName, packColor, shares } from "../history/data";

const no = (a: number) => String(a).padStart(2, "0");
const HOUR = 3_600_000;
const MAX_POINTS = 1500;

type PackMetric = "current" | "share" | "soc" | "voltage" | "cell_max" | "cell_min" | "delta" | "temp_max" | "temp_min";
type SystemMetric = "sysCurrent" | "sysSoc" | "sysVoltage" | "sysCells";
type Metric = PackMetric | SystemMetric | "cells";

const PACK: Record<PackMetric, { unit: string; dec: number }> = {
  current: { unit: "A", dec: 1 }, share: { unit: "", dec: 2 }, soc: { unit: "%", dec: 1 }, voltage: { unit: "V", dec: 2 },
  cell_max: { unit: "V", dec: 3 }, cell_min: { unit: "V", dec: 3 }, delta: { unit: "mV", dec: 0 },
  temp_max: { unit: "°C", dec: 1 }, temp_min: { unit: "°C", dec: 1 },
};
const PACK_METRICS = Object.keys(PACK) as PackMetric[];
const SYSTEM_METRICS: SystemMetric[] = ["sysCurrent", "sysSoc", "sysVoltage", "sysCells"];
/** Spans relative to now; "all" = everything of the installation. */
const PRESETS = [1, 6, 24, 24 * 7, 24 * 30] as const;

type Span = { preset: number | "all" } | { from: number; to: number };

// kept while the app runs, so switching views keeps the choice
let keptSpan: Span = { preset: 24 };
let keptMetrics: Metric[] = ["current", "soc", "delta", "sysCurrent"];

/** Recorded history from the database as synchronized charts, with export and import. */
export function History({ site: liveSite }: { site: string }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const dark = useDark();
  const [sites, setSites] = useState<SiteSpan[] | null>(null);
  const [site, setSite] = useState<string | null>(null);
  const [span, setSpanState] = useState<Span>(keptSpan);
  const setSpan = (s: Span) => { keptSpan = s; setSpanState(s); };
  const [metrics, setMetricsState] = useState<Metric[]>(keptMetrics);
  const setMetrics = (f: (m: Metric[]) => Metric[]) => setMetricsState((m) => (keptMetrics = f(m)));
  const [hidden, setHidden] = useState<Set<number>>(new Set());
  const [cellPack, setCellPack] = useState<number | null>(null);
  const [data, setData] = useState<Series | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const [format, setFormat] = useState<"jsonl" | "csv">("csv");

  const loadSites = () => api.historySites().then((s) => {
    setSites(s);
    setSite((cur) => (cur && s.some((x) => x.site === cur) ? cur : s.find((x) => x.site === liveSite)?.site ?? s[0]?.site ?? null));
  }).catch((e) => { setSites([]); setErr(String(e)); });
  useEffect(() => { loadSites(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const info = sites?.find((s) => s.site === site);
  const packs = useMemo(() => info?.packs ?? [], [info]);
  useEffect(() => { if (cellPack === null || !packs.includes(cellPack)) setCellPack(packs[0] ?? null); }, [packs]); // eslint-disable-line react-hooks/exhaustive-deps

  // spans relative to now follow the clock: refresh every 30 s
  const relative = "preset" in span;
  useEffect(() => {
    if (!relative) return;
    const id = window.setInterval(() => setTick((x) => x + 1), 30_000);
    return () => window.clearInterval(id);
  }, [relative]);

  const [from, to] = useMemo((): [number, number] => {
    if (!("preset" in span)) return [span.from, span.to];
    const now = Date.now();
    if (span.preset === "all") return [info?.from ?? now - HOUR, Math.max(info?.to ?? now, (info?.from ?? 0) + 60_000)];
    return [now - span.preset * HOUR, now];
  }, [span, info, tick]); // eslint-disable-line react-hooks/exhaustive-deps

  const wantCells = metrics.includes("cells");
  useEffect(() => {
    if (!site || !packs.length) { setData(null); return; }
    let alive = true;
    setBusy(true);
    api.historySeries(site, from, to, packs, wantCells ? cellPack : null, MAX_POINTS)
      .then((d) => { if (alive) { setData(d); setErr(null); } })
      .catch((e) => alive && setErr(String(e)))
      .finally(() => alive && setBusy(false));
    return () => { alive = false; };
  }, [site, from, to, packs, wantCells, cellPack]);

  const zoom = (a: number, b: number) => { if (data && b - a >= data.step * 4) setSpan({ from: a, to: b }); };
  const reset = () => setSpan({ preset: 24 });
  const toggleMetric = (m: Metric) => setMetrics((ms) => (ms.includes(m) ? ms.filter((x) => x !== m) : [...ms, m]));
  const shown = packs.filter((a) => !hidden.has(a));

  const doExport = async () => {
    if (!site) return;
    setNote(null); setErr(null);
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const [, exports] = await api.historyDirs();
      const path = await save({ defaultPath: `${exports}/${exportName(site, format)}`, filters: [{ name: format === "csv" ? "CSV" : "JSON Lines", extensions: [format] }] });
      if (!path) return;
      const r = await api.historyExport(site, from, to, shown, format, path);
      setNote(t("hist.exported", { count: r.rows, files: r.files.map((f) => f.split(/[\\/]/).pop()).join(", ") }));
    } catch (e) { setErr(String(e)); }
  };
  const doImport = async () => {
    setNote(null); setErr(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const [recordings] = await api.historyDirs();
      const picked = await open({ multiple: true, defaultPath: recordings, filters: [{ name: "JSON Lines", extensions: ["jsonl", "json", "txt"] }] });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      if (!paths.length) return;
      const target = site ?? liveSite ?? "import";
      let rows = 0;
      for (const p of paths) { setNote(t("hist.importing", { name: p.split(/[\\/]/).pop() })); rows += (await api.historyImport(p, target)).rows; }
      setNote(t("hist.imported", { count: rows, files: paths.length, site: target }));
      await loadSites();
      setSite(target);
      setSpan({ preset: "all" });
    } catch (e) { setErr(String(e)); }
  };

  const charts: { id: Metric; title: string; unit: string; dec: number; lines: Line[] }[] = [];
  if (data) {
    const ink = dark ? "#e6ebef" : "#18212b", charge = dark ? "#3fb6bf" : "#0e7c86", dis = dark ? "#e0a548" : "#b7791f";
    const shareCols = shares(new Map(packs.filter((a) => data.packs[a]).map((a) => [a, data.packs[a].current])));
    for (const m of metrics) {
      if (m in PACK) {
        const pm = m as PackMetric;
        charts.push({ id: m, title: t(`hist.m.${m}`), unit: PACK[pm].unit, dec: PACK[pm].dec,
          lines: shown.filter((a) => data.packs[a]).map((a) => ({ label: no(a), color: packColor(a, dark), values: pm === "share" ? shareCols.get(a)! : data.packs[a][pm] })) });
      } else if (m === "cells" && cellPack !== null && data.cells.length) {
        charts.push({ id: m, title: t("hist.m.cells", { pack: no(cellPack) }), unit: "V", dec: 3,
          lines: data.cells.map((c, i) => ({ label: String(i + 1), color: `hsl(${Math.round((i * 360) / data.cells.length)} 60% ${dark ? 64 : 45}%)`, values: c, width: 1.25 })) });
      } else if (data.system && SYSTEM_METRICS.includes(m as SystemMetric)) {
        const s = data.system;
        if (m === "sysCurrent") charts.push({ id: m, title: t("hist.m.sysCurrent"), unit: "A", dec: 1, lines: [
          { label: t("hist.l.current"), color: ink, values: s.current },
          { label: t("hist.l.ccl"), color: charge, values: s.ccl, dash: [6, 4] },
          { label: t("hist.l.dcl"), color: dis, values: s.dcl.map((x) => (x === null ? null : -x)), dash: [6, 4] }] });
        if (m === "sysSoc") charts.push({ id: m, title: t("hist.m.sysSoc"), unit: "%", dec: 1, lines: [{ label: "SOC", color: ink, values: s.soc }] });
        if (m === "sysVoltage") charts.push({ id: m, title: t("hist.m.sysVoltage"), unit: "V", dec: 2, lines: [
          { label: t("hist.l.voltage"), color: ink, values: s.voltage },
          { label: t("hist.l.cvl"), color: charge, values: s.cvl, dash: [6, 4] }] });
        if (m === "sysCells") charts.push({ id: m, title: t("hist.m.sysCells"), unit: "V", dec: 3, lines: [
          { label: t("hist.l.highest"), color: dark ? "#c79be0" : "#8b4fb0", values: s.cell_max },
          { label: t("hist.l.lowest"), color: dark ? "#7fa6e3" : "#3f6fb5", values: s.cell_min }] });
      }
    }
  }

  const dt = (ms: number) => new Intl.DateTimeFormat(lang, { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" }).format(ms);
  const tm = (ms: number) => new Intl.DateTimeFormat(lang, { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(ms);
  const presetLabel = (h: number | "all") => (h === "all" ? t("hist.all") : h < 24 ? t("hist.hours", { count: h }) : t("hist.daysShort", { count: h / 24 }));
  const hasData = !!sites?.length;

  return (
    <div className="view-in mx-auto max-w-7xl space-y-6">
      <header>
        <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight">
          <ChartLine className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("hist.title")}<InfoIcon id="topic.history" />
        </h1>
        <p className="mt-1 max-w-3xl text-muted">{t("hist.intro")}</p>
      </header>

      <Panel className="space-y-4 p-5">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          {hasData && (
            sites!.length > 1 ? (
              <select value={site ?? ""} onChange={(e) => { setSite(e.target.value); setHidden(new Set()); }}
                className="h-9 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge" aria-label={t("hist.site")}>
                {sites!.map((s) => <option key={s.site} value={s.site}>{s.site}</option>)}
              </select>
            ) : <span className="font-medium">{site}</span>
          )}
          {hasData && (
            <Segmented value={"preset" in span ? String(span.preset) : ""} onChange={(v) => setSpan({ preset: v === "all" ? "all" : Number(v) })}
              items={[...PRESETS, "all" as const].map((h) => [String(h), presetLabel(h)] as [string, string])} />
          )}
          <div className="ml-auto flex flex-wrap items-center gap-2">
            {hasData && (
              <>
                <Segmented value={format} onChange={(v) => setFormat(v as "jsonl" | "csv")} items={[["csv", "CSV"], ["jsonl", "JSON Lines"]]} />
                <Button variant="secondary" disabled={isDemo || !data} onClick={doExport}><Download className="h-4 w-4" />{t("hist.export")}</Button>
              </>
            )}
            <Button variant="ghost" disabled={isDemo} onClick={doImport}><FileUp className="h-4 w-4" />{t("hist.import")}</Button>
          </div>
        </div>
        {hasData && <p className="text-sm text-muted">{t("hist.exportHint")}</p>}
        {note && <p className="text-sm text-ok">{note}</p>}
        {err && <p className="text-sm text-alarm">{err}</p>}
      </Panel>

      {sites && !hasData ? (
        <div className="flex max-w-lg flex-col gap-3 py-10 text-lg text-muted">
          <ChartLine className="h-8 w-8" strokeWidth={1.5} aria-hidden />{t("hist.empty")}
        </div>
      ) : data && (
        <>
          <Panel className="space-y-4 p-5">
            <div className="space-y-2">
              <h3 className="text-sm font-medium">{t("hist.show")}</h3>
              <div className="flex flex-wrap gap-1.5">
                {PACK_METRICS.map((m) => <Chip key={m} on={metrics.includes(m)} onClick={() => toggleMetric(m)}>{t(`hist.m.${m}`)}</Chip>)}
                <Chip on={metrics.includes("cells")} onClick={() => toggleMetric("cells")}>{t("hist.m.cellsChip")}</Chip>
              </div>
              {info?.has_system && (
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="mr-1 text-sm text-muted">{t("hist.master")}</span>
                  {SYSTEM_METRICS.map((m) => <Chip key={m} on={metrics.includes(m)} onClick={() => toggleMetric(m)}>{t(`hist.m.${m}`)}</Chip>)}
                </div>
              )}
            </div>
            <div className="space-y-2">
              <div className="flex items-center gap-3">
                <h3 className="text-sm font-medium">{t("hist.packsTitle")}</h3>
                <button className="text-sm text-charge hover:underline" onClick={() => setHidden(new Set())}>{t("hist.allPacks")}</button>
                <button className="text-sm text-charge hover:underline" onClick={() => setHidden(new Set(packs))}>{t("hist.none")}</button>
              </div>
              <div className="flex flex-wrap gap-1.5">
                {packs.map((a) => (
                  <button key={a} onClick={() => setHidden((h) => { const n = new Set(h); if (n.has(a)) n.delete(a); else n.add(a); return n; })}
                    className={cn("flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-sm tabular-nums",
                      hidden.has(a) ? "border-line text-muted" : "border-ink/30 bg-surface font-medium")}>
                    <span className="h-2.5 w-2.5 rounded-full" style={{ background: hidden.has(a) ? "transparent" : packColor(a, dark), border: `1.5px solid ${packColor(a, dark)}` }} aria-hidden />
                    {no(a)}
                  </button>
                ))}
              </div>
              {wantCells && (
                <label className="flex items-center gap-2 text-sm">
                  <span className="text-muted">{t("hist.cellPack")}</span>
                  <select value={cellPack ?? ""} onChange={(e) => setCellPack(Number(e.target.value))}
                    className="h-8 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge">
                    {packs.map((a) => <option key={a} value={a}>{t("par.pack", { n: no(a) })}</option>)}
                  </select>
                </label>
              )}
            </div>
          </Panel>

          <div className="flex flex-wrap items-center gap-3 text-sm text-muted">
            <span className="text-ink">{dt(from)} – {dt(to)}</span>
            {"preset" in span ? <span>{t("hist.zoomHint")}</span>
              : <Button size="sm" variant="ghost" onClick={reset}><RotateCcw className="h-4 w-4" />{t("hist.reset")}</Button>}
            {busy && <span>{t("hist.loading")}</span>}
            <span className="ml-auto">{t("hist.resolution", { s: data.step < 60_000 ? fmt(data.step / 1000, 0, "s") : fmt(data.step / 60_000, data.step < 600_000 ? 1 : 0, "min") })}</span>
          </div>

          {charts.length === 0 && <p className="text-muted">{t("hist.noChart")}</p>}
          {charts.map((c) => (
            <Panel key={c.id} className="space-y-2 p-4">
              <h2 className="flex items-center gap-1.5 font-display text-lg font-semibold">
                {c.title}{c.unit && <span className="text-sm font-normal text-muted">{c.unit}</span>}
                {c.id === "share" && <><InfoIcon id="value.share" /><span className="text-sm font-normal text-muted">{t("hist.shareHint")}</span></>}
              </h2>
              {c.lines.length ? <Chart x={data.x} lines={c.lines} unit={c.unit} decimals={c.dec} lang={lang} timeLabel={t("hist.time")} syncKey="history" onZoom={zoom} onReset={reset} />
                : <p className="py-6 text-sm text-muted">{t("hist.noPacks")}</p>}
            </Panel>
          ))}

          <Events events={data.events.filter((e) => !hidden.has(e.pack))} lang={lang} tm={tm} onShow={(a, b) => setSpan({ from: a, to: b })} />
        </>
      )}
    </div>
  );
}

function Events({ events, lang, tm, onShow }: { events: Series["events"]; lang: string; tm: (ms: number) => string; onShow: (from: number, to: number) => void }) {
  const { t } = useTranslation();
  const [all, setAll] = useState(false);
  if (!events.length) return null;
  const list = all ? events : events.slice(0, 15);
  return (
    <Panel className="space-y-2 p-4">
      <h2 className="font-display text-lg font-semibold">{t("hist.events", { count: events.length })}</h2>
      <ul className="divide-y divide-line text-sm">
        {list.map((e, i) => {
          const end = e.end ?? e.start;
          return (
            <li key={i} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-1.5">
              {e.severity && <SevIcon severity={e.severity} className="h-4 w-4 shrink-0" />}
              <span className="w-20 tabular-nums text-muted">{t("par.pack", { n: no(e.pack) })}</span>
              <span className="font-medium">{helpTitle(e.key, lang)}</span>
              <InfoIcon id={e.key} />
              <button className="ml-auto tabular-nums text-charge hover:underline" title={t("hist.showEvent")}
                onClick={() => { const pad = Math.max(10 * 60_000, (end - e.start) * 0.5); onShow(e.start - pad, end + pad); }}>
                {tm(e.start)}{e.end === null ? ` – ${t("hist.ongoing")}` : e.end !== e.start ? ` – ${tm(e.end)}` : ""}
              </button>
            </li>
          );
        })}
      </ul>
      {events.length > list.length && <button className="text-sm text-charge hover:underline" onClick={() => setAll(true)}>{t("hist.moreEvents", { count: events.length - list.length })}</button>}
    </Panel>
  );
}

function Chip({ on, onClick, children }: { on: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button onClick={onClick} aria-pressed={on}
      className={cn("flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-sm", on ? "border-charge bg-charge/10 font-medium text-ink" : "border-line text-muted hover:text-ink")}>
      <Tick state={on ? "all" : "none"} />{children}
    </button>
  );
}
