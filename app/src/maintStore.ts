import { useCallback, useEffect, useRef, useState } from "react";
import { api, isDemo, setChosenInstallation, type Alerts, type Installation } from "./api";
import { migrateLegacy } from "./installData";
import { errorKey, latestOnly, nameCheck } from "./maint";

const NO_ALERTS: Alerts = { markers: [], pending: [] };

/** The chosen installation (null: „Nur ansehen“), kept across starts; the list of installations; and what stays
 *  visible until it is resolved: a set marker and packs that may hang in their bootloader. The banner reads from here,
 *  so it survives view changes, reconnects and (through the backend database) app restarts. */
export function useMaint() {
  const [installation, setInstallation] = useState<string | null>(() => (isDemo ? null : localStorage.getItem("maint.installation")));
  const [installations, setInstallations] = useState<Installation[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [alerts, setAlerts] = useState<Alerts>(NO_ALERTS);
  const [alertsFailed, setAlertsFailed] = useState(false);
  // „Neue Anlage …“: the name dialog before anything is created
  const [naming, setNaming] = useState(false);
  const installationRef = useRef(installation);
  const installationsRef = useRef<Installation[]>([]);
  installationsRef.current = installations;
  const lists = useRef(latestOnly()).current;
  setChosenInstallation(installation);
  /** Set by the sidebar: ends a connection (or a lost one waiting to reconnect), since the choice and the connection
   *  always belong together. Every change of the choice goes through it. */
  const beforeSwitch = useRef<(() => Promise<void>) | null>(null);

  const switchTo = useCallback(async (id: string | null) => {
    if (id === installationRef.current) return;
    await beforeSwitch.current?.();
    if (id) localStorage.setItem("maint.installation", id); else localStorage.removeItem("maint.installation");
    installationRef.current = id;
    setChosenInstallation(id);
    setInstallation(id);
  }, []);

  /** Reload the installations and the alerts; a failed load keeps the last ones and says so. A chosen installation
   *  that is gone (deleted, merged) falls back to „Nur ansehen“. */
  const loadLists = useCallback(async () => {
    if (isDemo) return;
    const ticket = lists.begin();
    const [list, a] = await Promise.all([api.maintInstallations().catch(() => null), api.maintAlerts().catch(() => null)]);
    if (!lists.accepts(ticket)) return;
    if (list) {
      setInstallations(list); setLoaded(true);
      if (installationRef.current && !list.some((i) => i.id === installationRef.current)) void switchTo(null);
    }
    if (a) setAlerts(a);
    setAlertsFailed(!a);
  }, [lists, switchTo]);

  /** The name dialog's confirm: creates the installation (its endpoints are linked when connecting) and chooses it.
   *  Returns the text key of a refusal (the dialog stays open), null on success. */
  const create = useCallback(async (name: string): Promise<{ key: string; params?: Record<string, unknown> } | null> => {
    const bad = nameCheck(name, installationsRef.current);
    if (bad) return { key: bad };
    try {
      const created = await api.maintCreateInstallation(name.trim(), null, []);
      setNaming(false);
      await switchTo(created.id);
      await loadLists();
      return null;
    } catch (e) { return errorKey(e); }
  }, [switchTo, loadLists]);

  // the old data first (once), then the lists; the alerts of every installation also every 30 s
  useEffect(() => {
    migrateLegacy().catch((e) => console.error("moving the old data:", e)).finally(() => void loadLists());
    const timer = setInterval(() => { void loadLists(); }, 30_000);
    return () => clearInterval(timer);
  }, [loadLists]);

  const current = installations.find((i) => i.id === installation) ?? null;
  return { installation, current, installations, loaded, alerts, alertsFailed, loadLists, switchTo, beforeSwitch, naming, setNaming, create };
}

export type Maint = ReturnType<typeof useMaint>;
