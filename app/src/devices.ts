/** Devices of an installation (setting key `devices`): for now only external active balancers with manual settings.
 *  Settings and assignments keep revisions in time (`from`, ms), so the activity of a recorded row is computed with the
 *  settings valid then (`balancer.rs`). A deleted definition gets `to` and its assignments end; the past stays
 *  computable. Nothing here is read from a device: `profile` is reserved for later connection profiles. */

import { fmt } from "./lib/utils";

export type BalancerWhen = "always" | "charging" | "not-discharging";
export const WHENS: BalancerWhen[] = ["always", "charging", "not-discharging"];

/** The manual settings (mV, A). stop_mv null: the trigger (no hysteresis); max_a is shown only. */
export interface BalancerValues { start_mv: number | null; trigger_mv: number; stop_mv: number | null; when: BalancerWhen; max_a: number | null }
export interface DeviceRev extends BalancerValues { from: number }
export interface DeviceDef { id: string; name: string; kind: "active-balancer"; profile: null; revs: DeviceRev[]; to: number | null }
export interface AssignRev { from: number; device: string | null }
export interface Devices { defs: DeviceDef[]; assign: Record<string, AssignRev[]> }

export const BALANCER_DEFAULTS: BalancerValues = { start_mv: null, trigger_mv: 5, stop_mv: null, when: "always", max_a: null };
export const MV_RANGE = [1, 5000] as const;

const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const obj = (v: unknown): Record<string, unknown> => (v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {});

function values(o: Record<string, unknown>): BalancerValues {
  return {
    start_mv: num(o.start_mv),
    trigger_mv: num(o.trigger_mv) ?? BALANCER_DEFAULTS.trigger_mv,
    stop_mv: num(o.stop_mv),
    when: WHENS.includes(o.when as BalancerWhen) ? (o.when as BalancerWhen) : "always",
    max_a: num(o.max_a),
  };
}

/** The stored value, read leniently (none or unreadable: no devices; an unreadable entry is left out). */
export function parseDevices(v: unknown): Devices {
  const o = obj(v);
  const defs: DeviceDef[] = (Array.isArray(o.defs) ? o.defs : []).flatMap((d) => {
    const x = obj(d);
    if (typeof x.id !== "string" || x.kind !== "active-balancer") return [];
    const revs = (Array.isArray(x.revs) ? x.revs : []).flatMap((r) => {
      const from = num(obj(r).from);
      return from === null ? [] : [{ from, ...values(obj(r)) }];
    });
    return [{ id: x.id, name: typeof x.name === "string" ? x.name : "", kind: "active-balancer" as const, profile: null, revs, to: num(x.to) }];
  });
  const assign: Record<string, AssignRev[]> = {};
  for (const [pack, list] of Object.entries(obj(o.assign))) {
    if (!/^\d+$/.test(pack) || !Array.isArray(list)) continue;
    assign[pack] = list.flatMap((a) => {
      const from = num(obj(a).from), device = obj(a).device;
      return from === null ? [] : [{ from, device: typeof device === "string" ? device : null }];
    });
  }
  return { defs, assign };
}

const last = <T extends { from: number }>(list: T[], at = Infinity): T | undefined =>
  list.reduce<T | undefined>((best, x) => (x.from <= at && (!best || x.from >= best.from) ? x : best), undefined);

/** The definitions not deleted. */
export const liveDefs = (d: Devices) => d.defs.filter((x) => x.to === null);

/** A definition's current settings. */
export const valuesOf = (def: DeviceDef): BalancerValues => {
  const r = last(def.revs);
  return r ? values(r as unknown as Record<string, unknown>) : BALANCER_DEFAULTS;
};

/** The device assigned to a pack now (null: none, or its definition was deleted). */
export function assignedNow(d: Devices, pack: number): DeviceDef | null {
  const id = last(d.assign[pack] ?? [])?.device;
  return liveDefs(d).find((x) => x.id === id) ?? null;
}

/** The packs a definition is assigned to now. */
export const packsOf = (d: Devices, id: string, packs: number[]) => packs.filter((p) => assignedNow(d, p)?.id === id);

/** Append a revision; one at the same time as the last replaces it (two changes within a millisecond). */
function push<T extends { from: number }>(list: T[], rev: T): T[] {
  const prev = list[list.length - 1];
  return prev && prev.from === rev.from ? [...list.slice(0, -1), rev] : [...list, rev];
}

