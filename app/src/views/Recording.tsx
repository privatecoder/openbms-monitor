import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Circle, FolderOpen, Square } from "lucide-react";
import { api, type RecordOptions, type RecordingStatus } from "../api";
import { Sheet, SheetContent } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { PackPicker, Segmented, Switch, packSections } from "../components/PackPicker";
import { useGroups } from "../groups";
import { InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";

const INTERVALS = [0, 60, 300] as const;
const OPTIONS_KEY = "recording.options";

/** Backend recording state, refreshed every second while the app is open. */
export function useRecording() {
  const [status, setStatus] = useState<RecordingStatus | null>(null);
  useEffect(() => {
    let alive = true;
    const tick = () => api.recordingStatus().then((s) => alive && setStatus(s)).catch(() => {});
    tick();
    const id = window.setInterval(tick, 1000);
    return () => { alive = false; window.clearInterval(id); };
  }, []);
  return { status, setStatus };
}

const size = (bytes: number) =>
  bytes < 1e6 ? fmt(bytes / 1e3, 0, "kB") : bytes < 1e9 ? fmt(bytes / 1e6, 1, "MB") : fmt(bytes / 1e9, 2, "GB");

const duration = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
};

/** Red dot that breathes while recording. */
export function RecDot({ active, className }: { active: boolean; className?: string }) {
  return (
    <span className={cn("relative flex h-2.5 w-2.5 shrink-0", className)} aria-hidden>
      {active && <span className="rec-pulse absolute inset-0 rounded-full bg-alarm" />}
      <span className={cn("relative h-2.5 w-2.5 rounded-full", active ? "bg-alarm" : "border border-muted")} />
    </span>
  );
}

/** Sidebar entry: opens the recording panel and shows a running recording at a glance. */
export function RecordingEntry({ site, packs, systemAvailable, connected }: { site: string; packs: number[]; systemAvailable: boolean; connected: boolean }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rec = useRecording();
  const s = rec.status;
  return (
    <>
      <button onClick={() => setOpen(true)}
        className="flex w-full items-center justify-between rounded-md px-2 py-1.5 text-sm hover:bg-sunken/60">
        <span className="flex items-center gap-2 font-medium"><RecDot active={!!s?.active} />{t("rec.title")}</span>
        <span className={cn("tabular-nums", s?.active ? "text-alarm" : "text-muted")}>
          {s?.active ? `${duration(Date.now() - s.started_ms)} · ${size(s.bytes)}` : t("rec.off")}
        </span>
      </button>
      <RecordingSheet open={open} onOpenChange={setOpen} rec={rec} site={site} packs={packs} systemAvailable={systemAvailable} connected={connected} />
    </>
  );
}

function readOptions(): RecordOptions {
  try {
    const o = JSON.parse(localStorage.getItem(OPTIONS_KEY) ?? "null") as RecordOptions | null;
    if (o && Array.isArray(o.packs)) return o;
  } catch { /* fall through */ }
  return { packs: [], system: true, interval_s: 0 };
}

