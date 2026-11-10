import { useEffect, useState } from "react";
import { decideUi, lockProps, type UiFacts } from "../locks";
import { useTranslation } from "react-i18next";
import { Download, Pause, Play, Radio, Trash2 } from "lucide-react";
import { api, isDemo, type CaptureStatus, type LinkTiming } from "../api";
import { durationText } from "../bmsMemory";
import { captureName } from "../capture";
import { Button } from "./ui/button";
import { SectionHead, StatusLine } from "./Section";

/** „Mitschnitt“ under Diagnosis: records what the app sends and receives on every connection, for an export. Works
 *  without a connection (export or clear an earlier capture; a started capture records the next connect). */
export function FrameCapture() {
  const { t, i18n } = useTranslation();
  const [s, setS] = useState<CaptureStatus | null>(null);
  const [note, setNote] = useState<{ text: string; error?: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [timing, setTiming] = useState<LinkTiming | null>(null);

  // polled only while the view is open
  useEffect(() => {
    let alive = true;
    const poll = () => {
      api.captureStatus().then((v) => { if (alive) setS(v); }).catch(() => {});
      // without a connection nothing; while an exchange holds it, the last view of the same connection stays
      api.linkTiming().then((v) => { if (alive) setTiming((old) => (!v ? null : !v.busy ? v : old?.gen === v.gen ? old : null)); }).catch(() => {});
    };
    poll();
    const id = window.setInterval(poll, 1000);
    return () => { alive = false; window.clearInterval(id); };
  }, []);

  const run = async (f: () => Promise<CaptureStatus>) => {
    try { setS(await f()); } catch (e) { setNote({ text: String(e), error: true }); }
  };
  const toggle = () => run(s?.running ? api.captureStop : api.captureStart);
  const clear = () => { setNote(null); void run(api.captureClear); };
  const save = async () => {
    setNote(null);
    try {
      const { save: pick } = await import("@tauri-apps/plugin-dialog");
      const [, exports] = await api.historyDirs();
      const path = await pick({ defaultPath: `${exports}/${captureName(new Date())}`, filters: [{ name: "Text", extensions: ["txt"] }] });
      if (!path) return;
      setBusy(true);
      const count = await api.captureExport(path, i18n.language);
      setNote({ text: t("cap.exported", { count, name: path.split(/[\\/]/).pop() ?? path }) });
    } catch (e) { setNote({ text: t("cap.exportFailed", { err: String(e) }), error: true }); }
    finally { setBusy(false); }
  };

  const entries = s?.entries ?? 0;
  // what may be done now (`locks.ts`)
  const cf: UiFacts = { source: null, chosen: null, demo: isDemo, loaded: !!s, full: !!s?.full, entries: entries > 0, busy };
  const summary = t("cap.summary", {
    entries: t("cap.entries", { count: entries }),
    span: durationText((s?.span_ms ?? 0) / 1000),
    timeouts: t("cap.timeouts", { count: s?.timeouts ?? 0 }),
  });
  const state = !s ? null
    : s.full ? t("cap.full", { limit: t(`cap.limit.${s.full}`), summary })
    : s.running ? (entries ? t("cap.running", { summary }) : t("cap.runningEmpty"))
    : entries ? t("cap.stopped", { summary }) : t("cap.none");

  return (
    <div className="space-y-2 text-sm">
      <SectionHead icon={Radio} title={t("cap.title")} help="topic.capture" />
      <p className="max-w-3xl text-muted">{t("cap.intro")}</p>
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" variant={s?.running ? "secondary" : "primary"} {...lockProps(decideUi("capture.toggle", cf), t)} onClick={toggle}>
          {s?.running ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}{s?.running ? t("cap.stop") : t("cap.start")}
        </Button>
        <Button size="sm" {...lockProps(decideUi("capture.export", cf), t)} onClick={save}><Download className="h-4 w-4" />{t("cap.export")}</Button>
        <Button size="sm" variant="ghost" {...lockProps(decideUi("capture.clear", cf), t)} onClick={clear}><Trash2 className="h-4 w-4" />{t("cap.clear")}</Button>
      </div>
      {state && <StatusLine tone={s?.full ? "warn" : s?.running ? "ok" : "muted"} className="tabular-nums">{state}</StatusLine>}
      {isDemo && <StatusLine>{t("cap.demo")}</StatusLine>}
      <p className="max-w-3xl text-xs text-muted">{t("cap.privacy")}</p>
      {note && <StatusLine tone={note.error ? "error" : "ok"}>{note.text}</StatusLine>}
      {timing && (
        <div className="space-y-0.5 pt-2" title={t("cap.timing.hint")}>
          <p className="font-medium">{t("cap.timing.title")}</p>
          {!timing.adaptive ? <p className="text-muted">{t("cap.timing.local")}</p> : timing.classes.map((c) => {
            const v = { cls: t(`cap.timing.cls.${c.class}`), budget: msText(c.budget_ms), n: c.samples };
            return <p key={c.class} className="text-muted tabular-nums">{c.p90_ms === null ? t("cap.timing.few", v) : t("cap.timing.row", { ...v, p90: msText(c.p90_ms), max: msText(c.max_ms ?? 0) })}</p>;
          })}
        </div>
      )}
    </div>
  );
}

const msText = (ms: number) => (ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1).replace(".", ",")} s`);
