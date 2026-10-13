import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, FolderOpen, Lock, RefreshCw, TriangleAlert } from "lucide-react";
import { api, isDemo, type DeviceInfo, type FirmwareDone, type FirmwareImage, type FirmwareProgress, type PlanView } from "../api";
import { Button } from "../components/ui/button";
import { Switch } from "../components/PackPicker";
import { useNow } from "../components/live";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { confirmationPhrase, gateHint, imageLabel, imageWarning, masterOk, progressFraction, resultActions, resultVersions, steps } from "../firmware";
import { acceptsProgress, canFlash, errorKey, flowOf, keepsInverter, packPortOf, planItems, planSectorNote, portLock, sectorNoteKey, type Activity, type Monitor } from "../maint";
import { oneAdapterOf, type useMaint } from "../maintStore";
import { PackBusPanel } from "./PackBusPanel";

// the measurement belongs to one flash: its result line at the button goes with it
const dropMeasure = (a: Activity | null) => (a?.key === "measure" ? null : a);

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

function Step({ n, title, active, children }: { n: number; title: string; active: boolean; children: ReactNode }) {
  return (
    <section className={cn("space-y-3 border-t border-line pt-5", !active && "opacity-50")}>
      <h2 className="font-display text-lg font-semibold"><span className="mr-2 text-muted">{n}</span>{title}</h2>
      {children}
    </section>
  );
}

