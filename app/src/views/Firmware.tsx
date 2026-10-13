import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, FolderOpen, RefreshCw, TriangleAlert } from "lucide-react";
import { api, isDemo, type DeviceInfo, type FirmwareDone, type FirmwareImage, type FirmwareProgress, type PlanView } from "../api";
import { Button } from "../components/ui/button";
import { Switch } from "../components/PackPicker";
import { useNow } from "../components/live";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { confirmationPhrase, progressFraction, steps } from "../firmware";
import { canFlash, errorKey } from "../maint";
import type { useMaint } from "../maintStore";
import { PackBusPanel } from "./PackBusPanel";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

function Step({ n, title, active, children }: { n: number; title: string; active: boolean; children: ReactNode }) {
  return (
    <section className={cn("space-y-3 border-t border-line pt-5", !active && "opacity-50")}>
      <h2 className="font-display text-lg font-semibold"><span className="mr-2 text-muted">{n}</span>{title}</h2>
      {children}
    </section>
  );
}

export function Firmware({ connected, disconnects, maint, onMonitorStopped, onRunning, onBack }: {
  connected: boolean;
  /** counts the monitor connection being lost or closed in the sidebar (not stopped by this view for a flash) */
  disconnects: number;
  maint: ReturnType<typeof useMaint>;
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
  // not remembered: every visit starts with the vendor allowlist only
  const [allowCustom, setAllowCustom] = useState(false);
  const [target, setTarget] = useState<DeviceInfo | null>(null);
  const [checkedFor, setCheckedFor] = useState<{ port: string; address: number } | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<FirmwareProgress | null>(null);
  const [done, setDone] = useState<FirmwareDone | null>(null);
  const [closeRefused, setCloseRefused] = useState(false);
  const [inverter, setInverter] = useState(false);
  const [measuredAt, setMeasuredAt] = useState<number | null>(null);
  const [portInstallation, setPortInstallation] = useState<string | null>(null);
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [dipOffConfirmed, setDipOffConfirmed] = useState(false);
  const [isolatedConfirmed, setIsolatedConfirmed] = useState(false);
  const [endNote, setEndNote] = useState<{ installation: string; text: string } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const startedAt = useRef(0);
  const addressRef = useRef(address);
  addressRef.current = address;
  const now = useNow(500);

  const loadPorts = () => { api.listPorts().then(setPorts).catch(() => setPorts([])); };
  useEffect(loadPorts, []);
  useEffect(() => {
    const subs = [
      api.onFirmwareProgress(setProgress),
      api.onFirmwareDone((d) => {
        setDone(d); setRunning(false); onRunning(false);
        void maint.refresh();
        // one measurement, one flash
        setMeasuredAt(null); setDipOffConfirmed(false); setIsolatedConfirmed(false);
        const notes: string[] = [];
        if (d.block) notes.push(t(`maint.panel.block.${d.block}`, { pack: addressRef.current }));
        if (d.master) notes.push(t(`maint.master.${d.master}`));
        setNotice(notes.length ? notes.join(" ") : null);
      }),
      api.onFirmwareBusy(() => setCloseRefused(true)),
    ];
    return () => { subs.forEach((s) => s.then((f) => f())); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const overview = maint.overview;
  // a new measurement is needed after every state change; the inverter statement after every new session
  useEffect(() => { setMeasuredAt(null); }, [overview?.state_at, overview?.installation.id]);
  useEffect(() => { setInverter(false); }, [overview?.session?.id, overview?.installation.id]);
  // after a lost or closed connection the operator confirms the inverter afresh and measures again
  useEffect(() => { setInverter(false); setMeasuredAt(null); }, [disconnects]);
  // the End result stays until a new session opens or another installation is shown
  useEffect(() => { setEndNote(null); }, [overview?.installation.id]);
  useEffect(() => { if (overview?.session) setEndNote(null); }, [overview?.session?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  // which installation owns the pack port
  const lookupPort = () => {
    if (!port) { setPortInstallation(null); return Promise.resolve(); }
    return api.maintInstallationOf({ kind: "serial", path: port, bus: "pack" })
      .then((i) => setPortInstallation(i?.id ?? null)).catch(() => setPortInstallation(null));
  };
  useEffect(() => { void lookupPort(); }, [port, overview]); // eslint-disable-line react-hooks/exhaustive-deps
  const portAssigned = !!overview && portInstallation === overview.installation.id;
  const portElsewhere = !!overview && !!port && portInstallation !== null && !portAssigned;
  const portFree = !!overview && !!port && portInstallation === null;

  // the master (address 0) has its own plan; refused plans keep the start locked
  const overviewId = overview?.installation.id;
  useEffect(() => {
    setPlan(null); setPlanError(null); setDipOffConfirmed(false); setIsolatedConfirmed(false);
    if (!overviewId || address !== 0 || !image) return;
    let live = true;
    api.maintMasterPlan(overviewId, image.kind)
      .then((p) => { if (live) setPlan(p); })
      .catch((e) => { if (live) { const k = errorKey(e); setPlanError(t(k.key, k.params)); } });
    return () => { live = false; };
  }, [overviewId, address, image?.kind, overview?.state?.state, overview?.session?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const masterPending = !!overview && address === 0 && !!image
    && (!plan || planError !== null || (plan.before.includes("dip-off") && !dipOffConfirmed) || (plan.before.includes("isolated-or-released") && !isolatedConfirmed));
  const gate = canFlash({ overview, inverter, measuredAt, address, imageKind: image?.kind ?? "custom", portAssigned });
  const s = steps({ running, done: done !== null, port, address, image, target, checkedFor, typed, unlocked: gate.ok && !masterPending });
  const phrase = confirmationPhrase(address, image?.custom);

  const chooseImage = async () => {
    setError(null);
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, filters: [{ name: "EMU1101 firmware", extensions: ["ehex"] }] });
    if (typeof picked !== "string") return;
    // a phrase typed for the previous image may not fit the new one
    setImage(null); setPath(null); setTyped("");
    try { setImage(await api.firmwareCheckImage(picked, allowCustom, lang)); setPath(picked); } catch (e) { setError(String(e)); }
  };

  const toggleCustom = (v: boolean) => {
    setAllowCustom(v);
    // switching off must not leave a custom image selected
    if (!v && image?.custom) { setImage(null); setPath(null); setTyped(""); }
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
    if (!path || !image) return;
    // the backend stops the monitor here too (it may have been reconnected since the check)
    if (connected) onMonitorStopped();
    const confirmation = typed;
    // one confirmation, one run: the pack has to be checked and confirmed again for the next one
    setError(null); setNotice(null); setDone(null); setProgress(null); setCloseRefused(false); setTyped(""); setTarget(null); setCheckedFor(null);
    try {
      setRunning(true); onRunning(true); startedAt.current = Date.now();
      await api.firmwareStart(port, address, path, allowCustom, image.sha256, confirmation, lang,
        overview ? { installation: overview.installation.id, inverterConfirmed: inverter, dipOffConfirmed, isolatedConfirmed } : null);
    } catch (e) {
      setRunning(false); onRunning(false);
      // gate refusals arrive as MaintError JSON; the backend may have changed state (check, stop, session kind)
      const k = errorKey(e); setError(t(k.key, k.params)); void maint.refresh();
    }
  };

  const reset = () => { setDone(null); setProgress(null); setTyped(""); setTarget(null); setCheckedFor(null); };

  if (isDemo) return <div className="max-w-2xl space-y-3"><h1 className="font-display text-2xl font-semibold">{t("fw.title")}</h1><p className="text-muted">{t("fw.demo")}</p></div>;

  return (
    <div className="max-w-2xl space-y-6">
      <header className="space-y-2">
        <h1 className="flex items-center gap-2 font-display text-2xl font-semibold">{t("fw.title")}<InfoIcon id="topic.firmware" /></h1>
        <p className="text-muted">{t("fw.intro")}</p>
      </header>

      <PackBusPanel key={`${overview?.installation.id ?? ""}:${overview?.session?.id ?? ""}`} maint={maint} port={port} inverter={inverter} setInverter={setInverter} measuredAt={measuredAt} setMeasuredAt={setMeasuredAt}
        setEndNote={(text) => setEndNote(text && overview ? { installation: overview.installation.id, text } : null)} />
      {endNote && endNote.installation === overview?.installation.id && <p className="text-sm">{endNote.text}</p>}

      <Step n={1} title={t("fw.s1")} active={!running && !done}>
        <div className="flex flex-wrap gap-3">
          <select className={input} value={port} disabled={running} onChange={(e) => setPort(e.target.value)} aria-label={t("fw.port")}>
            <option value="">{t("fw.choosePort")}</option>
            {ports.map((p) => <option key={p}>{p}</option>)}
          </select>
          <Button variant="ghost" size="icon" disabled={running} onClick={loadPorts} aria-label={t("fw.refreshPorts")} title={t("fw.refreshPorts")}><RefreshCw className="h-4 w-4" /></Button>
          <label className="flex items-center gap-2 text-sm text-muted">{t("fw.address")}
            <select className={input} value={address} disabled={running} onChange={(e) => setAddress(Number(e.target.value))}>
              {Array.from({ length: 16 }, (_, a) => <option key={a} value={a}>{a}</option>)}
            </select>
          </label>
        </div>
        {portFree && (
          <p className="flex flex-wrap items-center gap-3 text-sm text-discharge">{t("maint.panel.portUnassigned")}
            <Button variant="ghost" size="sm" onClick={async () => { await maint.assign({ kind: "serial", path: port, bus: "pack" }); await lookupPort(); }}>{t("maint.banner.assign")}</Button>
          </p>
        )}
        {portElsewhere && <p className="text-sm text-alarm">{t("maint.err.refused.installation-mismatch")}</p>}
        <p className="text-sm text-muted">{t("fw.busHint")}</p>
        {connected && <p className="text-sm text-discharge">{t("fw.monitorWillStop")}</p>}
      </Step>

      <Step n={2} title={t("fw.s2")} active={s.image}>
        <div className="flex flex-wrap items-center gap-4">
          <Button variant="ghost" disabled={!s.image} onClick={chooseImage}>{t("fw.chooseImage")}</Button>
          <label className="flex items-center gap-2 text-sm text-muted"><Switch checked={allowCustom} onChange={toggleCustom} disabled={!s.image} />{t("fw.allowCustom")}</label>
        </div>
        {image && <div className="text-sm"><p>{t("fw.imageInfo", { release: image.custom ? t("fw.customName") : image.release, blocks: image.blocks, bytes: image.bytes })}</p><p className="break-all text-muted">{t("fw.sha", { sha: image.sha256 })}</p></div>}
        {image?.custom && <p className="flex gap-2 rounded-md border border-alarm p-3 text-sm text-alarm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t("fw.customWarning")}</p>}
      </Step>

      <Step n={3} title={t("fw.s3")} active={s.inspect}>
        <Button variant="ghost" disabled={!s.inspect || busy} onClick={inspect}>{busy ? t("fw.inspecting") : t("fw.inspect")}</Button>
        {target && s.confirm && <p className="text-sm">{t("fw.target", { name: target.device_name, version: target.firmware_version, can: target.can_protocol })}</p>}
      </Step>

      <Step n={4} title={t("fw.s4")} active={s.confirm}>
        {s.confirm && image && target && <p className="font-medium">{t("fw.summary", { address, from: target.firmware_version, to: image.custom ? t("fw.customName") : image.release })}</p>}
        <p className="text-sm text-alarm">{t("fw.risks")}</p>
        <label className="block space-y-1 text-sm text-muted">{t("fw.typeToConfirm", { phrase })}
          <input className={cn(input, "block w-48 font-mono")} value={typed} disabled={!s.confirm} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} />
        </label>
        {overview && address === 0 && image && (
          <div className="space-y-1 text-sm">
            {planError && <p className="text-alarm">{planError}</p>}
            {plan && (
              <>
                <ul className="list-inside list-disc">
                  {plan.before.map((st) => <li key={st}>{t(`maint.plan.${st}`)}</li>)}
                  <li>{t(`maint.plan.after.${plan.after}`)}</li>
                </ul>
                {plan.before.includes("dip-off") && <label className="flex items-center gap-2"><input type="checkbox" checked={dipOffConfirmed} onChange={(e) => setDipOffConfirmed(e.target.checked)} />{t("maint.plan.dip-off")}</label>}
                {plan.before.includes("isolated-or-released") && <label className="flex items-center gap-2"><input type="checkbox" checked={isolatedConfirmed} onChange={(e) => setIsolatedConfirmed(e.target.checked)} />{t("maint.plan.isolated-or-released")}</label>}
                {plan.recovery && <p className="text-muted">{t("maint.plan.recovery")}</p>}
              </>
            )}
          </div>
        )}
        <Button variant="primary" disabled={!s.start} onClick={start}>{t("fw.start")}</Button>
        {!gate.ok && gate.reason && <p className="text-sm text-alarm">{t(errorKey(gate.reason).key)}</p>}
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
                  <p className={cn("text-sm", done.written ? "text-alarm" : "text-muted")}>{done.stage === "post_check" ? t("fw.failPostCheck") : done.written ? t("fw.failAfter") : t("fw.failBefore")}</p>
                </>
              )}
              {notice && <p className="text-sm text-alarm">{notice}</p>}
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
