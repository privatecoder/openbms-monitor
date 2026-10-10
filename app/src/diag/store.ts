import { useEffect, useRef, useState } from "react";
import type { PackEntry } from "../store";
import type { Pair } from "../pairs";
import { innerMilliohm, outerMilliohm, smooth, zeroOffset, type Offset, type Reading, type RefPoint } from "./calc";

/** Reference voltages: one busbar value, or one load-switch value per pair (keyed by the pair's plus pack). */
export interface Refs { bus?: number; perPair: Record<number, number | undefined> }
export interface Snapshot { at: number; point: RefPoint; refs: Refs; rows: Record<number, { current: number; outer?: number; inner?: number }> }
interface Saved { offsets: Record<number, Offset>; zeroAt?: number; zeroRefs?: Refs; snapshots: Snapshot[] }

const keyFor = (site: string) => `diag:${site}`;
const load = (site: string): Saved => {
  try {
    const s = { offsets: {}, snapshots: [], ...JSON.parse(localStorage.getItem(keyFor(site)) ?? "{}") } as Saved;
    // older saves had a plain number as outer offset and bus snapshots without a reference point
    for (const o of Object.values(s.offsets)) if (typeof o.outer === "number") o.outer = { bus: o.outer as unknown as number };
    s.snapshots = s.snapshots.filter((x) => x.point && x.refs);
    return s;
  } catch { return { offsets: {}, snapshots: [] }; }
};
const reading = (p: PackEntry): Reading | undefined =>
  p.telemetry ? { current: p.telemetry.current, pack_voltage: p.telemetry.pack_voltage, port_voltage: p.telemetry.port_voltage } : undefined;

/** Reference voltage that applies to a pack. */
export function refFor(addr: number, point: RefPoint, refs: Refs, pairs: Pair[]): number | undefined {
  if (point === "bus") return refs.bus;
  const pair = pairs.find((p) => p.plus === addr || p.minus === addr);
  return pair ? refs.perPair[pair.plus] : undefined;
}

/** Zero offsets, measurement history and the live (smoothed) inner resistance per pack. */
export function useDiag(site: string, packs: Record<number, PackEntry>, pairs: Pair[]) {
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

  /** Zero at rest; outer offsets for both reference points as far as their voltages were entered. */
  const zero = (refs: Refs) => {
    const offsets: Record<number, Offset> = {};
    for (const p of Object.values(packs)) {
      const r = reading(p);
      if (!r) continue;
      const bus = refFor(p.address, "bus", refs, pairs), sw = refFor(p.address, "switch", refs, pairs);
      offsets[p.address] = {
        inner: zeroOffset(r).inner,
        outer: {
          ...(bus !== undefined ? zeroOffset(r, bus, "bus").outer : {}),
          ...(sw !== undefined ? zeroOffset(r, sw, "switch").outer : {}),
        },
      };
    }
    setInner({}); seen.current = {};
    persist({ ...saved, offsets, zeroAt: Date.now(), zeroRefs: refs });
  };

  const measure = (point: RefPoint, refs: Refs) => {
    const rows: Snapshot["rows"] = {};
    for (const p of Object.values(packs)) {
      const r = reading(p), off = saved.offsets[p.address], ref = refFor(p.address, point, refs, pairs);
      if (!r || !off) continue;
      rows[p.address] = { current: r.current, outer: ref === undefined ? undefined : outerMilliohm(r, off, ref, point), inner: innerMilliohm(r, off) };
    }
    persist({ ...saved, snapshots: [{ at: Date.now(), point, refs, rows }, ...saved.snapshots].slice(0, 30) });
  };

  const clearHistory = () => persist({ ...saved, snapshots: [] });
  const hasOuter = (point: RefPoint) => Object.values(saved.offsets).some((o) => o.outer?.[point] !== undefined);

  return { ...saved, inner, zero, measure, clearHistory, hasOuter };
}
