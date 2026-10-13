import { useCallback, useEffect, useRef, useState } from "react";
import { api, isDemo, type Endpoint, type MaintOverview } from "./api";
import { attachDecision, endpointKey, latestGuard } from "./maint";

/** The installation in use and its maintenance overview. The banner reads from here, so it survives view
 *  changes, reconnects and (through the backend database) app restarts. */
export function useMaint() {
  const [installation, setInstallation] = useState<string | null>(() => localStorage.getItem("maint.installation"));
  const [overview, setOverview] = useState<MaintOverview | null>(null);
  const [unassigned, setUnassigned] = useState<Endpoint | null>(null);
  const [other, setOther] = useState<{ id: string; name: string } | null>(null);

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

  /** Re-read 0x64 (start-up, every new connection on the CAN socket bus); errors leave the stored state. */
  const check = useCallback(async (id = installationRef.current) => {
    if (!id || isDemo) return;
    const ticket = guard.begin("check", id);
    let o: MaintOverview | null = null;
    try { o = await api.maintCheck(id); } catch { /* stored state stays */ }
    if (guard.accepts(ticket, installationRef.current, o, overviewRef.current) && o) store(o);
    guard.end(ticket);
    if (!o) await refresh(id);
  }, [guard, refresh]);

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
    if (d.do === "use") await use(d.id, endpoint.bus === "can" && !!known?.control);
    if (d.do === "create") {
      const created = await api.maintInstallationFor(endpoint).catch(() => null);
      if (created) await use(created.id, false);
    }
  }, [refresh, use]);

  /** Explicit switch from the banner. */
  const switchTo = useCallback(async (id: string) => {
    setOther(null);
    await use(id, false);
  }, [use]);

  /** Add an endpoint to the installation in use (explicit, from the banner or the firmware view). */
  const assign = useCallback(async (endpoint: Endpoint) => {
    const current = overviewRef.current;
    if (!current) return;
    const inst = current.installation;
    const key = endpointKey(endpoint);
    if (!inst.endpoints.includes(key)) await api.maintSaveInstallation({ ...inst, endpoints: [...inst.endpoints, key] });
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

  return { installation, overview, setOverview: store, unassigned, other, attach, assign, switchTo, refresh, check };
}
