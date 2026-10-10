/** Helpers for the history charts; the data itself comes bucketed from the database (history.rs). */

/** Below this mean current per pack (A) the share is not meaningful (equalizing currents dominate). */
export const SHARE_MIN_A = 5;
/** Share of the packs that must have a value in a bucket for a meaningful mean. */
export const SHARE_MIN_PRESENT = 0.75;

/**
 * Each pack's current relative to the mean of the packs with a value in the same bucket (1 = average
 * share). Only where the mean is at least SHARE_MIN_A and at least SHARE_MIN_PRESENT of the packs have one.
 */
export function shares(currents: Map<number, (number | null)[]>): Map<number, (number | null)[]> {
  const cols = [...currents.values()];
  const n = cols[0]?.length ?? 0;
  const mean: (number | null)[] = Array.from({ length: n }, (_, b) => {
    const vs = cols.map((c) => c[b]).filter((x): x is number => x !== null);
    if (vs.length < 2 || vs.length < cols.length * SHARE_MIN_PRESENT) return null;
    const m = vs.reduce((s, x) => s + x, 0) / vs.length;
    return Math.abs(m) >= SHARE_MIN_A ? m : null;
  });
  return new Map([...currents].map(([a, c]) => [a, c.map((x, b) => (x === null || mean[b] === null ? null : x / mean[b]!))]));
}

/** A colour per pack address that stays the same across charts (golden-angle hues). */
export const packColor = (address: number, dark: boolean) =>
  `hsl(${Math.round((address * 137.508 + 190) % 360)} ${dark ? 62 : 58}% ${dark ? 64 : 42}%)`;

/** File name for an export: openbms_192.168.1.10-4196_2026-10-10_18-05.jsonl (local time). */
export function exportName(site: string, ext: string, now = new Date()) {
  const z = (n: number) => String(n).padStart(2, "0");
  const s = site.replace(/^.*[\\/]/, "").replace(/[^A-Za-z0-9.-]+/g, "-").replace(/^-+|-+$/g, "") || "history";
  return `openbms_${s}_${now.getFullYear()}-${z(now.getMonth() + 1)}-${z(now.getDate())}_${z(now.getHours())}-${z(now.getMinutes())}.${ext}`;
}
