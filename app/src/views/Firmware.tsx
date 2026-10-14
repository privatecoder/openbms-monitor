import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, FolderOpen, RefreshCw, TriangleAlert } from "lucide-react";
import { api, isDemo, type DeviceInfo, type FirmwareDone, type FirmwareImage, type FirmwareProgress, type MasterView } from "../api";
import { Button } from "../components/ui/button";
import { Switch } from "../components/PackPicker";
import { useNow } from "../components/live";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { confirmationPhrase, imageLabel, imageWarning, progressFraction, resultVersions, steps } from "../firmware";
import { errorKey, otherBank, packPortOf, packVersion, releaseFirst } from "../maint";
import type { Maint } from "../maintStore";
import { PackKindSelect } from "./MaintSetup";
import { PackBusPanel } from "./PackBusPanel";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

export function Firmware({ connected, maint, onMonitorStopped, onRunning, onBack, onUserAction }: {
  connected: boolean;
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
  // the port chosen in the panel without an installation
  const [ownPort, setOwnPort] = useState("");
  // the single pack tab's own port, independent of the installation (starts with the panel's)
  const [manualPort, setManualPort] = useState<string | null>(null);
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
  const [checkedFor, setCheckedFor] = useState<{ port: string; address: number } | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ text: string; serial: string | null } | null>(null);
  // the answer of „Pack prüfen“ goes next to that button, the flash's refusals next to its own
  const [inspectError, setInspectError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<FirmwareProgress | null>(null);
  const [done, setDone] = useState<FirmwareDone | null>(null);
  // what the last run flashed: the pack, the image and the version known before (for the result's version line)
  const [flashed, setFlashed] = useState<{ address: number; image: FirmwareImage; checked: string | null; bank: string } | null>(null);
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
  // with an installation the RS485-1/2 port is the one of its setup
  const port = inst ? packPortOf(inst) ?? "" : ownPort;
  const mPort = manualPort ?? port;
  useEffect(() => { setTarget(null); setCheckedFor(null); setTyped(""); }, [mPort, address, image?.sha256]);
  // the master read belongs to the port it came from
  useEffect(() => { setMaster(null); }, [port]);
  const s = steps({ running, done: done !== null, port: mPort, address, image, target, checkedFor, typed, busy });
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
  const startFlash = async (p: string, a: number, confirmation: string, from: string | null, installation: string | null, releaseMaster: boolean) => {
    if (!path || !image) return;
    // the backend stops the monitor (it may have been reconnected meanwhile)
    if (connected) onMonitorStopped();
    onUserAction();
    setError(null); setDone(null); setProgress(null); setCloseRefused(false);
    setRunning(true); onRunning(true); startedAt.current = Date.now();
    try {
      const r = await api.firmwareStart(p, a, path, allowCustom, image.sha256, confirmation, lang, installation, releaseMaster);
      setFlashed({ address: a, image, checked: from, bank: r.bank });
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
    try { setTarget(await api.firmwareInspect(mPort, address, lang)); setCheckedFor({ port: mPort, address }); void maint.loadLists(); }
    catch (e) { setInspectError(String(e)); }
    finally { setBusy(false); }
  };

  // the master read and the installation's checks count only on the installation's own port
  const samePort = mPort === port;
  // C' in this tab too: the master's flash releases a marker that is on
  const release = samePort && releaseFirst(master, address, image?.kind ?? null);
  const targetVersion = target ? packVersion(address, target.firmware_version, samePort ? master : null) : null;
  const manualStart = async () => {
    const confirmation = typed;
    // one confirmation, one run: the pack has to be checked and confirmed again for the next one
    setTyped(""); setTarget(null); setCheckedFor(null);
    try { await startFlash(mPort, address, confirmation, targetVersion, samePort ? inst?.id ?? null : null, release); }
    catch (e) { const k = errorKey(e); setError({ text: t(k.key, k.params), serial: otherBank(e) }); }
  };

  const reset = () => { setDone(null); setProgress(null); setTyped(""); setTarget(null); setCheckedFor(null); setError(null); setInspectError(null); setFlashed(null); };
  const versions = done && flashed ? resultVersions(done, flashed.checked, flashed.image, t) : null;

  if (isDemo) return <div className="max-w-2xl space-y-3"><h1 className="font-display text-2xl font-semibold">{t("fw.title")}</h1><p className="text-muted">{t("fw.demo")}</p></div>;

  const imagePicker = (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-4">
        <Button variant="secondary" size="sm" disabled={inputsLocked} onClick={chooseImage}>{t("fw.chooseImage")}</Button>
        <label className="flex items-center gap-2 text-sm text-muted"><Switch checked={allowCustom} onChange={toggleCustom} disabled={inputsLocked} />{t("fw.allowCustom")}</label>
      </div>
      {image && <div className="text-sm"><p>{t("fw.imageInfo", { release: imageLabel(image, t, true), blocks: image.blocks, bytes: image.bytes })}</p><p className="break-all text-muted">{t("fw.sha", { sha: image.sha256 })}</p></div>}
      {image && imageWarning(image) && <p className="flex gap-2 rounded-md border border-alarm p-3 text-sm text-alarm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t(imageWarning(image)!)}</p>}
      {pending && <p className="flex gap-2 rounded-md border border-discharge p-3 text-sm"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-discharge" />{t("fw.needsCustom", { release: imageLabel(pending.info, t, true) })}</p>}
      {imageError && <p className="text-sm text-alarm">{imageError}</p>}
    </div>
  );

  return (
    <div className="max-w-2xl space-y-6">
      <header className="space-y-2">
        <h1 className="flex items-center gap-2 font-display text-2xl font-semibold">{t("fw.title")}<InfoIcon id="topic.firmware" /></h1>
        <p className="text-muted">{t("fw.intro")}</p>
        {connected && <p className="text-sm text-discharge">{t("fw.monitorWillStop")}</p>}
      </header>

      {/* two ways to flash, one image: the pack bus panel (live status, packs) or one pack by hand */}
      <div className="flex rounded-md bg-sunken p-0.5 text-sm" role="group">
        {(["bus", "manual"] as const).map((k) => (
          <button key={k} aria-pressed={tab === k} disabled={running} onClick={() => setTab(k)}
            className={cn("flex-1 rounded px-2 py-1 disabled:opacity-50", tab === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>{t(`fw.tab.${k}`)}</button>
        ))}
      </div>

      {tab === "bus" && <PackBusPanel maint={maint} port={port} setPort={setOwnPort} ports={ports} onPortsChanged={loadPorts} imagePicker={imagePicker} image={image}
        running={running} flashTick={flashTick} onUserAction={onUserAction}
        onFlash={(a, from, release) => startFlash(port, a, confirmationPhrase(a, image?.custom), from, inst?.id ?? null, release)} onMaster={setMaster} />}

      {(running || done) && (
        <section className="space-y-3 border-t border-line pt-5">
          <h2 className="font-display text-lg font-semibold">{flashed ? t("fw.runTitle", { address: flashed.address }) : t("fw.s5")}</h2>
          {flashed && flashed.bank !== "none" && <p className="text-sm text-muted">{t(`maint.bank.flash.${flashed.bank}`)}</p>}
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

      {/* one pack by hand: address, check, typed confirmation; the same checks in the backend. No live status here:
          the master's full version is the panel's last read */}
      {tab === "manual" && <section className="space-y-3 rounded-md border border-line p-4">
        <p className="text-sm text-muted">{t("fw.manualHint")}</p>
        <div className="flex flex-wrap items-center gap-3 text-sm">
          {/* the RS485-1/2 port of this tab: always free to choose; the installation's checks apply on its own port */}
          <label className="flex flex-wrap items-center gap-2 text-muted">{t("maint.setup.rs485")}
            <PackKindSelect disabled={inputsLocked} />
            <select className={input} value={mPort} disabled={inputsLocked} onChange={(e) => setManualPort(e.target.value)}>
              <option value="">{t("fw.choosePort")}</option>
              {mPort && !ports.includes(mPort) && <option value={mPort}>{t("maint.setup.notPlugged", { port: mPort })}</option>}
              {ports.map((p) => <option key={p} value={p}>{p}</option>)}
            </select>
            <Button variant="ghost" size="icon" disabled={running} onClick={loadPorts} aria-label={t("fw.refreshPorts")} title={t("fw.refreshPorts")}><RefreshCw className="h-4 w-4" /></Button>
          </label>
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
        {target && s.confirm && <p className="text-sm">{t("fw.target", { name: target.device_name, version: targetVersion, can: target.can_protocol })}</p>}
        {s.confirm && image && target && <p className="font-medium">{t("fw.summary", { address, from: targetVersion, to: imageLabel(image, t) })}</p>}
        <p className="text-sm text-alarm">{t("fw.risks")}</p>
        <label className="block space-y-1 text-sm text-muted">{t("fw.typeToConfirm", { phrase })}
          <input className={cn(input, "block w-48 font-mono")} value={typed} disabled={!s.confirm} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} />
        </label>
        {release && <p className="text-sm text-discharge">{t("maint.packs.cPrime")}</p>}
        <Button variant="primary" disabled={!s.start} onClick={manualStart}>{t(release ? "maint.packs.releaseAndFlash" : "fw.start")}</Button>
        {error && (
          <div className="space-y-1 text-sm text-alarm">
            <p>{error.text}</p>
            {error.serial && inst && <Button variant="secondary" size="sm" onClick={() => api.maintAdoptMasterSerial(inst.id, error.serial!).then(() => { setError(null); void maint.loadLists(); }).catch((e) => { const k = errorKey(e); setError({ text: t(k.key, k.params), serial: null }); })}>{t("maint.bank.adopt")}</Button>}
          </div>
        )}
      </section>}
    </div>
  );
}
