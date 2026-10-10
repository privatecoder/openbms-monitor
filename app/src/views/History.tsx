import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChartLine, FileUp, FolderOpen, RotateCcw } from "lucide-react";
import { api, isDemo, type RecordingFile } from "../api";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { Tick } from "../components/PackPicker";
import { InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";
import { Chart, useDark, type Line } from "../history/Chart";
import { RecordingParser, makeGrid, onGrid, packColor, shares, typicalInterval, type Grid, type PackSeries, type Recording, type Reduce } from "../history/data";

const no = (a: number) => String(a).padStart(2, "0");

type PackMetric = "current" | "share" | "soc" | "voltage" | "cellMax" | "cellMin" | "delta" | "tempMax" | "tempMin";
type SystemMetric = "sysCurrent" | "sysSoc" | "sysVoltage" | "sysCells";
type Metric = PackMetric | SystemMetric | "cells";

/** Per pack metric: unit, decimals, how a bucket is reduced (extremes keep their extreme). */
const PACK: Record<PackMetric, { unit: string; dec: number; how: Reduce }> = {
  current: { unit: "A", dec: 1, how: "mean" },
  share: { unit: "", dec: 2, how: "mean" },
  soc: { unit: "%", dec: 1, how: "mean" },
  voltage: { unit: "V", dec: 2, how: "mean" },
  cellMax: { unit: "V", dec: 3, how: "max" },
  cellMin: { unit: "V", dec: 3, how: "min" },
  delta: { unit: "mV", dec: 0, how: "max" },
  tempMax: { unit: "°C", dec: 1, how: "max" },
  tempMin: { unit: "°C", dec: 1, how: "min" },
};
const PACK_METRICS = Object.keys(PACK) as PackMetric[];
const SYSTEM_METRICS: SystemMetric[] = ["sysCurrent", "sysSoc", "sysVoltage", "sysCells"];

// kept while the app runs, so switching views keeps the open recording
let kept: { name: string; rec: Recording } | null = null;

const size = (bytes: number) =>
  bytes < 1e6 ? fmt(bytes / 1e3, 0, "kB") : bytes < 1e9 ? fmt(bytes / 1e6, 1, "MB") : fmt(bytes / 1e9, 2, "GB");

/** Chunks of text into the parser, yielding between them so the progress shows. */
async function parseChunks(next: () => Promise<Uint8Array | null>, total: number, progress: (f: number) => void) {
  const parser = new RecordingParser(), dec = new TextDecoder();
  let done = 0, last = 0;
  for (let c = await next(); c; c = await next()) {
    parser.push(dec.decode(c, { stream: true }));
    done += c.byteLength;
    if (performance.now() - last > 100) { progress(total ? done / total : 0); last = performance.now(); await new Promise((r) => setTimeout(r)); }
  }
  parser.push(dec.decode());
  return parser.finish();
}

const CHUNK = 4 << 20;
const fromBuffer = (buf: ArrayBuffer, progress: (f: number) => void) => {
  let at = 0;
  return parseChunks(async () => { if (at >= buf.byteLength) return null; const c = new Uint8Array(buf, at, Math.min(CHUNK, buf.byteLength - at)); at += CHUNK; return c; }, buf.byteLength, progress);
};
const fromFile = (f: File, progress: (f: number) => void) => {
  const reader = f.stream().getReader();
  return parseChunks(async () => { const r = await reader.read(); return r.done ? null : r.value; }, f.size, progress);
};

/** Open a recording and look at it over time: per pack, the master's values and the cells of one pack. */
export function History() {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const dark = useDark();
  const [files, setFiles] = useState<RecordingFile[] | null>(null);
  const [open, setOpen] = useState(kept);
  const [loading, setLoading] = useState<{ name: string; f: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [range, setRange] = useState<[number, number] | null>(null);
  const [metrics, setMetrics] = useState<Metric[]>(["current", "soc", "delta", "sysCurrent"]);
  const [hidden, setHidden] = useState<Set<number>>(new Set());
  const [cellPack, setCellPack] = useState<number | null>(null);
  const input = useRef<HTMLInputElement>(null);

  const refresh = () => api.listRecordings().then(setFiles).catch(() => setFiles([]));
  useEffect(() => { refresh(); }, []);

  const show = (name: string, rec: Recording) => {
    kept = { name, rec };
    setOpen(kept); setRange(null); setHidden(new Set());
    setCellPack([...rec.packs.keys()].sort((a, b) => a - b)[0] ?? null);
  };
  const load = async (name: string, read: (p: (f: number) => void) => Promise<Recording>) => {
    setErr(null); setLoading({ name, f: 0 });
    try { show(name, await read((f) => setLoading({ name, f }))); }
    catch (e) { setErr(String(e)); }
    finally { setLoading(null); }
  };

  const rec = open?.rec;
  const packs = useMemo(() => (rec ? [...rec.packs.keys()].sort((a, b) => a - b) : []), [rec]);
  const shown = packs.filter((a) => !hidden.has(a));
  const hasSystem = !!rec?.system.t.length;
  const interval = useMemo(() => (rec ? typicalInterval(rec) : 1000), [rec]);
  const grid: Grid | null = useMemo(() => (rec && rec.to >= rec.from ? makeGrid(range?.[0] ?? rec.from, range?.[1] ?? rec.to, interval) : null), [rec, range, interval]);
  const bridge = grid ? Math.max(3 * interval, grid.step) : 0;

  // every pack metric on the grid, computed once per grid
  const packData = useMemo(() => {
    if (!rec || !grid) return null;
    const col = (p: PackSeries, m: PackMetric): number[] =>
      m === "delta" ? p.cellMax.map((x, i) => (x - p.cellMin[i]) * 1000) : m === "share" ? p.current : p[m];
    const out = new Map<PackMetric, Map<number, (number | null)[]>>();
    for (const m of PACK_METRICS) {
      if (m === "share") continue;
      out.set(m, new Map(packs.map((a) => { const p = rec.packs.get(a)!; return [a, onGrid(p.t, col(p, m), grid, PACK[m].how, bridge)]; })));
    }
    out.set("share", shares(out.get("current")!));
    return out;
  }, [rec, grid, packs, bridge]);

  const toggleMetric = (m: Metric) => setMetrics((ms) => (ms.includes(m) ? ms.filter((x) => x !== m) : [...ms, m]));
  const zoom = (from: number, to: number) => { if (to - from >= interval * 3) setRange([from, to]); };
  const reset = () => setRange(null);

  const charts: { id: Metric; title: string; unit: string; dec: number; lines: Line[] }[] = [];
  if (rec && grid && packData) {
    for (const m of metrics) {
      if (m in PACK) {
        const pm = m as PackMetric, d = packData.get(pm)!;
        charts.push({ id: m, title: t(`hist.m.${m}`), unit: PACK[pm].unit, dec: PACK[pm].dec,
          lines: shown.map((a) => ({ label: no(a), color: packColor(a, dark), values: d.get(a)! })) });
      } else if (m === "cells" && cellPack !== null && rec.packs.has(cellPack)) {
        const p = rec.packs.get(cellPack)!;
        charts.push({ id: m, title: t("hist.m.cells", { pack: no(cellPack) }), unit: "V", dec: 3,
          lines: p.cells.map((c, i) => ({ label: String(i + 1), color: `hsl(${Math.round((i * 360) / p.cells.length)} 60% ${dark ? 64 : 45}%)`, values: onGrid(p.t, c, grid, "mean", bridge), width: 1.25 })) });
      } else if (hasSystem && SYSTEM_METRICS.includes(m as SystemMetric)) {
        const s = rec.system, g = (v: number[], how: Reduce = "mean") => onGrid(s.t, v, grid, how, bridge);
        const ink = dark ? "#e6ebef" : "#18212b", charge = dark ? "#3fb6bf" : "#0e7c86", dis = dark ? "#e0a548" : "#b7791f";
        if (m === "sysCurrent") charts.push({ id: m, title: t("hist.m.sysCurrent"), unit: "A", dec: 1, lines: [
          { label: t("hist.l.current"), color: ink, values: g(s.current) },
          { label: t("hist.l.ccl"), color: charge, values: g(s.ccl, "min"), dash: [6, 4] },
          { label: t("hist.l.dcl"), color: dis, values: g(s.dcl.map((x) => -x), "max"), dash: [6, 4] }] });
        if (m === "sysSoc") charts.push({ id: m, title: t("hist.m.sysSoc"), unit: "%", dec: 1, lines: [{ label: "SOC", color: ink, values: g(s.soc) }] });
        if (m === "sysVoltage") charts.push({ id: m, title: t("hist.m.sysVoltage"), unit: "V", dec: 2, lines: [
          { label: t("hist.l.voltage"), color: ink, values: g(s.voltage) },
          { label: t("hist.l.cvl"), color: charge, values: g(s.cvl, "min"), dash: [6, 4] }] });
        if (m === "sysCells") charts.push({ id: m, title: t("hist.m.sysCells"), unit: "V", dec: 3, lines: [
          { label: t("hist.l.highest"), color: dark ? "#c79be0" : "#8b4fb0", values: g(s.cellMax, "max") },
          { label: t("hist.l.lowest"), color: dark ? "#7fa6e3" : "#3f6fb5", values: g(s.cellMin, "min") }] });
      }
    }
  }

  const dt = (ms: number) => new Intl.DateTimeFormat(lang, { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(ms);
  const tm = (ms: number) => new Intl.DateTimeFormat(lang, { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(ms);
  const dur = (ms: number) => { const m = Math.round(ms / 60000); return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min`; };

  return (
    <div className="view-in mx-auto max-w-7xl space-y-6">
      <header>
        <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight">
          <ChartLine className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("hist.title")}<InfoIcon id="topic.history" />
        </h1>
        <p className="mt-1 max-w-3xl text-muted">{t("hist.intro")}</p>
      </header>

      <Panel className="space-y-3 p-5">
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="primary" onClick={() => input.current?.click()} disabled={!!loading}><FileUp className="h-4 w-4" />{t("hist.openFile")}</Button>
          <input ref={input} type="file" accept=".jsonl,.json,.txt" className="hidden"
            onChange={(e) => { const f = e.target.files?.[0]; if (f) load(f.name, (p) => fromFile(f, p)); e.target.value = ""; }} />
          {!isDemo && <Button variant="secondary" onClick={() => api.revealRecording(null)}><FolderOpen className="h-4 w-4" />{t("rec.folder")}</Button>}
          {loading && <span className="text-sm text-muted">{t("hist.loading", { name: loading.name, pct: Math.round(loading.f * 100) })}</span>}
          {err && <span className="text-sm text-alarm">{t("hist.error", { err })}</span>}
        </div>
        {files && files.length > 0 && (
          <div className="max-h-48 overflow-y-auto rounded-md border border-line">
            {files.map((f) => (
              <button key={f.name} disabled={!!loading} onClick={() => load(f.name, async (p) => fromBuffer(await api.readRecording(f.name), p))}
                className={cn("flex w-full items-center gap-3 border-b border-line px-3 py-1.5 text-left text-sm last:border-b-0 hover:bg-sunken/60",
                  open?.name === f.name && "bg-sunken font-medium")}>
                <span className="truncate">{f.name}</span>
                <span className="ml-auto shrink-0 tabular-nums text-muted">{dt(f.modified_ms)}</span>
                <span className="w-20 shrink-0 text-right tabular-nums text-muted">{size(f.bytes)}</span>
              </button>
            ))}
          </div>
        )}
        {files && files.length === 0 && !isDemo && <p className="text-sm text-muted">{t("hist.noFiles")}</p>}
      </Panel>

      {!rec ? (
        <div className="flex max-w-md flex-col gap-3 py-10 text-lg text-muted">
          <ChartLine className="h-8 w-8" strokeWidth={1.5} aria-hidden />{t("hist.empty")}
        </div>
      ) : rec.to < rec.from ? (
        <p className="text-alarm">{t("hist.noData", { name: open!.name })}</p>
      ) : (
        <>
          <div className="flex flex-wrap items-baseline gap-x-6 gap-y-1 text-sm">
            <span className="font-medium">{open!.name}</span>
            <span className="text-muted">{dt(rec.from)} – {tm(rec.to)} ({dur(rec.to - rec.from)})</span>
            <span className="text-muted">{t("hist.packs", { count: packs.length })}{hasSystem ? `, ${t("hist.withSystem")}` : ""}</span>
            {rec.start && <span className="text-muted">{rec.start.interval_s ? t("hist.every", { s: rec.start.interval_s }) : t("hist.everyPoll")}</span>}
            {rec.errors + rec.skipped > 0 && <span className="text-discharge">{t("hist.gaps", { e: rec.errors, s: rec.skipped })}</span>}
          </div>

          <Panel className="space-y-4 p-5">
            <div className="space-y-2">
              <h3 className="text-sm font-medium">{t("hist.show")}</h3>
              <div className="flex flex-wrap gap-1.5">
                {PACK_METRICS.map((m) => <Chip key={m} on={metrics.includes(m)} onClick={() => toggleMetric(m)}>{t(`hist.m.${m}`)}</Chip>)}
                <Chip on={metrics.includes("cells")} onClick={() => toggleMetric("cells")}>{t("hist.m.cellsChip")}</Chip>
              </div>
              {hasSystem && (
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="mr-1 text-sm text-muted">{t("hist.master")}</span>
                  {SYSTEM_METRICS.map((m) => <Chip key={m} on={metrics.includes(m)} onClick={() => toggleMetric(m)}>{t(`hist.m.${m}`)}</Chip>)}
                </div>
              )}
            </div>
            <div className="space-y-2">
              <div className="flex items-center gap-3">
                <h3 className="text-sm font-medium">{t("hist.packsTitle")}</h3>
                <button className="text-sm text-charge hover:underline" onClick={() => setHidden(new Set())}>{t("hist.all")}</button>
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
              {metrics.includes("cells") && (
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
            {range ? (
              <>
                <span className="text-ink">{t("hist.zoomed", { from: tm(range[0]), to: tm(range[1]) })}</span>
                <Button size="sm" variant="ghost" onClick={reset}><RotateCcw className="h-4 w-4" />{t("hist.reset")}</Button>
              </>
            ) : <span>{t("hist.zoomHint")}</span>}
            {grid && <span className="ml-auto">{t("hist.resolution", { s: fmt(grid.step / 1000, grid.step < 10000 ? 1 : 0, "s") })}</span>}
          </div>

          {charts.length === 0 && <p className="text-muted">{t("hist.noChart")}</p>}
          {charts.map((c) => (
            <Panel key={c.id} className="space-y-2 p-4">
              <h2 className="flex items-center gap-1.5 font-display text-lg font-semibold">
                {c.title}{c.unit && <span className="text-sm font-normal text-muted">{c.unit}</span>}
                {c.id === "share" && <InfoIcon id="value.share" />}{c.id === "share" && <span className="text-sm font-normal text-muted">{t("hist.shareHint")}</span>}
              </h2>
              {c.lines.length ? <Chart x={grid!.x} lines={c.lines} unit={c.unit} decimals={c.dec} lang={lang} timeLabel={t("hist.time")} syncKey="history" onZoom={zoom} onReset={reset} />
                : <p className="py-6 text-sm text-muted">{t("hist.noPacks")}</p>}
            </Panel>
          ))}
        </>
      )}
    </div>
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
