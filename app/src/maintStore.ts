import { useCallback, useEffect, useRef, useState } from "react";
import { api, isDemo, type Endpoint, type Installation, type MaintOverview, type OpenWork } from "./api";
import i18n from "./i18n";
import { assignEndpoint, attachDecision, controlFromAttach, createFromAttach, defaultName, errorKey, latestGuard, nameCheck } from "./maint";

/** The setup's one-adapter confirmation ("one adapter for both buses, I move the cable"), per installation. The
 *  backend stores only the keys; without this the control adapter is never taken as the RS485-1/2 port
 *  (`packPortOf`), so a sidebar connection on the CAN socket adapter cannot pass for a one-adapter setup. */
export function oneAdapterOf(id: string | null | undefined): boolean {
  return !!id && localStorage.getItem(`maint.oneAdapter.${id}`) === "1";
}
export function setOneAdapter(id: string, on: boolean) {
  if (on) localStorage.setItem(`maint.oneAdapter.${id}`, "1"); else localStorage.removeItem(`maint.oneAdapter.${id}`);
}

/** The installation in use and its maintenance overview. The banner reads from here, so it survives view
 *  changes, reconnects and (through the backend database) app restarts. */
export function useMaint() {
  const [installation, setInstallation] = useState<string | null>(() => localStorage.getItem("maint.installation"));
  const [overview, setOverview] = useState<MaintOverview | null>(null);
  const [unassigned, setUnassigned] = useState<Endpoint | null>(null);
  const [other, setOther] = useState<{ id: string; name: string } | null>(null);
  // the fresh 0x64 check of this app run, per installation; cleared when the monitor connection is lost or closed
  const [fresh, setFresh] = useState<{ id: string; result: "ok" | "failed" } | null>(null);
  // single-pack slave scans of this run, per installation and pack port (memory only)
  const [scans, setScans] = useState<Record<string, number | null>>({});
  // all installations (picker) and those with open work (banner); a failed load keeps the last list and says so
  const [installations, setInstallations] = useState<Installation[]>([]);
  const [work, setWork] = useState<OpenWork[]>([]);
  const [workFailed, setWorkFailed] = useState(false);
  // a switch, create or delete is loading: the picker and the panel actions wait
  const [switching, setSwitching] = useState(false);
  // a sidebar connection that belongs to no installation: the name dialog before anything is created
  const [naming, setNaming] = useState<{ endpoint: Endpoint; suggested: string } | null>(null);

  // refs: callbacks fired from a click or a connect must see the current values, not those of an older render
  const installationRef = useRef(installation);
  const overviewRef = useRef(overview);
  const guard = useRef(latestGuard()).current;
  const installationsRef = useRef<Installation[]>([]);
  installationsRef.current = installations;

  const store = (o: MaintOverview | null) => { overviewRef.current = o; setOverview(o); };

  /** Reload the installation list and the open work of every installation (banner). */
  const loadLists = useCallback(async () => {
    if (isDemo) return;
    const [list, open] = await Promise.all([api.maintInstallations().catch(() => null), api.maintOpenWork().catch(() => null)]);
    if (list) setInstallations(list);
    if (open) setWork(open);
    setWorkFailed(!open);
  }, []);

  /** Load the stored overview; returns what was loaded (also when a newer request made this answer stale). The
   *  installation list and the open work are reloaded with it (after every panel action, flash end, switch). */
  const refresh = useCallback(async (id = installationRef.current): Promise<MaintOverview | null> => {
    if (isDemo) return null;
    void loadLists();
    if (!id) return null;
    const ticket = guard.begin("refresh", id);
    const o = await api.maintOverview(id).catch(() => null);
    if (guard.accepts(ticket, installationRef.current, o, overviewRef.current)) store(o);
    return o;
  }, [guard, loadLists]);

  /** Re-read 0x64 (start-up, every new connection on the CAN socket bus, the panel's "check master"); errors
   *  leave the stored state. Any answer counts as this run's fresh check (an unreachable master is stored as
   *  unknown); a refusal (no control channel, control adapter on the pack bus) counts as a failed one. With
   *  `rethrow` the caller gets the error to show it. */
  const check = useCallback(async (id = installationRef.current, rethrow = false): Promise<MaintOverview | null> => {
    if (!id || isDemo) return null;
    const ticket = guard.begin("check", id);
    let o: MaintOverview | null = null;
    let error: unknown = null;
    try {
      try { o = await api.maintCheck(id); } catch (e) { error = e; }
      if (guard.accepts(ticket, installationRef.current, o, overviewRef.current) && o) store(o);
      // a busy port is no answer at all: the previous result stays
      if (o || errorKey(error).key !== "maint.err.busy") setScans({});
      if (o) setFresh({ id, result: "ok" });
      else if (errorKey(error).key !== "maint.err.busy") setFresh({ id, result: "failed" });
    } finally { guard.end(ticket); }
    if (!o) await refresh(id); else void loadLists();
    if (error && rethrow) throw error;
    return o;
  }, [guard, refresh, loadLists]);

  /** The monitor connection was lost or closed: the next work phase needs a fresh check (and a new slave search).
   *  Every new master check clears the slave searches too. */
  const invalidateCheck = useCallback(() => { setFresh(null); setScans({}); }, []);
  const freshFor = (id: string | null | undefined) => (id && fresh?.id === id ? fresh.result : null);
  const scanOf = (id: string | null | undefined, port: string) => (id && port ? scans[`${id}|${port}`] : undefined);
  const setScan = useCallback((id: string, port: string, slave: number | null) => setScans((m) => ({ ...m, [`${id}|${port}`]: slave })), []);

  /** Show an installation. Another one than shown drops the in-memory phase data of this run (fresh check, slave
   *  scans); the panel's own (inverter, measurement, End progress) goes with its remount. Open work stays in the banner. */
  const use = useCallback(async (id: string, recheck: boolean) => {
    if (installationRef.current !== id) { setFresh(null); setScans({}); }
    localStorage.setItem("maint.installation", id);
    installationRef.current = id;
    overviewRef.current = overviewRef.current?.installation.id === id ? overviewRef.current : null;
    setInstallation(id);
    setSwitching(true);
    try { if (recheck) await check(id); else await refresh(id); } finally { setSwitching(false); }
  }, [check, refresh]);

  /** A deleted installation: its selection, overview, fresh check, scans and one-adapter mark go; nothing is shown
   *  until the user picks another one. */
  const forget = useCallback(async (id: string) => {
    setOneAdapter(id, false);
    if (installationRef.current === id) {
      localStorage.removeItem("maint.installation");
      installationRef.current = null;
      store(null);
      setInstallation(null);
      setFresh(null); setScans({}); setUnassigned(null); setOther(null);
    }
    await loadLists();
  }, [loadLists]);

  /** A new connection (`attachDecision`): never silently replaces an installation with open work (its
   *  session and blocks would vanish from view); the banner offers to assign or to switch instead. */
  const attach = useCallback(async (endpoint: Endpoint) => {
    if (isDemo) return;
    const known = await api.maintInstallationOf(endpoint).catch(() => null);
    const id = installationRef.current;
    // the start-up refresh may not be back yet: load first; if that fails, assume open work
    let current = overviewRef.current;
    if (id && !current) current = await refresh(id);
    const d = attachDecision(id ? current : null, known, id && !current ? id : null);
    setUnassigned(d.do === "offer-assign" ? endpoint : null);
    setOther(d.do === "offer-switch" ? { id: d.id, name: d.name } : null);
    // a connection on the CAN socket bus becomes the CAN socket of an installation without one, never a listed
    // endpoint (that could pass for the RS485-1/2 port); refused while the installation has open work
    const promote = async (inst: Installation) => {
      const next = controlFromAttach(inst, endpoint);
      if (!next) return !!inst.control;
      return api.maintSaveInstallation(next).then(() => true).catch(() => !!inst.control);
    };
    if (d.do === "use") {
      const hasControl = known ? await promote(known) : false;
      await use(d.id, endpoint.bus === "can" && hasControl);
    }
    // nothing is created before the user named it (the name dialog; cancel leaves the endpoint unassigned)
    if (d.do === "create") {
      const list = await api.maintInstallations().catch(() => installationsRef.current);
      setNaming({ endpoint, suggested: defaultName(list, i18n.t("maint.picker.base")) });
    }
  }, [refresh, use]);

  /** The name dialog's confirm: creates the installation with this endpoint (one backend transaction) and shows it.
   *  Returns the text key of a refusal (the dialog stays open), null on success. */
  const confirmNaming = useCallback(async (name: string): Promise<{ key: string; params?: Record<string, unknown> } | null> => {
    const n = naming;
    if (!n) return null;
    const bad = nameCheck(name, installationsRef.current);
    if (bad) return { key: bad };
    try {
      const { control, endpoints } = createFromAttach(n.endpoint);
      const created = await api.maintCreateInstallation(name.trim(), control, endpoints);
      setNaming(null);
      await use(created.id, false);
      return null;
    } catch (e) { return errorKey(e); }
  }, [naming, use]);
  const cancelNaming = useCallback(() => setNaming(null), []);

  /** Explicit switch from the banner. */
  const switchTo = useCallback(async (id: string) => {
    setOther(null);
    await use(id, false);
  }, [use]);

  /** Assign a connection to the installation in use (explicit, from the banner): on the CAN socket bus as its CAN
   *  socket, on the pack bus as an endpoint (`assignEndpoint`). */
  const assign = useCallback(async (endpoint: Endpoint) => {
    const current = overviewRef.current;
    if (!current) return;
    const inst = current.installation;
    const next = assignEndpoint(inst, endpoint);
    if (next !== inst) await api.maintSaveInstallation(next);
    setUnassigned(null);
    await refresh(inst.id);
  }, [refresh]);

  // the open work of every installation: also every 30 s (another installation's state is not reloaded otherwise)
  useEffect(() => {
    const timer = setInterval(() => { void loadLists(); }, 30_000);
    return () => clearInterval(timer);
  }, [loadLists]);

  // start-up: show the last confirmed state at once, then re-check if a control channel is set
  useEffect(() => {
    void loadLists();
    (async () => {
      const o = await refresh();
      if (o?.installation.control) await check();
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  /** installation and session in use right now (refs, not render values): for actions that outlive a render */
  const current = useCallback(() => ({ installation: installationRef.current, session: overviewRef.current?.session?.id ?? null }), []);

  return { installation, overview, current, setOverview: store, unassigned, other, attach, assign, switchTo, refresh, check, invalidateCheck, freshFor, scanOf, setScan,
    installations, work, workFailed, loadLists, switching, forget, naming, confirmNaming, cancelNaming };
}