const sameValues = (a: BalancerValues, b: BalancerValues) =>
  a.start_mv === b.start_mv && a.trigger_mv === b.trigger_mv && a.stop_mv === b.stop_mv && a.when === b.when && a.max_a === b.max_a;

const newId = (d: Devices, now: number) => {
  let id = `bal-${now.toString(36)}`;
  for (let n = 2; d.defs.some((x) => x.id === id); n++) id = `bal-${now.toString(36)}-${n}`;
  return id;
};

/** A new active balancer; returns the devices and its id. */
export function addBalancer(d: Devices, name: string, v: BalancerValues, now: number): [Devices, string] {
  const id = newId(d, now);
  const def: DeviceDef = { id, name: name.trim(), kind: "active-balancer", profile: null, revs: [{ from: now, ...v }], to: null };
  return [{ ...d, defs: [...d.defs, def] }, id];
}

/** Rename and change settings: a settings change appends a revision, the name is changed in place (it has no effect). */
export function editBalancer(d: Devices, id: string, name: string, v: BalancerValues, now: number): Devices {
  return {
    ...d,
    defs: d.defs.map((x) => (x.id !== id ? x : { ...x, name: name.trim(), revs: sameValues(valuesOf(x), v) ? x.revs : push(x.revs, { from: now, ...v }) })),
  };
}

/** Delete a definition: it ends now (`to`), and the packs that have it now get „none“ from now on. */
export function removeBalancer(d: Devices, id: string, packs: number[], now: number): Devices {
  const assign = { ...d.assign };
  for (const p of packsOf(d, id, packs)) assign[p] = push(assign[p] ?? [], { from: now, device: null });
  return { defs: d.defs.map((x) => (x.id === id ? { ...x, to: now } : x)), assign };
}

/** Assign a definition (or none) to packs from now on; packs that already have it get no revision. */
export function assignPacks(d: Devices, packs: number[], id: string | null, now: number): Devices {
  const next = { ...d.assign };
  for (const p of packs) {
    if ((assignedNow(d, p)?.id ?? null) === id) continue;
    next[p] = push(next[p] ?? [], { from: now, device: id });
  }
  return { ...d, assign: next };
}

/** The settings as a short line, e.g. „ab 5 mV, bis 3 mV, ab 3,40 V, beim Laden, 4 A“ (texts from `dev.*`). */
export function summary(v: BalancerValues, t: (k: string, o?: Record<string, unknown>) => string): string {
  return [
    t("dev.sum.trigger", { mv: v.trigger_mv }),
    v.stop_mv !== null && v.stop_mv < v.trigger_mv ? t("dev.sum.stop", { mv: v.stop_mv }) : null,
    v.start_mv !== null ? t("dev.sum.start", { v: fmt(v.start_mv / 1000, 3, "V") }) : null,
    t(`dev.when.${v.when}`),
    v.max_a !== null ? t("dev.sum.maxA", { a: fmt(v.max_a, v.max_a % 1 ? 1 : 0, "A") }) : null,
  ].filter(Boolean).join(", ");
}

/** The editor's fields as typed. */
export interface BalancerForm { name: string; trigger_mv: string; stop_mv: string; start_mv: string; when: BalancerWhen; max_a: string }

export const toForm = (name: string, v: BalancerValues): BalancerForm => ({
  name, trigger_mv: String(v.trigger_mv), stop_mv: v.stop_mv === null ? "" : String(v.stop_mv), start_mv: v.start_mv === null ? "" : String(v.start_mv),
  when: v.when, max_a: v.max_a === null ? "" : String(v.max_a),
});

/** The settings of the editor's fields; null if one is out of range (trigger required, the others empty or valid). */
export function fromForm(f: BalancerForm): BalancerValues | null {
  const mv = (s: string, required: boolean): number | null | undefined => {
    const x = s.trim();
    if (!x) return required ? undefined : null;
    const n = Number(x.replace(",", "."));
    return Number.isInteger(n) && n >= MV_RANGE[0] && n <= MV_RANGE[1] ? n : undefined;
  };
  const trigger = mv(f.trigger_mv, true), stop = mv(f.stop_mv, false), start = mv(f.start_mv, false);
  const a = f.max_a.trim() ? Number(f.max_a.trim().replace(",", ".")) : null;
  if (trigger == null || stop === undefined || start === undefined || (a !== null && !(a > 0 && a < 1000))) return null;
  return { trigger_mv: trigger, stop_mv: stop, start_mv: start, when: f.when, max_a: a };
}
