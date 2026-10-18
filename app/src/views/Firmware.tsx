import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, Cpu, FolderOpen, TriangleAlert } from "lucide-react";
import { api, isDemo, type DeviceInfo, type FirmwareDone, type FirmwareImage, type FirmwareProgress, type MasterView } from "../api";
import { Button } from "../components/ui/button";
import { Switch } from "../components/PackPicker";
import { useNow } from "../components/live";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { confirmationPhrase, flashGateKeys, imageLabel, imageWarning, imageWarningIsAlarm, progressFraction, resultVersions, steps } from "../firmware";
import { errorKey, keyParts, otherBank, packPortOf, packVersion, portKey, releaseFirst, type PortKind } from "../maint";
import type { Maint } from "../maintStore";
import { KindSelect, PortInput } from "./MaintSetup";
import { PackBusPanel } from "./PackBusPanel";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

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
  // the single pack tab's own port, independent of the installation (starts with the panel's)
  const [manualPort, setManualPort] = useState<{ kind: PortKind; target: string } | null>(null);
  const [address, setAddress] = useState(0);
  const [path, setPath] = useState<string | null>(null);
  const [image, setImage] = useState<FirmwareImage | null>(null);
  // not remembered: every visit starts with the vendor allowlist only
  const [allowCustom, setAllowCustom] = useState(false);
  // a valid image that needs "allow custom firmware": kept so switching it on takes the same file
  const [pending, setPending] = useState<{ path: string; info: FirmwareImage } | null>(null);
  const [imageError, setImageError] = useState<string | null>(null);
  const [target, setTarget] = useState<DeviceInfo | null>(null);
  const [master, setMaster] = useState<MasterView | null>(null);
  const [checkedFor, setCheckedFor] = useState<{ port: string; address: number; inst: string | null } | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ text: string; serial: string | null } | null>(null);
  // the answer of „Pack prüfen“ goes next to that button, the flash's refusals next to its own
  const [inspectError, setInspectError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<FirmwareProgress | null>(null);
  const [done, setDone] = useState<FirmwareDone | null>(null);
  // what the last run flashed: the pack, the image and the version known before (for the result's version line)
  const [flashed, setFlashed] = useState<{ address: number; image: FirmwareImage; checked: string | null; bank: string; marker?: string } | null>(null);
  const [flashTick, setFlashTick] = useState(0);
  const [tab, setTab] = useState<"bus" | "manual">("bus");
  const [closeRefused, setCloseRefused] = useState(false);
  const startedAt = useRef(0);
  const now = useNow(500);

  const loadPorts = () => { api.listPorts().then(setPorts).catch(() => setPorts([])); };
  useEffect(loadPorts, []);
  useEffect(() => {
    const subs = [
      api.onFirmwareProgress(setProgress),
      api.onFirmwareDone((d) => { setDone(d); setRunning(false); onRunning(false); setFlashTick((n) => n + 1); setMaster(null); void maint.loadLists(); }),
      api.onFirmwareBusy(() => setCloseRefused(true)),
    ];
    return () => { subs.forEach((s) => s.then((f) => f())); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const inst = maint.current;
  // the RS485-1/2 port of the installation as its endpoint key; flashing needs an installation (its log and checks)
  const port = inst ? packPortOf(inst) ?? "" : "";
  const manual = manualPort ?? keyParts(port) ?? { kind: "serial" as PortKind, target: "" };
  const mPort = manualPort ? portKey(manualPort.kind, manualPort.target) : port;
  useEffect(() => { setTarget(null); setCheckedFor(null); setTyped(""); }, [mPort, address, image?.sha256]);
  // another installation: the single pack tab's port, check and confirmation belonged to the earlier one; the port
  // falls back to the new installation's own (shown in the field)
  useEffect(() => { setManualPort(null); setTarget(null); setCheckedFor(null); setTyped(""); setError(null); setInspectError(null); }, [inst?.id]);
  // the master read belongs to the port it came from
  useEffect(() => { setMaster(null); }, [port]);
  const s = steps({ running, done: done !== null, port: mPort, address, image, target, checkedFor, typed, busy, inst: inst?.id ?? null });
  const inputsLocked = running || busy;
  const phrase = confirmationPhrase(address, image?.custom);

  const chooseImage = async () => {
    setError(null); setInspectError(null); setImageError(null); setPending(null);
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

  /** Start a flash (panel or manual): refusals are thrown back as the backend sent them. */
  const startFlash = async (p: string, a: number, confirmation: string, from: string | null, checks: boolean) => {
    if (!path || !image || !inst) return;
    // the backend stops the monitor (it may have been reconnected meanwhile)
    if (connected) onMonitorStopped();
    onUserAction();
    setError(null); setDone(null); setProgress(null); setCloseRefused(false);
    setRunning(true); onRunning(true); startedAt.current = Date.now();
    try {
      const r = await api.firmwareStart(p, a, path, allowCustom, image.sha256, confirmation, lang, inst.id, checks);
      setFlashed({ address: a, image, checked: from, bank: r.bank, marker: r.marker });
      // the pending record is written now
      void maint.loadLists();
    } catch (e) {
      setRunning(false); onRunning(false);
      throw e;
    }
  };

  const inspect = async () => {
    onUserAction(); setBusy(true); setError(null); setInspectError(null); setTarget(null); setCheckedFor(null);
    // the backend drops the monitor connection first; the UI must forget it too
    if (connected) onMonitorStopped();
    try { setTarget(await api.firmwareInspect(mPort, address, lang, mPort === port ? inst?.id ?? null : null)); setCheckedFor({ port: mPort, address, inst: inst?.id ?? null }); void maint.loadLists(); }
    catch (e) { setInspectError(String(e)); }
    finally { setBusy(false); }
  };

  // the master read and the installation's checks count only on the installation's own port
  const samePort = mPort === port;
  // C': a master with its marker on needs „Master freigeben“ before an image without maintenance functions; the master
  // belongs to the installation, whatever pack bus endpoint is chosen (the backend checks again)
  const release = releaseFirst(master, address, image?.kind ?? null);
  const targetVersion = target ? packVersion(address, target.firmware_version, samePort ? master : null) : null;
  const manualStart = async () => {
    const confirmation = typed;
    // a check and confirmation made for another installation never start a flash for this one
    if (!s.start) return;
    // one confirmation, one run: the pack has to be checked and confirmed again for the next one
    setTyped(""); setTarget(null); setCheckedFor(null);
    try { await startFlash(mPort, address, confirmation, targetVersion, samePort); }
    catch (e) { const k = errorKey(e); setError({ text: t(k.key, k.params), serial: otherBank(e) }); }
  };

  const reset = () => { setDone(null); setProgress(null); setTyped(""); setTarget(null); setCheckedFor(null); setError(null); setInspectError(null); setFlashed(null); };
  const versions = done && flashed ? resultVersions(done, flashed.checked, flashed.image, t) : null;

  if (isDemo) return <div className="view-in mx-auto max-w-7xl space-y-3"><h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight"><Cpu className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("fw.title")}</h1><p className="text-muted">{t("fw.demo")}</p></div>;

  const imagePicker = (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-4">
        <Button variant="secondary" size="sm" disabled={inputsLocked} onClick={chooseImage}>{t("fw.chooseImage")}</Button>
        <label className="flex items-center gap-2 text-sm text-muted"><Switch checked={allowCustom} onChange={toggleCustom} disabled={inputsLocked} />{t("fw.allowCustom")}</label>
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
      {image?.kind === "maintenance" && <PatchImageNotes inst={inst?.id ?? null} port={mPort} start={address} release={image.release} locked={inputsLocked}
        latent={!!inst && maint.alerts.markers.some((m) => m.installation === inst.id && m.latent)} />}
    </div>
  );

  return (
    <div className="view-in mx-auto max-w-7xl space-y-6">
      <header className="space-y-2">
        <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight">
          <Cpu className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("fw.title")}<InfoIcon id="topic.firmware" />
        </h1>
        <p className="mt-1 max-w-3xl text-muted">{t("fw.intro")}</p>
        {connected && <p className="text-sm text-discharge">{t("fw.monitorWillStop")}</p>}
      </header>

      {/* two ways to flash, one image: the pack bus panel (live status, packs) or one pack by hand */}
      <div className="flex rounded-md bg-sunken p-0.5 text-sm" role="group">
        {(["bus", "manual"] as const).map((k) => (
          <button key={k} aria-pressed={tab === k} disabled={running} onClick={() => setTab(k)}
            className={cn("flex-1 rounded px-2 py-1 disabled:opacity-50", tab === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>{t(`fw.tab.${k}`)}</button>
        ))}
      </div>

      {tab === "bus" && <PackBusPanel maint={maint} port={port} ports={ports} onEditPorts={onEditPorts} imagePicker={imagePicker} image={image}
        running={running} flashTick={flashTick} onUserAction={onUserAction}
        onFlash={(a, from) => startFlash(port, a, confirmationPhrase(a, image?.custom), from, true)} onMaster={setMaster} />}

      {/* one pack by hand: address, check, typed confirmation; the same checks in the backend. No live status here:
          the master's full version is the panel's last read */}
      {tab === "manual" && <section className="space-y-3 rounded-md border border-line p-4">
        <p className="text-sm text-muted">{t("fw.manualHint")}</p>
        <div className="flex flex-wrap items-center gap-3 text-sm">
          {/* the RS485-1/2 port of this tab: always free to choose; the installation's checks apply on its own port */}
          <span className="flex flex-wrap items-center gap-2 text-muted">{t("maint.setup.rs485")}
            <KindSelect value={manual.kind} disabled={inputsLocked} onChange={(k) => setManualPort({ kind: k, target: "" })} label={t("maint.setup.rs485Kind")} />
            <PortInput kind={manual.kind} value={manual.target} ports={ports} onRefresh={loadPorts} onChange={(v) => setManualPort({ kind: manual.kind, target: v })} label={t("maint.setup.rs485")} disabled={inputsLocked} />
          </span>
          <label className="flex items-center gap-2 text-muted">{t("fw.address")}
            <select className={input} value={address} disabled={inputsLocked} onChange={(e) => setAddress(Number(e.target.value))}>
              {Array.from({ length: 16 }, (_, a) => <option key={a} value={a}>{a}</option>)}
            </select>
          </label>
        </div>
        {imagePicker}
        <Button variant="secondary" disabled={!s.inspect || busy} onClick={inspect}>{busy ? t("fw.inspecting") : t("fw.inspect")}</Button>
        {/* a polling master is the usual cause: busy bus or no answer */}
        {inspectError && (
          <div className="space-y-1 text-sm">
            <p className="flex gap-2 text-alarm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{inspectError}</p>
            <p>{t("fw.manualQuiet")}</p>
          </div>
        )}
        {target && s.confirm && <p className="text-sm">{t("fw.target", { name: target.device_name, version: targetVersion })}</p>}
        {s.confirm && image && target && <p className="font-medium">{t("fw.summary", { address, from: targetVersion, to: imageLabel(image, t) })}</p>}
        <p className="text-sm text-alarm">{t("fw.risks")}</p>
        {keyParts(mPort)?.kind === "tcp" && <p className="text-sm text-discharge">{t("fw.tcpUntested")}</p>}
        <label className="block space-y-1 text-sm text-muted">{t("fw.typeToConfirm", { phrase })}
          <input className={cn(input, "block w-48 font-mono")} value={typed} disabled={!s.confirm} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} />
        </label>
        {release && <p className="text-sm text-discharge">{t("maint.packs.cPrime")}</p>}
        {!inst && <p className="text-sm text-discharge">{t("maint.panel.noInstallation")}</p>}
        <Button variant="primary" disabled={!s.start || !inst} onClick={manualStart}>{t("fw.start")}</Button>
        {error && (
          <div className="space-y-1 text-sm text-alarm">
            <p>{error.text}</p>
            {error.serial && inst && <Button variant="secondary" size="sm" onClick={() => api.maintAdoptMasterSerial(inst.id, error.serial!).then(() => { setError(null); void maint.loadLists(); }).catch((e) => { const k = errorKey(e); setError({ text: t(k.key, k.params), serial: null }); })}>{t("maint.bank.adopt")}</Button>}
          </div>
        )}
      </section>}

      {/* progress and result below the tab that started the flash */}
      {(running || done) && (
        <section className="space-y-3 border-t border-line pt-5">
          <h2 className="font-display text-lg font-semibold">{flashed ? t("fw.runTitle", { address: flashed.address }) : t("fw.s5")}</h2>
          {flashed && flashed.bank !== "none" && <p className="text-sm text-muted">{t(`maint.bank.flash.${flashed.bank}`)}</p>}
          {flashed?.marker === "unchecked" && <p className="text-sm text-discharge">{t("fw.markerUnchecked")}</p>}
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
              {done.ok ? (
                <p className="flex items-center gap-2 font-medium text-ok"><CircleCheck className="h-5 w-5" />{t("fw.okTitle")}: {t("fw.okText", versions ?? { from: "?", to: "?" })}</p>
              ) : (
                <>
                  <p className="flex items-center gap-2 font-medium text-alarm"><TriangleAlert className="h-5 w-5" />{t("fw.failTitle")}</p>
                  <p className="text-sm">{done.error}</p>
                  {done.stage && <p className="text-sm text-muted">{t("fw.failStage", { stage: t(`fw.stage.${done.stage}`, { n: (progress?.block ?? 0) + 1, total: progress?.total ?? 0 }) })}</p>}
                  <p className={cn("text-sm", done.written ? "text-alarm" : "text-muted")}>{done.stage === "post_check" ? t("fw.failPostCheck") : done.written ? t("fw.failAfter") : t("fw.failBefore")}</p>
                </>
              )}
              {done.pending && flashed && <p className="text-sm text-alarm">{t("fw.pendingAfter", { pack: flashed.address })}</p>}
              {done.log_error && <p className="text-sm text-alarm">{t("fw.logError", { err: done.log_error })}</p>}
              <p className="break-all text-sm text-muted">{t("fw.log")}: {done.log}</p>
              <div className="flex gap-2">
                <Button variant="ghost" size="sm" onClick={() => api.revealFirmwareLogs(done.log)}><FolderOpen className="h-4 w-4" />{t("fw.showLog")}</Button>
                <Button variant="secondary" size="sm" onClick={reset}>{t("fw.again")}</Button>
                <Button variant="primary" size="sm" onClick={onBack}>{t("fw.back")}</Button>
              </div>
            </div>
          )}
        </section>
      )}
    </div>
  );
}

/** Before flashing an image with the patch functions: the update way, and the target's release bits, which the flash
 *  takes over unchanged (a fresh read on the pack bus, else the last stored block with its age, not checked). */
function PatchImageNotes({ inst, port, start, release, locked, latent }: { inst: string | null; port: string; start: number; release: string; locked: boolean; latent: boolean }) {
  const { t, i18n } = useTranslation();
  const [address, setAddress] = useState(start);
  const [fresh, setFresh] = useState<number | null | undefined>(undefined);
  const [snap, setSnap] = useState<{ g0: number; at: number } | null>(null);
  const [reading, setReading] = useState(false);
  useEffect(() => setAddress(start), [start]);
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
  const keys = g0 === null ? [] : flashGateKeys(g0);
  return (
    <div className="space-y-1 rounded-md border border-line p-3 text-sm">
      <p className="flex items-center gap-1 font-medium">{t("fw.patch.title", { release })}<InfoIcon id="topic.patchGates" /></p>
      <p className="text-muted">{t("fw.patch.update")}</p>
      {latent && <p className="text-alarm">{t("fw.markerLatent")}</p>}
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-muted">{t("fw.address")}
          <input type="number" min={0} max={15} value={address} disabled={locked} onChange={(e) => setAddress(Math.max(0, Math.min(15, Number(e.target.value) || 0)))}
            className="w-16 rounded border border-line bg-surface px-2 py-1" />
        </label>
        <Button variant="ghost" size="sm" disabled={locked || reading || !port} onClick={() => void readNow()}>{t("fw.patch.readNow")}</Button>
      </div>
      {fresh !== undefined && fresh !== null && <p>{t("fw.patch.fresh", { value: fresh.toString(16).toUpperCase().padStart(2, "0") })}</p>}
      {fresh === null && <p className="text-muted">{t("fw.patch.unread")}</p>}
      {(fresh === undefined || fresh === null) && snap && (
        <p className="text-discharge">{t("fw.patch.snapshot", { value: snap.g0.toString(16).toUpperCase().padStart(2, "0"), at: new Date(snap.at).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" }) })}</p>
      )}
      {g0 === null && fresh !== undefined && !snap && <p className="text-muted">{t("fw.patch.none")}</p>}
      {keys.map((k) => <p key={k} className={k === "fw.patch.ok" ? "text-ok" : "text-discharge"}>{t(k)}</p>)}
      <p className="text-muted">{t("fw.patch.wayBack")}</p>
    </div>
  );
}
