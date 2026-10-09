import { useEffect, useState } from "react";
import { api, type DeviceInfo, type PackUpdate, type SystemUpdate } from "./api";

export interface PackEntry extends PackUpdate { device?: DeviceInfo; updated: number }

/** Live data of all packs and the system, fed by backend events. */
export function useLiveData() {
  const [packs, setPacks] = useState<Record<number, PackEntry>>({});
  const [system, setSystem] = useState<SystemUpdate | null>(null);

  useEffect(() => {
    const p = api.onPack((u) =>
      setPacks((prev) => ({
        ...prev,
        [u.address]: {
          ...prev[u.address],
          ...u,
          // keep the last good values if a single poll fails
          telemetry: u.telemetry ?? prev[u.address]?.telemetry ?? null,
          status: u.status ?? prev[u.address]?.status ?? null,
          updated: Date.now(),
        },
      })),
    );
    const s = api.onSystem((u) => setSystem(u));
    return () => { p.then((f) => f()); s.then((f) => f()); };
  }, []);

  const setDevices = (found: [number, DeviceInfo][]) =>
    setPacks(Object.fromEntries(found.map(([a, d]) => [a, { address: a, device: d, telemetry: null, status: null, error: null, updated: 0 }])));

  return { packs, system, setDevices, reset: () => { setPacks({}); setSystem(null); } };
}
