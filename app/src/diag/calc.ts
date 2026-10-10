/**
 * Resistance diagnosis from BMS readings.
 *
 * Every BMS reports the sum of its cells (pack_voltage) and the voltage at its output
 * (port_voltage). Under current I (charging positive) the difference is the drop inside the pack;
 * the difference between output and busbar (entered by the user) is the drop outside (cables,
 * connectors). Each measurement has a fixed error per pack, removed by a zero reading at rest.
 * Signed formulas keep R positive for charging and discharging alike.
 */

/** Below this current a drop of 10 mV resolution says too little. */
export const MIN_CURRENT_A = 3;
/** "At rest" for the zero reading. */
export const REST_CURRENT_A = 0.5;

export interface Reading { current: number; pack_voltage: number; port_voltage: number }
/**
 * Where "outside" ends: the busbar (one value for all packs) or the pack-side input of the pair's
 * load switch (one value per pair). The outer offset is kept per reference point.
 */
export type RefPoint = "bus" | "switch";
export interface Offset { inner: number; outer?: Partial<Record<RefPoint, number>> }

/** Offsets at rest: port − pack, and (if the reference voltage is known) reference − port. */
export function zeroOffset(r: Reading, ref?: number, point: RefPoint = "bus"): Offset {
  return { inner: r.port_voltage - r.pack_voltage, outer: ref === undefined ? undefined : { [point]: ref - r.port_voltage } };
}

/** Resistance inside the pack in mΩ, or undefined if the current is too small. */
export function innerMilliohm(r: Reading, off: Offset): number | undefined {
  if (Math.abs(r.current) < MIN_CURRENT_A) return undefined;
  return ((r.port_voltage - r.pack_voltage - off.inner) / r.current) * 1000;
}

/** Resistance outside the pack (to the reference point) in mΩ. */
export function outerMilliohm(r: Reading, off: Offset, ref: number, point: RefPoint = "bus"): number | undefined {
  const o = off.outer?.[point];
  if (Math.abs(r.current) < MIN_CURRENT_A || o === undefined) return undefined;
  return ((ref - r.port_voltage - o) / r.current) * 1000;
}

export const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b), m = s.length >> 1;
  return s.length === 0 ? NaN : s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};

/**
 * Marks values that stand out against the other packs: more than 1.5 × the median and at
 * least 1 mΩ above it (the second condition avoids flagging tiny absolute differences).
 */
export function flagHigh(values: Record<number, number | undefined>): Set<number> {
  const known = Object.entries(values).filter((e): e is [string, number] => e[1] !== undefined);
  const m = median(known.map(([, v]) => v));
  return new Set(known.filter(([, v]) => v > m * 1.5 && v - m >= 1).map(([a]) => Number(a)));
}

/** Exponential smoothing; the 10 mV voltage resolution makes single readings jumpy. */
export const smooth = (prev: number | undefined, next: number, alpha = 0.3) => (prev === undefined ? next : prev + alpha * (next - prev));
