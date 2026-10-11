import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, FolderOpen, TriangleAlert } from "lucide-react";
import { api, isDemo, type DeviceInfo, type FirmwareDone, type FirmwareImage, type FirmwareProgress } from "../api";
import { Button } from "../components/ui/button";
import { useNow } from "../components/live";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { confirmationPhrase, progressFraction, steps } from "../firmware";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

function Step({ n, title, active, children }: { n: number; title: string; active: boolean; children: ReactNode }) {
  return (
    <section className={cn("space-y-3 border-t border-line pt-5", !active && "opacity-50")}>
      <h2 className="font-display text-lg font-semibold"><span className="mr-2 text-muted">{n}</span>{title}</h2>
      {children}
    </section>
  );
}

export function Firmware({ connected, onMonitorStopped, onRunning, onBack }: {
  connected: boolean;
  onMonitorStopped: () => void;
  onRunning: (running: boolean) => void;
  onBack: () => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [ports, setPorts] = useState<string[]>([]);
  const [port, setPort] = useState("");
  const [address, setAddress] = useState(0);
  const [path, setPath] = useState<string | null>(null);
  const [image, setImage] = useState<FirmwareImage | null>(null);
  const [target, setTarget] = useState<DeviceInfo | null>(null);
  const [checkedFor, setCheckedFor] = useState<{ port: string; address: number } | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<FirmwareProgress | null>(null);
  const [done, setDone] = useState<FirmwareDone | null>(null);
  const [closeRefused, setCloseRefused] = useState(false);
  const startedAt = useRef(0);
  const now = useNow(500);

  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);
  useEffect(() => {
    const subs = [
      api.onFirmwareProgress(setProgress),
      api.onFirmwareDone((d) => { setDone(d); setRunning(false); onRunning(false); }),
      api.onFirmwareBusy(() => setCloseRefused(true)),
    ];
    return () => { subs.forEach((s) => s.then((f) => f())); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const s = steps({ running, port, address, image, target, checkedFor, typed });
  const phrase = confirmationPhrase(address);

  const chooseImage = async () => {
    setError(null);
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, filters: [{ name: "EMU1101 firmware", extensions: ["ehex"] }] });
    if (typeof picked !== "string") return;
    setImage(null); setPath(null);
    try { setImage(await api.firmwareCheckImage(picked, lang)); setPath(picked); } catch (e) { setError(String(e)); }
  };

  const inspect = async () => {
    setBusy(true); setError(null); setTarget(null); setCheckedFor(null);
    // the backend drops the monitor connection first; the UI must forget it too
    if (connected) onMonitorStopped();
    try { setTarget(await api.firmwareInspect(port, address, lang)); setCheckedFor({ port, address }); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const start = async () => {
    if (!path) return;
    setError(null); setDone(null); setProgress(null); setCloseRefused(false);
    try {
      setRunning(true); onRunning(true); startedAt.current = Date.now();
      await api.firmwareStart(port, address, path, typed, lang);
    } catch (e) { setRunning(false); onRunning(false); setError(String(e)); }
  };

  const reset = () => { setDone(null); setProgress(null); setTyped(""); setTarget(null); setCheckedFor(null); };

  if (isDemo) return <div className="max-w-2xl space-y-3"><h1 className="font-display text-2xl font-semibold">{t("fw.title")}</h1><p className="text-muted">{t("fw.demo")}</p></div>;

  return (
    <div className="max-w-2xl space-y-6">
      <header className="space-y-2">
        <h1 className="flex items-center gap-2 font-display text-2xl font-semibold">{t("fw.title")}<InfoIcon id="topic.firmware" /></h1>
        <p className="text-muted">{t("fw.intro")}</p>
      </header>

      <Step n={1} title={t("fw.s1")} active={!running && !done}>
        <div className="flex flex-wrap gap-3">
          <select className={input} value={port} disabled={running} onChange={(e) => setPort(e.target.value)} aria-label={t("fw.port")}>
            <option value="">{t("fw.choosePort")}</option>
            {ports.map((p) => <option key={p}>{p}</option>)}
          </select>
          <label className="flex items-center gap-2 text-sm text-muted">{t("fw.address")}
            <select className={input} value={address} disabled={running} onChange={(e) => setAddress(Number(e.target.value))}>
              {Array.from({ length: 16 }, (_, a) => <option key={a} value={a}>{a}</option>)}
            </select>
          </label>
        </div>
        <p className="text-sm text-muted">{t("fw.busHint")}</p>
        {connected && <p className="text-sm text-discharge">{t("fw.monitorWillStop")}</p>}
      </Step>

      <Step n={2} title={t("fw.s2")} active={s.image}>
        <Button variant="ghost" disabled={!s.image} onClick={chooseImage}>{t("fw.chooseImage")}</Button>
        {image && <div className="text-sm"><p>{t("fw.imageInfo", { release: image.release, blocks: image.blocks, bytes: image.bytes })}</p><p className="break-all text-muted">{t("fw.sha", { sha: image.sha256 })}</p></div>}
      </Step>

      <Step n={3} title={t("fw.s3")} active={s.inspect}>
        <Button variant="ghost" disabled={!s.inspect || busy} onClick={inspect}>{busy ? t("fw.inspecting") : t("fw.inspect")}</Button>
        {target && s.confirm && <p className="text-sm">{t("fw.target", { name: target.device_name, version: target.firmware_version, can: target.can_protocol })}</p>}
      </Step>

      <Step n={4} title={t("fw.s4")} active={s.confirm}>
        {s.confirm && image && target && <p className="font-medium">{t("fw.summary", { address, from: target.firmware_version, to: image.release })}</p>}
        <p className="text-sm text-alarm">{t("fw.risks")}</p>
        <label className="block space-y-1 text-sm text-muted">{t("fw.typeToConfirm", { phrase })}
          <input className={cn(input, "block w-48 font-mono")} value={typed} disabled={!s.confirm} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} />
        </label>
        <Button variant="primary" disabled={!s.start} onClick={start}>{t("fw.start")}</Button>
      </Step>

      {(running || done) && (
        <Step n={5} title={t("fw.s5")} active>
          {running && (
            <>
              <p>{progress ? t(`fw.stage.${progress.stage}`, { n: (progress.block ?? 0) + 1, total: progress.total ?? 0 }) : t("fw.stage.preflight")}</p>
              <div className="h-2 overflow-hidden rounded bg-sunken"><div className="h-full bg-charge transition-all" style={{ width: `${progressFraction(progress) * 100}%` }} /></div>
              <p className="text-sm text-muted">{t("fw.elapsed", { s: Math.round((now - startedAt.current) / 1000) })}</p>
              {closeRefused && <p className="text-sm text-discharge">{t("fw.busy")}</p>}
            </>
          )}
          {done && (
            <div className="space-y-2">
              {done.ok ? (
                <p className="flex items-center gap-2 font-medium text-ok"><CircleCheck className="h-5 w-5" />{t("fw.okTitle")}: {t("fw.okText", { from: done.before?.firmware_version, to: done.after?.firmware_version })}</p>
              ) : (
                <>
                  <p className="flex items-center gap-2 font-medium text-alarm"><TriangleAlert className="h-5 w-5" />{t("fw.failTitle")}</p>
                  <p className="text-sm">{done.error}</p>
                  {done.stage && <p className="text-sm text-muted">{t("fw.failStage", { stage: t(`fw.stage.${done.stage}`, { n: (progress?.block ?? 0) + 1, total: progress?.total ?? 0 }) })}</p>}
                  <p className={cn("text-sm", done.written ? "text-alarm" : "text-muted")}>{done.written ? t("fw.failAfter") : t("fw.failBefore")}</p>
                </>
              )}
              <p className="break-all text-sm text-muted">{t("fw.log")}: {done.log}</p>
              <div className="flex gap-2">
                <Button variant="ghost" size="sm" onClick={() => api.revealFirmwareLogs(done.log)}><FolderOpen className="h-4 w-4" />{t("fw.showLog")}</Button>
                <Button variant="ghost" size="sm" onClick={reset}>{t("fw.again")}</Button>
                <Button variant="primary" size="sm" onClick={onBack}>{t("fw.back")}</Button>
              </div>
            </div>
          )}
        </Step>
      )}

      {error && <p className="text-sm text-alarm">{error}</p>}
    </div>
  );
}
