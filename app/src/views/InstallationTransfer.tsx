import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Upload } from "lucide-react";
import { api, type ImportApplied, type ImportInto, type ImportPreview, type Installation, type OpenImport } from "../api";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { errorKey } from "../maint";

const mb = (b: number) => (b / 1e6).toLocaleString(undefined, { maximumFractionDigits: 1 });

/** `openbms-<name>-<YYYY-MM-DD>.zip`, the name reduced to safe characters. */
export function bundleName(name: string, now = new Date()): string {
  const safe = name.normalize("NFKD").replace(/\p{M}+/gu, "").replace(/[^A-Za-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "") || "anlage";
  const d = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  return `openbms-${safe}-${d}.zip`;
}

/** The choices of the import dialog: what the backend accepts for this preview, the default first. */
export function importChoices(p: ImportPreview): { into: ImportInto; label: string; params?: Record<string, unknown> }[] {
  if (p.same && !p.master_differs) return [{ into: { kind: "same" }, label: "inst.imp.same", params: { name: p.same.name } }];
  if (p.same) return [
    { into: { kind: "new", copy: true }, label: "inst.imp.copy" },
    { into: { kind: "merge-anyway" }, label: "inst.imp.mergeAnyway", params: { name: p.same.name } },
  ];
  return [
    { into: { kind: "new", copy: false }, label: "inst.imp.new", params: { name: p.name } },
    ...p.master_matches.map((m) => ({ into: { kind: "merge", id: m.id } as ImportInto, label: "inst.imp.mergeMaster", params: { name: m.name } })),
  ];
}

/** The result line of an import. */
export function importResult(r: ImportApplied, name: string, t: (k: string, o?: Record<string, unknown>) => string): string {
  if (r.already) return t("inst.imp.already", { name });
  const parts = [
    t("inst.imp.res.boards", { count: r.report.boards_added ?? 0 }),
    t("inst.imp.res.snapshots", { count: r.report.snapshots_added ?? 0 }),
    t("inst.imp.res.logs", { count: r.logs_copied }),
  ];
  if (r.recording_rows || r.recording_events) parts.push(t("inst.imp.res.recording", { rows: r.recording_rows, events: r.recording_events }));
  const nt = r.report.not_taken;
  const notTaken = !!(nt && (Object.keys(nt.cell_types ?? {}).length || nt.groups || nt.pairs || nt.diagnosis)) || !!r.report.master_serial_not_taken;
  return `${t("inst.imp.done", { name, list: parts.join(", ") })}${notTaken ? ` ${t("inst.imp.notTaken")}` : ""}`;
}

/** Import a ZIP: pick the file, see what it holds and where it goes, import. Unfinished imports can be resumed. */
export function ImportPanel({ installations, onDone }: { installations: Installation[]; onDone: () => Promise<void> }) {
  const { t, i18n } = useTranslation();
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [choice, setChoice] = useState(0);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ text: string; error: boolean } | null>(null);
  const [open, setOpen] = useState<OpenImport[]>([]);
  const loadOpen = () => api.installationImportsOpen().then(setOpen).catch(() => setOpen([]));
  useEffect(() => { void loadOpen(); }, []);
  const date = (ms: number) => new Date(ms).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" });
  const fail = (e: unknown) => { const k = errorKey(e); setMsg({ text: t(k.key, k.params), error: true }); };
  const nameOf = (id: string) => installations.find((i) => i.id === id)?.name ?? id;

  const pick = async () => {
    setMsg(null); setPreview(null);
    const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
    const path = await openDialog({ multiple: false, filters: [{ name: "ZIP", extensions: ["zip"] }] });
    if (typeof path !== "string") return;
    setBusy(true);
    try { setPreview(await api.installationImportInspect(path)); setChoice(0); }
    catch (e) { fail(e); }
    finally { setBusy(false); }
  };
  const run = async (exportId: string, into: ImportInto, name: string) => {
    setBusy(true); setMsg(null);
    try {
      const r = await api.installationImportApply(exportId, into);
      setPreview(null);
      await onDone();
      setMsg({ text: importResult(r, nameOf(r.target) || name, t), error: false });
    } catch (e) { fail(e); }
    finally { setBusy(false); void loadOpen(); }
  };

  const choices = preview ? importChoices(preview) : [];
  return (
    <div className="space-y-3">
      <Button variant="secondary" disabled={busy} onClick={() => void pick()}><Upload className="h-4 w-4" />{t("inst.imp.button")}</Button>
      {open.map((o) => (
        <div key={o.export_id} className="flex flex-wrap items-center gap-3 rounded-md border border-discharge p-3 text-sm">
          <p className="flex-1">{t("inst.imp.open", { name: o.name, at: date(o.started) })}</p>
          <Button size="sm" variant="secondary" disabled={busy} onClick={() => void run(o.export_id, { kind: "same" }, o.name)}>{t("inst.imp.resume")}</Button>
        </div>
      ))}
      {preview && (
        <Panel className="space-y-3 p-4 text-sm">
          <p className="font-display text-lg font-semibold">{t("inst.imp.title", { name: preview.name })}</p>
          <p className="text-muted">
            {t("inst.imp.holds", { at: date(preview.created), places: preview.places, boards: preview.boards, snapshots: preview.snapshots, logs: preview.logs })}
            {" "}{preview.recording_bytes != null ? t("inst.imp.withRecording", { mb: mb(preview.recording_bytes) }) : t("inst.imp.noRecording")}
          </p>
          {preview.completed != null ? (
            <p>{t("inst.imp.completed", { at: date(preview.completed) })}</p>
          ) : preview.open_into ? (
            <p>{t("inst.imp.resumeInto", { name: preview.open_into.name })}</p>
          ) : (
            <fieldset className="space-y-1.5">
              {choices.map((c, i) => (
                <label key={i} className="flex cursor-pointer items-start gap-2">
                  <input type="radio" name="into" className="mt-0.5 accent-[var(--charge)]" checked={choice === i} onChange={() => setChoice(i)} disabled={choices.length === 1} />
                  <span>{t(c.label, c.params)}</span>
                </label>
              ))}
            </fieldset>
          )}
          <p className="text-muted">{t("inst.imp.rules")}</p>
          <div className="flex gap-2">
            {preview.completed == null && (
              <Button size="sm" variant="primary" disabled={busy}
                onClick={() => void run(preview.export_id, preview.open_into ? { kind: "same" } : choices[choice].into, preview.name)}>
                {busy ? t("inst.imp.running") : preview.open_into ? t("inst.imp.resume") : t("inst.imp.go")}
              </Button>
            )}
            <Button size="sm" variant="secondary" disabled={busy} onClick={() => setPreview(null)}>{t("maint.setup.cancel")}</Button>
          </div>
        </Panel>
      )}
      {msg && <p className={msg.error ? "text-sm text-alarm" : "text-sm text-ok"}>{msg.text}</p>}
    </div>
  );
}

