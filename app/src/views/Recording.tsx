import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChartLine, FolderOpen } from "lucide-react";
import { api, isDemo, type HistorySettings, type HistoryStatus } from "../api";
import { Sheet, SheetContent } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { PackPicker, Segmented, Switch, packSections } from "../components/PackPicker";
import { useGroups } from "../groups";
import { InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";

const INTERVALS = [0, 60, 300] as const;
const FULL_DAYS = [7, 30, 90, 365] as const;
const MINUTE_DAYS = [0, 365, 90] as const;

/** Recording state of the backend, refreshed every 2 s while the app is open. */
export function useHistoryStatus() {
  const [status, setStatus] = useState<HistoryStatus | null>(null);
  useEffect(() => {
    let alive = true;
    const tick = () => api.historyStatus().then((s) => alive && setStatus(s)).catch(() => {});
    tick();
    const id = window.setInterval(tick, 2000);
    return () => { alive = false; window.clearInterval(id); };
  }, []);
  return { status, setStatus };
}

export const size = (bytes: number) =>
  bytes < 1e6 ? fmt(bytes / 1e3, 0, "kB") : bytes < 1e9 ? fmt(bytes / 1e6, 1, "MB") : fmt(bytes / 1e9, 2, "GB");

/** Red dot that breathes while recording. */
export function RecDot({ active, className }: { active: boolean; className?: string }) {
  return (
    <span className={cn("relative flex h-2.5 w-2.5 shrink-0", className)} aria-hidden>
      {active && <span className="rec-pulse absolute inset-0 rounded-full bg-alarm" />}
      <span className={cn("relative h-2.5 w-2.5 rounded-full", active ? "bg-alarm" : "border border-muted")} />
    </span>
  );
}

/** Sidebar entry: opens the recording settings and shows at a glance whether the app records. */
export function RecordingEntry({ site, packs, systemAvailable, connected, onOpenHistory }: {
  site: string; packs: number[]; systemAvailable: boolean; connected: boolean; onOpenHistory: () => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rec = useHistoryStatus();
  const s = rec.status;
  const on = !!s?.settings.enabled;
  return (
    <>
      <button onClick={() => setOpen(true)}
        className="flex w-full items-center justify-between rounded-md px-2 py-1.5 text-sm hover:bg-sunken/60">
        <span className="flex items-center gap-2 font-medium"><RecDot active={!!s?.active} />{t("rec.title")}</span>
        <span className={cn("tabular-nums", s?.active ? "text-alarm" : "text-muted")}>
          {!on ? t("rec.off") : s?.active ? t("rec.onShort") : t("rec.waiting")}
        </span>
      </button>
      <RecordingSheet open={open} onOpenChange={setOpen} rec={rec} site={site} packs={packs} systemAvailable={systemAvailable} connected={connected}
        onOpenHistory={() => { setOpen(false); onOpenHistory(); }} />
    </>
  );
}

function RecordingSheet({ open, onOpenChange, rec, site, packs, systemAvailable, connected, onOpenHistory }: {
  open: boolean; onOpenChange: (o: boolean) => void; rec: ReturnType<typeof useHistoryStatus>;
  site: string; packs: number[]; systemAvailable: boolean; connected: boolean; onOpenHistory: () => void;
}) {
  const { t } = useTranslation();
  const { groups } = useGroups(site);
  const [err, setErr] = useState<string | null>(null);
  const s = rec.status;
  if (!s) return null;
  const opts = s.settings;
  const all = opts.packs.length === 0;
  const chosen = all ? packs : opts.packs.filter((a) => packs.includes(a));

  const update = async (next: HistorySettings) => {
    setErr(null);
    rec.setStatus({ ...s, settings: next });
    try { rec.setStatus(await api.setHistorySettings(next)); } catch (e) { setErr(String(e)); }
  };
  const toggle = (list: number[], on: boolean) => {
    const base = new Set(all ? packs : opts.packs);
    list.forEach((a) => (on ? base.add(a) : base.delete(a)));
    const next = [...base].sort((x, y) => x - y);
    // keeping every pack selected means "all", so packs found later are included too
    update({ ...opts, packs: next.length === packs.length && packs.every((a) => base.has(a)) ? [] : next });
  };

  // ~125 bytes per pack row in the database (measured with 12 packs, index included); a polling round takes ~0.8 s per pack on the CAN socket bus
  const round = Math.max(2, packs.length * 0.8);
  const nPacks = chosen.length || 1;
  const perHour = (nPacks * 125 + (opts.system && systemAvailable ? 60 : 0)) * (3600 / Math.max(round, opts.interval_s));
  const minutesPerYear = (nPacks * 125 + 60) * 60 * 24 * 365;
  const sections = packSections(groups, packs, t("rec.ungrouped"), t("rec.packs"));

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent title={t("rec.title")}>
        <div className="space-y-7">
          <p className="flex items-center gap-1.5 text-sm text-muted">{t("rec.intro")}<InfoIcon id="topic.recording" /></p>

          <section className={cn("rounded-lg border p-4", s.active ? "border-alarm/40 bg-alarm/5" : "border-line bg-sunken/40")}>
            <label className="flex cursor-pointer items-start gap-3">
              <Switch checked={opts.enabled} onChange={(v) => update({ ...opts, enabled: v })} />
              <span>
                <span className="block font-medium">{t("rec.enable")}</span>
                <span className={cn("flex items-center gap-2 text-sm", s.active ? "text-alarm" : "text-muted")}>
                  <RecDot active={s.active} />
                  {s.active ? t("rec.running") : opts.enabled ? t("rec.waitingLong") : t("rec.stopped")}
                </span>
              </span>
            </label>
            <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-muted">{t("rec.database")}</dt><dd className="tabular-nums">{size(s.bytes)}</dd>
              {s.rows > 0 && <><dt className="text-muted">{t("rec.rows")}</dt><dd className="tabular-nums">{s.rows.toLocaleString()}</dd></>}
            </dl>
            {s.error && <p className="mt-2 text-sm text-alarm">{t("rec.writeError", { err: s.error })}</p>}
            {err && <p className="mt-2 text-sm text-alarm">{err}</p>}
            <div className="mt-4 flex flex-wrap gap-2">
              <Button variant="secondary" onClick={onOpenHistory}><ChartLine className="h-4 w-4" />{t("rec.openHistory")}</Button>
              {!isDemo && <Button variant="ghost" onClick={() => api.revealHistory(s.path)}><FolderOpen className="h-4 w-4" />{t("rec.reveal")}</Button>}
            </div>
            {opts.enabled && !connected && <p className="mt-2 text-sm text-muted">{t("rec.needConnection")}</p>}
          </section>

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

          <section className="space-y-3">
            <h3 className="font-medium">{t("rec.interval")}</h3>
            <Segmented value={String(opts.interval_s)} onChange={(v) => update({ ...opts, interval_s: Number(v) })}
              items={INTERVALS.map((x) => [String(x), t(`rec.every${x}`)] as [string, string])} />
            <p className="text-sm text-muted">
              {t("rec.estimate", { size: size(perHour), day: size(perHour * 24) })} {opts.interval_s === 0 ? t("rec.everyPollHint", { s: Math.round(round) }) : t("rec.intervalHint")}
            </p>
          </section>

          <section className="space-y-3">
            <h3 className="font-medium">{t("rec.keep")}</h3>
            <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
              <span className="text-muted">{t("rec.keepFull")}</span>
              <Segmented value={String(opts.keep_full_days)} onChange={(v) => update({ ...opts, keep_full_days: Number(v) })}
                items={FULL_DAYS.map((d) => [String(d), t("rec.days", { count: d })] as [string, string])} />
            </div>
            <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
              <span className="text-muted">{t("rec.keepMinutes")}</span>
              <Segmented value={String(opts.keep_minutes_days)} onChange={(v) => update({ ...opts, keep_minutes_days: Number(v) })}
                items={MINUTE_DAYS.map((d) => [String(d), d === 0 ? t("rec.forever") : t("rec.days", { count: d })] as [string, string])} />
            </div>
            <p className="text-sm text-muted">{t("rec.keepHint", { year: size(minutesPerYear) })}</p>
          </section>
        </div>
      </SheetContent>
    </Sheet>
  );
}
