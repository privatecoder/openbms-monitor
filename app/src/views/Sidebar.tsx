import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { BatteryCharging, Cpu, Database, Gauge, Languages, LayoutDashboard, Moon, Network, SlidersHorizontal, Sun, Usb, ChartLine, Wrench } from "lucide-react";
import { api, isDemo, type Bus, type DeviceInfo, type Endpoint, type SerialNumbers } from "../api";
import { Button } from "../components/ui/button";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { CONN_TIMEOUT_MS, RECONNECT_MS, useNow } from "../components/live";
import { busyMessage, busyWaitStep, portBusy } from "../maint";
import { RecordingEntry } from "./Recording";

/** a wait for a busy port that was ended (cancel, new settings, a user action, a flash, the firmware view) */
class Cancelled extends Error {}

// a busy port: try again every BUSY_POLL_MS, give up after BUSY_WAIT_MS
const BUSY_POLL_MS = 500;
const BUSY_WAIT_MS = 30_000;

const input = "h-9 w-full rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function Sidebar({ userActions, connected, paused, lastUpdate, onConnected, onDevices, onSerials, onDisconnected, view, setView, site, packs, locked, maintenance, setMaintenance, onEndpoint }: {
  connected: boolean;
  /** the app stopped the monitor itself for a maintenance step (not lost, not closed by the user) */
  paused: boolean;
  site: string;
  packs: number[];
  lastUpdate: number;
  onConnected: (found: [number, DeviceInfo | undefined][], site: string, bus: Bus) => void;
  onDevices: (found: [number, DeviceInfo][]) => void;
  onSerials: (address: number, serials: SerialNumbers) => void;
  /** `lost`: the link died or timed out; false: closed by the user */
  onDisconnected: (lost: boolean) => void;
  view: string;
  setView: (v: string) => void;
  locked: boolean;
  /** counts actions the user started in the firmware view or the pack bus panel: a wait for a busy port stops */
  userActions: number;
  maintenance: boolean;
  setMaintenance: (v: boolean) => void;
  onEndpoint: (endpoint: Endpoint) => void;
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
  // lost: no data for CONN_TIMEOUT_MS; then the connection is dropped and retried every RECONNECT_MS
  const [lost, setLost] = useState(false);
  const [since, setSince] = useState(0);
  const [lastTry, setLastTry] = useState(0);
  const now = useNow();
  const [dark, setDark] = useState(document.documentElement.classList.contains("dark"));

  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, [kind]);
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
  useEffect(() => stopWaiting, []);
  const connectWhenFree = async (ep: Endpoint, auto: boolean) => {
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
        try { await api.connect(ep); return; } catch (e) {
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
    for (const a of addresses) {
      if (gen !== serialGen.current) return;
      const s = await api.serialNumbers(a).catch(() => null);
      if (s && gen === serialGen.current) onSerials(a, s);
    }
  };

  // a maintenance step that stops the monitor ends the reads as well
  useEffect(() => {
    const un = api.onMonitorStopped(() => { serialGen.current += 1; });
    return () => { un.then((f) => f()); };
  }, []);

  const connect = async (retry = false) => {
    serialGen.current += 1;
    setBusy(true); if (!retry) { setMsg(null); setLost(false); }
    setLastTry(Date.now());
    try {
      localStorage.setItem("conn.kind", kind); localStorage.setItem(`conn.${kind}`, target); localStorage.setItem("conn.bus", bus);
      localStorage.setItem("conn.packs", String(packCount));
      const ep: Endpoint = kind === "tcp" ? { kind, addr: target, bus } : { kind, path: target, bus };
      await connectWhenFree(ep, retry);
      void onEndpoint(ep);
      const range = (n: number) => Array.from({ length: n }, (_, i) => i);
      // pack addresses known without a search: from the master (patched firmware, CAN socket bus), else a fixed count
      let addrs: number[] | null = null;
      let note = "";
      if (bus === "can") {
        // the master answers Modbus at once: show its system values before anything slower
        const sys = await api.systemOnce().then(() => true, () => false);
        if (sys) { onConnected([], target, bus); setSince(Date.now()); setLost(false); }
        const m = await api.maintStatus().catch(() => null);
        if (m && !m.marker) { addrs = range(m.effective_slaves + 1); note = t("conn.fromMaster", { count: addrs.length }); }
        else if (m?.marker) note = t("conn.maintActive");
      }
      if (!addrs && packCount > 0) { addrs = range(packCount); note = t("conn.fixed", { count: packCount }); }
      if (addrs) {
        // start polling at once; name and firmware version follow from a short device query
        onConnected(addrs.map((a) => [a, undefined]), target, bus);
        setSince(Date.now()); setLost(false);
        setMsg(note);
        await api.startPolling(addrs, 2000);
        // serial numbers only of the packs that answered the device query
        api.scan(addrs).then((found) => { onDevices(found); void readSerials(found.map(([a]) => a)); }).catch(() => {});
        return;
      }
      const found = await api.scan();
      if (retry && !found.length && bus !== "can") throw new Error(t("conn.none"));
      setMsg([note, found.length ? t("conn.found", { count: found.length }) : t("conn.none")].filter(Boolean).join(" "));
      onConnected(found, target, bus);
      setSince(Date.now()); setLost(false);
      // on the CAN socket bus the system values are polled even if no pack answered
      if (found.length || bus === "can") await api.startPolling(found.map(([a]) => a), 2000);
      void readSerials(found.map(([a]) => a));
    } catch (e) {
      // a cancelled wait never opened the port: nothing to close, no error
      if (e instanceof Cancelled) { setMsg(t("conn.busyCancelled")); return; }
      if (retry) await api.disconnect().catch(() => {});
      const err = e instanceof Error ? e.message : String(e);
      setMsg(retry ? t("conn.retryFailed", { err }) : err);
    } finally { setBusy(false); }
  };

  // Browser preview: connect straight away so the recorded system is visible.
  useEffect(() => { if (isDemo) connect(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const disconnect = async () => { stopWaiting(); serialGen.current += 1; await api.disconnect(); onDisconnected(false); setMsg(null); setLost(false); };

  // The UI forgets the link first: a backend call that hangs must not keep it showing "connected".
  const markLost = (text: string) => {
    serialGen.current += 1;
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
    if (now - Math.max(lastUpdate, since || now) < CONN_TIMEOUT_MS) return;
    markLost(t("conn.lost", { s: CONN_TIMEOUT_MS / 1000 }));
  }, [now]); // eslint-disable-line react-hooks/exhaustive-deps

  // The backend reports a dead link at once; reconnecting starts after RECONNECT_MS.
  const onLinkLost = useRef((_: string) => {});
  onLinkLost.current = (err) => { if (connected && !busy) markLost(t("conn.dropped", { err })); };
  useEffect(() => {
    const un = api.onLinkLost((err) => onLinkLost.current(err));
    return () => { un.then((f) => f()); };
  }, []);

  // While lost, reconnect automatically with the last settings (not while the firmware view is open: it needs the port).
  useEffect(() => {
    if (lost && !busy && !connected && !locked && view !== "firmware" && now - lastTry >= RECONNECT_MS) connect(true);
  }, [now]); // eslint-disable-line react-hooks/exhaustive-deps


  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-line bg-surface">
      <div className="flex shrink-0 items-center gap-2.5 px-5 pb-4 pt-6">
        <BatteryCharging className="h-6 w-6 text-ok" aria-hidden />
        <span className="font-display text-xl font-semibold">{t("app")}</span>
      </div>
      {/* navigation scrolls if it ever gets long; connection and settings stay fixed at the bottom */}
      <nav className="min-h-0 flex-1 space-y-1 overflow-y-auto px-3 pb-4">
        {[["dashboard", LayoutDashboard], ["history", ChartLine], ["cells", Database],
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
        <div className="grid grid-cols-2 gap-1 rounded-md bg-sunken p-1">
          {([["tcp", Network], ["serial", Usb]] as const).map(([k, I]) => (
            <button key={k} onClick={() => setKind(k)}
              className={cn("flex items-center justify-center gap-1.5 rounded py-1.5 text-sm", kind === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>
              <I className="h-3.5 w-3.5" />{t(`conn.${k}`)}
            </button>
          ))}
        </div>
        {kind === "tcp" ? (
          <input className={input} value={target} onChange={(e) => setTarget(e.target.value)} placeholder="192.168.1.10:4196" aria-label={t("conn.host")} />
        ) : (
          <select className={input} value={target} onChange={(e) => setTarget(e.target.value)} aria-label={t("conn.port")}>
            <option value="">{t("conn.port")}</option>
            {ports.map((p) => <option key={p}>{p}</option>)}
          </select>
        )}
        <label className="block">
          <span className="mb-1 flex items-center gap-1.5 text-sm text-muted">{t("conn.bus")}<InfoIcon id="topic.buses" /></span>
          <select className={input} value={bus} onChange={(e) => setBus(e.target.value as Bus)}>
            <option value="can">{t("conn.busCan")}</option>
            <option value="pack">{t("conn.busPack")}</option>
          </select>
        </label>
        <label className="flex items-center justify-between gap-3 text-sm text-muted">
          {t("conn.packs")}
          <select className={cn(input, "w-28")} value={packCount} onChange={(e) => setPackCount(Number(e.target.value))}>
            <option value={0}>{t("conn.packsAuto")}</option>
            {Array.from({ length: 16 }, (_, i) => i + 1).map((n) => <option key={n} value={n}>{n}</option>)}
          </select>
        </label>
        <div className="flex gap-2">
          <Button variant="primary" className="flex-1" disabled={busy || !target || locked} onClick={() => connect()}>
            {busy ? (packCount > 0 ? t("conn.starting") : t("conn.scanning")) : connected ? t("conn.reconnect") : t("conn.connect")}
          </Button>
          {waiting && <Button variant="ghost" onClick={stopWaiting}>{t("conn.cancel")}</Button>}
          {(connected || lost) && <Button variant="ghost" disabled={locked} onClick={disconnect}>{t("conn.disconnect")}</Button>}
        </div>
        {/* fixed room for three lines, so the block does not jump when a status or error appears; long texts are clamped, the full text is the tooltip */}
        <div className="min-h-[3.75rem] space-y-0.5">
          {msg && <p title={msg} className={cn("line-clamp-2 break-words text-sm", lost ? "text-alarm" : "text-muted")}>{msg}</p>}
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
        <RecordingEntry site={site} packs={packs} systemAvailable={bus === "can"} connected={connected} onOpenHistory={() => setView("history")} />
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
