import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { BatteryCharging, Building2, Cpu, Database, Gauge, Languages, LayoutDashboard, Moon, Network, RefreshCw, SlidersHorizontal, Sun, Usb, ChartLine, Download, Info, Wrench } from "lucide-react";
import { api, isDemo, linkNow, type Bus, type Connected, type DeviceInfo, type Endpoint, type IdentityView, type MqttSettings, type SerialNumbers } from "../api";
import { feedText, getSource, useFeed } from "../source";
import { loadedSettings, useSettings, useSettingsRevision } from "../installData";
import { decideUi, lockProps, type UiFacts } from "../locks";
import { patched, profileLine, recheckable, setProfile, useProfile } from "../profile";
import { bankLine, clearUnchecked, noSerialPacks, packList, withIdentity } from "../bank";
import type { Maint } from "../maintStore";
import { Button } from "../components/ui/button";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { CONN_TIMEOUT_MS, RECONNECT_TRIES, reconnectDelay, useNow, waitText } from "../components/live";
import { packBusy } from "../control";
import { busyMessage, busyWaitStep, errorKey, pickable, portBusy, portKey, preselect, refusalParams, saveOffer, storedPort, withPort, type PortKind } from "../maint";
import { RecordingEntry, useHistoryStatus } from "./Recording";
import { readMark } from "../bmsMemory";
import { pollsBus, refusalOf, useWiring } from "../wiring";
import { DIP_MAX } from "../access";
import { useUpdate } from "../update";
import { BAUD, connectCancelled, connectedForOther, endBeforeSwitch, explain, fillFields, firstLine, editBroker, fixedFallback, profileLoadNeeded, targetsForInstallation, gapLine, initialWay, lastWay, mergeAfterSearch, mqttSaveAction, pruneGap, lastConn, noAnswer, otherBus, packsLine, packsOf, padAddress, parseSel, saveLastConn, saveLastWay, selAddresses, selText, type Gap, type PackSel } from "../connCheck";

/** a wait for a busy port that was ended (cancel, new settings, a user action, a flash, the firmware view) */
class Cancelled extends Error {}
/** „Abbrechen“ while connecting, noticed between two steps */
class ConnectCancelled extends Error {}

// a busy port: try again every BUSY_POLL_MS, give up after BUSY_WAIT_MS
const BUSY_POLL_MS = 500;
const BUSY_WAIT_MS = 30_000;

const input = "h-9 w-full rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-60";
// the select's entry that opens the name dialog
const NEW = "__new__";
// „Nein“ to saving an endpoint: not asked again for it in this session
const declined = new Set<string>();

/** The offer under the connection status to store the connected endpoint as a port of the installation. */
interface SaveAsk { inst: string; name: string; bus: Bus; kind: PortKind; target: string }

const MQTT_EMPTY: MqttSettings = { host: "", port: 1883, user: "", password: "", tls: false, base: "seplos" };

