import { useEffect, useState } from "react";
import { api, type DeviceInfo, type PackUpdate, type SystemUpdate } from "./api";

/** updated: time of the last successful answer; interval: smoothed time between two answers (ms). */
export interface PackEntry extends PackUpdate { device?: DeviceInfo; updated: number; interval?: number }

function smoothInterval(prev: PackEntry | undefined, ok: boolean): number | undefined {
  if (!ok || !prev?.updated) return prev?.interval;
  const dt = Date.now() - prev.updated;
  return prev.interval ? prev.interval * 0.7 + dt * 0.3 : dt;
}

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
          // only a successful answer counts as fresh data; failed polls keep the old timestamp
          updated: u.telemetry ? Date.now() : prev[u.address]?.updated ?? 0,
          // a full polling round takes longer with more packs and slower buses, so learn it per pack
          interval: smoothInterval(prev[u.address], !!u.telemetry),
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
