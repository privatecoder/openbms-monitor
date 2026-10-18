import { useEffect, useState } from "react";
import { api, type HardStopSeen } from "./api";
import { useProfile } from "./profile";

/** The hard-stop observations of the connection, by address: loaded per connection and followed from the poll thread. */
export function useHardStops(): Record<number, HardStopSeen> {
  const profile = useProfile();
  const [seen, setSeen] = useState<Record<number, HardStopSeen>>({});
  useEffect(() => {
    let alive = true;
    setSeen({});
    if (profile) api.hardStops().then((l) => { if (alive) setSeen(Object.fromEntries(l.map((h) => [h.address, h]))); }, () => {});
    const u = api.onHardStop((h) => setSeen((s) => ({ ...s, [h.address]: h })));
    return () => { alive = false; u.then((f) => f()); };
  }, [profile?.gen]); // eslint-disable-line react-hooks/exhaustive-deps
  return seen;
}