export function Sidebar({ userActions, connected, paused, lastUpdate, onConnected, onDevices, onSerials, onBoard, onDisconnected, view, setView, maint, packs, answering, locked, maintenance, setMaintenance }: {
  connected: boolean;
  /** the app stopped the monitor itself for a maintenance step (not lost, not closed by the user) */
  paused: boolean;
  /** the chosen installation (null: „Nur ansehen“) and the list */
  maint: Maint;
  packs: number[];
  /** the packs with fresh values now (the connect's gap note names only the ones still silent) */
  answering: number[];
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
  const update = useUpdate();
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
  // the data source: the monitor's own RS485 connection, or the Seplos BMS add-on's feed over MQTT (read-only)
  const [src, setSrc] = useState<"bus" | "mqtt">(() => (localStorage.getItem("conn.source") === "mqtt" ? "mqtt" : "bus"));
  const [mq, setMq] = useState<MqttSettings>(MQTT_EMPTY);
  // the broker stored for the chosen installation (or the "view only" one): what a successful connect compares with
  const [mqStored, setMqStored] = useState<MqttSettings | null>(null);
  // after a connect with another broker than the installation's: the offer to keep it
  const [mqttAsk, setMqttAsk] = useState<{ inst: string; name: string; settings: MqttSettings } | null>(null);
  // the broker of the chosen installation is being read: its form and „Verbinden“ wait, so no other installation's
  // broker is ever used for it
  const [mqLoading, setMqLoading] = useState(false);
  const settingsRev = useSettingsRevision();
  const feed = useFeed();
  const setTarget = (v: string) => setTargets((cur) => ({ ...cur, [kind]: v }));
  const [bus, setBus] = useState<Bus>(() => (localStorage.getItem("conn.bus") as Bus) ?? "can");
  // 0 = search addresses 0..15; n = packs at addresses 0..n-1, polled right away without a search
  // which packs are polled: search, a count, or one fixed address (migrated from the old stored count)
  // stored under the old key: "search" / "count:n" / "address:k", or the old plain count
  // „Nur ansehen“ keeps its own selection here; with an installation its setting „packs“ counts (Installations → Ports)
  const [viewSel, setViewSel] = useState<PackSel>(() => { const v = localStorage.getItem("conn.packs"); return parseSel(v, v); });
  const instSettings = useSettings(maint.installation);
  const packSel: PackSel = maint.installation ? packsOf(instSettings.settings.packs) ?? { mode: "search" } : viewSel;
  // a connect with an installation waits for its setting (never a search instead of a stored count)
  const packsLoading = !!maint.installation && !instSettings.loaded;
  const [ports, setPorts] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  // the packs the connect missed, shown after `msg` as long as any of them is still silent
  const [gap, setGap] = useState<Gap | null>(null);
  // the last connect found no pack: the message is an error; `offer`: the other bus answered, one click switches
  const [failed, setFailed] = useState(false);
  const [offer, setOffer] = useState<Bus | null>(null);
  // lost: no data for CONN_TIMEOUT_MS; then the connection is dropped and retried with growing pauses
  // (`reconnectDelay`); `tries`: the automatic attempts since the loss
  const [lost, setLost] = useState(false);
  const [since, setSince] = useState(0);
  const [lastTry, setLastTry] = useState(0);
  const [tries, setTries] = useState(0);
  const now = useNow();
  // a pack that answered leaves the gap for good; the note shows only while the monitor polls
  useEffect(() => { if (gap) { const g = pruneGap(gap, answering); if (g !== gap) setGap(g); } }, [gap, answering.join()]); // eslint-disable-line react-hooks/exhaustive-deps
  const shown = [msg, gap && connected ? gapLine(gap, answering, t) : ""].filter(Boolean).join(" ") || null;
  const maintRef = useRef(maint); maintRef.current = maint;
  const recStatus = useHistoryStatus().status;
  const [dark, setDark] = useState(document.documentElement.classList.contains("dark"));
  // what the serial reads of this connection say about each pack's board (right bank before writing)
  const [ids, setIds] = useState<IdentityView[]>([]);
  const [confirming, setConfirming] = useState(false);
  const [bankError, setBankError] = useState<string | null>(null);
  const [saveAsk, setSaveAsk] = useState<SaveAsk | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

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
  useEffect(stopWaiting, [target, kind, bus, selText(packSel), userActions]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { setOffer(null); }, [target, kind, bus]);
  useEffect(() => stopWaiting, []);
  const connectWhenFree = async (ep: Endpoint, auto: boolean, installation: string | null, check: number[]): Promise<Connected> => {
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
  // `linkGen`: the backend's connection generation; when the reads end (also with no pack, or all failed) the backend
  // decides whether this connection records (`bank_reads_done`)
  const readSerials = async (addresses: number[], linkGen: number) => {
    const gen = ++serialGen.current;
    try { await readEach(addresses, gen); } finally { if (gen === serialGen.current) void api.bankReadsDone(linkGen).catch(() => {}); }
  };
  const readEach = async (addresses: number[], gen: number) => {
    // a pack busy with another action (a write, the parameter read) is asked again at the end, at most three times; a
    // failed read once more at the end (reads that collided would otherwise keep the recorder waiting)
    const queue = addresses.map((a) => ({ a, tries: 0, again: false }));
    for (let q = queue.shift(); q; q = queue.shift()) {
      const { a } = q;
      if (gen !== serialGen.current) return;
      let busyNow = false;
      const s = await api.serialNumbers(a).catch((e) => { busyNow = !!packBusy(e); return null; });
      if (busyNow && q.tries < 3) {
        queue.push({ ...q, tries: q.tries + 1 });
        await new Promise((r) => setTimeout(r, 1000));
        continue;
      }
      if ((!s || s.identity?.kind === "failed") && !q.again) {
        queue.push({ ...q, again: true });
        if (!s) continue;
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
  // RS485-1/2: the endpoint is linked to the installation once the bank is decided
  useEffect(() => {
    const un = api.onBankLinked((name) => { setMsg((m) => [t("conn.linked", { name }), m].filter(Boolean).join(" ")); void maintRef.current.loadLists(); });
    return () => { un.then((f) => f()); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const forgetBank = () => { setIds([]); setBankError(null); clearUnchecked(); setSaveAsk(null); setSaveError(null); };

  // the installation of the connection: chosen at a manual connect, reused by every automatic reconnect
  const connInst = useRef<string | null>(null);
  // `useBus`: the bus offered after a failed check (the select is set as well); `inst`: the installation to connect
  // with when it was switched just now („Mit „B“ verbinden“), before this closure sees the new choice
  // „Abbrechen“ while connecting: the backend stops the check or the search before the next address, the UI between
  // its steps; whatever was opened is closed again
  const cancelRef = useRef(false);
  // the running connect, for a switch of the installation meanwhile (`endBeforeSwitch`); whether a link is open
  const running = useRef<Promise<void> | null>(null);
  const linkOpen = useRef(false);
  const cancelConnect = () => { cancelRef.current = true; stopWaiting(); void api.connectCancel().catch(() => {}); };
  const stopIfCancelled = () => { if (cancelRef.current) throw new ConnectCancelled(); };
  const connect = async (retry = false, useBus: Bus = bus, inst?: string | null) => {
    let done = () => {};
    const p = new Promise<void>((r) => { done = r; });
    running.current = p;
    try { await connectOnce(retry, useBus, inst); } finally { if (running.current === p) running.current = null; done(); }
  };
  // a failed read of the installation's settings is an error, never taken as „search“
  const selFor = async (inst: string | null): Promise<PackSel> => {
    if (!inst) return viewSel;
    const st = loadedSettings(inst) ?? await api.installationSettings(inst);
    return packsOf(st.packs) ?? { mode: "search" };
  };
  // the selection the running RS485 connection started with (the setting may change meanwhile, for the next one)
  const [runSel, setRunSel] = useState<PackSel | null>(null);
  const connectOnce = async (retry: boolean, useBus: Bus, inst?: string | null) => {
    cancelRef.current = false;
    serialGen.current += 1;
    // a broker still being read belongs to before this connect: its answer changes neither the form nor the way chosen
    // (it is read again once the connection ends)
    mqGen.current += 1;
    setMqLoading(false);
    forgetBank();
    if (!retry) connInst.current = inst !== undefined ? inst : maint.installation;
    const bus = useBus;
    setBusy(true); setFailed(false); setOffer(null); setGap(null); if (!retry) { setMsg(null); setLost(false); setTries(0); }
    setLastTry(Date.now());
    // the packs this connection polls (RS485 only), fixed for its whole run: the setting of the installation it is for,
    // as stored now (`selFor`, read below)
    let sel: PackSel = { mode: "search" };
    let addrsFixed: number[] = [];
    try {
      localStorage.setItem("conn.source", src);
      if (src === "mqtt") {
        const used = mq, owner = connInst.current, stored = mqStored;
        await api.connectMqtt(used, owner);
        linkOpen.current = true;
        stopIfCancelled();
        onConnected([], "can");
        setSince(Date.now()); setLost(false);
        let note = "";
        // kept only after a connect that worked: a first broker for the installation, the "view only" one always
        if (!retry) {
          const name = maint.installations.find((i) => i.id === owner)?.name ?? "";
          const action = owner ? mqttSaveAction(stored, used) : "save";
          if (action === "save") {
            await api.saveMqttSettings(owner, used).then(() => { setMqStored(used); if (owner) note = t("conn.mqtt.saved", { name }); }, () => {});
          } else if (action === "ask" && owner) setMqttAsk({ inst: owner, name, settings: used });
          if (owner) saveLastWay(owner, "mqtt");
        }
        setMsg([t("conn.mqtt.connected", { host: used.host }), note].filter(Boolean).join(" "));
        return;
      }
      sel = await selFor(connInst.current);
      addrsFixed = selAddresses(sel) ?? [];
      // cancelled, or the choice changed, while the setting was read: nothing is opened
      stopIfCancelled();
      if (connectedForOther(connInst.current, maintRef.current.chosen())) throw new ConnectCancelled();
      localStorage.setItem("conn.kind", kind); localStorage.setItem(`conn.${kind}`, target); localStorage.setItem("conn.bus", bus);
      if (!connInst.current) localStorage.setItem("conn.packs", selText(sel));
      const ep: Endpoint = kind === "tcp" ? { kind, addr: target, bus } : { kind, path: target, bus };
      const range = (n: number) => Array.from({ length: n }, (_, i) => i);
      // the check asks the addresses to be polled first, then the rest of 0..15: one answer is enough
      const { linked, answered, gen: linkGen, profile } = await connectWhenFree(ep, retry, connInst.current, addrsFixed);
      linkOpen.current = true;
      // only now is this the running selection: a reconnect cancelled while it waited keeps the old one shown
      setRunSel(sel);
      stopIfCancelled();
      if (connInst.current && !retry) saveLastWay(connInst.current, kind);
      // the choice changed while connecting: this link belongs to the earlier one, it is closed again
      if (connectedForOther(connInst.current, maintRef.current.chosen())) throw new ConnectCancelled();
      if (connInst.current) saveLastConn(connInst.current, { kind, target, bus });
      // another port stored for this bus: offer to replace it (never stored without „Speichern“)
      const owner = maint.installations.find((i) => i.id === connInst.current) ?? null;
      const ask = owner && { inst: owner.id, name: owner.name, bus, kind, target };
      if (ask && !retry && !linked && saveOffer(owner, bus, kind, target) && !declined.has(askKey(ask))) setSaveAsk(ask);
      // „Anschluss wird Anlage X zugeordnet“: said once, no dialog
      const linkedNote = linked ? t("conn.linked", { name: maint.installations.find((i) => i.id === connInst.current)?.name ?? maint.current?.name ?? "" }) : "";
      if (linked) void maint.loadLists();
      let answeredNote = t("conn.answered", { addr: padAddress(answered) });
      // pack addresses known without a search: from the master's state in the firmware profile (patched firmware, CAN
      // socket bus, read on this endpoint by the connect), else a fixed count
      let addrs: number[] | null = null;
      let note = "";
      if (bus === "can") {
        // the master answers Modbus at once: show its system values before anything slower
        const sys = await api.systemOnce().then(() => true, () => false);
        if (sys) { onConnected([], bus); setSince(Date.now()); setLost(false); }
        const m = profile.master;
        // the master's slave count goes into the answered note, and only when it has slaves
        // only when searching: a count or an address the user set counts as set
        if (sel.mode === "search" && patched(m) && m.marker === "off" && m.slaves !== null) { addrs = range(m.slaves + 1); if (m.slaves > 0) answeredNote = t("conn.answeredSlaves", { addr: padAddress(answered), count: m.slaves }); }
        else if (sel.mode === "search" && patched(m) && m.marker === "on") note = t("conn.maintActive");
      }
      stopIfCancelled();
      // a fixed count polls 0..n-1 at once, unless the connect check answered outside them: then the normal search
      let fixedMissed = "";
      let fixedNote = "";
      if (!addrs && addrsFixed.length) {
        if (fixedFallback(addrsFixed, answered, null) === "keep") {
          addrs = addrsFixed;
          note = fixedNote = sel.mode === "address" ? t("conn.fixedAddress", { addr: padAddress(sel.k) }) : t("conn.fixed", { count: addrsFixed.length });
        } else fixedMissed = t("conn.fixedOutside", { addr: padAddress(answered) });
      }
      if (addrs) {
        // start polling at once; name and firmware version follow from a short device query
        onConnected(addrs.map((a) => [a, undefined]), bus);
        setSince(Date.now()); setLost(false);
        const base = [linkedNote, answeredNote, note].filter(Boolean).join(" ");
        setMsg(base);
        await api.startPolling(addrs, 2000);
        stopIfCancelled();
        // serial numbers only of the packs that answered the device query; gaps are named
        const gen = serialGen.current;
        const fixedCount = !!fixedNote;
        api.scan(addrs, true).then(async (found) => {
          // a late answer of an earlier connection changes nothing
          if (gen !== serialGen.current) return;
          // a fixed count whose addresses all stay silent: the normal search, and the message says so
          const got = found.map(([a]) => a);
          // „Anzahl n“ searches when any expected address is missing, a fixed address only when it does not answer
          if (fixedCount && fixedFallback(addrs, answered, got, sel.mode === "address" ? "address" : "count") === "search") {
            const all = await api.scan(undefined, true);
            if (gen !== serialGen.current) return;
            const at = all.map(([a]) => a);
            // the expected packs stay (a silent one is polled on, „ohne aktuelle Antwort“), the ones found elsewhere join
            const merged = mergeAfterSearch(addrs, at);
            setMsg([linkedNote, answeredNote].filter(Boolean).join(" "));
            setGap({ expected: addrs, missing: addrs.filter((a) => !got.includes(a)), searched: at });
            const device = (a: number) => all.find(([b]) => b === a)?.[1] ?? found.find(([b]) => b === a)?.[1];
            onConnected(merged.map((a) => [a, device(a)]), bus);
            await api.startPolling(merged, 2000);
            void readSerials(at, linkGen);
            return;
          }
          setGap({ expected: addrs, missing: addrs.filter((a) => !got.includes(a)), searched: null });
          onDevices(found); void readSerials(found.map(([a]) => a), linkGen);
        }).catch(() => { if (gen === serialGen.current) void api.bankReadsDone(linkGen).catch(() => {}); });
        return;
      }
      const found = await api.scan(undefined, true);
      stopIfCancelled();
      if (retry && !found.length && bus !== "can") throw new Error(t("conn.none"));
      setMsg([linkedNote, answeredNote, note, fixedMissed, found.length ? t("conn.found", { count: found.length }) : t("conn.none")].filter(Boolean).join(" "));
      onConnected(found, bus);
      setSince(Date.now()); setLost(false);
      // on the CAN socket bus the system values are polled even if no pack answered
      if (found.length || bus === "can") await api.startPolling(found.map(([a]) => a), 2000);
      stopIfCancelled();
      void readSerials(found.map(([a]) => a), linkGen);
    } catch (e) {
      // a cancelled wait never opened the port: nothing to close, no error
      if (e instanceof Cancelled && !cancelRef.current) { setMsg(t("conn.busyCancelled")); return; }
      if (cancelRef.current || e instanceof ConnectCancelled || connectCancelled(e)) {
        await disconnect().catch(() => {});
        setMsg(t("conn.connectCancelled"));
        return;
      }
      if (retry) await api.disconnect(true).catch(() => {});
      // the bus rule sent nothing on this bus (`wiring.rs`): no other baud rate is tried either
      const refused = refusalOf(e);
      if (refused) { setFailed(true); setMsg(t(`wiring.refused.${refused}`)); return; }
      const n = noAnswer(e);
      if (n && !retry) {
        // nothing answered: the port is closed. A USB port is asked once more at the other baud rate
        setFailed(true);
        if (kind === "serial") {
          setMsg(t("conn.check.probing", { baud: BAUD[otherBus(bus)] }));
          const other = { refused: null as string | null };
          const probe = await api.probeOtherBus(target, bus, addrsFixed).catch((pe) => { other.refused = refusalOf(pe); return null; });
          if (cancelRef.current) { await api.disconnect().catch(() => {}); setFailed(false); setMsg(t("conn.connectCancelled")); return; }
          // the bus rule kept the other bus silent: say so, not „nothing answered there“
          if (other.refused) { setMsg(`${firstLine(bus, n, t)} ${t(`wiring.refused.${other.refused}`)}`); return; }
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
    stopWaiting(); serialGen.current += 1; forgetBank(); linkOpen.current = false;
    // a running pack action (a restart up to 15 s) ends first
    setMsg(t("conn.disconnectWait"));
    await api.disconnect();
    onDisconnected(false); setMsg(null); setGap(null); setLost(false);
  };
  // another installation chosen anywhere (installations view, banner, new installation): this connection ends first
  const live = useRef(false);
  live.current = connected || lost;
  const disconnectRef = useRef(disconnect);
  disconnectRef.current = disconnect;
  useEffect(() => {
    maint.beforeSwitch.current = () => endBeforeSwitch({
      running: () => running.current, cancel: () => cancelRef.current || cancelConnect(),
      live: () => live.current || linkOpen.current, disconnect: () => disconnectRef.current(),
    });
    return () => { maint.beforeSwitch.current = null; };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // The UI forgets the link first: a backend call that hangs must not keep it showing "connected".
  const markLost = (text: string) => {
    serialGen.current += 1; forgetBank(); linkOpen.current = false;
    setLost(true);
    setMsg(text); setGap(null);
    setLastTry(Date.now()); setTries(0);
    onDisconnected(true);
    api.disconnect(true).catch(() => {});
  };

  // The clock starts whenever the app counts as connected, also if this component was mounted afterwards
  // (otherwise the timeout below would never run).
  useEffect(() => { setSince(connected ? Date.now() : 0); }, [connected]);

  // a valid answer of the poll loop counts before its pack event (which waits for the status read after it)
  const answeredAt = useRef(0);
  useEffect(() => {
    let off: (() => void) | undefined;
    void api.onAnswer(() => { answeredAt.current = Date.now(); }).then((u) => { off = u; });
    return () => off?.();
  }, []);

  // Fixed timeout: no successful answer for CONN_TIMEOUT_MS (since connecting or since the last data) = disconnected.
  useEffect(() => {
    if (isDemo || busy || !connected) return;
    // the add-on's feed says itself why no data come (switched off, add-on offline); the MQTT client reconnects alone
    if (getSource() === "mqtt") return;
    // a BMS history read pauses polling: that time is no lost connection
    if (now - Math.max(lastUpdate, answeredAt.current, since || now, readMark()) < CONN_TIMEOUT_MS) return;
    markLost(t("conn.lost", { s: CONN_TIMEOUT_MS / 1000 }));
  }, [now]); // eslint-disable-line react-hooks/exhaustive-deps

  // The backend reports a dead link at once; reconnecting starts after `reconnectDelay(0)`.
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
    // only for the RS485 network tab: a gateway hidden behind the Home Assistant tab chooses nothing
    if (isDemo || src !== "bus" || kind !== "tcp" || connected || lost || busy || !target.trim()) { setPickHint(false); return; }
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
  }, [src, kind, target, connected, lost, maint.loaded]); // eslint-disable-line react-hooks/exhaustive-deps

  // Another installation or bus chosen while not connected: the installation's stored port of the bus fills the
  // fields, the last working connection the bus and pack count (`fillFields`). The address counts as asked, so the
  // gateway preselection above does not switch back. A port edited in the installations view is taken over too.
  const apply = (f: ReturnType<typeof fillFields>) => {
    if (!f) return;
    setOther(false);
    setKind(f.kind);
    setTargets((cur) => ({ ...cur, [f.kind]: f.target }));
    if (f.kind === "tcp") asked.current = f.target.trim().toLowerCase();
    setBus(f.bus);
  };
  const cur = maint.installations.find((i) => i.id === maint.installation) ?? null;
  const storedPorts = JSON.stringify([cur?.control ?? null, cur?.endpoints ?? []]);
  const portsFor = useRef<string | null>(null);
  useEffect(() => {
    if (connected || lost || busy || !maint.installation) return;
    const f = fillFields(cur, bus, lastConn(maint.installation), "installation");
    // another installation: its own port only, the other transport's field emptied (never the one chosen before)
    if (portsFor.current !== maint.installation) { setTargets(targetsForInstallation(f)); portsFor.current = maint.installation; }
    apply(f);
  }, [maint.installation, maint.loaded, storedPorts]); // eslint-disable-line react-hooks/exhaustive-deps
  // The broker of the chosen installation (or the "view only" one) fills the Home Assistant tab, and the way last used
  // with it is preselected; an answer for an installation chosen before is dropped. Never while connected.
  const mqGen = useRef(0);
  const mqFor = useRef<string | null | undefined>(undefined);
  // what the form holds was read for this installation at this settings revision
  const mqLoaded = useRef<{ inst: string | null; rev: number } | null>(null);
  useEffect(() => {
    if (connected || lost || busy || !maint.loaded) return;
    const inst = maint.installation;
    const rev = settingsRev;
    if (!profileLoadNeeded(mqLoaded.current, inst, rev)) return;
    const gen = ++mqGen.current;
    // the form no longer holds what was read before: until this read is accepted nothing counts as loaded (a switch
    // back to an installation read earlier reads it again and makes this read's answer stale)
    mqLoaded.current = null;
    // another installation: its broker only, nothing of the one before (a reload of the same one keeps the draft
    // until the stored values are back)
    const switched = mqFor.current !== inst;
    mqFor.current = inst;
    setMqttAsk(null);
    setMqLoading(true);
    if (switched) { setMq(MQTT_EMPTY); setMqStored(null); }
    void api.loadMqttSettings(inst ?? null).then((s) => {
      if (gen !== mqGen.current) return;
      mqLoaded.current = { inst, rev };
      setMq(s ?? MQTT_EMPTY); setMqStored(s);
      if (inst && switched) setSrc(initialWay(lastWay(inst), !!s, !!(cur && (storedPort(cur, "can") || storedPort(cur, "pack")))));
    }, () => { if (gen === mqGen.current) { setMq(MQTT_EMPTY); setMqStored(null); } })
      .finally(() => { if (gen === mqGen.current) setMqLoading(false); });
  }, [maint.installation, maint.loaded, settingsRev, connected, lost, busy]); // eslint-disable-line react-hooks/exhaustive-deps
  // the installation's stored port of the chosen bus is shown as a fixed line; the free fields only without one, or
  // for a one-off other port („Anderer Anschluss“, not stored unless the offer after connecting is accepted)
  const [other, setOther] = useState(false);
  const stored = cur ? storedPort(cur, bus) : null;
  const useStored = !!stored && !other && stored.kind === kind && stored.target === target;
  const changeBus = (b: Bus) => {
    setBus(b);
    apply(fillFields(cur, b, maint.installation ? lastConn(maint.installation) : null, "bus"));
  };

  // While lost, reconnect automatically with the last settings (not while the firmware view is open: it needs the port),
  // at most RECONNECT_TRIES times. The timer runs also with the window hidden (`backgroundThrottling` in tauri.conf.json).
  const gaveUp = lost && tries >= RECONNECT_TRIES;
  useEffect(() => {
    // never for another installation than the connection's
    if (lost && !gaveUp && !busy && !connected && !locked && view !== "firmware" && maint.installation === connInst.current && now - lastTry >= reconnectDelay(tries)) {
      setTries(tries + 1);
      connect(true);
    }
  }, [now]); // eslint-disable-line react-hooks/exhaustive-deps


  // the connection's fields and what of them is shown (`locks.ts`)
  const facts: UiFacts = { source: getSource(), chosen: maint.installation, way: src, linkPhase: busy ? "connecting" : connected ? "open" : lost ? "lost" : "idle", portHeld: locked };
  const fixed = decideUi("connection.editFields", facts).locked;
  const picker = decideUi("connection.packsPicker", facts);
  const packsText = decideUi("connection.packsText", facts);
  const line = connected ? bankLine(ids) : null;
  const noSerial = connected ? noSerialPacks(ids) : [];
  const instName = maint.current?.name ?? "";
  // „Übernehmen“: the listed packs belong to this installation; they count as known from now on
  const confirm = async (addresses: number[]) => {
    const inst = connInst.current;
    if (!inst || !addresses.length) return;
    setConfirming(true); setBankError(null);
    // the answer counts only for the connection it was asked on: after a new one the boards at these addresses may differ
    const link = linkNow();
    try {
      const done = await api.bankConfirm(inst, addresses);
      if (linkNow() === link) setIds((all) => all.map((v) => (done.includes(v.address) ? { ...v, kind: "known" } : v)));
    } catch (e) { setBankError(t("bank.confirmFailed", { err: String(e) })); }
    finally { setConfirming(false); }
  };
  const askKey = (a: SaveAsk) => `${a.inst}|${a.bus}|${portKey(a.kind, a.target)}`;
  const savePort = async (a: SaveAsk) => {
    const inst = maint.installations.find((i) => i.id === a.inst);
    if (!inst) return;
    setSaveError(null);
    try {
      await api.maintSaveInstallation(withPort(inst, a.bus, a.kind, a.target));
      setSaveAsk(null); void maint.loadLists();
    } catch (e) {
      const k = errorKey(e);
      setSaveError(t(k.key, refusalParams(inst, maint.alerts.pending, k.params)));
    }
  };
  const saveMqtt = async (a: { inst: string; settings: MqttSettings }) => {
    setSaveError(null);
    try { await api.saveMqttSettings(a.inst, a.settings); setMqStored(a.settings); setMqttAsk(null); }
    catch (e) { setSaveError(String(e)); }
  };
  const declinePort = (a: SaveAsk) => { declined.add(askKey(a)); setSaveAsk(null); setSaveError(null); };

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
          ...(maintenance ? [["diag", Gauge], ["params", SlidersHorizontal], ["firmware", Cpu]] : []), ["about", Info]].map(([id, Icon]) => {
          const I = Icon as typeof LayoutDashboard;
          return (
            <button key={id as string} onClick={() => setView(id as string)} {...lockProps(decideUi("app.navigate", facts), t)}
              className={cn("flex w-full items-center gap-2 rounded-md px-3 py-2 disabled:opacity-50", view === id ? "bg-sunken font-medium text-ink" : "text-muted hover:bg-sunken/60 hover:text-ink")}>
              <I className="h-4 w-4" />{t(`nav.${id}`)}
            </button>
          );
        })}
        {update.phase === "available" && update.info && view !== "about" && (
          <button onClick={() => setView("about")} {...lockProps(decideUi("app.navigate", facts), t)} className="mt-2 flex w-full items-center gap-2 rounded-md border border-charge/50 px-3 py-2 text-sm text-charge hover:bg-sunken/60 disabled:opacity-50">
            <Download className="h-4 w-4" />{t("about.update.sidebar", { version: update.info.version })}
          </button>
        )}
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
          {(["bus", "mqtt"] as const).map((k) => (
            <button key={k} onClick={() => setSrc(k)} disabled={fixed}
              className={cn("rounded py-1.5 text-sm disabled:opacity-60", src === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink disabled:hover:text-muted")}>
              {t(`conn.source.${k}`)}
            </button>
          ))}
        </div>
        {src === "mqtt" ? (
          <div className="space-y-1.5">
            <p className="flex items-center gap-1.5 text-xs text-muted">{t("conn.mqtt.hint")}<InfoIcon id="topic.mqtt" /></p>
            <div className="flex gap-1">
              <input className={input} value={mq.host} {...lockProps(decideUi("connection.brokerFields", { ...facts, loaded: !mqLoading }), t)} onChange={(e) => setMq(editBroker(mq, { host: e.target.value.trim() }))} placeholder="homeassistant.local" aria-label={t("conn.mqtt.host")} />
              <input className={cn(input, "w-20")} value={mq.port} {...lockProps(decideUi("connection.brokerFields", { ...facts, loaded: !mqLoading }), t)} inputMode="numeric" aria-label={t("conn.mqtt.port")}
                onChange={(e) => setMq(editBroker(mq, { port: Math.min(65535, Number(e.target.value.replace(/\D/g, "")) || 0) }))} />
            </div>
            <input className={input} value={mq.user} {...lockProps(decideUi("connection.brokerFields", { ...facts, loaded: !mqLoading }), t)} onChange={(e) => setMq(editBroker(mq, { user: e.target.value }))} placeholder={t("conn.mqtt.user")} aria-label={t("conn.mqtt.user")} autoComplete="off" />
            <input className={input} type="password" value={mq.password} {...lockProps(decideUi("connection.brokerFields", { ...facts, loaded: !mqLoading }), t)} onChange={(e) => setMq(editBroker(mq, { password: e.target.value }))} placeholder={t("conn.mqtt.password")} aria-label={t("conn.mqtt.password")} autoComplete="off" />
            <div className="flex items-center gap-2">
              <input className={input} value={mq.base} {...lockProps(decideUi("connection.brokerFields", { ...facts, loaded: !mqLoading }), t)} onChange={(e) => setMq(editBroker(mq, { base: e.target.value.trim() }))} aria-label={t("conn.mqtt.base")} title={t("conn.mqtt.base")} />
              <label className="flex shrink-0 items-center gap-1 text-sm text-muted">
                <input type="checkbox" checked={mq.tls} {...lockProps(decideUi("connection.brokerFields", { ...facts, loaded: !mqLoading }), t)} onChange={(e) => setMq(editBroker(mq, { tls: e.target.checked }))} />TLS
              </label>
            </div>
          </div>
        ) : (
          <>
        <label className="block">
          <span className="mb-1 flex items-center gap-1.5 text-sm text-muted">{t("conn.bus")}<InfoIcon id="topic.buses" /></span>
          <select className={input} value={bus} disabled={fixed} onChange={(e) => changeBus(e.target.value as Bus)}>
            <option value="can">{t("conn.busCan")}</option>
            <option value="pack">{t("conn.busPack")}</option>
          </select>
        </label>
        {maint.installation && <WiringHint scope={maint.installation} onEdit={() => setView("installations")} />}
        {useStored ? (
          <div className="space-y-1">
            <div className="flex items-center gap-2 rounded-md bg-sunken px-3 py-2 text-sm" title={t("conn.storedHint")}>
              {kind === "tcp" ? <Network className="h-3.5 w-3.5 shrink-0 text-muted" /> : <Usb className="h-3.5 w-3.5 shrink-0 text-muted" />}
              <span className="truncate">{target}</span>
            </div>
            {!fixed && (
              <p className="flex gap-3 text-xs text-muted">
                <button className="underline hover:text-ink" onClick={() => setOther(true)}>{t("conn.otherPort")}</button>
                <button className="underline hover:text-ink" onClick={() => setView("installations")}>{t("conn.editPorts")}</button>
              </p>
            )}
          </div>
        ) : (
          <>
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
              {target && !ports.includes(target) && <option value={target}>{t("maint.setup.notPlugged", { port: target })}</option>}
              {ports.map((p) => <option key={p}>{p}</option>)}
            </select>
            <Button variant="ghost" size="icon" className="shrink-0" disabled={fixed} onClick={loadPorts} aria-label={t("fw.refreshPorts")} title={t("fw.refreshPorts")}><RefreshCw className="h-4 w-4" /></Button>
          </div>
        )}
            {stored && !fixed && (
              <p className="text-xs text-muted">
                <button className="underline hover:text-ink" onClick={() => apply(fillFields(cur, bus, null, "bus"))}>{t("conn.useStored")}</button>
              </p>
            )}
          </>
        )}
        {picker.visible && (
        <label className="flex items-center justify-between gap-3 text-sm text-muted">
          {t("conn.packs")}
          <span className="flex gap-1">
            <select className={cn(input, "w-28")} value={viewSel.mode} {...lockProps(picker, t)} aria-label={t("conn.packsMode")}
              onChange={(e) => setViewSel(e.target.value === "count" ? { mode: "count", n: 1 } : e.target.value === "address" ? { mode: "address", k: 0 } : { mode: "search" })}>
              <option value="search">{t("conn.packsAuto")}</option>
              <option value="count">{t("conn.packsCount")}</option>
              <option value="address">{t("conn.packsAddress")}</option>
            </select>
            {viewSel.mode === "count" && (
              <select className={cn(input, "w-16")} value={viewSel.n} {...lockProps(picker, t)} aria-label={t("conn.packsCount")} onChange={(e) => setViewSel({ mode: "count", n: Number(e.target.value) })}>
                {Array.from({ length: DIP_MAX + 1 }, (_, i) => i + 1).map((n) => <option key={n} value={n}>{n}</option>)}
              </select>
            )}
            {viewSel.mode === "address" && (
              <select className={cn(input, "w-16")} value={viewSel.k} {...lockProps(picker, t)} aria-label={t("conn.packsAddress")} onChange={(e) => setViewSel({ mode: "address", k: Number(e.target.value) })}>
                {Array.from({ length: DIP_MAX + 1 }, (_, i) => i).map((k) => <option key={k} value={k}>{padAddress(k)}</option>)}
              </select>
            )}
          </span>
        </label>
        )}
        {packsText.visible && (
          <div className="flex items-baseline justify-between gap-3 text-sm">
            <span className="text-muted">{t("conn.packs")}</span>
            <span className="text-right">
              {packsLoading ? t("lock.loading") : fixed && runSel ? packsLine(runSel, t) : packsLine(packSel, t)}
              {fixed && runSel && !packsLoading && selText(runSel) !== selText(packSel) && (
                <span className="block text-xs text-muted">{t("conn.packsLine.next", { line: packsLine(packSel, t) })}</span>
              )}
              {!fixed && <button className="ml-2 text-xs text-muted underline hover:text-ink" onClick={() => setView("installations")}>{t("conn.editPorts")}</button>}
            </span>
          </div>
        )}
          </>
        )}
        <div className="flex gap-2">
          <Button variant="primary" className="flex-1" {...lockProps(decideUi("connection.connect", { ...facts,
            ready: src === "mqtt" ? !!mq.host && !!mq.base && !!mq.port : !!target, loaded: src === "mqtt" ? !mqLoading : !packsLoading }), t)} onClick={() => connect()}>
            {busy ? t("conn.checking") : connected ? t("conn.reconnect") : t("conn.connect")}
          </Button>
          {decideUi("connection.cancel", { ...facts, waiting }).visible && <Button variant="ghost" onClick={waiting ? stopWaiting : cancelConnect}>{t("conn.cancel")}</Button>}
          {decideUi("connection.disconnect", facts).visible && <Button variant="ghost" {...lockProps(decideUi("connection.disconnect", facts), t)} onClick={disconnect}>{t("conn.disconnect")}</Button>}
        </div>
        {/* fixed room for three lines, so the block does not jump when a status or error appears; long texts are clamped, the full text is the tooltip */}
        <div className="min-h-[3.75rem] space-y-0.5">
          {shown && <p title={shown} className={cn("break-words text-sm", failed ? "line-clamp-5" : "line-clamp-2", lost || failed ? "text-alarm" : "text-muted")}>{shown}</p>}
          {connected && src === "mqtt" && feedText(feed, t) && <p className="text-sm text-alarm">{feedText(feed, t)}</p>}
          {connected && getSource() !== "mqtt" && <FirmwareProfileLine locked={locked} />}
          {mqttAsk && connected && (
            <div className="space-y-1 pt-1">
              <p className="text-sm">{t("conn.mqtt.saveAsk", { name: mqttAsk.name })}</p>
              <div className="flex gap-1">
                <Button variant="secondary" size="sm" {...lockProps(decideUi("connection.saveOffer", facts), t)} onClick={() => void saveMqtt(mqttAsk)}>{t("conn.saveYes")}</Button>
                <Button variant="ghost" size="sm" onClick={() => setMqttAsk(null)}>{t("conn.saveNo")}</Button>
              </div>
            </div>
          )}
          {saveAsk && connected && (
            <div className="space-y-1 pt-1">
              <p className="text-sm">{t(`conn.saveAsk.${saveAsk.bus}`, { name: saveAsk.name })}</p>
              <div className="flex gap-1">
                <Button variant="secondary" size="sm" {...lockProps(decideUi("connection.saveOffer", facts), t)} onClick={() => void savePort(saveAsk)}>{t("conn.saveYes")}</Button>
                <Button variant="ghost" size="sm" onClick={() => declinePort(saveAsk)}>{t("conn.saveNo")}</Button>
              </div>
              {saveError && <p className="text-sm text-alarm">{saveError}</p>}
            </div>
          )}
          {offer && !busy && !connected && (
            <Button variant="secondary" size="sm" className="mt-1" {...lockProps(decideUi("connection.saveOffer", facts), t)} onClick={() => { setBus(offer); void connect(false, offer); }}>
              {t("conn.check.switch", { bus: t(`conn.busName.${offer}`) })}
            </Button>
          )}
          {line?.kind === "foreign" && (
            <div className="space-y-1 pt-1">
              <p className="text-sm text-alarm">{t("bank.foreign", { count: line.packs.length, packs: packList(line.packs), other: line.other.name })}</p>
              <div className="flex flex-wrap gap-1">
                <Button variant="secondary" size="sm" {...lockProps(decideUi("bank.leave", { ...facts, busy: confirming }), t)} onClick={() => void disconnect()}>{t("bank.disconnect")}</Button>
                <Button variant="secondary" size="sm" {...lockProps(decideUi("bank.leave", { ...facts, busy: confirming }), t)} onClick={() => void connectWith(line.other.id)}>{t("bank.connectOther", { other: line.other.name })}</Button>
              </div>
              {!line.hint && line.confirm.length > 0 && (
                <button className="text-xs text-muted underline hover:text-ink disabled:opacity-60" {...lockProps(decideUi("bank.adopt", { ...facts, busy: confirming }), t)} onClick={() => void confirm(line.confirm)}>
                  {t("bank.adoptAnyway", { name: instName })}
                </button>
              )}
            </div>
          )}
          {line?.kind === "new" && (
            <div className="space-y-1 pt-1">
              <p className="text-sm text-discharge">
                {t("bank.new", { count: line.packs.length, packs: packList(line.packs), name: instName })}
              </p>
              <div className="flex flex-wrap gap-1">
                <Button variant="secondary" size="sm" {...lockProps(decideUi("bank.adopt", { ...facts, busy: confirming }), t)} onClick={() => void confirm(line.confirm)}>{t("bank.adopt")}</Button>
                {/* not this installation: nothing is adopted, the connection ends (another installation is chosen above) */}
                <Button variant="ghost" size="sm" {...lockProps(decideUi("bank.leave", { ...facts, busy: confirming }), t)} onClick={() => void disconnect()}>{t("bank.notOurs")}</Button>
              </div>
            </div>
          )}
          {noSerial.length > 0 && (
            <p className="pt-1 text-xs text-muted">
              {t("bank.noSerial", { count: noSerial.length, packs: packList(noSerial) })}{" "}
              <button className="underline hover:text-ink disabled:opacity-60" {...lockProps(decideUi("app.navigate", facts), t)} onClick={() => { setMaintenance(true); setView("params"); }}>{t("bank.openParams")}</button>
            </p>
          )}
          {bankError && <p className="text-sm text-alarm">{bankError}</p>}
          {/* from the backend's decision (it also learns from writes' checks and parameter reads) */}
          {connected && recStatus?.held && <p className="text-xs text-muted">{recStatus.held_reason === "confirm" ? t("bank.afterConfirm") : t("rec.held")}</p>}
          {lost && !busy && <p className="text-sm text-muted">{gaveUp ? t("conn.gaveUp", { n: tries }) : t("conn.retryIn", { time: waitText(reconnectDelay(tries) - (now - lastTry)), n: tries + 1, of: RECONNECT_TRIES })}</p>}
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
        <RecordingEntry scope={maint.installation} packs={packs} systemAvailable={bus === "can"} connected={connected} onOpenHistory={() => setView("history")} onOpenStorage={() => setView("about")} />
        {/* maintenance functions: an icon toggle, highlighted while on; the label is the tooltip */}
        <Button variant="ghost" size="icon" onClick={() => setMaintenance(!maintenance)} {...lockProps(decideUi("app.navigate", facts), t)}
          aria-pressed={maintenance} aria-label={t("maintToggle")} title={t("maintToggle")}
          className={cn(maintenance && "bg-sunken text-ink")}>
          <Wrench className="h-4 w-4" />
        </Button>
      </div>
    </aside>
  );
}

/** The firmware profile of the connection (CAN socket): the master's version only (release bits and hard stop are per
 *  pack: parameters and pack view); „Erneut prüfen“. */
function FirmwareProfileLine({ locked }: { locked: boolean }) {
  const { t } = useTranslation();
  const p = useProfile();
  const [checking, setChecking] = useState(false);
  // a write to the master updates the release bits (or marks them old) in the backend
  useEffect(() => { const u = api.onFirmwareProfile(setProfile); return () => { u.then((f) => f()); }; }, []);
  const line = profileLine(p, t);
  if (!p || !line) return null;
  const recheck = async () => {
    setChecking(true);
    try { await api.firmwareProfileCheck(p.gen); } catch { /* the line stays as it was */ } finally { setChecking(false); }
  };
  return (
    <div className="text-sm">
      <p className={cn("flex items-center gap-1", line.tone === "ok" ? "text-ok" : line.tone === "warn" ? "text-discharge" : "text-muted")}>
        <span className="min-w-0 truncate" title={line.title ?? line.text}>{line.text}</span><InfoIcon id="topic.firmwareProfile" />
      </p>
      {recheckable(p) && (
        <button className="text-xs text-ink underline disabled:opacity-40" {...lockProps(decideUi("profile.recheck", { source: null, chosen: null, portHeld: locked, busy: checking }), t)} onClick={() => void recheck()}>{t("profile.recheck")}</button>
      )}
    </div>
  );
}

/** The installation's inverter connection is set under „Anlagen“ (`wiring.ts`): here only a note while it is missing, and
 *  the session's „Wechselrichter pausiert“ where the inverter polls one of the RS485 buses. */
function WiringHint({ scope, onEdit }: { scope: string; onEdit: () => void }) {
  const { t } = useTranslation();
  const w = useWiring(scope);
  if (!w.loaded) return null;
  if (!w.inverter) {
    return (
      <p className="text-xs text-discharge">
        {t("wiring.unset")} <button className="underline hover:text-ink" onClick={onEdit}>{t("wiring.setUnder")}</button>
      </p>
    );
  }
  if (!pollsBus(w.inverter)) return null;
  return (
    <label className="flex items-start gap-2 text-xs text-muted" title={t("wiring.pausedHint")}>
      <input type="checkbox" className="mt-0.5" checked={w.paused} onChange={(e) => w.setPaused(e.target.checked)} />
      <span>{t("wiring.paused")}</span>
    </label>
  );
}
