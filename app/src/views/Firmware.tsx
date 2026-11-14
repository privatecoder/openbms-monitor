import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, Cpu, FolderOpen, TriangleAlert } from "lucide-react";
import { accessHint } from "../access";
import { api, isDemo, type BenchScan, type FirmwareDone, type FirmwareImage, type FirmwareProgress, type FlashComparison, type SocStoreRead } from "../api";
import { SocLines } from "../components/RemoteDiagnosis";
import { Button } from "../components/ui/button";
import { Switch } from "../components/PackPicker";
import { useNow } from "../components/live";
import { InfoIcon } from "../help";
import { GuideButton } from "../guides";
import { cn } from "../lib/utils";
import { decideUi, lockProps, type UiAction, type UiFacts } from "../locks";
import { benchMarkerKey, benchPick, compareLines, confirmationPhrase, recoveryPhrase, flashGateKeys, imageLabel, imageWarning, imageWarningIsAlarm, progressFraction, resultVersions } from "../firmware";
import { chargeLine } from "../control";
import { useWriteTexts } from "../components/ParamEdit";
import { errorKey, packPortOf, portKey, seriesOpen, type PortKind } from "../maint";
import type { Maint } from "../maintStore";
import { KindSelect, PortInput } from "./MaintSetup";
import { EndIdleSeries, PackBusPanel } from "./PackBusPanel";

