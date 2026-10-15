import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { BatteryCharging, Building2, Cpu, Database, Gauge, Languages, LayoutDashboard, Moon, Network, RefreshCw, SlidersHorizontal, Sun, Usb, ChartLine, Wrench } from "lucide-react";
import { api, isDemo, type Bus, type DeviceInfo, type Endpoint, type IdentityView, type SerialNumbers } from "../api";
import { bankLine, clearUnchecked, noSerialPacks, packList, withIdentity } from "../bank";
import type { Maint } from "../maintStore";
import { Button } from "../components/ui/button";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { CONN_TIMEOUT_MS, RECONNECT_MS, useNow } from "../components/live";
import { packBusy } from "../control";
import { busyMessage, busyWaitStep, pickable, portBusy, preselect } from "../maint";
import { RecordingEntry } from "./Recording";
import { readMark } from "../bmsMemory";
import { BAUD, explain, firstLine, noAnswer, otherBus, padAddress, partialLine } from "../connCheck";

/** a wait for a busy port that was ended (cancel, new settings, a user action, a flash, the firmware view) */
class Cancelled extends Error {}

// a busy port: try again every BUSY_POLL_MS, give up after BUSY_WAIT_MS
const BUSY_POLL_MS = 500;
const BUSY_WAIT_MS = 30_000;

const input = "h-9 w-full rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-60";
// the select's entry that opens the name dialog
const NEW = "__new__";