export function Firmware({ connected, monitor, disconnects, focusEnd, onFocusDone, maint, onMonitorStopped, onRunning, onBack, onUserAction, onPanelBusy }: {
  /** a pack bus panel action runs (the banner's „Anzeigen“ waits) */
  onPanelBusy: (busy: boolean) => void;
  connected: boolean;
  monitor: Monitor;
  /** set by "End" in the banner: scroll to the panel's End section */
  focusEnd: number;
  onFocusDone: () => void;
  /** counts the monitor connection being lost or closed in the sidebar (not stopped by this view for a flash) */
  disconnects: number;
  maint: ReturnType<typeof useMaint>;
  onMonitorStopped: () => void;
  onRunning: (running: boolean) => void;
  onBack: () => void;
  /** an action on the bus starts here or in the panel (the sidebar stops waiting for a busy port) */
  onUserAction: () => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [ports, setPorts] = useState<string[]>([]);
  // the port chosen here: only without an installation (a single pack without maintenance)
  const [ownPort, setOwnPort] = useState("");
  const [address, setAddress] = useState(0);
  const [path, setPath] = useState<string | null>(null);
  const [image, setImage] = useState<FirmwareImage | null>(null);
  // not remembered: every visit starts with the vendor allowlist only
  const [allowCustom, setAllowCustom] = useState(false);
  // a valid image that needs "allow custom firmware": kept so switching it on takes the same file
  const [pending, setPending] = useState<{ path: string; info: FirmwareImage } | null>(null);
  const [imageError, setImageError] = useState<string | null>(null);
  const [target, setTarget] = useState<DeviceInfo | null>(null);
  const [checkedFor, setCheckedFor] = useState<{ port: string; address: number } | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<FirmwareProgress | null>(null);
  const [done, setDone] = useState<FirmwareDone | null>(null);
  // what the last run flashed: the image and the version "check pack" read (for the result's version line)
  const [flashed, setFlashed] = useState<{ image: FirmwareImage; checked: string | null } | null>(null);
  // the master's post-flash check of the last run ("as-expected" is a success, the others are not)
  const [masterOutcome, setMasterOutcome] = useState<string | null>(null);
  // "End maintenance" under a result: the same scroll and highlight as the banner's End
  const [ownFocusEnd, setOwnFocusEnd] = useState(0);
  const [closeRefused, setCloseRefused] = useState(false);
  const [inverter, setInverter] = useState(false);
  const [measuredAt, setMeasuredAt] = useState<number | null>(null);
  const [portInstallation, setPortInstallation] = useState<string | null>(null);
  const [plan, setPlan] = useState<PlanView | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [dipOffConfirmed, setDipOffConfirmed] = useState(false);
  const [isolatedConfirmed, setIsolatedConfirmed] = useState(false);
  // the running or last pack bus panel action (spinner, phase, result at its button); goes with a view change
  const [activity, setActivity] = useState<Activity | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // a pack bus panel action runs (check, scan, measurement, End step): port, address and image wait
  const [panelBusy, setPanelBusy] = useState(false);
  useEffect(() => { onPanelBusy(panelBusy); }, [panelBusy]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => onPanelBusy(false), []); // eslint-disable-line react-hooks/exhaustive-deps
  // step 1 without a control channel, and the single-pack statement of that way (memory only)
  const [noControl, setNoControl] = useState(false);
  const [singlePack, setSinglePack] = useState(false);
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
        // the measurement's line at its button goes with it
        setActivity(dropMeasure);
        setNotice(d.block ? t(`maint.panel.block.${d.block}`, { pack: addressRef.current }) : null);
        setMasterOutcome(d.master);
      }),
      api.onFirmwareBusy(() => setCloseRefused(true)),
      api.onMaintProgress((ev) => setActivity((a) => (a && acceptsProgress(a, ev) ? { ...a, phase: ev } : a))),
    ];
    return () => { subs.forEach((s) => s.then((f) => f())); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const overview = maint.overview;
  // with an installation the RS485-1/2 port is the one of its setup (the single place to choose it)
  const port = overview ? packPortOf(overview.installation, oneAdapterOf(overview.installation.id)) ?? "" : ownPort;
  const portAvailable = ports.includes(port);
  useEffect(() => { setNoControl(false); setSinglePack(false); }, [overview?.installation.id]);
  // a new port, address or image: the measurement, the pack check and the typed confirmation no longer fit
  // the measurement is a property of the bus: only another port invalidates it; a new address or image
  // invalidates the pack check and the typed confirmation
  useEffect(() => { setMeasuredAt(null); }, [port]);
  useEffect(() => { setTarget(null); setCheckedFor(null); setTyped(""); }, [port, address, image?.sha256]);
  // a new measurement is needed after every state change; the inverter statement after every new session
  useEffect(() => { setMeasuredAt(null); }, [overview?.state_at, overview?.installation.id]);
  // one work phase: the statement survives the session its own activation opened (see keepsInverter)
  const phase = useRef<{ installation: string | null; session: number | null }>({ installation: null, session: null });
  useEffect(() => {
    const next = { installation: overview?.installation.id ?? null, session: overview?.session?.id ?? null };
    if (!keepsInverter(phase.current, next)) setInverter(false);
    phase.current = next;
  }, [overview?.session?.id, overview?.installation.id]);
  // after a lost or closed connection the operator confirms the inverter afresh and measures again
  useEffect(() => { setInverter(false); setMeasuredAt(null); setNoControl(false); setSinglePack(false); }, [disconnects]);
  // a result belongs to the installation it was reached for; a transient "no overview" (reload) keeps it
  useEffect(() => {
    const id = overview?.installation.id;
    if (id) setActivity((a) => (a && a.installation !== id ? null : a));
  }, [overview?.installation.id]);

  // which installation owns the pack port
  const lookupPort = () => {
    if (!port) { setPortInstallation(null); return Promise.resolve(); }
    return api.maintInstallationOf({ kind: "serial", path: port, bus: "pack" })
      .then((i) => setPortInstallation(i?.id ?? null)).catch(() => setPortInstallation(null));
  };
  useEffect(() => { void lookupPort(); }, [port, overview]); // eslint-disable-line react-hooks/exhaustive-deps
  // a foreign RS485-1/2 port locks every bus action (installation-mismatch) and offers to switch
  const lock = portLock(port ? portInstallation : null, overview?.installation.id ?? null, maint.installations);
  const portElsewhere = lock.owner === "other";

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
  const gate = canFlash({ overview, inverter, measuredAt, address, imageKind: image?.kind ?? "custom", portOwner: lock.owner });
  const flow = flowOf({
    overview, fresh: maint.freshFor(overview?.installation.id), noControl, singlePack, inverter, measuredAt, port, portAvailable,
    portOwner: lock.owner, address, imageKind: image?.kind ?? null,
    scan: maint.scanOf(overview?.installation.id, port), busy: busy || panelBusy, running,
  });
  const ready = flow.steps.flash.state === "current";
  const s = steps({ running, done: done !== null, port, address, image, target, checkedFor, typed, unlocked: gate.ok && !masterPending, ready, busy: busy || panelBusy });
  const inputsLocked = running || busy || panelBusy;
  const flowReason = !ready && !running && !done && flow.steps.flash.reason ? t(flow.steps.flash.reason.key, flow.steps.flash.reason.params) : null;
  const phrase = confirmationPhrase(address, image?.custom);

  const refreshPorts = <Button variant="ghost" size="icon" disabled={inputsLocked} onClick={loadPorts} aria-label={t("fw.refreshPorts")} title={t("fw.refreshPorts")}><RefreshCw className="h-4 w-4" /></Button>;
  // without an installation: the port select of a single pack without maintenance
  const portPicker = (
    <div className="flex flex-wrap items-center gap-3">
      <select className={input} value={ownPort} disabled={inputsLocked} onChange={(e) => setOwnPort(e.target.value)} aria-label={t("fw.port")}>
        <option value="">{t("fw.choosePort")}</option>
        {ports.map((p) => <option key={p}>{p}</option>)}
      </select>
      {refreshPorts}
    </div>
  );

  const chooseImage = async () => {
    setError(null); setImageError(null); setPending(null);
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, filters: [{ name: "EMU1101 firmware", extensions: ["ehex"] }] });
    if (typeof picked !== "string") return;
    // a phrase typed for the previous image may not fit the new one
    setImage(null); setPath(null); setTyped("");
    try { setImage(await api.firmwareCheckImage(picked, allowCustom, lang)); setPath(picked); }
    catch (e) {
      // refused only because it is not an allowlisted original? then say so here and keep the file
      if (!allowCustom) {
        try { setPending({ path: picked, info: await api.firmwareCheckImage(picked, true, lang) }); return; } catch { /* a real format error: show the first one */ }
      }
      setImageError(String(e));
    }
  };

  const toggleCustom = (v: boolean) => {
    setAllowCustom(v);
    if (v && pending) { setImage(pending.info); setPath(pending.path); setTyped(""); setPending(null); }
    if (!v) setPending(null);
    // switching off must not leave a custom image selected
    if (!v && image?.custom) { setImage(null); setPath(null); setTyped(""); }
  };

  const inspect = async () => {
    onUserAction(); setBusy(true); setError(null); setTarget(null); setCheckedFor(null);
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
    onUserAction();
    setFlashed({ image, checked: target?.firmware_version ?? null }); setMasterOutcome(null);
    // one confirmation, one run: the pack has to be checked and confirmed again for the next one
    setError(null); setNotice(null); setDone(null); setProgress(null); setCloseRefused(false); setTyped(""); setTarget(null); setCheckedFor(null);
    try {
      setRunning(true); onRunning(true); startedAt.current = Date.now();
      await api.firmwareStart(port, address, path, allowCustom, image.sha256, confirmation, lang,
        overview ? { installation: overview.installation.id, inverterConfirmed: inverter, dipOffConfirmed, isolatedConfirmed } : null);
      // the update-interrupted block is written now: the banner shows it at once
      void maint.loadLists();
    } catch (e) {
      setRunning(false); onRunning(false);
      // gate refusals arrive as MaintError JSON; the backend may have changed state (check, stop, session kind)
      const k = errorKey(e); setError(t(k.key, k.params)); void maint.refresh();
    }
  };

  const reset = () => { setDone(null); setProgress(null); setTyped(""); setTarget(null); setCheckedFor(null); setNotice(null); setMasterOutcome(null); };
  // the next pack of the open session: the firmware steps from the start (the port stays with the installation),
  // a fresh measurement and fresh master-plan confirmations; the inverter statement stays (same work phase)
  const nextPack = () => {
    reset();
    setAddress(0); setImage(null); setPath(null); setPending(null); setAllowCustom(false); setImageError(null); setError(null); setCloseRefused(false); setFlashed(null);
    setMeasuredAt(null); setActivity(dropMeasure); setDipOffConfirmed(false); setIsolatedConfirmed(false);
  };
  const versions = done ? resultVersions(done, flashed?.checked ?? null, flashed?.image ?? image, t) : null;
  const sessionOpen = !!overview?.session;
  const hint = gateHint(gate, { running, done: done !== null });

  if (isDemo) return <div className="max-w-2xl space-y-3"><h1 className="font-display text-2xl font-semibold">{t("fw.title")}</h1><p className="text-muted">{t("fw.demo")}</p></div>;

  return (
    <div className="max-w-2xl space-y-6">
      <header className="space-y-2">
        <h1 className="flex items-center gap-2 font-display text-2xl font-semibold">{t("fw.title")}<InfoIcon id="topic.firmware" /></h1>
        <p className="text-muted">{t("fw.intro")}</p>
      </header>

      <PackBusPanel key={`${overview?.installation.id ?? ""}:${overview?.session?.id ?? ""}`} maint={maint} flow={flow} port={port} ports={ports} onPortsChanged={loadPorts} monitor={monitor} focusEnd={focusEnd || ownFocusEnd} onFocusDone={() => { setOwnFocusEnd(0); onFocusDone(); }}
        busy={panelBusy} setBusy={setPanelBusy} inverter={inverter} setInverter={setInverter} measuredAt={measuredAt} setMeasuredAt={setMeasuredAt}
        noControl={noControl} setNoControl={setNoControl} singlePack={singlePack} setSinglePack={setSinglePack}
        activity={activity} setActivity={setActivity} onUserAction={onUserAction} mismatch={lock.switchTo} />

      <Step n={1} title={t("fw.s1")} active={!running && !done}>
        <div className="flex flex-wrap gap-3">
          {/* with an installation the port comes from its setup (pack bus panel, step 1); display only */}
          {!overview && portPicker}
          {overview && (
            <span className="flex items-center gap-1 text-sm">
              {port ? t("maint.flow.packPort", { port: portAvailable ? port : `${port} (${t("maint.flow.notPlugged")})` }) : t("maint.flow.noPackPort")}
              {refreshPorts}
            </span>
          )}
          <label className="flex items-center gap-2 text-sm text-muted">{t("fw.address")}
            <select className={input} value={address} disabled={inputsLocked} onChange={(e) => setAddress(Number(e.target.value))}>
              {Array.from({ length: 16 }, (_, a) => <option key={a} value={a}>{a}</option>)}
            </select>
          </label>
        </div>
        {overview && <p className="text-sm text-muted">{t("maint.flow.fwPortHint")}</p>}
        {portElsewhere && (
          <div className="flex flex-wrap items-center gap-3 text-sm text-alarm">
            <p>{t("maint.err.refused.installation-mismatch")}</p>
            {lock.switchTo && <Button variant="secondary" size="sm" disabled={inputsLocked || maint.switching} onClick={() => void maint.switchTo(lock.switchTo!.id)}>{t("maint.banner.switch", { other: lock.switchTo.name })}</Button>}
          </div>
        )}
        <p className="text-sm text-muted">{t("fw.busHint")}</p>
        {connected && <p className="text-sm text-discharge">{t("fw.monitorWillStop")}</p>}
      </Step>

      <Step n={2} title={t("fw.s2")} active={s.image}>
        <div className="flex flex-wrap items-center gap-4">
          <Button variant="secondary" disabled={!s.image} onClick={chooseImage}>{t("fw.chooseImage")}</Button>
          <label className="flex items-center gap-2 text-sm text-muted"><Switch checked={allowCustom} onChange={toggleCustom} disabled={!s.image} />{t("fw.allowCustom")}</label>
        </div>
        {image && <div className="text-sm"><p>{t("fw.imageInfo", { release: imageLabel(image, t, true), blocks: image.blocks, bytes: image.bytes })}</p><p className="break-all text-muted">{t("fw.sha", { sha: image.sha256 })}</p></div>}
        {image && imageWarning(image) && <p className="flex gap-2 rounded-md border border-alarm p-3 text-sm text-alarm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t(imageWarning(image)!)}</p>}
        {pending && <p className="flex gap-2 rounded-md border border-discharge p-3 text-sm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-discharge" />{t("fw.needsCustom", { release: imageLabel(pending.info, t, true) })}</p>}
        {imageError && <p className="text-sm text-alarm">{imageError}</p>}
      </Step>

      <Step n={3} title={t("fw.s3")} active={s.inspect}>
        <Button variant="secondary" disabled={!s.inspect || busy} onClick={inspect}>{busy ? t("fw.inspecting") : t("fw.inspect")}</Button>
        {flowReason && <p className="flex items-center gap-2 text-sm text-muted"><Lock className="h-4 w-4 shrink-0" />{t("maint.flow.fwLocked", { reason: flowReason })}</p>}
        {target && s.confirm && <p className="text-sm">{t("fw.target", { name: target.device_name, version: target.firmware_version, can: target.can_protocol })}</p>}
      </Step>

      <Step n={4} title={t("fw.s4")} active={s.confirm}>
        {s.confirm && image && target && <p className="font-medium">{t("fw.summary", { address, from: target.firmware_version, to: imageLabel(image, t) })}</p>}
        <p className="text-sm text-alarm">{t("fw.risks")}</p>
        <label className="block space-y-1 text-sm text-muted">{t("fw.typeToConfirm", { phrase })}
          <input className={cn(input, "block w-48 font-mono")} value={typed} disabled={!s.confirm} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} />
        </label>
        {overview && address === 0 && image && (
          <div className="space-y-1 text-sm">
            {planError && <p className="text-alarm">{planError}</p>}
            {/* the design: an open note on 0x23000 is shown again before a later patched flash (display only) */}
            {(() => {
              const note = planSectorNote(overview, address, image.kind);
              const key = note ? sectorNoteKey(note) : null;
              return note && <p role="alert" className="flex gap-2 rounded-md border border-discharge p-2 font-medium text-discharge"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t("maint.plan.sectorNote", { note: key ? t(key) : note })}</p>;
            })()}
            {plan && (
              <>
                {/* in plan order: a step confirmed by a checkbox stands in its position (e.g. DIP 5–8 OFF before stop and measuring) */}
                <ul className="list-inside list-disc space-y-1">
                  {planItems(plan.before).map(({ step, checkbox }) => checkbox ? (
                    <li key={step} className="list-none">
                      <label className="flex items-center gap-2"><input type="checkbox" checked={step === "dip-off" ? dipOffConfirmed : isolatedConfirmed}
                        onChange={(e) => (step === "dip-off" ? setDipOffConfirmed : setIsolatedConfirmed)(e.target.checked)} />{t(`maint.plan.${step}`)}</label>
                    </li>
                  ) : <li key={step}>{t(`maint.plan.${step}`)}</li>)}
                  <li>{t(`maint.plan.after.${plan.after}`)}</li>
                </ul>
                {plan.recovery && <p className="text-muted">{t("maint.plan.recovery")}</p>}
              </>
            )}
          </div>
        )}
        <Button variant="primary" disabled={!s.start} onClick={start}>{t("fw.start")}</Button>
        {hint && <p className="text-sm text-alarm">{t(errorKey(hint).key)}</p>}
        {flowReason && <p className="flex items-center gap-2 text-sm text-muted"><Lock className="h-4 w-4 shrink-0" />{t("maint.flow.fwLocked", { reason: flowReason })}</p>}
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
                <p className="flex items-center gap-2 font-medium text-ok"><CircleCheck className="h-5 w-5" />{t("fw.okTitle")}: {t("fw.okText", versions!)}</p>
              ) : (
                <>
                  <p className="flex items-center gap-2 font-medium text-alarm"><TriangleAlert className="h-5 w-5" />{t("fw.failTitle")}</p>
                  <p className="text-sm">{done.error}</p>
                  {done.stage && <p className="text-sm text-muted">{t("fw.failStage", { stage: t(`fw.stage.${done.stage}`, { n: (progress?.block ?? 0) + 1, total: progress?.total ?? 0 }) })}</p>}
                  <p className={cn("text-sm", done.written ? "text-alarm" : "text-muted")}>{done.stage === "post_check" ? t("fw.failPostCheck") : done.written ? t("fw.failAfter") : t("fw.failBefore")}</p>
                </>
              )}
              {masterOutcome && (masterOk(masterOutcome)
                ? <p className="flex items-center gap-2 text-sm text-ok"><CircleCheck className="h-4 w-4 shrink-0" />{t(`maint.master.${masterOutcome}`)}</p>
                : <p className="flex items-center gap-2 text-sm text-alarm"><TriangleAlert className="h-4 w-4 shrink-0" />{t(`maint.master.${masterOutcome}`)}</p>)}
              {notice && <p className="text-sm text-alarm">{notice}</p>}
              <p className="break-all text-sm text-muted">{t("fw.log")}: {done.log}</p>
              <div className="flex gap-2">
                <Button variant="ghost" size="sm" onClick={() => api.revealFirmwareLogs(done.log)}><FolderOpen className="h-4 w-4" />{t("fw.showLog")}</Button>
                {resultActions(sessionOpen).map((a) => a === "next" ? <Button key={a} variant="secondary" size="sm" onClick={nextPack}>{t("fw.next")}</Button>
                  : a === "end" ? <Button key={a} variant="primary" size="sm" onClick={() => setOwnFocusEnd(Date.now())}>{t("fw.endMaint")}</Button>
                  : a === "again" ? <Button key={a} variant="secondary" size="sm" onClick={reset}>{t("fw.again")}</Button>
                  : <Button key={a} variant="primary" size="sm" onClick={onBack}>{t("fw.back")}</Button>)}
              </div>
            </div>
          )}
        </Step>
      )}

      {error && <p className="text-sm text-alarm">{error}</p>}
    </div>
  );
}