export function Firmware({ connected, maint, onMonitorStopped, onRunning, onBack, onUserAction, onEditPorts }: {
  connected: boolean;
  /** open the installations view, where the ports are edited */
  onEditPorts: () => void;
  maint: Maint;
  onMonitorStopped: () => void;
  onRunning: (running: boolean) => void;
  onBack: () => void;
  /** an action on the bus starts here or in the panel (the sidebar stops waiting for a busy port) */
  onUserAction: () => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [ports, setPorts] = useState<string[]>([]);
  // „Platine flashen“: its RS485-1/2 port, independent of any installation
  const [benchPort, setBenchPort] = useState<{ kind: PortKind; target: string }>({ kind: "serial", target: "" });
  const [scan, setScan] = useState<BenchScan | null>(null);
  const [scanning, setScanning] = useState(false);
  // every search starts a new generation: what was read for the earlier one (Group0) no longer counts
  const [searchGen, setSearchGen] = useState(0);
  const [address, setAddress] = useState(0);
  const [path, setPath] = useState<string | null>(null);
  const [image, setImage] = useState<FirmwareImage | null>(null);
  // not remembered: every visit starts with the vendor allowlist only
  const [allowCustom, setAllowCustom] = useState(false);
  // a valid image that needs "allow custom firmware": kept so switching it on takes the same file
  const [pending, setPending] = useState<{ path: string; info: FirmwareImage } | null>(null);
  const [imageError, setImageError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<FirmwareProgress | null>(null);
  const [done, setDone] = useState<FirmwareDone | null>(null);
  // what the last run flashed: the pack, the image and the version known before (for the result's version line)
  const [flashed, setFlashed] = useState<{ address: number; image: FirmwareImage; checked: string | null; bank: string; marker?: string; bench: boolean; recovery?: boolean } | null>(null);
  // recovery of a board waiting in its bootloader: the probe's result on the bench port, and the operator's
  // confirmation that exactly one board is on the line
  const [recProbe, setRecProbe] = useState<"probing" | "ok" | string | null>(null);
  const [oneBoard, setOneBoard] = useState(false);
  const [flashTick, setFlashTick] = useState(0);
  const [tab, setTab] = useState<"bus" | "bench">(maint.current ? "bus" : "bench");
  const [closeRefused, setCloseRefused] = useState(false);
  const startedAt = useRef(0);
  const now = useNow(500);
  // a flash series runs (from its confirmation to its end): the port stays held between its packs, the inputs fixed;
  // `doneWait` hands a series pack's end event to the series
  const seriesOn = useRef(false);
  const seriesRun = useRef<number | null>(null);
  const [inSeries, setInSeries] = useState(false);
  // the last refusal was for a series left open: „Offene Serie beenden“ is offered
  const [orphan, setOrphan] = useState(false);
  const doneWait = useRef<((d: FirmwareDone) => void) | null>(null);

  const loadPorts = () => { api.listPorts().then(setPorts).catch(() => setPorts([])); };
  useEffect(loadPorts, []);
  useEffect(() => {
    const subs = [
      api.onFirmwareProgress(setProgress),
      api.onFirmwareDone((d) => {
        setDone(d); setRunning(false); onRunning(seriesOn.current); setFlashTick((n) => n + 1); setScan(null); void maint.loadLists();
        const wait = doneWait.current; doneWait.current = null; wait?.(d);
      }),
      api.onFirmwareBusy(() => setCloseRefused(true)),
    ];
    return () => { subs.forEach((s) => s.then((f) => f())); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const inst = maint.current;
  // the RS485-1/2 port of the installation as its endpoint key (the pack bus panel)
  const port = inst ? packPortOf(inst) ?? "" : "";
  const bPort = benchPort.target.trim() ? portKey(benchPort.kind, benchPort.target.trim()) : "";
  // a search belongs to the port it ran on
  useEffect(() => { setScan(null); setError(null); setRecProbe(null); setOneBoard(false); }, [bPort]);
  // a search or a bootloader probe holds the port
  const busy = scanning || recProbe === "probing";
  // what may be done now (`locks.ts`)
  const fw: UiFacts = { source: null, chosen: inst?.id ?? null, flashRunning: running || inSeries, busy, image: !!image, port: !!bPort };
  const may = (a: UiAction, more: Partial<UiFacts> = {}) => lockProps(decideUi(a, { ...fw, ...more }), t);
  const inputsLocked = decideUi("firmware.inputs", fw).locked;
  const phrase = confirmationPhrase(address, image?.custom);
  const owner = inst && bPort && bPort === port ? inst : null;

  const chooseImage = async () => {
    setError(null); setImageError(null); setPending(null);
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, filters: [{ name: "EMU1101 firmware", extensions: ["ehex"] }] });
    if (typeof picked !== "string") return;
    setImage(null); setPath(null);
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
    if (v && pending) { setImage(pending.info); setPath(pending.path); setPending(null); }
    if (!v) setPending(null);
    // switching off must not leave a custom image selected
    if (!v && image?.custom) { setImage(null); setPath(null); }
  };

  /** Start a flash (panel or board): refusals are thrown back as the backend sent them. `installation`: the panel's,
   *  null for „Platine flashen“ (`expected`: the String 2 the search showed). `series`: a pack of the confirmed series
   *  (its list and the phrase typed for it). */
  const startFlash = async (p: string, a: number, from: string | null, installation: string | null, expected: string | null, series?: { run: number; listed: number[]; phrase: string }) => {
    if (!path || !image) { if (series) throw new Error("no image"); return; }
    // the backend stops the monitor (it may have been reconnected meanwhile)
    if (connected) onMonitorStopped();
    onUserAction();
    setError(null); setDone(null); setProgress(null); setCloseRefused(false);
    setRunning(true); onRunning(true); startedAt.current = Date.now();
    try {
      const r = series
        ? await api.firmwareStart(p, a, path, allowCustom, image.sha256, series.phrase, lang, installation, expected, { run: series.run, listed: series.listed })
        : await api.firmwareStart(p, a, path, allowCustom, image.sha256, confirmationPhrase(a, image.custom), lang, installation, expected);
      setFlashed({ address: a, image, checked: from, bank: r.bank, marker: r.marker, bench: installation === null });
      // the pending record is written now
      void maint.loadLists();
    } catch (e) {
      setRunning(false); onRunning(seriesOn.current);
      throw e;
    }
  };

  // the flash series of the pack bus panel: confirmed once in the backend, then each pack through `startFlash`
  const series = inst && path && image ? {
    begin: async (addresses: number[], phrase: string) => {
      // held from the request on: nothing else starts while the backend confirms; undone by `seriesEnd`
      seriesOn.current = true; setInSeries(true); onRunning(true);
      const r = await api.firmwareSeriesBegin(port, inst.id, addresses, path, allowCustom, image.sha256, phrase, lang);
      seriesRun.current = r.run;
      return r.order;
    },
    flash: (a: number, from: string | null, listed: number[], phrase: string) => new Promise<FirmwareDone>((resolve, reject) => {
      const run = seriesRun.current;
      if (run === null) { reject(new Error("no series")); return; }
      doneWait.current = resolve;
      startFlash(port, a, from, inst.id, null, { run, listed, phrase }).catch((e) => { doneWait.current = null; reject(e); });
    }),
  } : null;
  // after the series in any case (done, stopped, cancelled, a refused begin); never during a flash
  const seriesEnd = () => {
    seriesOn.current = false; setInSeries(false); doneWait.current = null; onRunning(false);
    const run = seriesRun.current;
    seriesRun.current = null;
    if (run !== null) void api.firmwareSeriesEnd(run);
  };
  // the view goes away: its series' grant goes with it (single flashes wait while one is open)
  useEffect(() => () => { if (seriesRun.current !== null) void api.firmwareSeriesEnd(seriesRun.current); }, []);

  const search = async () => {
    onUserAction(); setScanning(true); setError(null); setScan(null); setSearchGen((g) => g + 1);
    // the backend drops the monitor connection first; the UI must forget it too
    if (connected) onMonitorStopped();
    try {
      const r = await api.benchScan(bPort);
      setScan(r);
      const one = benchPick(r.packs);
      if (one !== null) setAddress(one);
      void maint.loadLists();
    } catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
    finally { setScanning(false); }
  };

  const chosen = scan?.packs.find((p) => p.address === address) ?? null;
  const benchFlash = async () => {
    if (!chosen) return;
    setOrphan(false);
    try { await startFlash(bPort, chosen.address, chosen.info.firmware_version, null, chosen.serial); }
    catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); setOrphan(seriesOpen(e)); }
  };
  const markerKey = benchMarkerKey(chosen, image);

  const probeBootloader = async () => {
    onUserAction(); setRecProbe("probing"); setOneBoard(false); setError(null);
    if (connected) onMonitorStopped();
    try { await api.firmwareRecoveryProbe(bPort, lang); setRecProbe("ok"); }
    catch (e) { const k = errorKey(e); setRecProbe(t(k.key, k.params)); }
  };

  const recover = async () => {
    if (!path || !image || recProbe !== "ok" || !oneBoard) return;
    if (connected) onMonitorStopped();
    onUserAction();
    setError(null); setOrphan(false); setDone(null); setProgress(null); setCloseRefused(false);
    setRunning(true); onRunning(true); startedAt.current = Date.now();
    try {
      await api.firmwareRecoveryStart(bPort, path, allowCustom, image.sha256, recoveryPhrase(image.custom), lang);
      setFlashed({ address: 0, image, checked: null, bank: "none", bench: true, recovery: true });
      setRecProbe(null); setOneBoard(false);
    } catch (e) {
      setRunning(false); onRunning(false);
      const k = errorKey(e); setError(t(k.key, k.params)); setOrphan(seriesOpen(e));
    }
  };

  const reset = () => { setDone(null); setProgress(null); setError(null); setFlashed(null); };
  const versions = done && flashed ? resultVersions(done, flashed.checked, flashed.image, t) : null;

  if (isDemo) return <div className="view-in mx-auto max-w-7xl space-y-3"><h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight"><Cpu className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("fw.title")}</h1><p className="text-muted">{t("fw.demo")}</p></div>;

  const imagePicker = (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-4">
        <Button variant="secondary" size="sm" {...may("firmware.inputs")} onClick={chooseImage}>{t("fw.chooseImage")}</Button>
        {/* like every other input: fixed while a flash (or a check) runs */}
        <label className={cn("flex items-center gap-2 text-sm text-muted", inputsLocked && "opacity-60")}><Switch checked={allowCustom} onChange={toggleCustom} disabled={inputsLocked} />{t("fw.allowCustom")}</label>
      </div>
      {image && <div className="text-sm"><p>{t("fw.imageInfo", { release: imageLabel(image, t, true), blocks: image.blocks, bytes: image.bytes })}</p><p className="break-all text-muted">{t("fw.sha", { sha: image.sha256 })}</p></div>}
      {image && imageWarning(image) && (imageWarningIsAlarm(image)
        ? <p className="flex gap-2 rounded-md border border-alarm p-3 text-sm text-alarm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t(imageWarning(image)!)}</p>
        : <p className="flex gap-2 rounded-md border border-discharge p-3 text-sm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-discharge" />{t(imageWarning(image)!)}</p>)}
      {/* the same colour before and after „Eigene Firmware zulassen“: orange for a known maintenance build, red for an unknown checksum */}
      {pending && (imageWarningIsAlarm(pending.info)
        ? <p className="flex gap-2 rounded-md border border-alarm p-3 text-sm text-alarm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t("fw.needsCustom", { release: imageLabel(pending.info, t, true) })}</p>
        : <p className="flex gap-2 rounded-md border border-discharge p-3 text-sm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-discharge" />{t("fw.needsCustom", { release: imageLabel(pending.info, t, true) })}</p>)}
      {imageError && <p className="text-sm text-alarm">{imageError}</p>}
      {/* bound to the port and board it reads: another one starts without a fresh value */}
      {image?.kind === "maintenance" && <PatchImageNotes key={tab === "bus" ? `bus|${port}|${address}` : `bench|${searchGen}|${bPort}|${address}|${chosen?.serial ?? ""}`} inst={tab === "bus" ? inst?.id ?? null : null} port={tab === "bus" ? port : bPort} address={address} release={image.release} powerRtc={!!image.power_rtc} packCutoff={!!image.pack_cutoff} positive={!!image.positive} locked={inputsLocked}
        latent={tab === "bus" && !!inst && maint.alerts.markers.some((m) => m.installation === inst.id && m.latent)} />}
      {/* the bench target; on the pack bus each pack row reads its own (`PackBusPanel`) */}
      {image && tab === "bench" && <FlashSocNote key={`bench|${searchGen}|${bPort}|${address}|${chosen?.serial ?? ""}`} port={bPort} address={address} locked={inputsLocked} />}
    </div>
  );

  return (
    <div className="view-in mx-auto max-w-7xl space-y-6">
      <header className="space-y-2">
        <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight">
          <Cpu className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("fw.title")}<InfoIcon id="topic.firmware" /><GuideButton place="firmware" />
        </h1>
        <p className="mt-1 max-w-3xl text-muted">{t("fw.intro")}</p>
        {connected && <p className="text-sm text-discharge">{t("fw.monitorWillStop")}</p>}
      </header>

      {/* two ways to flash, one image: the pack bus panel (live status, packs) or one pack by hand */}
      <div className="flex rounded-md bg-sunken p-0.5 text-sm" role="group">
        {(["bus", "bench"] as const).map((k) => (
          <button key={k} aria-pressed={tab === k} {...may("firmware.tab")} onClick={() => setTab(k)}
            className={cn("flex-1 rounded px-2 py-1 disabled:opacity-50", tab === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>{t(`fw.tab.${k}`)}</button>
        ))}
      </div>

      {tab === "bus" && <PackBusPanel maint={maint} port={port} ports={ports} onEditPorts={onEditPorts} imagePicker={imagePicker} image={image}
        running={running} flashTick={flashTick} onUserAction={onUserAction} series={series} onSeriesEnd={seriesEnd}
        onFlash={(a, from) => inst ? startFlash(port, a, from, inst.id, null) : Promise.resolve()} onMaster={() => {}} />}

      {/* one board without an installation: any RS485-1/2 port, a search with String 2 and, where the build shows it
          there, the marker; the same flash checks in the backend */}
      {tab === "bench" && <section className="space-y-3 rounded-md border border-line p-4">
        <p className="text-sm text-muted">{t("fw.bench.hint")}</p>
        <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
          <span className="flex flex-wrap items-center gap-2 text-muted">{t("fw.bench.port")}
            <KindSelect value={benchPort.kind} disabled={inputsLocked} onChange={(k) => setBenchPort({ kind: k, target: "" })} label={t("maint.setup.rs485Kind")} />
            <PortInput kind={benchPort.kind} value={benchPort.target} ports={ports} onRefresh={loadPorts} onChange={(v) => setBenchPort({ kind: benchPort.kind, target: v })} label={t("fw.bench.port")} disabled={inputsLocked} />
          </span>
        </div>
        {owner && <p className="text-sm text-discharge">{t("fw.bench.useBus", { name: owner.name })}</p>}
        {imagePicker}
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="secondary" {...may("firmware.benchSearch")} onClick={() => void search()}>{scanning ? t("fw.bench.searching") : t("fw.bench.search")}</Button>
          {scanning && <Button variant="ghost" size="sm" onClick={() => void api.maintScanCancel()}>{t("maint.packs.cancel")}</Button>}
        </div>
        {scan && scan.traffic > 0 && <p className="text-sm text-discharge">{t("fw.bench.traffic", { bytes: scan.traffic })}</p>}
        {scan && scan.traffic === 0 && scan.packs.length === 0 && <p className="text-sm text-muted">{t("fw.bench.none")}</p>}
        {scan?.cancelled && <p className="text-sm text-muted">{t("maint.packs.cancelled")}</p>}
        {scan && scan.packs.length > 1 && <p className="flex gap-2 text-sm text-discharge"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t("fw.bench.several")}</p>}
        {scan && scan.packs.length > 0 && (
          <div className="space-y-1 text-sm" role="radiogroup">
            {scan.packs.map((p) => (
              <label key={p.address} className="flex items-center gap-2">
                <input type="radio" name="bench-pack" checked={address === p.address} disabled={inputsLocked} onChange={() => setAddress(p.address)} />
                <span className="font-medium">{t("maint.packs.pack", { address: p.address })}</span>
                <span className="text-muted">{p.info.device_name} · {p.info.firmware_version} · {p.serial ? t("fw.bench.serial", { serial: p.serial }) : t("fw.bench.noSerial")}
                  {p.version && ` · ${p.version}`}{p.marker && ` · ${t(`fw.bench.marker.${p.marker}`)}`}</span>
              </label>
            ))}
          </div>
        )}
        {markerKey && <p className={cn("text-sm", markerKey === "fw.bench.markerOn" ? "text-ink" : "text-muted")}>{t(markerKey)}</p>}
        {chosen && image && <p className="font-medium">{t("fw.summary", { address: chosen.address, from: chosen.info.firmware_version, to: imageLabel(image, t) })}</p>}
        <p className="text-sm text-alarm">{t("fw.risks")}</p>
        {benchPort.kind === "tcp" && <p className="text-sm text-discharge">{t("fw.tcpUntested")}</p>}
        <Button variant="primary" disabled={decideUi("firmware.benchFlash", { ...fw, target: !!chosen }).locked} title={phrase} onClick={() => void benchFlash()}>{t("fw.bench.flash", { address: chosen?.address ?? address })}</Button>
        {error && <p className="text-sm text-alarm">{error}</p>}
        {orphan && <EndIdleSeries busy={busy} running={running || inSeries} onEnded={() => { setOrphan(false); setError(null); }} />}
        {/* a board whose application is gone answers no search: its bootloader may still wait on the line */}
        <details className="rounded-md border border-line p-3 text-sm" open={recProbe !== null || (scan !== null && scan.packs.length === 0)}>
          <summary className="cursor-pointer font-medium">{t("fw.rec.title")}</summary>
          <div className="mt-2 space-y-2">
            <p className="text-muted">{t("fw.rec.hint")}</p>
            <Button variant="secondary" size="sm" {...may("firmware.recoverProbe")} onClick={() => void probeBootloader()}>
              {recProbe === "probing" ? t("fw.rec.probing") : t("fw.rec.probe")}</Button>
            {recProbe && recProbe !== "probing" && recProbe !== "ok" && <p className="text-alarm">{recProbe}</p>}
            {recProbe === "ok" && (
              <>
                <p>{t("fw.rec.found")}</p>
                <label className="flex items-start gap-2"><input type="checkbox" className="mt-1" checked={oneBoard} disabled={inputsLocked} onChange={(e) => setOneBoard(e.target.checked)} />{t("fw.rec.oneBoard")}</label>
                <Button variant="primary" size="sm" disabled={decideUi("firmware.recover", { ...fw, target: oneBoard }).locked} title={recoveryPhrase(image?.custom)} onClick={() => void recover()}>{t("fw.rec.start")}</Button>
                {!image && <p className="text-muted">{t("fw.rec.needImage")}</p>}
              </>
            )}
          </div>
        </details>
      </section>}

      {/* progress and result below the tab that started the flash */}
      {(running || done) && (
        <section className="space-y-3 border-t border-line pt-5">
          <h2 className="font-display text-lg font-semibold">{flashed?.recovery ? t("fw.rec.runTitle") : flashed ? t("fw.runTitle", { address: flashed.address }) : t("fw.s5")}</h2>
          {flashed && flashed.bank !== "none" && <p className="text-sm text-muted">{t(`maint.bank.flash.${flashed.bank}`)}</p>}
          {flashed?.marker === "unchecked" && (!flashed.bench || flashed.address === 0) && <p className="text-sm text-discharge">{t(flashed.bench ? "fw.bench.markerUnchecked" : "fw.markerUnchecked")}</p>}
          {flashed?.marker === "released" && <p className="text-sm text-muted">{t("fw.bench.markerReleased")}</p>}
          {flashed?.marker === "latent" && <p className="text-sm text-alarm">{t("fw.markerLatent")}</p>}
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
              {done.ok && done.recovery ? (
                <p className="flex items-center gap-2 font-medium text-ok"><CircleCheck className="h-5 w-5" />
                  {done.recovered_at !== null && done.recovered_at !== undefined
                    ? t("fw.rec.okFound", { address: done.recovered_at, version: done.after?.firmware_version ?? "?" })
                    : t("fw.rec.okUnchecked")}</p>
              ) : done.ok ? (
                <p className="flex items-center gap-2 font-medium text-ok"><CircleCheck className="h-5 w-5" />{t("fw.okTitle")}: {t("fw.okText", versions ?? { from: "?", to: "?" })}</p>
              ) : (
                <>
                  <p className="flex items-center gap-2 font-medium text-alarm"><TriangleAlert className="h-5 w-5" />{t("fw.failTitle")}</p>
                  <p className="text-sm">{done.error}</p>
                  {done.stage && <p className="text-sm text-muted">{t("fw.failStage", { stage: t(`fw.stage.${done.stage}`, { n: (progress?.block ?? 0) + 1, total: progress?.total ?? 0 }) })}</p>}
                  <p className={cn("text-sm", done.written ? "text-alarm" : "text-muted")}>{done.stage === "post_check" ? t("fw.failPostCheck") : done.written ? t("fw.failAfter") : done.recovery ? t("fw.rec.failBefore") : t("fw.failBefore")}</p>
                </>
              )}
              {/* the image this run flashed, not the one chosen since */}
              {done.comparison && <FlashCompare c={done.comparison} image={flashed?.image ?? null} />}
              {done.pending && flashed && <p className="text-sm text-alarm">{t("fw.pendingAfter", { pack: flashed.address })}</p>}
              {done.log_error && <p className="text-sm text-alarm">{t("fw.logError", { err: done.log_error })}</p>}
              <p className="break-all text-sm text-muted">{t("fw.log")}: {done.log}</p>
              <div className="flex gap-2">
                <Button variant="ghost" size="sm" onClick={() => api.revealFirmwareLogs(done.log)}><FolderOpen className="h-4 w-4" />{t("fw.showLog")}</Button>
                <Button variant="secondary" size="sm" onClick={reset}>{t("fw.again")}</Button>
                <Button variant="primary" size="sm" {...may("app.navigate", { portHeld: inSeries })} onClick={onBack}>{t("fw.back")}</Button>
              </div>
            </div>
          )}
        </section>
      )}
    </div>
  );
}