/** Export one installation: with or without its recording, to a file the user picks. */
export function ExportRow({ inst, recorded }: { inst: Installation; recorded: number }) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ text: string; error: boolean } | null>(null);
  const run = async () => {
    setMsg(null);
    const { save } = await import("@tauri-apps/plugin-dialog");
    const [, exports] = await api.historyDirs().catch(() => ["", ""] as [string, string]);
    const path = await save({ defaultPath: exports ? `${exports}/${bundleName(inst.name)}` : bundleName(inst.name), filters: [{ name: "ZIP", extensions: ["zip"] }] });
    if (!path) return;
    setBusy(true);
    try {
      const r = await api.installationExport(inst.id, path, recording);
      const file = r.path.split(/[\\/]/).pop() ?? r.path;
      setMsg({ text: t("inst.exp.done", { file, mb: mb(r.bytes), logs: r.logs }) + (r.logs_missing ? ` ${t("inst.exp.missing", { count: r.logs_missing })}` : ""), error: false });
    } catch (e) { const k = errorKey(e); setMsg({ text: t(k.key, k.params), error: true }); }
    finally { setBusy(false); }
  };
  return (
    <div className="space-y-1">
      <div className="flex flex-wrap items-center gap-3 text-sm">
        <Button size="sm" variant="secondary" disabled={busy} onClick={() => void run()}><Download className="h-4 w-4" />{busy ? t("inst.exp.running") : t("inst.exp.button")}</Button>
        <label className="flex cursor-pointer items-center gap-2" title={t("inst.exp.recordingHint")}>
          <input type="checkbox" className="h-4 w-4 accent-[var(--charge)]" checked={recording} disabled={busy || !recorded} onChange={(e) => setRecording(e.target.checked)} />
          <span className={recorded ? "" : "text-muted"}>{t("inst.exp.recording", { count: recorded })}</span>
        </label>
      </div>
      {msg && <p className={msg.error ? "text-sm text-alarm" : "text-sm text-ok"}>{msg.text}</p>}
    </div>
  );
}
