import { useCallback, useEffect, useRef, useState } from "react";
import { api, isDemo, type Endpoint, type Installation, type MaintOverview } from "./api";
import { assignEndpoint, attachDecision, controlFromAttach, errorKey, latestGuard } from "./maint";

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

  // refs: callbacks fired from a click or a connect must see the current values, not those of an older render
  const installationRef = useRef(installation);
  const overviewRef = useRef(overview);
  const guard = useRef(latestGuard()).current;
  const store = (o: MaintOverview | null) => { overviewRef.current = o; setOverview(o); };

  /** Load the stored overview; returns what was loaded (also when a newer request made this answer stale). */
  const refresh = useCallback(async (id = installationRef.current): Promise<MaintOverview | null> => {
    if (!id || isDemo) return null;
    const ticket = guard.begin("refresh", id);
    const o = await api.maintOverview(id).catch(() => null);
    if (guard.accepts(ticket, installationRef.current, o, overviewRef.current)) store(o);
    return o;
  }, [guard]);

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
    if (!o) await refresh(id);
    if (error && rethrow) throw error;
    return o;
  }, [guard, refresh]);

  /** The monitor connection was lost or closed: the next work phase needs a fresh check (and a new slave search).
   *  Every new master check clears the slave searches too. */
  const invalidateCheck = useCallback(() => { setFresh(null); setScans({}); }, []);
  const freshFor = (id: string | null | undefined) => (id && fresh?.id === id ? fresh.result : null);
  const scanOf = (id: string | null | undefined, port: string) => (id && port ? scans[`${id}|${port}`] : undefined);
  const setScan = useCallback((id: string, port: string, slave: number | null) => setScans((m) => ({ ...m, [`${id}|${port}`]: slave })), []);

  const use = useCallback(async (id: string, recheck: boolean) => {
    localStorage.setItem("maint.installation", id);
    installationRef.current = id;
    overviewRef.current = overviewRef.current?.installation.id === id ? overviewRef.current : null;
    setInstallation(id);
    if (recheck) await check(id); else await refresh(id);
  }, [check, refresh]);

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
    if (d.do === "create") {
      const created = await api.maintInstallationFor(endpoint).catch(() => null);
      if (created) { await promote(created); await use(created.id, false); }
    }
  }, [refresh, use]);

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

  // start-up: show the last confirmed state at once, then re-check if a control channel is set
  useEffect(() => {
    (async () => {
      const o = await refresh();
      if (o?.installation.control) await check();
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  /** installation and session in use right now (refs, not render values): for actions that outlive a render */
  const current = useCallback(() => ({ installation: installationRef.current, session: overviewRef.current?.session?.id ?? null }), []);

  return { installation, overview, current, setOverview: store, unassigned, other, attach, assign, switchTo, refresh, check, invalidateCheck, freshFor, scanOf, setScan };
}
