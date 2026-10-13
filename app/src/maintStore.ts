import { useCallback, useEffect, useRef, useState } from "react";
import { api, isDemo, type Alerts, type Endpoint, type Installation } from "./api";
import i18n from "./i18n";
import { assignEndpoint, attachDecision, controlFromAttach, createFromAttach, defaultName, errorKey, latestOnly, nameCheck } from "./maint";

const NO_ALERTS: Alerts = { markers: [], pending: [] };

/** The installation shown (an address book entry) and what stays visible until it is resolved: a set marker and
 *  packs that may hang in their bootloader. The banner reads from here, so it survives view changes, reconnects and
 *  (through the backend database) app restarts. */
export function useMaint() {
  const [installation, setInstallation] = useState<string | null>(() => localStorage.getItem("maint.installation"));
  const [installations, setInstallations] = useState<Installation[]>([]);
  const [alerts, setAlerts] = useState<Alerts>(NO_ALERTS);
  const [alertsFailed, setAlertsFailed] = useState(false);
  const [unassigned, setUnassigned] = useState<Endpoint | null>(null);
  // a sidebar connection that belongs to no installation: the name dialog before anything is created
  const [naming, setNaming] = useState<{ endpoint: Endpoint; suggested: string } | null>(null);
  const installationRef = useRef(installation);
  const installationsRef = useRef<Installation[]>([]);
  installationsRef.current = installations;
  const lists = useRef(latestOnly()).current;

  /** Reload the installations and the alerts; a failed load keeps the last ones and says so. */
  const loadLists = useCallback(async () => {
    if (isDemo) return;
    const ticket = lists.begin();
    const [list, a] = await Promise.all([api.maintInstallations().catch(() => null), api.maintAlerts().catch(() => null)]);
    if (!lists.accepts(ticket)) return;
    if (list) setInstallations(list);
    if (a) setAlerts(a);
    setAlertsFailed(!a);
  }, [lists]);

  const switchTo = useCallback(async (id: string | null) => {
    if (installationRef.current !== id) setUnassigned(null);
    if (id) localStorage.setItem("maint.installation", id); else localStorage.removeItem("maint.installation");
    installationRef.current = id;
    setInstallation(id);
    await loadLists();
  }, [loadLists]);

  /** A deleted installation: nothing is shown until the user picks another one. */
  const forget = useCallback(async (id: string) => {
    if (installationRef.current === id) await switchTo(null); else await loadLists();
  }, [switchTo, loadLists]);

  /** A new sidebar connection (`attachDecision`). A connection on the CAN socket bus becomes the CAN socket of an
   *  installation without one. */
  const attach = useCallback(async (endpoint: Endpoint) => {
    if (isDemo) return;
    const known = await api.maintInstallationsOf(endpoint).catch(() => [] as Installation[]);
    const d = attachDecision(installationRef.current, known);
    setUnassigned(d.do === "offer-assign" ? endpoint : null);
    const id = d.do === "keep" ? installationRef.current : d.do === "use" ? d.id : null;
    const inst = known.find((k) => k.id === id);
    const next = inst && controlFromAttach(inst, endpoint);
    if (next) await api.maintSaveInstallation(next).catch(() => null);
    if (d.do === "use") await switchTo(d.id); else void loadLists();
    // nothing is created before the user named it (the name dialog; cancel leaves the endpoint unassigned)
    if (d.do === "create") {
      const list = await api.maintInstallations().catch(() => installationsRef.current);
      setNaming({ endpoint, suggested: defaultName(list, i18n.t("maint.picker.base")) });
    }
  }, [switchTo, loadLists]);

  /** The name dialog's confirm: creates the installation with this endpoint and shows it. Returns the text key of a
   *  refusal (the dialog stays open), null on success. */
  const confirmNaming = useCallback(async (name: string): Promise<{ key: string; params?: Record<string, unknown> } | null> => {
    const n = naming;
    if (!n) return null;
    const bad = nameCheck(name, installationsRef.current);
    if (bad) return { key: bad };
    try {
      const { control, endpoints } = createFromAttach(n.endpoint);
      const created = await api.maintCreateInstallation(name.trim(), control, endpoints);
      setNaming(null);
      await switchTo(created.id);
      return null;
    } catch (e) { return errorKey(e); }
  }, [naming, switchTo]);
  const cancelNaming = useCallback(() => setNaming(null), []);

  /** Assign a connection to the installation shown (explicit, from the banner). */
  const assign = useCallback(async (endpoint: Endpoint) => {
    const inst = installationsRef.current.find((i) => i.id === installationRef.current);
    if (!inst) return;
    const next = assignEndpoint(inst, endpoint);
    if (next !== inst) await api.maintSaveInstallation(next);
    setUnassigned(null);
    await loadLists();
  }, [loadLists]);

  // the alerts of every installation: also every 30 s
  useEffect(() => {
    void loadLists();
    const timer = setInterval(() => { void loadLists(); }, 30_000);
    return () => clearInterval(timer);
  }, [loadLists]);

  const current = installations.find((i) => i.id === installation) ?? null;
  return { installation, current, installations, alerts, alertsFailed, loadLists, switchTo, forget, attach, assign, unassigned, naming, confirmNaming, cancelNaming };
}

export type Maint = ReturnType<typeof useMaint>;
