import { useCallback, useEffect, useState } from "react";
import { actLocked } from "../locks";
import { useTranslation } from "react-i18next";
import { Archive, Eraser, FolderOpen, HardDrive, Trash2 } from "lucide-react";
import { api, type InstallationData, type StorageOverview } from "../api";
import { Panel } from "../components/ui/card";
import { Button } from "../components/ui/button";
import { updateErrorKey } from "../update";
import { InfoIcon } from "../help";
import { size } from "./Recording";

type Kind = "clear" | "backups" | "empty";

const date = (ms: number) => new Date(ms).toLocaleDateString();
const KNOWN = ["backup-inside-data", "open-import", "maint-missing", "history-preparing"];

/** The text of a refusal or error of a storage command. */
function errorText(t: (k: string, o?: Record<string, unknown>) => string, e: unknown): string {
  const msg = String(e);
  if (msg.startsWith("busy:")) return t(updateErrorKey(msg));
  if (KNOWN.includes(msg)) return t(`about.storage.err.${msg}`);
  return t("about.storage.d.failed", { err: msg });
}

/** About → Data & storage: what the data folder holds and freeing space by hand (`storage.rs`). */
export function StoragePanel() {
  const { t } = useTranslation();
  const [o, setO] = useState<StorageOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Kind | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const load = useCallback(() => api.storageOverview().then((v) => { setO(v); return v; }, (e) => { setError(String(e)); return null; }), []);
  useEffect(() => { void load(); }, [load]);

  const bytesOf = (key: string) => o?.categories.find((c) => c.key === key)?.bytes ?? 0;
  const finished = async (before: number) => {
    const after = await load();
    if (after) setDone(t("about.storage.before", { a: size(before), b: size(after.total) }));
  };

  return (
    <Panel className="flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 font-medium"><HardDrive className="h-4 w-4 text-muted" aria-hidden />{t("about.storage.title")}<InfoIcon id="topic.storage" /></div>
          <p className="text-sm text-muted">{t("about.storage.hint")}</p>
          {o?.dir && <code className="mt-1 block break-all rounded bg-sunken px-1.5 py-0.5 text-xs">{o.dir}</code>}
        </div>
        <Button size="sm" variant="ghost" onClick={() => api.revealHistory(null).catch((e) => setError(String(e)))}>
          <FolderOpen className="h-4 w-4" />{t("about.storage.show")}
        </Button>
      </div>

      {o && (
        <>
          <p className="text-sm tabular-nums">
            <span className="font-medium">{t("about.storage.total", { size: size(o.total) })}</span>
            {o.free != null && <span className="text-muted"> · {t("about.storage.free", { size: size(o.free) })}</span>}
          </p>
          <dl className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-6 gap-y-1 text-sm">
            {o.categories.filter((c) => c.bytes > 0 || c.key === "recording").map((c) => (
              <div key={c.key} className="contents">
                <dt className="min-w-0">
                  {t(`about.storage.cat.${c.key}`)}
                  {c.key === "recording" && (
                    <span className="block text-xs text-muted">
                      {o.recording_span && o.recording_rows > 0
                        ? t("about.storage.rows", { rows: o.recording_rows.toLocaleString(), from: date(o.recording_span[0]), to: date(o.recording_span[1]) })
                        : t("about.storage.noRecording")}
                    </span>
                  )}
                  {c.key === "installations" && o.bms_records > 0 && <span className="block text-xs text-muted">{t("about.storage.bms", { n: o.bms_records.toLocaleString() })}</span>}
                </dt>
                <dd className="text-right tabular-nums">{size(c.bytes)}</dd>
              </div>
            ))}
          </dl>
          {o.reclaimable > 0 && <p className="text-sm text-discharge">{t("about.storage.reclaimable", { size: size(o.reclaimable) })}</p>}
          {o.job?.state === "requested" && <p className="text-sm text-discharge">{t("about.storage.pending")}</p>}
          {o.job && o.job.state !== "requested" && (
            <div className="space-y-2 text-sm text-alarm">
              <p>{t("about.storage.emptyError")} <span className="block text-xs text-muted">{o.job.error}</span></p>
              <div className="flex flex-wrap gap-2">
                <Button size="sm" variant="secondary" onClick={() => api.storageEmptyRetry().catch((e) => setError(errorText(t, e)))}>{t("about.storage.retry")}</Button>
                <Button size="sm" variant="ghost" onClick={() => api.storageEmptyDrop().then(() => load(), (e) => setError(errorText(t, e)))}>{t("about.storage.dropJob")}</Button>
              </div>
            </div>
          )}
          {done && <p className="text-sm text-ok">{done}</p>}
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant="secondary" onClick={() => { setDone(null); setDialog("clear"); }}><Eraser className="h-4 w-4" />{t("about.storage.clear")}</Button>
            {bytesOf("backups") > 0 && <Button size="sm" variant="ghost" onClick={() => { setDone(null); setDialog("backups"); }}><Archive className="h-4 w-4" />{t("about.storage.backups")}</Button>}
            <Button size="sm" variant="ghost" onClick={() => { setDone(null); setDialog("empty"); }}><Trash2 className="h-4 w-4" />{t("about.storage.empty")}</Button>
          </div>
        </>
      )}
      {error && <p className="text-sm text-alarm">{error}</p>}
      {o && dialog && (
        <DeleteDialog kind={dialog} overview={o} onClose={() => setDialog(null)}
          onDone={(before) => { setDialog(null); void finished(before); }} />
      )}
    </Panel>
  );
}