const toneClass = { ok: "text-ok", warn: "text-discharge", alarm: "text-alarm", muted: "text-muted" } as const;

/** The target before and once back (`compareLines`): shown, nothing acts on it. */
function FlashCompare({ c, image }: { c: FlashComparison; image: FirmwareImage | null }) {
  const { t, i18n } = useTranslation();
  const { pname } = useWriteTexts();
  const fmt = (v: number, d: number) => new Intl.NumberFormat(i18n.language, { minimumFractionDigits: d, maximumFractionDigits: d }).format(v);
  return (
    <div className="space-y-1 rounded border border-line p-3 text-sm">
      <p className="font-medium">{t("fw.cmp.title")}</p>
      {compareLines(c, image, t).map((l, i) => (
        <p key={i} className={cn(toneClass[l.tone], l.param !== undefined && "pl-4")}>
          {l.param !== undefined ? `${pname(l.param)}: ` : ""}{t(l.key, l.vars)}
        </p>
      ))}
      {c.charge === null ? <p className="text-muted">{t("fw.cmp.chargeUnchecked")}</p> : (
        <>
          <p className={c.charge ? toneClass.warn : toneClass.muted}>{chargeLine(c.before.charge, c.after.charge, fmt, t)}</p>
          {c.charge && <p className="text-discharge">{t("ctl.charge.hint")}</p>}
        </>
      )}
    </div>
  );
}

