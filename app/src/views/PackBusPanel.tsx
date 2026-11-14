import { useEffect, useReducer, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle, TriangleAlert } from "lucide-react";
import { api, type DeviceInfo, type FirmwareDone, type FirmwareImage, type LiveStatus, type MasterView, type PackScan } from "../api";
import { imageLabel, seriesPhrase } from "../firmware";
import { judge, seriesOrder, seriesRowText, seriesView, startFlashSeries, summarizeSeries, withEnd, type FlashSeriesEnd, type PackEnd, type SeriesView } from "../flashSeries";
import { decideUi, lockProps, type UiAction, type UiFacts } from "../locks";
import { seriesOpen, busLine, keyParts, packIsCanAdapter, errorKey, masterLine, notOnBus, otherBank, packVersion, packVersionText, releaseFirst, dipOffTick, RECOVERY_PACK, showDone, storedPort, type Tone } from "../maint";
import type { Maint } from "../maintStore";
import { Button } from "../components/ui/button";
import { InfoIcon } from "../help";
import { GuideButton } from "../guides";
import { cn } from "../lib/utils";
import { RowSocNote } from "../components/RemoteDiagnosis";

const select = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";
const toneClass: Record<Tone, string> = { ok: "text-ok", warn: "text-discharge", muted: "text-muted", alarm: "text-alarm" };

/** A button with a spinner while its action runs. */
function ActButton({ spinning, children, className, ...props }: Parameters<typeof Button>[0] & { spinning: boolean }) {
  return <Button {...props} className={cn(spinning && "disabled:opacity-100", className)}>{spinning && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}{children}</Button>;
}

/** The pack bus panel (design: simple flash flow): one screen with a live status line, „Bus ruhig machen“ while the
 *  bus is not quiet, the packs that answer with one „Flashen“ each, and „Fertig“. No steps to tick: the backend
 *  re-checks a quiet bus and the target right before each flash. */
