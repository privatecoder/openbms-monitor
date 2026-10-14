import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Archive, ArchiveRestore, Building2, ChevronDown, ChevronRight, FolderOpen, Merge, Pencil, Plus, Trash2 } from "lucide-react";
import { api, isDemo, type Installation, type InstallationOverview, type PlaceView } from "../api";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { errorKey, mergeSuggestion, nameCheck } from "../maint";
import type { Maint } from "../maintStore";
import { reloadSettings } from "../installData";
import { useCellDb } from "../cells/store";
import { cellName } from "./CellAssign";

const input = "h-8 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";
const packNo = (a: number) => String(a).padStart(2, "0");
const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/** The installations: everything the app keeps about a bank belongs to one (recording, boards, cell types, groups and
 *  pairs, diagnosis, parameter states, firmware logs). Rename, merge, archive, delete; per installation its packs. */
export function Installations({ maint, connected, onParameters }: {
  maint: Maint;
  connected: boolean;
  /** open the Parameters view with the stored state of this installation */
  onParameters: (id: string) => void;
}) {
  const { t } = useTranslation();
  const [archived, setArchived] = useState(false);
  const [open, setOpen] = useState<string | null>(maint.installation);
  const [unassigned, setUnassigned] = useState<string[]>([]);
  useEffect(() => { void maint.loadLists(); api.logsUnassigned().then(setUnassigned).catch(() => setUnassigned([])); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const nArchived = maint.installations.filter((i) => i.archived).length;
  const list = maint.installations.filter((i) => archived || !i.archived);

  return (
    <div className="view-in mx-auto max-w-5xl space-y-6">
      <header>
        <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight">
          <Building2 className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("inst.title")}<InfoIcon id="topic.installations" />
        </h1>
        <p className="mt-1 max-w-3xl text-muted">{t("inst.intro")}</p>
      </header>
      {isDemo ? <p className="text-muted">{t("inst.demo")}</p> : <>
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="secondary" onClick={() => maint.setNaming(true)}><Plus className="h-4 w-4" />{t("maint.picker.new")}</Button>
          {nArchived > 0 && (
            <label className="ml-auto flex cursor-pointer items-center gap-2 text-sm">
              <input type="checkbox" className="h-4 w-4 accent-[var(--charge)]" checked={archived} onChange={(e) => setArchived(e.target.checked)} />
              {t("inst.showArchived", { count: nArchived })}
            </label>
          )}
        </div>
        {maint.loaded && !list.length && <p className="text-muted">{t("inst.none")}</p>}
        <div className="space-y-3">
          {list.map((i) => (
            <Card key={i.id} inst={i} maint={maint} connected={connected} expanded={open === i.id}
              onToggle={() => setOpen(open === i.id ? null : i.id)} onParameters={onParameters} />
          ))}
        </div>
        {unassigned.length > 0 && (
          <Panel className="space-y-2 p-5">
            <h2 className="font-display text-lg font-semibold">{t("inst.logsWithout")}</h2>
            <p className="text-sm text-muted">{t("inst.logsWithoutHint")}</p>
            <ul className="space-y-1 text-sm">
              {unassigned.slice(0, 50).map((p) => (
                <li key={p} className="flex items-center gap-2">
                  <span className="font-mono text-xs">{fileName(p)}</span>
                  <button className="text-muted hover:text-ink" title={t("fw.showLog")} aria-label={t("fw.showLog")} onClick={() => api.revealFirmwareLogs(p)}><FolderOpen className="h-4 w-4" /></button>
                </li>
              ))}
            </ul>
          </Panel>
        )}
      </>}
    </div>
  );
}

function Card({ inst, maint, connected, expanded, onToggle, onParameters }: {
  inst: Installation; maint: Maint; connected: boolean; expanded: boolean; onToggle: () => void; onParameters: (id: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const [view, setView] = useState<InstallationOverview | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [target, setTarget] = useState("");
  const [deleting, setDeleting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const others = maint.installations.filter((o) => o.id !== inst.id);
  const suggestion = mergeSuggestion(inst, maint.installations);
  const chosen = maint.installation === inst.id;
  // the connected installation cannot be merged away or deleted: its answers still arrive
  const live = chosen && connected;
  const date = (ms: number) => new Date(ms).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" });

  const load = () => api.installationOverview(inst.id).then(setView).catch((e) => setError(String(e)));
  useEffect(() => { if (expanded || deleting) void load(); }, [expanded, deleting]); // eslint-disable-line react-hooks/exhaustive-deps

  const act = async (f: () => Promise<unknown>, after?: string) => {
    setBusy(true); setError(null); setDone(null);
    try { await f(); if (after) setDone(after); reloadSettings(); await maint.loadLists(); }
    catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
    finally { setBusy(false); }
  };
  const rename = () => {
    const bad = nameCheck(renaming ?? "", maint.installations, inst.id);
    if (bad) { setError(t(bad)); return; }
    void act(() => api.installationRename(inst.id, renaming!.trim())).then(() => setRenaming(null));
  };
  const merge = (to: Installation) => act(() => api.installationMerge(inst.id, to.id).then(() => { if (chosen) maint.switchTo(to.id); }), t("inst.merged", { name: to.name }));

  return (
    <Panel className={cn("space-y-3 p-4", chosen && "border-charge/60")}>
      <div className="flex flex-wrap items-start gap-3">
        <button onClick={onToggle} className="mt-0.5 text-muted hover:text-ink" aria-label={t("inst.packs")} aria-expanded={expanded}>
          {expanded ? <ChevronDown className="h-5 w-5" /> : <ChevronRight className="h-5 w-5" />}
        </button>
        <div className="min-w-0 flex-1 space-y-0.5">
          {renaming !== null ? (
            <form className="flex flex-wrap items-center gap-2" onSubmit={(e) => { e.preventDefault(); rename(); }}>
              <input className={input} value={renaming} autoFocus onChange={(e) => setRenaming(e.target.value)} aria-label={t("maint.setup.name")} />
              <Button size="sm" variant="primary" type="submit" disabled={busy}>{t("maint.setup.save")}</Button>
              <Button size="sm" variant="ghost" type="button" onClick={() => setRenaming(null)}>{t("maint.setup.cancel")}</Button>
            </form>
          ) : (
            <p className="flex flex-wrap items-center gap-2 font-display text-lg font-semibold">
              {inst.name}
              {chosen && <Badge tone="charge">{t(connected ? "inst.connected" : "inst.chosen")}</Badge>}
              {inst.legacy != null && <Badge tone="discharge" title={t("inst.legacyHint", { endpoint: inst.legacy })}>{t("inst.legacy")}</Badge>}
              {inst.archived && <Badge tone="muted">{t("inst.archived")}</Badge>}
            </p>
          )}
          <p className="text-sm text-muted">
            {[inst.endpoints.length || inst.control ? t("inst.endpoints", { list: [inst.control ? (inst.control.kind === "tcp" ? inst.control.addr : inst.control.path) : null, ...inst.endpoints.map((k) => k.replace(/^(tcp|serial):/, ""))].filter(Boolean).join(", ") }) : t("inst.noEndpoints"),
              inst.master_serial ? t("inst.masterSerial", { serial: inst.master_serial }) : null,
              inst.last_connect ? t("inst.lastConnect", { at: date(inst.last_connect) }) : null].filter(Boolean).join(" · ")}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-1">
          {!chosen && !connected && <Button size="sm" variant="secondary" onClick={() => maint.switchTo(inst.id)}>{t("inst.choose")}</Button>}
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRenaming(inst.name)} title={t("inst.rename")} aria-label={t("inst.rename")}><Pencil className="h-4 w-4" /></Button>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => act(() => api.installationArchive(inst.id, !inst.archived))} title={t(inst.archived ? "inst.unarchive" : "inst.archive")} aria-label={t(inst.archived ? "inst.unarchive" : "inst.archive")}>
            {inst.archived ? <ArchiveRestore className="h-4 w-4" /> : <Archive className="h-4 w-4" />}
          </Button>
          <Button size="sm" variant="ghost" disabled={busy || live} onClick={() => setDeleting(!deleting)} title={t("inst.delete")} aria-label={t("inst.delete")}><Trash2 className="h-4 w-4" /></Button>
        </div>
      </div>

      {suggestion && (
        <div className="flex flex-wrap items-center gap-3 rounded-md border border-discharge p-3 text-sm">
          <p className="flex-1">{t("inst.suggest", { endpoint: inst.legacy, name: suggestion.name })}</p>
          <Button size="sm" variant="secondary" disabled={busy || live} onClick={() => merge(suggestion)}><Merge className="h-4 w-4" />{t("inst.mergeWith", { name: suggestion.name })}</Button>
        </div>
      )}

      {deleting && (
        <div className="space-y-2 rounded-md border border-alarm/50 p-3 text-sm">
          <p className="font-medium">{t("inst.deleteTitle", { name: inst.name })}</p>
          {view ? (
            <ul className="list-disc pl-5">
              <li>{t("inst.goes.recording", { count: view.recorded })}</li>
              <li>{t("inst.goes.boards", { count: view.counts.boards })}</li>
              <li>{t("inst.goes.snapshots", { count: view.counts.snapshots })}</li>
              <li>{t("inst.goes.cells", { count: view.counts.places })}</li>
              {view.counts.layout && <li>{t("inst.goes.layout")}</li>}
              {view.counts.diagnosis && <li>{t("inst.goes.diagnosis")}</li>}
            </ul>
          ) : <p className="text-muted">{t("hist.loading")}</p>}
          <p className="text-muted">{t("inst.deleteKeeps")}</p>
          <div className="flex gap-2">
            <Button size="sm" variant="primary" disabled={busy || !view} onClick={() => act(() => api.installationDelete(inst.id)).then(() => setDeleting(false))}>{t("inst.deleteYes")}</Button>
            <Button size="sm" variant="secondary" onClick={() => setDeleting(false)}>{t("maint.setup.cancel")}</Button>
          </div>
        </div>
      )}

      {expanded && (
        <div className="space-y-3">
          {others.length > 0 && (
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <span className="text-muted">{t("inst.mergeInto")}</span>
              <select className={input} value={target} disabled={busy || live} onChange={(e) => setTarget(e.target.value)}>
                <option value="">{t("inst.mergePick")}</option>
                {others.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}
              </select>
              <Button size="sm" variant="secondary" disabled={busy || live || !target} onClick={() => { const to = others.find((o) => o.id === target); if (to) void merge(to); }}>
                <Merge className="h-4 w-4" />{t("inst.merge")}
              </Button>
              <InfoIcon id="topic.installations" />
            </div>
          )}
          {view && <p className="text-sm text-muted">{view.recorded ? t("inst.recorded", { count: view.recorded, from: date(view.recorded_from ?? 0), to: date(view.recorded_to ?? 0) }) : t("inst.notRecorded")}</p>}
          {view && <Places places={view.places} logDir={view.log_dir} onParameters={() => onParameters(inst.id)} />}
        </div>
      )}
      {done && <p className="text-sm text-ok">{done}</p>}
      {error && <p className="text-sm text-alarm">{error}</p>}
    </Panel>
  );
}

function Badge({ tone, title, children }: { tone: "charge" | "discharge" | "muted"; title?: string; children: React.ReactNode }) {
  return <span title={title} className={cn("rounded px-1.5 py-0.5 font-sans text-xs font-medium", tone === "charge" ? "bg-charge/15 text-charge" : tone === "discharge" ? "bg-discharge/15 text-discharge" : "bg-sunken text-muted")}>{children}</span>;
}

/** Per place: the board now (and the one before a swap), the cell type, the newest parameter state, the last update. */
function Places({ places, logDir, onParameters }: { places: PlaceView[]; logDir: string | null; onParameters: () => void }) {
  const { t, i18n } = useTranslation();
  const db = useCellDb();
  const date = (ms: number) => new Date(ms).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" });
  const sep = logDir?.includes("\\") ? "\\" : "/";
  if (!places.length) return <p className="text-sm text-muted">{t("inst.noPlaces")}</p>;
  const cell = (json: string | null) => {
    if (!json) return "–";
    try {
      const a = JSON.parse(json) as { cell: string; parallel: number };
      const c = db.entries.find((e) => e.cell.id === a.cell)?.cell;
      return `${c ? cellName(c) : a.cell} ${a.parallel}P`;
    } catch { return "?"; }
  };
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-line text-left text-muted">
            <th className="py-1.5 pr-4 font-normal">{t("inst.col.pack")}</th>
            <th className="py-1.5 pr-4 font-normal">{t("inst.col.board")}</th>
            <th className="py-1.5 pr-4 font-normal">{t("inst.col.cell")}</th>
            <th className="py-1.5 pr-4 font-normal">{t("inst.col.params")}</th>
            <th className="py-1.5 font-normal">{t("inst.col.flash")}</th>
          </tr>
        </thead>
        <tbody>
          {places.map((p) => {
            const [now, before] = p.boards;
            return (
              <tr key={p.address} className="border-b border-line align-top">
                <td className="py-1.5 pr-4 tabular-nums">{packNo(p.address)}</td>
                <td className="py-1.5 pr-4">
                  {now ? <span className="font-mono text-xs" title={now.battery_serial ? t("inst.battery", { serial: now.battery_serial }) : undefined}>{now.bms_serial}</span> : <span className="text-muted">{t("inst.boardUnclear")}</span>}
                  {before && <div className="text-xs text-discharge">{t("inst.previousBoard", { serial: before.bms_serial, at: date(before.last_seen) })}</div>}
                </td>
                <td className="py-1.5 pr-4">{cell(p.cell_type)}</td>
                <td className="py-1.5 pr-4">
                  {p.snapshot ? (
                    <button className="text-left text-charge hover:underline" onClick={onParameters}>
                      {date(p.snapshot.at)} · {t(`par.kind.${p.snapshot.kind}`)}
                      {p.snapshot.board === "previous" && <span className="block text-xs text-discharge">{t("par.previousBoard", { serial: p.snapshot.bms_serial ?? "" })}</span>}
                      {p.snapshot.board === "unclear" && <span className="block text-xs text-muted">{t("par.boardUnclearShort")}</span>}
                    </button>
                  ) : "–"}
                </td>
                <td className="py-1.5">
                  {p.flash ? (
                    <span className="flex flex-wrap items-center gap-x-2">
                      {date(p.flash.at)} · {p.flash.release ?? "?"} ·
                      <span className={p.flash.result === "ok" ? "text-ok" : p.flash.result ? "text-alarm" : "text-muted"}>{t(`inst.flash.${p.flash.result ?? "open"}`)}</span>
                      {logDir && <button className="text-muted hover:text-ink" title={t("fw.showLog")} aria-label={t("fw.showLog")} onClick={() => api.revealFirmwareLogs(`${logDir}${sep}${p.flash!.file}`)}><FolderOpen className="h-4 w-4" /></button>}
                    </span>
                  ) : "–"}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