/** Before a flash: the target's stored state against its live one (0x64/06, read-capability bits), read on demand on the flash
 *  target itself (its port and address, quiet bus), bound to them by the key above. Only information: a flash restarts the
 *  board, which starts from its stored state; nothing here is a condition for flashing. */
function FlashSocNote({ port, address, locked }: { port: string; address: number; locked: boolean }) {
  const { t } = useTranslation();
  const [read, setRead] = useState<SocStoreRead | null | undefined>(undefined);
  const [reading, setReading] = useState(false);
  const readNow = async () => {
    setReading(true);
    try { setRead(await api.firmwareSocStore(port, address)); } catch { setRead(null); } finally { setReading(false); }
  };
  return (
    <div className="space-y-1 rounded-md border border-line p-3 text-sm">
      <p className="flex items-center gap-1 font-medium">{t("fw.soc.title")}<InfoIcon id="topic.remoteDiagnosis" /></p>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="ghost" size="sm" disabled={decideUi("firmware.readTarget", { source: null, chosen: null, portHeld: locked, busy: reading, port: !!port }).locked} onClick={() => void readNow()}>{reading ? t("rdiag.reading") : t("fw.soc.read")}</Button>
      </div>
      {read === null && <p className="text-muted">{t("fw.soc.unavailable")}</p>}
      {read && <SocLines read={read} />}
      <p className="text-muted">{accessHint(t, "fw.soc.hint")}</p>
    </div>
  );
}