export function PackBusPanel({ maint, port, ports, onEditPorts, imagePicker, image, running, flashTick, onFlash, onMaster, onUserAction, series, onSeriesEnd }: {
  maint: Maint;
  /** the RS485-1/2 port of the installation as its endpoint key ("" = none) */
  port: string;
  ports: string[];
  /** „In Anlagen ändern“: the ports are edited in the installations view */
  onEditPorts: () => void;
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
  /** the flash series (null without an image): its one confirmation in the backend, then each pack's flash until its end */
  series: { begin: (addresses: number[], phrase: string) => Promise<number[]>; flash: (address: number, from: string | null, listed: number[], phrase: string) => Promise<FirmwareDone> } | null;
  /** the series ended (after its last flash): the backend admits no further pack */
  onSeriesEnd: () => void;
}) {
  const { t } = useTranslation();
  const inst = maint.current;
  const [live, setLive] = useState<LiveStatus | null>(null);
  const [scan, setScan] = useState<PackScan | null>(null);
  const [scanning, setScanning] = useState(false);
  // halt, release, adopt: the live loop waits meanwhile
  const [action, setAction] = useState<string | null>(null);
  // `orphan`: refused for a series left open (the way to end it is offered)
  const [actionError, setActionError] = useState<{ text: string; serial: string | null; orphan?: boolean } | null>(null);
  // the pack whose „Prüfen & flashen“ was pressed last (its button spins while the check and the flash run)
  const [target, setTarget] = useState<number | null>(null);
  const [flashed, setFlashed] = useState(false);
  // after a flash the master polls again (DIP back or released): the maintenance is over
  const [back, setBack] = useState(false);
  // the flash series: the packs picked, its confirmation (the phrase typed), its rows and how it ended
  const [picked, setPicked] = useState<number[]>([]);
  const [ask, setAsk] = useState(false);
  const [typed, setTyped] = useState("");
  // the series shown: its rows with the label of its own image, replaced together (`seriesView`)
  const [view, setView] = useState<SeriesView | null>(null);
  const rows = view?.rows ?? null;
  const [seriesEnd, setSeriesEnd] = useState<FlashSeriesEnd | null>(null);
  const [current, setCurrent] = useState<number | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const cancelled = useRef(false);
  // a series stops before its next pack when the panel goes away
  useEffect(() => () => { cancelled.current = true; }, []);

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

  // C′ (the master gets an image without maintenance functions): the user says DIP 5–8 are OFF before releasing (`dipOffTick`)
  const [dipOffTicked, dipOff] = useReducer(dipOffTick, false);
  // another port or installation: nothing of the old one counts (a series running stops before its next pack by itself)
  useEffect(() => { liveRef.current = null; setLive(null); setScan(null); setTarget(null); setActionError(null); setBack(false); setAskSlaves(null); setReleased(null); setPicked([]); setAsk(false); dipOff({ kind: "context" }); wantScan.current = true; }, [port, inst?.id]);
  useEffect(() => { if (flashTick) { setFlashed(true); setBack(false); wantScan.current = true; } }, [flashTick]);
  useEffect(() => { if (flashed && !running && live?.bus === "polling") { setFlashed(false); setBack(true); } }, [flashed, running, live?.bus]);

  const master: MasterView | null = live?.master ?? null;
  const cPrime = releaseFirst(master, 0, image?.kind ?? null);
  useEffect(() => { onMaster(master); }, [master]); // eslint-disable-line react-hooks/exhaustive-deps
  const quiet = live?.bus === "quiet";
  const busy = !!action || scanning;
  // what may be done now (`locks.ts`)
  const facts: UiFacts = { source: null, chosen: inst?.id ?? null, busy, flashRunning: running, quiet };
  const may = (a: UiAction) => lockProps(decideUi(a, facts), t);
  // the running flash stores its own record before the restart: it counts only once the flash has ended (like the banner)
  const pendingHere = running ? [] : maint.alerts.pending.filter((p) => port && p.endpoint === port);

  const run = async (key: string, f: () => Promise<void>) => {
    onUserAction(); setAction(key); setActionError(null); setBack(false);
    try { await f(); } catch (e) { const k = errorKey(e); setActionError({ text: t(k.key, k.params), serial: otherBank(e), orphan: seriesOpen(e) }); }
    finally { setAction(null); void maint.loadLists(); }
  };
  // the release waits for the master's DIP slave count: the one kept by the halt, else the user confirms it
  const [askSlaves, setAskSlaves] = useState<number | null>(null);
  const [released, setReleased] = useState<MasterView["polling"] | null>(null);
  const marker = (release: boolean, confirmed: number | null = null) => run(release ? "release" : "halt", async () => {
    if (!inst) return;
    setReleased(null);
    // C′, confirmed by the user: DIP 5–8 of the master OFF, so the master ends at 0 slaves, not the kept count
    const off = release && cPrime && dipOffTicked;
    if (release && !off && confirmed === null && (await api.maintExpectedSlaves(inst.id)) === null) {
      setAskSlaves(Math.max(0, packs.length - 1));
      return;
    }
    // the tick counts for this one attempt only, whatever its outcome
    if (release) dipOff({ kind: "release" });
    const m = release ? await api.maintRelease(inst.id, confirmed, off) : await api.maintHalt(inst.id);
    setAskSlaves(null);
    if (release) setReleased(m.polling ?? null);
    setLive((l) => (l ? { ...l, master: m } : l));
  });
  const adopt = (serial: string) => run("adopt", async () => {
    if (!inst) return;
    await api.maintAdoptMasterSerial(inst.id, serial);
    setScan((s) => (s ? { ...s, bank: { state: "match", serial: null } } : s));
  });
  const flash = (address: number, from: string | null) => run("flash", async () => { setTarget(address); await onFlash(address, from); });

  const line = port ? busLine(live, running, t) : { tone: "muted" as Tone, text: t("maint.live.noPort") };
  // „USB /dev/… (nicht eingesteckt)“ or „Netzwerk 192.0.2.1:4196“
  const portText = (key: string) => {
    const p = keyParts(key);
    if (!p) return key;
    const text = `${t(`conn.${p.kind}`)} ${p.target}`;
    return p.kind === "serial" && !ports.includes(p.target) ? `${text} (${t("maint.panel.notPlugged")})` : text;
  };
  const can = storedPort(inst, "can");
  const canText = can ? `${t(`conn.${can.kind}`)} ${can.target}` : t("maint.panel.noCan");
  const mLine = master ? masterLine(master, t) : null;
  const bank = scan?.bank;
  const packs: [number, DeviceInfo | null][] = [
    ...(scan?.packs ?? []).map(([a, d]) => [a, d] as [number, DeviceInfo | null]),
    // a pending pack does not answer 0x51: it is listed without a version
    ...pendingHere.filter((p) => p.pack !== RECOVERY_PACK && !scan?.packs.some(([a]) => a === p.pack)).map((p) => [p.pack, null] as [number, DeviceInfo | null]),
  ].sort((x, y) => x[0] - y[0]);
  // the series: packs that answer can be picked; each is weighed like its single flash (`locks.ts`)
  const versionOf = (a: number, d: DeviceInfo | null) => (d ? packVersion(a, d.firmware_version, master) : null);
  const answering = packs.filter(([, d]) => d);
  const chosen = picked.filter((a) => answering.some(([x]) => x === a));
  const order = seriesOrder(chosen);
  const phrase = seriesPhrase(order.length, image?.custom);
  const seriesFacts: UiFacts = { ...facts, picked: chosen, available: answering.length >= 2, flash: { image, quiet, busy, running, address: 0, master, pending: maint.alerts.pending, port } };
  const pick = decideUi("maintenance.seriesPick", seriesFacts);
  const packName = (a: number) => (a === 0 ? t("maint.packs.master") : t("maint.packs.pack", { address: a }));
  const toggle = (a: number) => { setAsk(false); setPicked((p) => (p.includes(a) ? p.filter((x) => x !== a) : [...p, a])); };
  const startSeries = () => {
    if (!series || !image || !inst) return;
    const img = image, typedNow = typed.trim(), chosenOrder = order;
    const from = new Map(answering.map(([a, d]) => [a, versionOf(a, d)]));
    setAsk(false); setTyped(""); setCancelling(false); cancelled.current = false;
    const label = imageLabel(img, t);
    void run("series", async () => {
      try {
        // the link is taken before the backend is asked: a change meanwhile ends the series before its first pack
        const end = await startFlashSeries({
          link: () => `${args.current.installation}|${args.current.port}`,
          begin: () => series.begin(chosen, typedNow),
          onOrder: (listed) => { setView(seriesView(listed, from, label)); setSeriesEnd(null); },
          end: onSeriesEnd,
          cancelled: () => cancelled.current,
          // read fresh: the list shown may still hold the record of the flash just finished; a failed read goes on, the
          // backend refuses a series pack with any open record
          pending: async () => ((await api.maintAlerts().catch(() => null))?.pending.length ?? 0) > 0,
          flash: (a) => series.flash(a, from.get(a) ?? null, chosenOrder, typedNow),
          judge: (d) => judge(d, img),
          report: (a, e) => setView((v) => withEnd(v, a, e)),
          start: (a) => { setCurrent(a); setTarget(a); },
        });
        setSeriesEnd(end);
      } finally { setCurrent(null); setPicked([]); }
    });
  };
  const errText = (e: unknown) => { const k = errorKey(e); return t(k.key, k.params); };
  const rowText = (r: { address: number; end?: PackEnd }) => seriesRowText(r.address, r.end, current, view?.label ?? "", t, errText);
  const rowTone = (e?: PackEnd) => (!e ? "text-muted" : e.kind === "flashed" ? "text-ok" : e.kind === "notReached" ? "text-discharge" : "text-alarm");

  // flashing belongs to an installation: without one the panel only says so
  if (!inst) return (
    <section className="space-y-2 rounded-md border border-line p-4 text-sm">
      <h2 className="flex items-center gap-1.5 font-display text-lg font-semibold">{t("maint.panel.title")}<InfoIcon id="topic.maintenance" /><GuideButton place="packBus" /></h2>
      <p className="text-discharge">{t("maint.panel.noInstallation")}</p>
    </section>
  );

  return (
    <section className="space-y-4 rounded-md border border-line p-4 text-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="flex items-center gap-1.5 font-display text-lg font-semibold">{t("maint.panel.title")}<InfoIcon id="topic.maintenance" /><GuideButton place="packBus" /></h2>
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-medium">{inst.name}</span>
          <Button variant="ghost" size="sm" {...may("maintenance.editPorts")} onClick={onEditPorts}>{t("maint.panel.editPorts")}</Button>
        </div>
      </div>

      {/* the ports of the installation, read-only: they are edited under Anlagen */}
      {packIsCanAdapter(inst) ? <p className="text-discharge">{t("maint.panel.canAdapter")}</p>
        : !port ? <p className="text-discharge">{t("maint.panel.noPortFor", { name: inst.name })}{" "}<button className="underline hover:text-ink" onClick={onEditPorts}>{t("maint.panel.setUpThere")}</button></p>
        : (
          <div className="space-y-0.5">
            <p>{t("maint.panel.port", { port: portText(port) })}</p>
            <p className="text-muted">{t("maint.panel.can", { port: canText })}</p>
            <p className="text-muted">{t("maint.panel.monitorStops")}</p>
          </div>
        )}

      {/* live status */}
      {port && <div role="status" className="space-y-1 rounded-md bg-sunken p-3">
        <p className={cn("font-medium", toneClass[line.tone])}>{line.text}</p>
        {mLine && <p className={toneClass[mLine.tone]} title={master?.build_id ? t("maint.live.buildId", { id: master.build_id }) : undefined}>{t("maint.live.master", { text: mLine.text })}</p>}
        {notOnBus(live?.bus, master) && !running && <p className="text-discharge">{t("maint.live.notOnBus")}</p>}
        {back && <p className="text-ok">{t("maint.done.back")}</p>}
        {live?.master_error && <p className="text-muted">{t(errorKey(live.master_error).key, errorKey(live.master_error).params)}</p>}
        {inst && !inst.control && <p className="text-muted">{t("maint.live.noControl")}</p>}
      </div>}

      {/* make the bus quiet: only while the master (or anything else) still talks */}
      {live && !quiet && !running && live.bus !== "no-port" && (
        <div className="space-y-2">
          <h3 className="font-medium">{t("maint.quiet.title")}</h3>
          {/* a halted master may finish the round it had started: no DIP advice for that */}
          {master?.marker === "on" && master.slaves === 0 ? <p>{t("maint.quiet.halted")}</p>
          : master?.may_halt && inst ? (
            <ActButton variant="primary" size="sm" spinning={action === "halt"} {...may("maintenance.halt")} onClick={() => marker(false)}>{t("maint.halt")}</ActButton>
          ) : <p>{t("maint.quiet.dip")}</p>}
          {master && master.kind !== "patched" && <p className="text-muted">{t("maint.quiet.noPatch")}</p>}
          <p className="text-muted">{t("maint.quiet.hint")}</p>
        </div>
      )}

      {/* the packs on the quiet bus */}
      {(quiet || running) && (
        <div className="space-y-3">
          <p className="text-muted">{t("maint.packs.inverter")}</p>
          {keyParts(port)?.kind === "tcp" && <p className="text-discharge">{t("fw.tcpUntested")}</p>}
          {imagePicker}
          {bank && bank.state !== "none" && (
            bank.state === "mismatch" ? (
              <div className="space-y-1 text-alarm">
                <p className="flex gap-2"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{t("maint.err.other-bank", { serial: bank.serial })}</p>
                <ActButton variant="secondary" size="sm" spinning={action === "adopt"} {...may("maintenance.adopt")} onClick={() => adopt(bank.serial!)}>{t("maint.bank.adopt")}</ActButton>
              </div>
            ) : <p className="text-muted">{t(`maint.bank.${bank.state}`, { serial: bank.serial })}</p>
          )}
          <div className="flex items-center gap-2">
            <h3 className="font-medium">{t("maint.packs.title")}</h3>
            <ActButton variant="ghost" size="sm" spinning={scanning} {...may("maintenance.rescan")} onClick={() => { onUserAction(); void doScan(inst?.id ?? null, port); }}>{t("maint.packs.rescan")}</ActButton>
            {scanning && <Button variant="ghost" size="sm" onClick={() => void api.maintScanCancel()}>{t("maint.packs.cancel")}</Button>}
          </div>
          {scan?.cancelled && !scanning && <p className="text-muted">{t("maint.packs.cancelled")}</p>}
          {scan && !scan.cancelled && !packs.length && <p className="text-muted">{t("maint.packs.none")}</p>}
          <ul className="space-y-2">
            {packs.map(([a, d]) => {
              const pending = pendingHere.find((p) => p.pack === a);
              const row = decideUi("maintenance.flash", { ...facts, flash: { image, quiet, busy, running, address: a, master, pending: maint.alerts.pending, port } });
              const blocked = row.locked ? row.reason ?? null : null;
              const name = a === 0 ? t("maint.packs.master") : t("maint.packs.pack", { address: a });
              const version = d ? packVersion(a, d.firmware_version, master) : null;
              return (
                <li key={a} className="space-y-1 rounded-md border border-line p-2">
                  <div className="flex flex-wrap items-center gap-3">
                    {d && pick.visible && <input type="checkbox" className="h-4 w-4 accent-[var(--charge)]" checked={chosen.includes(a)} {...lockProps(pick, t)}
                      aria-label={t("maint.series.pickPack", { name })} onChange={() => toggle(a)} />}
                    <span className="font-medium">{name}</span>
                    <span className="text-muted">{version ? packVersionText(version, scan?.identities?.find((x) => x.address === a), t) : t("maint.packs.silent")}</span>
                    {/* one button in a fixed place: the backend checks quiet bus, target and installation, then flashes */}
                    {d && <ActButton variant="secondary" size="sm" className="ml-auto" spinning={(action === "flash" || running) && target === a} disabled={!!blocked} title={blocked ? t(blocked.key, blocked.params) : undefined} onClick={() => flash(a, version)}>{t("maint.packs.flash")}</ActButton>}
                  </div>
                  {d && image && <RowSocNote port={port} address={a} disabled={decideUi("firmware.readTarget", { ...facts, port: !!port }).locked} />}
                  {pending && <p className="text-alarm">{t("maint.packs.pending", { pack: a })}</p>}
                  {a === 0 && cPrime && <p className="text-discharge">{t("maint.packs.cPrime")}</p>}
                </li>
              );
            })}
          </ul>
          {/* several packs, one confirmation: the same checks per pack, the master last, stops at the first failure */}
          {pick.visible && (
            <div className="flex flex-wrap items-center gap-2">
              <Button variant="ghost" size="sm" {...lockProps(pick, t)} onClick={() => { setAsk(false); setPicked(answering.map(([a]) => a)); }}>{t("maint.series.all")}</Button>
              <Button variant="ghost" size="sm" {...lockProps(pick, t)} onClick={() => { setAsk(false); setPicked([]); }}>{t("maint.series.none")}</Button>
              <Button variant="secondary" size="sm" {...lockProps(decideUi("maintenance.flashSeries", seriesFacts), t)} onClick={() => { setAsk(true); setTyped(""); }}>{t("maint.series.open", { count: chosen.length })}</Button>
              <InfoIcon id="topic.maintenance" />
            </div>
          )}
          {ask && image && (
            <div className="space-y-2 rounded-md border border-charge/40 bg-charge/5 p-3">
              <p className="font-medium">{t("maint.series.confirm", { count: order.length, image: imageLabel(image, t) })}</p>
              <ol className="list-decimal pl-5 tabular-nums">
                {order.map((a) => <li key={a}>{t("maint.series.line", { name: packName(a), from: versionOf(a, answering.find(([x]) => x === a)?.[1] ?? null) ?? "?", to: imageLabel(image, t) })}</li>)}
              </ol>
              <p className="text-muted">{t("maint.series.rules")}</p>
              <p className="text-alarm">{t("fw.risks")}</p>
              <label className="flex flex-wrap items-center gap-2">{t("maint.series.type", { phrase })}
                <input value={typed} onChange={(e) => setTyped(e.target.value)} className="rounded border border-line bg-surface px-2 py-1 font-mono" aria-label={t("maint.series.type", { phrase })} />
              </label>
              <div className="flex flex-wrap gap-2">
                <Button variant="primary" size="sm" {...lockProps(decideUi("maintenance.seriesStart", { ...seriesFacts, ready: typed.trim() === phrase }), t)} onClick={startSeries}>{t("maint.series.start", { count: order.length })}</Button>
                <Button variant="ghost" size="sm" onClick={() => setAsk(false)}>{t("maint.series.back")}</Button>
              </div>
            </div>
          )}
        </div>
      )}
      {/* the series' progress and, at its end, the summary */}
      {rows && (
        <div className="space-y-2 rounded-md border border-line p-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium">{seriesEnd === null ? t("maint.series.running", { n: rows.filter((r) => r.end).length, of: rows.length }) : t(`maint.series.end.${seriesEnd}`)}</span>
            <span className="ml-auto flex gap-2">
              {seriesEnd === null && <Button variant="ghost" size="sm" onClick={() => { cancelled.current = true; setCancelling(true); }}>{t(cancelling ? "maint.series.cancelling" : "maint.series.cancel")}</Button>}
              {seriesEnd !== null && <Button variant="ghost" size="sm" onClick={() => setView(null)}>{t("maint.series.close")}</Button>}
            </span>
          </div>
          {seriesEnd !== null && <p className="tabular-nums">{t("maint.series.sum", { ...summarizeSeries(rows) })}</p>}
          <ul className="space-y-0.5 tabular-nums">
            {rows.map((r) => <li key={r.address} className={rowTone(r.end)}>{t("maint.series.row", { name: packName(r.address), from: r.from ?? "?", text: rowText(r) })}</li>)}
          </ul>
        </div>
      )}
      {actionError && (
        <div className="space-y-1 text-alarm">
          <p className="flex gap-2"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />{actionError.text}</p>
          {actionError.orphan && <EndIdleSeries busy={busy} running={running} onEnded={() => setActionError(null)} />}
          {actionError.serial && inst && <ActButton variant="secondary" size="sm" spinning={action === "adopt"} {...may("maintenance.adopt")} onClick={() => adopt(actionError.serial!)}>{t("maint.bank.adopt")}</ActButton>}
        </div>
      )}

      {released && <p className={released === "unverified" ? "text-discharge" : "text-ok"}>{t(`maint.done.polling.${released}`)}</p>}
      {/* done: give the bus back to the master */}
      {showDone(flashed, master) && !running && (
        <div className="space-y-2 border-t border-line pt-3">
          <h3 className="font-medium">{t("maint.done.title")}</h3>
          {master?.may_release && inst ? (
            askSlaves === null ? (
              <div className="space-y-2">
                {cPrime && (
                  <label className="flex items-start gap-2">
                    <input type="checkbox" className="mt-0.5 h-4 w-4 accent-[var(--charge)]" checked={dipOffTicked} {...may("maintenance.release")} onChange={(e) => dipOff({ kind: "set", on: e.target.checked })} />
                    {t("maint.done.dipOff")}
                  </label>
                )}
                <ActButton variant="primary" size="sm" spinning={action === "release"} {...may("maintenance.release")} onClick={() => marker(true)}>{t("maint.release")}</ActButton>
              </div>
            ) : (
              <div className="space-y-1">
                <p>{t("maint.done.askSlaves")}</p>
                <div className="flex items-center gap-2">
                  <input type="number" min={0} max={15} value={askSlaves} onChange={(e) => setAskSlaves(Math.max(0, Math.min(15, Number(e.target.value) || 0)))}
                    className="w-16 rounded border border-line bg-surface px-2 py-1" aria-label={t("maint.done.askSlaves")} />
                  <ActButton variant="primary" size="sm" spinning={action === "release"} {...may("maintenance.release")} onClick={() => marker(true, askSlaves)}>{t("maint.release")}</ActButton>
                  <Button variant="ghost" size="sm" onClick={() => setAskSlaves(null)}>{t("maint.packs.cancel")}</Button>
                </div>
              </div>
            )
          ) : <p>{t("maint.done.dip")}</p>}
          <p className="text-muted">{t("maint.done.hint")}</p>
        </div>
      )}
    </section>
  );
}

/** „Offene Serie beenden“: after a refusal for a flash series left open (a view that went away without ending it). The
 *  backend ends it only while none of its packs is admitted; pending records and markers stay as they are. */
export function EndIdleSeries({ busy, running, onEnded }: { busy: boolean; running: boolean; onEnded: () => void }) {
  const { t } = useTranslation();
  const [ending, setEnding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const d = decideUi("maintenance.seriesEndIdle", { source: null, chosen: null, available: true, busy: busy || ending, flashRunning: running });
  const end = async () => {
    setEnding(true); setError(null);
    try { await api.firmwareSeriesEndIdle(); onEnded(); }
    catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
    finally { setEnding(false); }
  };
  if (!d.visible) return null;
  return (
    <div className="space-y-1">
      <ActButton variant="secondary" size="sm" spinning={ending} {...lockProps(d, t)} onClick={() => void end()}>{t("maint.series.endIdle")}</ActButton>
      {error && <p className="text-alarm">{error}</p>}
    </div>
  );
}
