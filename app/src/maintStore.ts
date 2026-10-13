import { useCallback, useEffect, useState } from "react";
import { api, isDemo, type Endpoint, type MaintOverview } from "./api";
import { attachDecision, endpointKey } from "./maint";

/** The installation in use and its maintenance overview. The banner reads from here, so it survives view
 *  changes, reconnects and (through the backend database) app restarts. */
export function useMaint() {
  const [installation, setInstallation] = useState<string | null>(() => localStorage.getItem("maint.installation"));
  const [overview, setOverview] = useState<MaintOverview | null>(null);
  const [unassigned, setUnassigned] = useState<Endpoint | null>(null);
  const [other, setOther] = useState<{ id: string; name: string } | null>(null);

  const refresh = useCallback(async (id = installation) => {
    if (!id || isDemo) return;
    setOverview(await api.maintOverview(id).catch(() => null));
  }, [installation]);

  /** Re-read 0x64 (start-up, every new connection on the CAN socket bus); errors leave the stored state. */
  const check = useCallback(async (id = installation) => {
    if (!id || isDemo) return;
    try { setOverview(await api.maintCheck(id)); } catch { await refresh(id); }
  }, [installation, refresh]);

  const use = useCallback(async (id: string, recheck: boolean) => {
    localStorage.setItem("maint.installation", id);
    setInstallation(id);
    if (recheck) await check(id); else await refresh(id);
  }, [check, refresh]);

  /** A new connection (`attachDecision`): never silently replaces an installation with open work (its
   *  session and blocks would vanish from view); the banner offers to assign or to switch instead. */
  const attach = useCallback(async (endpoint: Endpoint) => {
    if (isDemo) return;
    const known = await api.maintInstallationOf(endpoint).catch(() => null);
    const d = attachDecision(installation ? overview : null, known);
    setUnassigned(d.do === "offer-assign" ? endpoint : null);
    setOther(d.do === "offer-switch" ? { id: d.id, name: d.name } : null);
    if (d.do === "use") await use(d.id, endpoint.bus === "can" && !!known?.control);
    if (d.do === "create") {
      const created = await api.maintInstallationFor(endpoint).catch(() => null);
      if (created) await use(created.id, false);
    }
  }, [installation, overview, use]);

  /** Explicit switch from the banner. */
  const switchTo = useCallback(async (id: string) => {
    setOther(null);
    await use(id, false);
  }, [use]);

  /** Add an endpoint to the installation in use (explicit, from the banner or the firmware view). */
  const assign = useCallback(async (endpoint: Endpoint) => {
    if (!overview) return;
    const inst = overview.installation;
    const key = endpointKey(endpoint);
    if (!inst.endpoints.includes(key)) await api.maintSaveInstallation({ ...inst, endpoints: [...inst.endpoints, key] });
    setUnassigned(null);
    await refresh(inst.id);
  }, [overview, refresh]);

  // start-up: show the last confirmed state at once, then re-check if a control channel is set
  useEffect(() => {
    (async () => {
      await refresh();
      const o = installation && !isDemo ? await api.maintOverview(installation).catch(() => null) : null;
      if (o?.installation.control) await check();
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return { installation, overview, setOverview, unassigned, other, attach, assign, switchTo, refresh, check };
}