function RecordingSheet({ open, onOpenChange, rec, site, packs, systemAvailable, connected }: {
  open: boolean; onOpenChange: (o: boolean) => void; rec: ReturnType<typeof useRecording>;
  site: string; packs: number[]; systemAvailable: boolean; connected: boolean;
}) {
  const { t } = useTranslation();
  const { groups } = useGroups(site);
  const [opts, setOpts] = useState<RecordOptions>(readOptions);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const s = rec.status;
  const active = !!s?.active;
  const all = opts.packs.length === 0;
  const chosen = all ? packs : opts.packs.filter((a) => packs.includes(a));

  const update = (o: RecordOptions) => {
    setOpts(o);
    try { localStorage.setItem(OPTIONS_KEY, JSON.stringify(o)); } catch { /* preview without storage */ }
  };
  const toggle = (list: number[], on: boolean) => {
    const base = new Set(all ? packs : opts.packs);
    list.forEach((a) => (on ? base.add(a) : base.delete(a)));
    const next = [...base].sort((x, y) => x - y);
    // keeping every pack selected means "all", so packs found later are included too
    update({ ...opts, packs: next.length === packs.length && packs.every((a) => base.has(a)) ? [] : next });
  };

  // ~1.6 kB per pack answer; a polling round takes ~0.8 s per pack on the CAN socket bus
  const round = Math.max(2, packs.length * 0.8);
  const perHour = chosen.length * 1600 * (3600 / Math.max(round, opts.interval_s)) + (opts.system && systemAvailable ? 600 * (3600 / Math.max(round, opts.interval_s)) : 0);

  const start = async () => {
    setBusy(true); setErr(null);
    try { rec.setStatus(await api.startRecording({ ...opts, system: opts.system && systemAvailable })); }
    catch (e) { setErr(String(e)); }
    finally { setBusy(false); }
  };
  const stop = async () => {
    setBusy(true);
    try { rec.setStatus(await api.stopRecording()); } catch (e) { setErr(String(e)); } finally { setBusy(false); }
  };
  const fileName = s?.path?.split(/[\\/]/).pop();

  const sections = packSections(groups, packs, t("rec.ungrouped"), t("rec.packs"));

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent title={t("rec.title")}>
        <div className="space-y-7">
          <p className="flex items-center gap-1.5 text-sm text-muted">{t("rec.intro")}<InfoIcon id="topic.recording" /></p>
          {/* status */}
          <section className={cn("rounded-lg border p-4", active ? "border-alarm/40 bg-alarm/5" : "border-line bg-sunken/40")}>
            <div className="flex items-center gap-2.5">
              <RecDot active={active} />
              <span className={cn("font-medium", active && "text-alarm")}>{active ? t("rec.running") : t("rec.stopped")}</span>
              {active && <span className="ml-auto font-display text-lg tabular-nums">{duration(Date.now() - s!.started_ms)}</span>}
            </div>
            {s?.path && (
              <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
                <dt className="text-muted">{t("rec.file")}</dt><dd className="truncate" title={s.path}>{fileName}</dd>
                <dt className="text-muted">{t("rec.lines")}</dt><dd className="tabular-nums">{s.lines.toLocaleString()}</dd>
                <dt className="text-muted">{t("rec.size")}</dt><dd className="tabular-nums">{size(s.bytes)}</dd>
              </dl>
            )}
            {s?.error && <p className="mt-2 text-sm text-alarm">{t("rec.writeError", { err: s.error })}</p>}
            <div className="mt-4 flex gap-2">
              {active
                ? <Button variant="primary" onClick={stop} disabled={busy}><Square className="h-3.5 w-3.5 fill-current" />{t("rec.stop")}</Button>
                : <Button variant="primary" onClick={start} disabled={busy || !connected || chosen.length === 0}><Circle className="h-3.5 w-3.5 fill-alarm text-alarm" />{t("rec.start")}</Button>}
              <Button variant="secondary" onClick={() => api.revealRecording(s?.path ?? null)}><FolderOpen className="h-4 w-4" />{s?.path ? t("rec.reveal") : t("rec.folder")}</Button>
            </div>
            {!connected && !active && <p className="mt-2 text-sm text-muted">{t("rec.needConnection")}</p>}
            {err && <p className="mt-2 text-sm text-alarm">{err}</p>}
          </section>

          <fieldset disabled={active} className={cn("space-y-7", active && "opacity-60")}>
            {active && <p className="text-sm text-muted">{t("rec.lockedHint")}</p>}

            {/* what */}
            <section className="space-y-3">
              <div className="flex items-center justify-between">
                <h3 className="font-medium">{t("rec.what")}</h3>
                <Segmented value={all ? "all" : "pick"} onChange={(v) => update({ ...opts, packs: v === "all" ? [] : [...packs] })}
                  items={[["all", t("rec.allPacks")], ["pick", t("rec.pick")]]} />
              </div>
              {!all && <PackPicker sections={sections} chosen={chosen} onToggle={toggle} count={(a, b) => t("rec.ofCount", { a, b })} />}
              <label className={cn("flex items-start gap-3 rounded-md border border-line p-3 text-sm", !systemAvailable && "opacity-50")}>
                <Switch checked={opts.system && systemAvailable} disabled={!systemAvailable} onChange={(v) => update({ ...opts, system: v })} />
                <span>
                  <span className="block font-medium">{t("rec.system")}</span>
                  <span className="text-muted">{systemAvailable ? t("rec.systemHint") : t("rec.systemUnavailable")}</span>
                </span>
              </label>
            </section>

            {/* how often */}
            <section className="space-y-3">
              <h3 className="font-medium">{t("rec.interval")}</h3>
              <Segmented value={String(opts.interval_s)} onChange={(v) => update({ ...opts, interval_s: Number(v) })}
                items={INTERVALS.map((s) => [String(s), t(`rec.every${s}`)] as [string, string])} />
              <p className="text-sm text-muted">
                {t("rec.estimate", { size: size(perHour), day: size(perHour * 24) })} {opts.interval_s === 0 ? t("rec.everyPollHint", { s: Math.round(round) }) : t("rec.intervalHint")}
              </p>
            </section>

            <p className="text-xs text-muted">{t("rec.format")}</p>
          </fieldset>
        </div>
      </SheetContent>
    </Sheet>
  );
}