/** One dialog per action: the scope named, then back up and delete / delete without backup / cancel. */
function DeleteDialog({ kind, overview: o, onClose, onDone }: {
  kind: Kind; overview: StorageOverview; onClose: () => void; onDone: (before: number) => void;
}) {
  const { t } = useTranslation();
  const withRecording = o.installations.filter((i) => i.recording_rows + i.recording_events > 0);
  const [which, setWhich] = useState<string>("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const bytes = (key: string) => o.categories.find((c) => c.key === key)?.bytes ?? 0;
  // the installations a backup takes (the backend chooses them again when it runs)
  const targets: InstallationData[] = o.installations.filter((i) => i.has_data && (kind !== "clear" || !which || i.id === which));

  const run = async (backup: boolean) => {
    setWorking(true); setError(null);
    try {
      let folder: string | null = null;
      if (backup) {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const picked = await open({ directory: true, multiple: false, title: t("about.storage.d.pick") });
        if (typeof picked !== "string") { setWorking(false); return; }
        folder = picked;
      }
      if (kind === "clear") await api.storageClear(which || null, folder);
      else if (kind === "backups") await api.storageDeleteBackups();
      else await api.storageEmpty(folder);
      onDone(o.total);
    } catch (e) {
      setError(errorText(t, e));
      setWorking(false);
    }
  };

  const title = t(`about.storage.d.${kind === "clear" ? "clearTitle" : kind === "backups" ? "backupsTitle" : "emptyTitle"}`);
  const scopeBytes = kind === "clear" ? (which ? null : bytes("recording")) : null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-ink/20 p-4">
      <div role="dialog" aria-modal="true" aria-labelledby="storage-dialog-title" className="w-full max-w-lg space-y-3 rounded-md border border-line bg-surface p-5 text-sm">
        <h2 id="storage-dialog-title" className="font-display text-lg font-semibold">{title}</h2>
        {kind === "clear" && (
          <>
            <label className="block space-y-1">{t("about.storage.d.clearWhich")}
              <select className="h-9 w-full rounded-md border border-line bg-surface px-2 text-sm" value={which} onChange={(e) => setWhich(e.target.value)}>
                <option value="">{t("about.storage.all")}</option>
                {withRecording.map((i) => <option key={i.id} value={i.id}>{i.name} ({i.recording_rows.toLocaleString()})</option>)}
              </select>
            </label>
            <p>{t("about.storage.d.clearText", { size: scopeBytes != null ? size(scopeBytes) : "…" })}</p>
          </>
        )}
        {kind === "backups" && <p>{t("about.storage.d.backupsText", { size: size(bytes("backups")) })}</p>}
        {kind === "empty" && (
          <>
            <p>{t("about.storage.d.emptyText")}</p>
            <p className="text-discharge">{t("about.storage.d.emptyExports")}</p>
            <Button size="sm" variant="ghost" onClick={() => api.revealHistory(null).catch((e) => setError(String(e)))}><FolderOpen className="h-4 w-4" />{t("about.storage.show")}</Button>
          </>
        )}
        {kind !== "backups" && targets.length > 0 && <p className="text-muted">{t("about.storage.d.backupText")}</p>}
        {error && <p className="text-alarm">{error}</p>}
        <div className="flex flex-wrap justify-end gap-2">
          <Button type="button" variant="secondary" size="sm" disabled={actLocked(!!working, false)} onClick={onClose}>{t("about.storage.d.cancel")}</Button>
          {kind === "backups" ? (
            <Button type="button" variant="primary" size="sm" disabled={actLocked(!!working, false)} onClick={() => void run(false)}>{working ? t("about.storage.d.working") : t("about.storage.d.delete")}</Button>
          ) : (
            <>
              <Button type="button" variant="ghost" size="sm" disabled={actLocked(!!working, false)} onClick={() => void run(false)}>{t(`about.storage.d.${kind === "empty" ? "emptyOnly" : "deleteOnly"}`)}</Button>
              {targets.length > 0 && (
                <Button type="button" variant="primary" size="sm" disabled={actLocked(!!working, false)} onClick={() => void run(true)}>
                  {working ? t("about.storage.d.working") : t(`about.storage.d.${kind === "empty" ? "emptyNow" : "backupDelete"}`)}
                </Button>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