export function Sidebar({ userActions, connected, paused, lastUpdate, onConnected, onDevices, onSerials, onBoard, onDisconnected, view, setView, maint, packs, locked, maintenance, setMaintenance }: {
  connected: boolean;
  /** the app stopped the monitor itself for a maintenance step (not lost, not closed by the user) */
  paused: boolean;
  /** the chosen installation (null: „Nur ansehen“) and the list */
  maint: Maint;
  packs: number[];
  lastUpdate: number;
  onConnected: (found: [number, DeviceInfo | undefined][], bus: Bus) => void;
  onDevices: (found: [number, DeviceInfo][]) => void;
  onSerials: (address: number, serials: SerialNumbers) => void;
  /** a serial read found another board at a place than it had */
  onBoard: (n: { address: number; serial: string; previous: string }) => void;
  /** `lost`: the link died or timed out; false: closed by the user */
  onDisconnected: (lost: boolean) => void;
  view: string;
  setView: (v: string) => void;
  locked: boolean;
  /** counts actions the user started in the firmware view or the pack bus panel: a wait for a busy port stops */
  userActions: number;
  maintenance: boolean;
  setMaintenance: (v: boolean) => void;
}) {
  const { t, i18n } = useTranslation();
  const [kind, setKind] = useState<"tcp" | "serial">(() => (localStorage.getItem("conn.kind") as "tcp" | "serial") ?? "tcp");
  // network address and USB port are kept apart, so switching the kind does not overwrite the other entry
  // (conn.target is the single field of earlier versions and seeds whichever kind was in use)
  const [targets, setTargets] = useState(() => {
    const legacy = localStorage.getItem("conn.target");
    const legacyKind = localStorage.getItem("conn.kind") ?? "tcp";
    return {
      tcp: localStorage.getItem("conn.tcp") ?? (legacyKind === "tcp" ? legacy : null) ?? (isDemo ? "demo:4196" : ""),
      serial: localStorage.getItem("conn.serial") ?? (legacyKind === "serial" ? legacy : null) ?? "",
    };
  });
  const target = targets[kind];
  const setTarget = (v: string) => setTargets((cur) => ({ ...cur, [kind]: v }));
  const [bus, setBus] = useState<Bus>(() => (localStorage.getItem("conn.bus") as Bus) ?? "can");
  // 0 = search addresses 0..15; n = packs at addresses 0..n-1, polled right away without a search
  const [packCount, setPackCount] = useState(() => Number(localStorage.getItem("conn.packs") ?? 0) || 0);
  const [ports, setPorts] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  // the last connect found no pack: the message is an error; `offer`: the other bus answered, one click switches
  const [failed, setFailed] = useState(false);
  const [offer, setOffer] = useState<Bus | null>(null);
  // lost: no data for CONN_TIMEOUT_MS; then the connection is dropped and retried every RECONNECT_MS
  const [lost, setLost] = useState(false);
  const [since, setSince] = useState(0);
  const [lastTry, setLastTry] = useState(0);
  const now = useNow();
  const [dark, setDark] = useState(document.documentElement.classList.contains("dark"));
  // what the serial reads of this connection say about each pack's board (right bank before writing)
  const [ids, setIds] = useState<IdentityView[]>([]);
  const [confirming, setConfirming] = useState(false);
  const [bankError, setBankError] = useState<string | null>(null);

  const loadPorts = () => { api.listPorts().then(setPorts).catch(() => setPorts([])); };
  useEffect(loadPorts, [kind]);
  useEffect(() => { document.documentElement.classList.toggle("dark", dark); try { localStorage.setItem("theme", dark ? "dark" : "light"); } catch { /* ignore */ } }, [dark]);

  // a maintenance action or a pack check holds the port: say which, and connect once it has finished. The wait
  // belongs to one generation (`waitToken`); cancel, disconnect, new settings, unmount and a user action elsewhere
  // start a new one, which ends it. `locked` and the open view are read through refs on every round.
  const waitToken = useRef(0);
  const lockedRef = useRef(locked); lockedRef.current = locked;
  const viewRef = useRef(view); viewRef.current = view;
  const [waiting, setWaiting] = useState(false);
  const stopWaiting = () => { waitToken.current += 1; };
  useEffect(stopWaiting, [target, kind, bus, packCount, userActions]);
  useEffect(() => { setOffer(null); }, [target, kind, bus]);
  useEffect(() => stopWaiting, []);
  const connectWhenFree = async (ep: Endpoint, auto: boolean, installation: string | null, check: number[]): Promise<{ linked: boolean; answered: number }> => {
    const token = ++waitToken.current;
    const until = Date.now() + BUSY_WAIT_MS;
    let action: string | null = null;
    try {
      for (let first = true; ; first = false) {
        // before every new attempt: stopped meanwhile, or the time is up
        if (!first) {
          const step = busyWaitStep({ token, current: waitToken.current, locked: lockedRef.current, auto, firmwareOpen: viewRef.current === "firmware", now: Date.now(), until });
          if (step === "stop") throw new Cancelled();
          if (step === "give-up") throw new Error(busyMessage(action, true, t));
        }
        try { return await api.connect(ep, installation, check); } catch (e) {
          const b = portBusy(e);
          if (!b) throw e;
          action = b.action;
          setMsg(busyMessage(action, false, t)); setWaiting(true);
          await new Promise((r) => setTimeout(r, BUSY_POLL_MS));
        }
      }
    } finally { setWaiting(false); }
  };

  // Serial numbers do not change while connected: read once per pack after the pack list is known, one after the
  // other between the polls. A new connection or a disconnect starts a new generation, which ends the old reads;
  // a failed read (port held by a flash, pack silent) leaves the value unknown without a message.
  const serialGen = useRef(0);
  const readSerials = async (addresses: number[]) => {
    const gen = ++serialGen.current;
    // a pack busy with another action (a write, the parameter read) is asked again at the end, at most three times
    const queue = addresses.map((a) => ({ a, tries: 0 }));
    for (let q = queue.shift(); q; q = queue.shift()) {
      const { a } = q;
      if (gen !== serialGen.current) return;
      let busyNow = false;
      const s = await api.serialNumbers(a).catch((e) => { busyNow = !!packBusy(e); return null; });
      if (busyNow && q.tries < 3) {
        queue.push({ a, tries: q.tries + 1 });
        await new Promise((r) => setTimeout(r, 1000));
        continue;
      }
      if (s && gen === serialGen.current) {
        onSerials(a, s);
        const id = s.identity;
        if (id) setIds((all) => withIdentity(all, id));
        if (s.board?.kind === "other" && s.bms.kind === "set") onBoard({ address: a, serial: s.bms.value, previous: s.board.previous });
      }
    }
  };

  // a maintenance step that stops the monitor ends the reads as well
  useEffect(() => {
    const un = api.onMonitorStopped(() => { serialGen.current += 1; });
    return () => { un.then((f) => f()); };
  }, []);
  // a write's check (or a read elsewhere) found a pack this installation does not know yet: the line lists it too
  useEffect(() => {
    const un = api.onBankUnconfirmed((v) => setIds((all) => withIdentity(all, v)));
    return () => { un.then((f) => f()); };
  }, []);
  const forgetBank = () => { setIds([]); setBankError(null); clearUnchecked(); };

  // the installation of the connection: chosen at a manual connect, reused by every automatic reconnect
  const connInst = useRef<string | null>(null);
  // `useBus`: the bus offered after a failed check (the select is set as well); `inst`: the installation to connect
  // with when it was switched just now („Mit „B“ verbinden“), before this closure sees the new choice
  const connect = async (retry = false, useBus: Bus = bus, inst?: string | null) => {
    serialGen.current += 1;
    forgetBank();
    if (!retry) connInst.current = inst !== undefined ? inst : maint.installation;
    const bus = useBus;
    setBusy(true); setFailed(false); setOffer(null); if (!retry) { setMsg(null); setLost(false); }
    setLastTry(Date.now());
    try {
      localStorage.setItem("conn.kind", kind); localStorage.setItem(`conn.${kind}`, target); localStorage.setItem("conn.bus", bus);
      localStorage.setItem("conn.packs", String(packCount));
      const ep: Endpoint = kind === "tcp" ? { kind, addr: target, bus } : { kind, path: target, bus };
      const range = (n: number) => Array.from({ length: n }, (_, i) => i);
      // the check asks the addresses to be polled first, then the rest of 0..15: one answer is enough
      const { linked, answered } = await connectWhenFree(ep, retry, connInst.current, range(packCount));
      // „Anschluss wird Anlage X zugeordnet“: said once, no dialog
      const linkedNote = linked ? t("conn.linked", { name: maint.installations.find((i) => i.id === connInst.current)?.name ?? maint.current?.name ?? "" }) : "";
      if (linked) void maint.loadLists();
      const answeredNote = t("conn.answered", { addr: padAddress(answered), bus: t(`conn.busName.${bus}`) });
      // pack addresses known without a search: from the master (patched firmware, CAN socket bus), else a fixed count
      let addrs: number[] | null = null;
      let note = "";
      if (bus === "can") {
        // the master answers Modbus at once: show its system values before anything slower
        const sys = await api.systemOnce().then(() => true, () => false);
        if (sys) { onConnected([], bus); setSince(Date.now()); setLost(false); }
        const m = await api.maintStatus().catch(() => null);
        if (m && !m.marker) { addrs = range(m.effective_slaves + 1); note = t("conn.fromMaster", { count: addrs.length }); }
        else if (m?.marker) note = t("conn.maintActive");
      }
      if (!addrs && packCount > 0) { addrs = range(packCount); note = t("conn.fixed", { count: packCount }); }
      if (addrs) {
        // start polling at once; name and firmware version follow from a short device query
        onConnected(addrs.map((a) => [a, undefined]), bus);
        setSince(Date.now()); setLost(false);
        const base = [linkedNote, answeredNote, note].filter(Boolean).join(" ");
        setMsg(base);
        await api.startPolling(addrs, 2000);
        // serial numbers only of the packs that answered the device query; gaps are named
        const gen = serialGen.current;
        api.scan(addrs).then((found) => {
          // a late answer of an earlier connection changes nothing
          if (gen !== serialGen.current) return;
          const gaps = found.length ? partialLine(addrs, found.map(([a]) => a), t) : t("conn.noAnswer");
          if (gaps) setMsg(`${base} ${gaps}`);
          onDevices(found); void readSerials(found.map(([a]) => a));
        }).catch(() => {});
        return;
      }
      const found = await api.scan();
      if (retry && !found.length && bus !== "can") throw new Error(t("conn.none"));
      setMsg([linkedNote, answeredNote, note, found.length ? t("conn.found", { count: found.length }) : t("conn.none")].filter(Boolean).join(" "));
      onConnected(found, bus);
      setSince(Date.now()); setLost(false);
      // on the CAN socket bus the system values are polled even if no pack answered
      if (found.length || bus === "can") await api.startPolling(found.map(([a]) => a), 2000);
      void readSerials(found.map(([a]) => a));
    } catch (e) {
      // a cancelled wait never opened the port: nothing to close, no error
      if (e instanceof Cancelled) { setMsg(t("conn.busyCancelled")); return; }
      if (retry) await api.disconnect().catch(() => {});
      const n = noAnswer(e);
      if (n && !retry) {
        // nothing answered: the port is closed. A USB port is asked once more at the other baud rate
        setFailed(true);
        if (kind === "serial") {
          setMsg(t("conn.check.probing", { baud: BAUD[otherBus(bus)] }));
          const probe = await api.probeOtherBus(target, bus, Array.from({ length: packCount }, (_, i) => i)).catch(() => null);
          if (probe) { const r = explain(bus, n, probe, t); setMsg(r.text); setOffer(r.offer); }
          else setMsg(`${firstLine(bus, n, t)} ${t("conn.check.hintSerial")}`);
        } else setMsg(explain(bus, n, null, t).text);
        return;
      }
      const err = n ? firstLine(bus, n, t) : e instanceof Error ? e.message : String(e);
      setMsg(retry ? t("conn.retryFailed", { err }) : err);
    } finally { setBusy(false); }
  };

  // Browser preview: connect straight away so the recorded system is visible.
  useEffect(() => { if (isDemo) connect(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const disconnect = async () => {
    stopWaiting(); serialGen.current += 1; forgetBank();
    // a running pack action (a restart up to 15 s) ends first
    setMsg(t("conn.disconnectWait"));
    await api.disconnect();
    onDisconnected(false); setMsg(null); setLost(false);
  };
  // another installation chosen anywhere (installations view, banner, new installation): this connection ends first
  const live = useRef(false);
  live.current = connected || lost;
  const disconnectRef = useRef(disconnect);
  disconnectRef.current = disconnect;
  useEffect(() => {
    maint.beforeSwitch.current = async () => { if (live.current) await disconnectRef.current().catch(() => {}); };
    return () => { maint.beforeSwitch.current = null; };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // The UI forgets the link first: a backend call that hangs must not keep it showing "connected".
  const markLost = (text: string) => {
    serialGen.current += 1; forgetBank();
    setLost(true);
    setMsg(text);
    setLastTry(Date.now());
    onDisconnected(true);
    api.disconnect().catch(() => {});
  };

  // The clock starts whenever the app counts as connected, also if this component was mounted afterwards
  // (otherwise the timeout below would never run).
  useEffect(() => { setSince(connected ? Date.now() : 0); }, [connected]);

  // Fixed timeout: no successful answer for CONN_TIMEOUT_MS (since connecting or since the last data) = disconnected.
  useEffect(() => {
    if (isDemo || busy || !connected) return;
    // a BMS history read pauses polling: that time is no lost connection
    if (now - Math.max(lastUpdate, since || now, readMark()) < CONN_TIMEOUT_MS) return;
    markLost(t("conn.lost", { s: CONN_TIMEOUT_MS / 1000 }));
  }, [now]); // eslint-disable-line react-hooks/exhaustive-deps

  // The backend reports a dead link at once; reconnecting starts after RECONNECT_MS.
  const onLinkLost = useRef((_: string) => {});
  onLinkLost.current = (err) => { if (connected && !busy) markLost(t("conn.dropped", { err })); };
  useEffect(() => {
    const un = api.onLinkLost((err) => onLinkLost.current(err));
    return () => { un.then((f) => f()); };
  }, []);

  // A gateway (TCP) linked to exactly one active installation chooses it; several: the current one if it is among
  // them, else a hint to pick. A serial port never changes the choice. Only a new address asks again, so an explicit
  // choice afterwards (also „Nur ansehen“) stays.
  const [pickHint, setPickHint] = useState(false);
  const asked = useRef<string | null>(null);
  useEffect(() => {
    if (isDemo || kind !== "tcp" || connected || lost || busy || !target.trim()) { setPickHint(false); return; }
    if (!maint.loaded || asked.current === target.trim().toLowerCase()) return;
    const timer = setTimeout(() => {
      asked.current = target.trim().toLowerCase();
      api.maintInstallationsOf({ kind: "tcp", addr: target, bus }, true).then((known) => {
        const d = preselect(maint.installation, known);
        setPickHint(d.do === "pick");
        if (d.do === "use") void maint.switchTo(d.id);
      }).catch(() => {});
    }, 400);
    return () => clearTimeout(timer);
  }, [kind, target, connected, lost, maint.loaded]); // eslint-disable-line react-hooks/exhaustive-deps

  // While lost, reconnect automatically with the last settings (not while the firmware view is open: it needs the port).
  useEffect(() => {
    // never for another installation than the connection's
    if (lost && !busy && !connected && !locked && view !== "firmware" && maint.installation === connInst.current && now - lastTry >= RECONNECT_MS) connect(true);
  }, [now]); // eslint-disable-line react-hooks/exhaustive-deps


  const fixed = connected || lost || busy || locked;
  const line = connected ? bankLine(ids) : null;
  const noSerial = connected ? noSerialPacks(ids) : [];
  const instName = maint.current?.name ?? "";
  // „Übernehmen“: the listed packs belong to this installation; they count as known from now on
  const confirm = async (addresses: number[]) => {
    const inst = connInst.current;
    if (!inst || !addresses.length) return;
    setConfirming(true); setBankError(null);
    try {
      const done = await api.bankConfirm(inst, addresses);
      setIds((all) => all.map((v) => (done.includes(v.address) ? { ...v, kind: "known" } : v)));
    } catch (e) { setBankError(t("bank.confirmFailed", { err: String(e) })); }
    finally { setConfirming(false); }
  };
  // „Mit „B“ verbinden“: disconnect, choose B, connect with the same settings
  const connectWith = async (id: string) => {
    await disconnect();
    await maint.switchTo(id);
    await connect(false, bus, id);
  };

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-line bg-surface">
      <div className="flex shrink-0 items-center gap-2.5 px-5 pb-4 pt-6">
        <BatteryCharging className="h-6 w-6 text-ok" aria-hidden />
        <span className="font-display text-xl font-semibold">{t("app")}</span>
      </div>
      {/* navigation scrolls if it ever gets long; connection and settings stay fixed at the bottom */}
      <nav className="min-h-0 flex-1 space-y-1 overflow-y-auto px-3 pb-4">
        {[["dashboard", LayoutDashboard], ["history", ChartLine], ["cells", Database], ["installations", Building2],
          ...(maintenance ? [["diag", Gauge], ["params", SlidersHorizontal], ["firmware", Cpu]] : [])].map(([id, Icon]) => {
          const I = Icon as typeof LayoutDashboard;
          return (
            <button key={id as string} onClick={() => setView(id as string)} disabled={locked}
              className={cn("flex w-full items-center gap-2 rounded-md px-3 py-2 disabled:opacity-50", view === id ? "bg-sunken font-medium text-ink" : "text-muted hover:bg-sunken/60 hover:text-ink")}>
              <I className="h-4 w-4" />{t(`nav.${id}`)}
            </button>
          );
        })}
      </nav>
      <div className="shrink-0 space-y-3 border-t border-line px-5 py-5">
        <div className="flex items-center justify-between text-sm">
          <span className="flex items-center gap-1.5 font-medium">{t("conn.title")}<InfoIcon id="topic.connection" /></span>
          <span className={cn("flex items-center gap-1.5", connected ? "text-charge" : lost ? "text-alarm" : "text-muted")}>
            <span className="relative flex h-2 w-2">
              {connected && lastUpdate > 0 && <span key={lastUpdate} className="ping-once absolute inset-0 rounded-full bg-charge" />}
              <span className={cn("relative h-2 w-2 rounded-full", connected ? "bg-charge" : lost ? "bg-alarm" : "border border-muted")} />
            </span>
            {connected ? t("conn.connected") : lost ? t("conn.lostShort") : paused ? t("conn.paused") : t("conn.offline")}
          </span>
        </div>
        {/* the installation first: everything stored belongs to it. It and the connection fields are fixed while
            connected (also while lost and retrying): a change takes effect only with the next connect, so until then
            the fields would show something else than the running connection. To change them: disconnect. */}
        {!isDemo && (
          <div className="space-y-1">
            <span className="flex items-center gap-1.5 text-sm text-muted">{t("conn.installation")}<InfoIcon id="topic.installations" /></span>
            <select className={input} value={maint.installation ?? ""} disabled={fixed} aria-label={t("conn.installation")}
              onChange={(e) => { const v = e.target.value; if (v === NEW) maint.setNaming(true); else void maint.switchTo(v || null); }}>
              {maint.installation && !maint.installations.some((i) => i.id === maint.installation) && <option value={maint.installation}>…</option>}
              {pickable(maint.installations, maint.installation).map((i) => <option key={i.id} value={i.id}>{i.name}{i.archived ? ` (${t("inst.archived")})` : ""}</option>)}
              <option value={NEW}>{t("maint.picker.new")}</option>
              <option value="">{t("conn.viewOnly")}</option>
            </select>
            {!maint.installation && <p className="text-xs text-muted">{t("conn.viewOnlyHint")}</p>}
            {pickHint && <p className="text-xs text-discharge">{t("conn.pickHint")}</p>}
          </div>
        )}
        <div className="grid grid-cols-2 gap-1 rounded-md bg-sunken p-1">
          {([["tcp", Network], ["serial", Usb]] as const).map(([k, I]) => (
            <button key={k} onClick={() => setKind(k)} disabled={fixed}
              className={cn("flex items-center justify-center gap-1.5 rounded py-1.5 text-sm disabled:opacity-60", kind === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink disabled:hover:text-muted")}>
              <I className="h-3.5 w-3.5" />{t(`conn.${k}`)}
            </button>
          ))}
        </div>
        {kind === "tcp" ? (
          <input className={input} value={target} disabled={fixed} onChange={(e) => setTarget(e.target.value)} placeholder="192.168.1.10:4196" aria-label={t("conn.host")} />
        ) : (
          <div className="flex items-center gap-1">
            <select className={input} value={target} disabled={fixed} onChange={(e) => setTarget(e.target.value)} aria-label={t("conn.port")}>
              <option value="">{t("conn.port")}</option>
              {ports.map((p) => <option key={p}>{p}</option>)}
            </select>
            <Button variant="ghost" size="icon" className="shrink-0" disabled={fixed} onClick={loadPorts} aria-label={t("fw.refreshPorts")} title={t("fw.refreshPorts")}><RefreshCw className="h-4 w-4" /></Button>
          </div>
        )}
        <label className="block">
          <span className="mb-1 flex items-center gap-1.5 text-sm text-muted">{t("conn.bus")}<InfoIcon id="topic.buses" /></span>
          <select className={input} value={bus} disabled={fixed} onChange={(e) => setBus(e.target.value as Bus)}>
            <option value="can">{t("conn.busCan")}</option>
            <option value="pack">{t("conn.busPack")}</option>
          </select>
        </label>
        <label className="flex items-center justify-between gap-3 text-sm text-muted">
          {t("conn.packs")}
          <select className={cn(input, "w-28")} value={packCount} disabled={fixed} onChange={(e) => setPackCount(Number(e.target.value))}>
            <option value={0}>{t("conn.packsAuto")}</option>
            {Array.from({ length: 16 }, (_, i) => i + 1).map((n) => <option key={n} value={n}>{n}</option>)}
          </select>
        </label>
        <div className="flex gap-2">
          <Button variant="primary" className="flex-1" disabled={busy || !target || locked} onClick={() => connect()}>
            {busy ? t("conn.checking") : connected ? t("conn.reconnect") : t("conn.connect")}
          </Button>
          {waiting && <Button variant="ghost" onClick={stopWaiting}>{t("conn.cancel")}</Button>}
          {(connected || lost) && <Button variant="ghost" disabled={locked} onClick={disconnect}>{t("conn.disconnect")}</Button>}
        </div>
        {/* fixed room for three lines, so the block does not jump when a status or error appears; long texts are clamped, the full text is the tooltip */}
        <div className="min-h-[3.75rem] space-y-0.5">
          {msg && <p title={msg} className={cn("break-words text-sm", failed ? "line-clamp-5" : "line-clamp-2", lost || failed ? "text-alarm" : "text-muted")}>{msg}</p>}
          {offer && !busy && !connected && (
            <Button variant="secondary" size="sm" className="mt-1" disabled={locked} onClick={() => { setBus(offer); void connect(false, offer); }}>
              {t("conn.check.switch", { bus: t(`conn.busName.${offer}`) })}
            </Button>
          )}
          {line?.kind === "foreign" && (
            <div className="space-y-1 pt-1">
              <p className="text-sm text-alarm">{t("bank.foreign", { count: line.packs.length, packs: packList(line.packs), other: line.other.name })}</p>
              <div className="flex flex-wrap gap-1">
                <Button variant="secondary" size="sm" disabled={locked || busy} onClick={() => void disconnect()}>{t("bank.disconnect")}</Button>
                <Button variant="secondary" size="sm" disabled={locked || busy} onClick={() => void connectWith(line.other.id)}>{t("bank.connectOther", { other: line.other.name })}</Button>
              </div>
              {!line.hint && line.confirm.length > 0 && (
                <button className="text-xs text-muted underline hover:text-ink disabled:opacity-60" disabled={confirming || locked} onClick={() => void confirm(line.confirm)}>
                  {t("bank.adoptAnyway", { name: instName })}
                </button>
              )}
            </div>
          )}
          {line?.kind === "new" && (
            <div className="space-y-1 pt-1">
              <p className="text-sm text-discharge">{t("bank.new", { count: line.packs.length, packs: packList(line.packs), name: instName })}</p>
              <Button variant="secondary" size="sm" disabled={confirming || locked} onClick={() => void confirm(line.confirm)}>{t("bank.adopt")}</Button>
            </div>
          )}
          {noSerial.length > 0 && (
            <p className="pt-1 text-xs text-muted">
              {t("bank.noSerial", { count: noSerial.length, packs: packList(noSerial) })}{" "}
              <button className="underline hover:text-ink disabled:opacity-60" disabled={locked} onClick={() => { setMaintenance(true); setView("params"); }}>{t("bank.openParams")}</button>
            </p>
          )}
          {bankError && <p className="text-sm text-alarm">{bankError}</p>}
          {lost && !busy && <p className="text-sm text-muted">{t("conn.retryIn", { s: Math.max(0, Math.ceil((RECONNECT_MS - (now - lastTry)) / 1000)) })}</p>}
          {isDemo && <p className="text-sm text-discharge">{t("conn.demo")}</p>}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1 border-t border-line px-3 py-3">
        <Button variant="ghost" size="sm" onClick={() => i18n.changeLanguage(i18n.language === "de" ? "en" : "de")}>
          <Languages className="h-4 w-4" />{i18n.language.toUpperCase()}
        </Button>
        <span className="flex-1" />
        <Button variant="ghost" size="icon" onClick={() => setDark(!dark)} aria-label="Theme">
          {dark ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
        </Button>
        <RecordingEntry scope={maint.installation} packs={packs} systemAvailable={bus === "can"} connected={connected} onOpenHistory={() => setView("history")} />
        {/* maintenance functions: an icon toggle, highlighted while on; the label is the tooltip */}
        <Button variant="ghost" size="icon" onClick={() => setMaintenance(!maintenance)} disabled={locked}
          aria-pressed={maintenance} aria-label={t("maintToggle")} title={t("maintToggle")}
          className={cn(maintenance && "bg-sunken text-ink")}>
          <Wrench className="h-4 w-4" />
        </Button>
      </div>
    </aside>
  );
}
