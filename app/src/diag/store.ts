import { useEffect, useRef, useState } from "react";
import type { PackEntry } from "../store";
import { useSettings } from "../installData";
import type { Pair } from "../pairs";
import { innerMilliohm, outerMilliohm, smooth, zeroOffset, type Offset, type Reading, type RefPoint } from "./calc";

/** Reference voltages: one busbar value, or one load-switch value per pair (keyed by the pair's plus pack). */
export interface Refs { bus?: number; perPair: Record<number, number | undefined> }
export interface Snapshot { at: number; point: RefPoint; refs: Refs; rows: Record<number, { current: number; outer?: number; inner?: number }> }
interface Saved { offsets: Record<number, Offset>; zeroAt?: number; zeroRefs?: Refs; snapshots: Snapshot[] }

/** Stored diagnosis data, made fit for this version. */
export const parseDiag = (v: unknown): Saved => {
  if (!v || typeof v !== "object") return { offsets: {}, snapshots: [] };
  const s = { offsets: {}, snapshots: [], ...structuredClone(v) } as Saved;
  // older saves had a plain number as outer offset and bus snapshots without a reference point
  for (const o of Object.values(s.offsets)) if (typeof o.outer === "number") o.outer = { bus: o.outer as unknown as number };
  s.snapshots = Array.isArray(s.snapshots) ? s.snapshots.filter((x) => x.point && x.refs) : [];
  return s;
};
/** Pack values at most this old (ms) are taken together with a reference voltage entered now. */
export const FRESH_MS = 10_000;
const stale = (p: PackEntry, now: number) => now - p.updated > FRESH_MS;

const reading = (p: PackEntry): Reading | undefined =>
  p.telemetry ? { current: p.telemetry.current, pack_voltage: p.telemetry.pack_voltage, port_voltage: p.telemetry.port_voltage } : undefined;

/** Reference voltage that applies to a pack. */
export function refFor(addr: number, point: RefPoint, refs: Refs, pairs: Pair[]): number | undefined {
  if (point === "bus") return refs.bus;
  const pair = pairs.find((p) => p.plus === addr || p.minus === addr);
  return pair ? refs.perPair[pair.plus] : undefined;
}

/** Zero offsets, measurement history and the live (smoothed) inner resistance per pack. */
export function useDiag(scope: string | null, packs: Record<number, PackEntry>, pairs: Pair[]) {
  const { settings, save } = useSettings(scope);
  const raw = settings.diagnosis;
  const [saved, setSaved] = useState<Saved>(() => parseDiag(raw));
  const [inner, setInner] = useState<Record<number, number>>({});
  const seen = useRef<Record<number, number>>({});
  // another installation, or its stored data arrived
  useEffect(() => { setSaved(parseDiag(raw)); }, [raw]);
  useEffect(() => { setInner({}); seen.current = {}; }, [scope]);

  const persist = (s: Saved) => { setSaved(s); save("diagnosis", s); };

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

  /** Zero at rest; outer offsets for both reference points as far as their voltages were entered. A pack whose values
   *  are older than FRESH_MS gets no outer offset (the reference was measured now); returns those packs. */
  const zero = (refs: Refs): number[] => {
    const offsets: Record<number, Offset> = {}, old: number[] = [], now = Date.now();
    for (const p of Object.values(packs)) {
      const r = reading(p);
      if (!r) continue;
      let bus = refFor(p.address, "bus", refs, pairs), sw = refFor(p.address, "switch", refs, pairs);
      if ((bus !== undefined || sw !== undefined) && stale(p, now)) { old.push(p.address); bus = sw = undefined; }
      offsets[p.address] = {
        inner: zeroOffset(r).inner,
        outer: {
          ...(bus !== undefined ? zeroOffset(r, bus, "bus").outer : {}),
          ...(sw !== undefined ? zeroOffset(r, sw, "switch").outer : {}),
        },
      };
    }
    setInner({}); seen.current = {};
    persist({ ...saved, offsets, zeroAt: now, zeroRefs: refs });
    return old;
  };

  /** The outer resistance against the reference measured now; a pack whose values are older than FRESH_MS is left
   *  out of the measurement. Returns those packs. */
  const measure = (point: RefPoint, refs: Refs): number[] => {
    const rows: Snapshot["rows"] = {}, old: number[] = [], now = Date.now();
    for (const p of Object.values(packs)) {
      const r = reading(p), off = saved.offsets[p.address], ref = refFor(p.address, point, refs, pairs);
      if (!r || !off) continue;
      if (ref !== undefined && stale(p, now)) { old.push(p.address); continue; }
      rows[p.address] = { current: r.current, outer: ref === undefined ? undefined : outerMilliohm(r, off, ref, point), inner: innerMilliohm(r, off) };
    }
    if (Object.keys(rows).length) persist({ ...saved, snapshots: [{ at: now, point, refs, rows }, ...saved.snapshots].slice(0, 30) });
    return old;
  };

  const clearHistory = () => persist({ ...saved, snapshots: [] });
  const hasOuter = (point: RefPoint) => Object.values(saved.offsets).some((o) => o.outer?.[point] !== undefined);

  return { ...saved, inner, zero, measure, clearHistory, hasOuter };
}