/** Before flashing an image with the patch functions: the update way, and the target's release bits, which the flash
 *  takes over unchanged (a fresh read on the pack bus, else the last stored block with its age, not checked). */
function PatchImageNotes({ inst, port, address, release, powerRtc, packCutoff, positive, locked, latent }: { inst: string | null; port: string; address: number; release: string; powerRtc: boolean; packCutoff: boolean; positive: boolean; locked: boolean; latent: boolean }) {
  const { t, i18n } = useTranslation();
  const [fresh, setFresh] = useState<number | null | undefined>(undefined);
  const [snap, setSnap] = useState<{ g0: number; at: number } | null>(null);
  const [reading, setReading] = useState(false);
  useEffect(() => {
    setFresh(undefined); setSnap(null);
    if (!inst) return;
    let alive = true;
    api.installationSnapshots(inst).then((rows) => {
      const r = rows.find((x) => x.address === address && x.parameters);
      if (alive && r?.parameters) setSnap({ g0: r.parameters.function_switches[0], at: r.at });
    }, () => {});
    return () => { alive = false; };
  }, [inst, address]);
  const readNow = async () => {
    setReading(true);
    try { setFresh(await api.firmwareGroup0(port, address)); } catch { setFresh(null); } finally { setReading(false); }
  };
  const g0 = fresh ?? snap?.g0 ?? null;
  const keys = g0 === null ? [] : flashGateKeys(g0, powerRtc, packCutoff, positive);
  return (
    <div className="space-y-1 rounded-md border border-line p-3 text-sm">
      <p className="flex items-center gap-1 font-medium">{t("fw.patch.title", { release })}<InfoIcon id="topic.patchGates" /></p>
      <p className="text-muted">{accessHint(t, "fw.patch.update")}</p>
      {latent && <p className="text-alarm">{t("fw.markerLatent")}</p>}
      {/* the pack chosen above (Pack-Adresse) */}
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="ghost" size="sm" disabled={decideUi("firmware.readTarget", { source: null, chosen: null, portHeld: locked, busy: reading, port: !!port }).locked} onClick={() => void readNow()}>{t("fw.patch.readNow")}</Button>
      </div>
      {fresh !== undefined && fresh !== null && <p>{t("fw.patch.fresh", { value: fresh.toString(16).toUpperCase().padStart(2, "0") })}</p>}
      {fresh === null && <p className="text-muted">{t("fw.patch.unread")}</p>}
      {(fresh === undefined || fresh === null) && snap && (
        <p className="text-discharge">{t("fw.patch.snapshot", { value: snap.g0.toString(16).toUpperCase().padStart(2, "0"), at: new Date(snap.at).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" }) })}</p>
      )}
      {g0 === null && fresh !== undefined && !snap && <p className="text-muted">{t("fw.patch.none")}</p>}
      {keys.map((k) => <p key={k} className={k === "fw.patch.ok" || k === "fw.patch.powerOpen" || k === "fw.patch.cutoffOff" ? "text-ok" : k === "fw.patch.lockKept" ? "text-alarm" : "text-discharge"}>{t(k)}</p>)}
      <p className="text-muted">{t("fw.patch.wayBack")}</p>
    </div>
  );
}
