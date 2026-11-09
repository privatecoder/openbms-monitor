import { useEffect, useState } from "react";
import { api, type DeviceInfo, type PackUpdate, type SerialNumbers } from "./api";
import type { SystemEntry } from "./freshness";
import { addAnswer, resetLivePairs } from "./livePairs";
import { setFeed } from "./source";

/** updated / statusUpdated: time of the last successful telemetry / status answer; telemetryOk / statusOk: the last
 *  poll of each succeeded (the values kept after a failure are old, `freshness.ts`); interval: smoothed time between two
 *  answers (ms). serials: read once per connection; missing = not read (yet) or the read failed. */
export interface PackEntry extends PackUpdate {
  device?: DeviceInfo; serials?: SerialNumbers; updated: number; statusUpdated: number; telemetryOk: boolean; statusOk: boolean; interval?: number;
}

function smoothInterval(prev: PackEntry | undefined, ok: boolean): number | undefined {
  if (!ok || !prev?.updated) return prev?.interval;
  const dt = Date.now() - prev.updated;
  return prev.interval ? prev.interval * 0.7 + dt * 0.3 : dt;
}

/** Live data of all packs and the system, fed by backend events. */
export function useLiveData() {
  const [packs, setPacks] = useState<Record<number, PackEntry>>({});
  const [system, setSystem] = useState<SystemEntry | null>(null);

  useEffect(() => {
    const p = api.onPack((u) => {
      addAnswer(u);
      setPacks((prev) => ({
        ...prev,
        [u.address]: {
          ...prev[u.address],
          ...u,
          // keep the last good values if a poll fails; they count as old then (freshness.ts)
          telemetry: u.telemetry ?? prev[u.address]?.telemetry ?? null,
          status: u.status ?? prev[u.address]?.status ?? null,
          // only a successful answer counts as fresh data; failed polls keep the old timestamp
          updated: u.telemetry ? Date.now() : prev[u.address]?.updated ?? 0,
          statusUpdated: u.status ? Date.now() : prev[u.address]?.statusUpdated ?? 0,
          telemetryOk: !!u.telemetry,
          statusOk: !!u.status,
          // a full polling round takes longer with more packs and slower buses, so learn it per pack
          interval: smoothInterval(prev[u.address], !!u.telemetry),
        },
      }));
    });
    // like the packs: a failed poll keeps the last good values, marked old (not ok)
    const s = api.onSystem((u) => setSystem((prev) => ({ ...u, values: u.values ?? prev?.values ?? null, ok: !!u.values, updated: u.values ? Date.now() : prev?.updated ?? 0 })));
    // the add-on's feed: a restart of the add-on starts the live pair check over; its identity reads fill the packs
    const fd = api.onFeed((u) => {
      if (u.boot_changed) resetLivePairs();
      setFeed(u);
      setPacks((prev) => {
        const next = { ...prev };
        for (const m of u.packs) {
          const e = next[m.address];
          if (!e) continue;
          next[m.address] = {
            ...e,
            device: m.device ?? e.device,
            serials: m.battery_serial || m.bms_serial
              ? { battery: m.battery_serial ?? e.serials?.battery ?? { kind: "unavailable" }, bms: m.bms_serial ?? e.serials?.bms ?? { kind: "unavailable" } }
              : e.serials,
          };
        }
        return next;
      });
    });
    return () => { p.then((f) => f()); s.then((f) => f()); fd.then((f) => f()); };
  }, []);

  const setDevices = (found: [number, DeviceInfo | undefined][]) =>
    setPacks(Object.fromEntries(found.map(([a, d]) => [a, { address: a, device: d, telemetry: null, status: null, error: null, updated: 0, statusUpdated: 0, telemetryOk: false, statusOk: false }])));

  // device info read after polling has started (fixed pack count)
  const addDevices = (found: [number, DeviceInfo][]) =>
    setPacks((prev) => {
      const next = { ...prev };
      for (const [a, d] of found) if (next[a]) next[a] = { ...next[a], device: d };
      return next;
    });

  const setSerials = (address: number, serials: SerialNumbers) =>
    setPacks((prev) => (prev[address] ? { ...prev, [address]: { ...prev[address], serials } } : prev));

  return { packs, system, setDevices, addDevices, setSerials, reset: () => { setPacks({}); setSystem(null); resetLivePairs(); } };
}
