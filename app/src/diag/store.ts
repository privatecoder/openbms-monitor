import { useEffect, useRef, useState } from "react";
import type { PackEntry } from "../store";
import { innerMilliohm, outerMilliohm, smooth, zeroOffset, type Offset, type Reading } from "./calc";

export interface Snapshot { at: number; bus: number; rows: Record<number, { current: number; outer?: number; inner?: number }> }
interface Saved { offsets: Record<number, Offset>; zeroAt?: number; busRest?: number; snapshots: Snapshot[] }

const keyFor = (site: string) => `diag:${site}`;
const load = (site: string): Saved => {
  try { return { offsets: {}, snapshots: [], ...JSON.parse(localStorage.getItem(keyFor(site)) ?? "{}") }; } catch { return { offsets: {}, snapshots: [] }; }
};
const reading = (p: PackEntry): Reading | undefined =>
  p.telemetry ? { current: p.telemetry.current, pack_voltage: p.telemetry.pack_voltage, port_voltage: p.telemetry.port_voltage } : undefined;

/** Zero offsets, measurement history and the live (smoothed) inner resistance per pack. */
export function useDiag(site: string, packs: Record<number, PackEntry>) {
  const [saved, setSaved] = useState<Saved>(() => load(site));
  const [inner, setInner] = useState<Record<number, number>>({});
  const seen = useRef<Record<number, number>>({});
  useEffect(() => { setSaved(load(site)); setInner({}); seen.current = {}; }, [site]);

  const persist = (s: Saved) => {
    setSaved(s);
    try { localStorage.setItem(keyFor(site), JSON.stringify(s)); } catch { /* storage unavailable */ }
  };

  // smooth each new reading into the live inner resistance
  useEffect(() => {
    const next = { ...inner };
    let changed = false;
    for (const p of Object.values(packs)) {
      const r = reading(p), off = saved.offsets[p.address];
      if (!r || !off || seen.current[p.address] === p.updated) continue;
      seen.current[p.address] = p.updated;
      const v = innerMilliohm(r, off);
      if (v !== undefined) { next[p.address] = smooth(next[p.address], v); changed = true; }
    }
    if (changed) setInner(next);
  }, [packs, saved.offsets]); // eslint-disable-line react-hooks/exhaustive-deps

  const zero = (busRest?: number) => {
    const offsets: Record<number, Offset> = {};
    for (const p of Object.values(packs)) { const r = reading(p); if (r) offsets[p.address] = zeroOffset(r, busRest); }
    setInner({}); seen.current = {};
    persist({ ...saved, offsets, zeroAt: Date.now(), busRest });
  };

  const measure = (bus: number) => {
    const rows: Snapshot["rows"] = {};
    for (const p of Object.values(packs)) {
      const r = reading(p), off = saved.offsets[p.address];
      if (!r || !off) continue;
      rows[p.address] = { current: r.current, outer: outerMilliohm(r, off, bus), inner: innerMilliohm(r, off) };
    }
    persist({ ...saved, snapshots: [{ at: Date.now(), bus, rows }, ...saved.snapshots].slice(0, 30) });
  };

  const clearHistory = () => persist({ ...saved, snapshots: [] });

  return { ...saved, inner, zero, measure, clearHistory };
}
