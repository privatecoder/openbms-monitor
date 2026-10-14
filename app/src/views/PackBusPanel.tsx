import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle, RefreshCw, TriangleAlert } from "lucide-react";
import { api, type DeviceInfo, type FirmwareImage, type LiveStatus, type MasterView, type PackScan } from "../api";
import { busLine, endpointKey, packIsCanAdapter, errorKey, flashBlocked, masterLine, otherBank, packVersion, releaseFirst, showDone, type Tone } from "../maint";
import type { Maint } from "../maintStore";
import { Button } from "../components/ui/button";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { MaintSetup, PackKindSelect } from "./MaintSetup";

const select = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";
const toneClass: Record<Tone, string> = { ok: "text-ok", warn: "text-discharge", muted: "text-muted", alarm: "text-alarm" };

/** A button with a spinner while its action runs. */
function ActButton({ spinning, children, className, ...props }: Parameters<typeof Button>[0] & { spinning: boolean }) {
  return <Button {...props} className={cn(spinning && "disabled:opacity-100", className)}>{spinning && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}{children}</Button>;
}

/** The pack bus panel (design: simple flash flow): one screen with a live status line, „Bus ruhig machen“ while the
 *  bus is not quiet, the packs that answer with one „Flashen“ each, and „Fertig“. No steps to tick: the backend
 *  re-checks a quiet bus and the target right before each flash. */
