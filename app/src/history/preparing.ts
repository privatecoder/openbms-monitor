import { useEffect, useState } from "react";
import { api } from "../api";

/** The history database is being created or migrated after the start (pack writes wait for it). Asks until it is done. */
export function useHistoryPreparing(): boolean {
  const [preparing, setPreparing] = useState(false);
  useEffect(() => {
    let alive = true, timer = 0;
    const tick = () => api.historyStatus().then((s) => {
      if (!alive) return;
      setPreparing(!!s.preparing);
      if (s.preparing) timer = window.setTimeout(tick, 2000);
    }).catch(() => {});
    tick();
    return () => { alive = false; window.clearTimeout(timer); };
  }, []);
  return preparing;
}
