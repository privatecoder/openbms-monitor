import type { Cell } from "./types";

/** Value at a dotted path ("voltage.nominal_v"). */
export const get = (o: unknown, path: string) =>
  path.split(".").reduce<unknown>((x, k) => (x && typeof x === "object" ? (x as Record<string, unknown>)[k] : undefined), o);

/** Structural equality of two JSON values (key order independent). */
export function sameJson(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a as object), kb = Object.keys(b as object);
  return ka.length === kb.length && ka.every((k) => sameJson((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}

/**
 * A proof belongs to the value it was made for: if a value changed but its provenance entry was left
 * as it was, the entry is no longer "verified". A new datasheet URL invalidates all untouched proofs.
 */
export function unverifyChanged(next: Cell, before: Cell): Cell {
  const prov = { ...(next.provenance ?? {}) };
  const urlChanged = next.datasheet.url !== before.datasheet.url;
  for (const [key, p] of Object.entries(prov)) {
    const untouched = sameJson(p, before.provenance?.[key]);
    if (p.verified && untouched && (urlChanged || !sameJson(get(next, key), get(before, key)))) prov[key] = { ...p, verified: false };
  }
  return { ...next, provenance: prov };
}