export function PackBusPanel({ maint, port, setPort, ports, onPortsChanged, imagePicker, image, running, flashTick, onFlash, onMaster, onUserAction }: {
  maint: Maint;
  /** the RS485-1/2 port: the installation's, or chosen here without one ("" = none) */
  port: string;
  setPort: (p: string) => void;
  ports: string[];
  onPortsChanged: () => void;
  imagePicker: ReactNode;
  image: FirmwareImage | null;
  /** a flash runs: the panel sends nothing on the pack bus */
  running: boolean;
  /** counts finished flashes: the list is scanned again and „Fertig“ shows */
  flashTick: number;
  /** start a flash of `address` (confirmed here); throws a refusal */
  onFlash: (address: number, from: string | null) => Promise<void>;
  /** the master read of the live status (for the full version of address 0 elsewhere) */
  onMaster: (m: MasterView | null) => void;
  onUserAction: () => void;
}) {
  const { t } = useTranslation();
  const inst = maint.current;
  const [live, setLive] = useState<LiveStatus | null>(null);
  const [scan, setScan] = useState<PackScan | null>(null);
  const [scanning, setScanning] = useState(false);
  // halt, release, adopt: the live loop waits meanwhile
  const [action, setAction] = useState<string | null>(null);
  const [actionError, setActionError] = useState<{ text: string; serial: string | null } | null>(null);
  // the pack whose „Prüfen & flashen“ was pressed last (its button spins while the check and the flash run)
  const [target, setTarget] = useState<number | null>(null);
  const [flashed, setFlashed] = useState(false);
  // after a flash the master polls again (DIP back or released): the maintenance is over
  const [back, setBack] = useState(false);
  const [setup, setSetup] = useState<"edit" | "new" | null>(null);

  // the live loop reads the newest values through refs
  const args = useRef({ installation: inst?.id ?? null, port, running, action });
  args.current = { installation: inst?.id ?? null, port, running, action };
  const liveRef = useRef<LiveStatus | null>(null);
  const wantScan = useRef(true);

  const doScan = async (installation: string | null, p: string) => {
    setScanning(true);
    try {
      const r = await api.maintScanPacks(installation, p);
      if (args.current.port === p && args.current.installation === installation) setScan(r);
      if (r.cleared.length) void maint.loadLists();
    } catch (e) {
      if (errorKey(e).key !== "maint.err.busy") { const k = errorKey(e); setActionError({ text: t(k.key, k.params), serial: null }); }
      else wantScan.current = true;
    } finally { setScanning(false); }
  };

  // every 2 s while the panel is open and no flash or panel action runs: listen on the pack bus, read the master;
  // when the bus turns quiet, scan the packs once
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      const a = args.current;
      if (alive && a.port && !a.running && !a.action) {
        try {
          const l = await api.maintLive(a.installation, a.port);
          if (alive && args.current.port === a.port && args.current.installation === a.installation) {
            if (l.bus === "quiet" && liveRef.current?.bus !== "quiet") wantScan.current = true;
            if (l.master?.marker !== liveRef.current?.master?.marker) void maint.loadLists();
            liveRef.current = l; setLive(l);
            // a user action that started meanwhile goes first: a scan of 16 addresses holds the port for seconds
            if (wantScan.current && l.bus === "quiet" && !args.current.action && !args.current.running) { wantScan.current = false; await doScan(a.installation, a.port); }
          }
        } catch (e) {
          // busy: another operation holds the port, the last status stays
          if (alive && errorKey(e).key !== "maint.err.busy") { liveRef.current = null; setLive(null); }
        }
      }
      if (alive) timer = setTimeout(tick, 2000);
    };
    void tick();
    return () => { alive = false; clearTimeout(timer); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // another port or installation: nothing of the old one counts
  useEffect(() => { liveRef.current = null; setLive(null); setScan(null); setTarget(null); setActionError(null); setBack(false); wantScan.current = true; }, [port, inst?.id]);
  useEffect(() => { if (flashTick) { setFlashed(true); setBack(false); wantScan.current = true; } }, [flashTick]);
  useEffect(() => { if (flashed && !running && live?.bus === "polling") { setFlashed(false); setBack(true); } }, [flashed, running, live?.bus]);

  const master: MasterView | null = live?.master ?? null;
  useEffect(() => { onMaster(master); }, [master]); // eslint-disable-line react-hooks/exhaustive-deps
  const quiet = live?.bus === "quiet";
  const busy = !!action || scanning;
  const pendingHere = maint.alerts.pending.filter((p) => port && p.endpoint === endpointKey({ kind: "serial", path: port, bus: "pack" }));

  const run = async (key: string, f: () => Promise<void>) => {
    onUserAction(); setAction(key); setActionError(null); setBack(false);
    try { await f(); } catch (e) { const k = errorKey(e); setActionError({ text: t(k.key, k.params), serial: otherBank(e) }); }
    finally { setAction(null); void maint.loadLists(); }
  };
  const marker = (release: boolean) => run(release ? "release" : "halt", async () => {
    if (!inst) return;
    const m = release ? await api.maintRelease(inst.id) : await api.maintHalt(inst.id);
    setLive((l) => (l ? { ...l, master: m } : l));
  });
  const adopt = (serial: string) => run("adopt", async () => {
    if (!inst) return;
    await api.maintAdoptMasterSerial(inst.id, serial);
    setScan((s) => (s ? { ...s, bank: { state: "match", serial: null } } : s));
  });
  const flash = (address: number, from: string | null) => run("flash", async () => { setTarget(address); await onFlash(address, from); });

  const line = port ? busLine(live, running, t) : { tone: "muted" as Tone, text: t("maint.live.noPort") };
  const mLine = master ? masterLine(master, t) : null;
  const bank = scan?.bank;
  const packs: [number, DeviceInfo | null][] = [
    ...(scan?.packs ?? []).map(([a, d]) => [a, d] as [number, DeviceInfo | null]),
    // a pending pack does not answer 0x51: it is listed without a version
    ...pendingHere.filter((p) => !scan?.packs.some(([a]) => a === p.pack)).map((p) => [p.pack, null] as [number, DeviceInfo | null]),
  ].sort((x, y) => x[0] - y[0]);

  return (
    <section className="space-y-4 rounded-md border border-line p-4 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="flex items-center gap-1.5 font-display text-lg font-semibold">{t("maint.panel.title")}<InfoIcon id="topic.maintenance" /></h2>
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-2">{t("maint.picker.label")}
            <select className={select} value={inst?.id ?? ""} disabled={running || busy} onChange={(e) => { const v = e.target.value; if (v === "__new__") setSetup("new"); else { setSetup(null); void maint.switchTo(v || null); } }}>
              <option value="">{t("maint.picker.none")}</option>
              {maint.installations.map((i) => <option key={i.id} value={i.id}>{i.name}</option>)}
              <option value="__new__">{t("maint.picker.new")}</option>
            </select>
          </label>
          {inst && <Button variant="secondary" size="sm" disabled={running || busy} onClick={() => setSetup(setup === "edit" ? null : "edit")}>{t("maint.panel.setup")}</Button>}
        </div>
      </div>
      {setup && (
        <MaintSetup key={setup === "edit" ? inst?.id : "new"} installation={setup === "edit" ? inst : null} installations={maint.installations} disabled={running || busy}
          onSaved={(i) => { setSetup(null); onPortsChanged(); void maint.switchTo(i.id); }}
          onDeleted={(i) => { setSetup(null); void maint.forget(i.id); }} onCancel={() => setSetup(null)} />
      )}

      {/* the RS485-1/2 port: from the installation, or chosen here */}
      {inst ? (
        packIsCanAdapter(inst) ? <p className="text-discharge">{t("maint.panel.canAdapter")}</p>
          : <p>{port ? t("maint.panel.port", { port: ports.includes(port) ? port : `${port} (${t("maint.panel.notPlugged")})` }) : t("maint.panel.noPort")}</p>
      ) : (
        <label className="flex flex-wrap items-center gap-2">{t("maint.setup.rs485")}
          <PackKindSelect disabled={running || busy} />
          <select className={select} value={port} disabled={running || busy} onChange={(e) => setPort(e.target.value)}>
            <option value="">{t("fw.choosePort")}</option>
            {ports.map((p) => <option key={p}>{p}</option>)}
          </select>
          <Button variant="ghost" size="icon" disabled={running} onClick={onPortsChanged} aria-label={t("fw.refreshPorts")} title={t("fw.refreshPorts")}><RefreshCw className="h-4 w-4" /></Button>
        </label>
      )}

      {port && <p className="text-muted">{t("maint.panel.monitorStops")}</p>}

      {/* live status */}
      <div role="status" className="space-y-1 rounded-md bg-sunken p-3">
        <p className={cn("font-medium", toneClass[line.tone])}>{line.text}</p>
        {mLine && <p className={toneClass[mLine.tone]} title={master?.build_id ? t("maint.live.buildId", { id: master.build_id }) : undefined}>{t("maint.live.master", { text: mLine.text })}</p>}
        {back && <p className="text-ok">{t("maint.done.back")}</p>}
        {live?.master_error && <p className="text-muted">{t(errorKey(live.master_error).key, errorKey(live.master_error).params)}</p>}
        {inst && !inst.control && <p className="text-muted">{t("maint.live.noControl")}</p>}
      </div>

      {/* make the bus quiet: only while the master (or anything else) still talks */}
      {live && !quiet && !running && live.bus !== "no-port" && (
        <div className="space-y-2">
          <h3 className="font-medium">{t("maint.quiet.title")}</h3>
          {/* a halted master may finish the round it had started: no DIP advice for that */}
          {master?.marker === "on" && master.slaves === 0 ? <p>{t("maint.quiet.halted")}</p>
          : master?.may_halt && inst ? (
            <ActButton variant="primary" size="sm" spinning={action === "halt"} disabled={busy} onClick={() => marker(false)}>{t("maint.halt")}</ActButton>
          ) : <p>{t("maint.quiet.dip")}</p>}
          <p className="text-muted">{t("maint.quiet.hint")}</p>
        </div>
      )}

      {/* the packs on the quiet bus */}
      {(quiet || running) && (
        <div className="space-y-3">
          <p className="text-muted">{t("maint.packs.inverter")}</p>
          {imagePicker}
          {bank && bank.state !== "none" && (
            bank.state === "mismatch" ? (
              <div className="space-y-1 text-alarm">
                <p className="flex gap-2"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t("maint.err.other-bank", { serial: bank.serial })}</p>
                <ActButton variant="secondary" size="sm" spinning={action === "adopt"} disabled={busy || running} onClick={() => adopt(bank.serial!)}>{t("maint.bank.adopt")}</ActButton>
              </div>
            ) : <p className="text-muted">{t(`maint.bank.${bank.state}`, { serial: bank.serial })}</p>
          )}
          <div className="flex items-center gap-2">
            <h3 className="font-medium">{t("maint.packs.title")}</h3>
            <ActButton variant="ghost" size="sm" spinning={scanning} disabled={busy || running || !quiet} onClick={() => { onUserAction(); void doScan(inst?.id ?? null, port); }}>{t("maint.packs.rescan")}</ActButton>
          </div>
          {scan && !packs.length && <p className="text-muted">{t("maint.packs.none")}</p>}
          <ul className="space-y-2">
            {packs.map(([a, d]) => {
              const pending = pendingHere.find((p) => p.pack === a);
              const blocked = flashBlocked({ image, quiet, busy, running, address: a, master, pending: maint.alerts.pending, port });
              const name = a === 0 ? t("maint.packs.master") : t("maint.packs.pack", { address: a });
              const version = d ? packVersion(a, d.firmware_version, master) : null;
              return (
                <li key={a} className="space-y-1 rounded-md border border-line p-2">
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="font-medium">{name}</span>
                    <span className="text-muted">{version ? t("maint.packs.version", { version }) : t("maint.packs.silent")}</span>
                    {/* one button in a fixed place: the backend checks quiet bus, target and installation, then flashes */}
                    {d && <ActButton variant="secondary" size="sm" className="ml-auto" spinning={(action === "flash" || running) && target === a} disabled={!!blocked} title={blocked ? t(blocked.key, blocked.params) : undefined} onClick={() => flash(a, version)}>{t("maint.packs.flash")}</ActButton>}
                  </div>
                  {pending && <p className="text-alarm">{t("maint.packs.pending", { pack: a })}</p>}
                  {a === 0 && releaseFirst(master, 0, image?.kind ?? null) && <p className="text-discharge">{t("maint.packs.cPrime")}</p>}
                </li>
              );
            })}
          </ul>
        </div>
      )}
      {actionError && (
        <div className="space-y-1 text-alarm">
          <p className="flex gap-2"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{actionError.text}</p>
          {actionError.serial && inst && <ActButton variant="secondary" size="sm" spinning={action === "adopt"} disabled={busy || running} onClick={() => adopt(actionError.serial!)}>{t("maint.bank.adopt")}</ActButton>}
        </div>
      )}

      {/* done: give the bus back to the master */}
      {showDone(flashed, master) && !running && (
        <div className="space-y-2 border-t border-line pt-3">
          <h3 className="font-medium">{t("maint.done.title")}</h3>
          {master?.may_release && inst ? (
            <ActButton variant="primary" size="sm" spinning={action === "release"} disabled={busy} onClick={() => marker(true)}>{t("maint.release")}</ActButton>
          ) : <p>{t("maint.done.dip")}</p>}
          <p className="text-muted">{t("maint.done.hint")}</p>
        </div>
      )}
    </section>
  );
}
