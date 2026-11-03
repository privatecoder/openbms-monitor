/** The time axis under the heatmap and the balancing timeline (one axis for both, so they stay aligned), and the share
 *  wording of the timeline. Local time, locale-aware. */

const HOUR = 3_600_000, DAY = 24 * HOUR;

/** Tick times inside (from, to): every 6 h up to a day and a half, every local midnight up to 8 days, else every
 *  Monday at midnight. Ticks closer than `margin` (a share of the span) to an end are left out: the end labels sit
 *  there; the axis also hides a tick whose label would touch an end label on a narrow chart (`overlapping`). */
export function axisTicks(from: number, to: number, margin = 0.04): number[] {
  const span = to - from;
  if (span <= 0) return [];
  const out: number[] = [];
  const d = new Date(from);
  d.setMinutes(0, 0, 0);
  if (span <= 1.5 * DAY) {
    d.setHours(Math.floor(d.getHours() / 6) * 6);
    for (; d.getTime() < to; d.setHours(d.getHours() + 6)) out.push(d.getTime());
  } else {
    d.setHours(0);
    const weekly = span > 8 * DAY;
    // the first Monday on or before (getDay: 0 = Sunday)
    if (weekly) d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
    for (; d.getTime() < to; d.setDate(d.getDate() + (weekly ? 7 : 1))) out.push(d.getTime());
  }
  const m = span * margin;
  return out.filter((t) => t > from + m && t < to - m);
}

/** The ticks (by index) to hide so no label touches another: end labels first, then ticks from left to right, each
 *  kept only if it stays `gap` px clear of the end labels and of the last tick kept. Boxes are [left, right] in px. */
export function overlapping(start: [number, number], end: [number, number], ticks: [number, number][], gap = 6): Set<number> {
  const hide = new Set<number>();
  let last = start[1];
  ticks.forEach(([l, r], i) => {
    if (l < last + gap || r > end[0] - gap) hide.add(i);
    else last = r;
  });
  return hide;
}

const sameDay = (a: number, b: number) => new Date(a).toDateString() === new Date(b).toDateString();
const time = (ms: number, lang: string) => new Date(ms).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" });
const date = (ms: number, lang: string) => new Date(ms).toLocaleDateString(lang, { day: "2-digit", month: "2-digit" });

/** An end of the axis: the time alone when the span stays within one day, else date and time. */
export function endLabel(ms: number, from: number, to: number, lang: string): string {
  return sameDay(from, to) ? time(ms, lang) : `${date(ms, lang)} ${time(ms, lang)}`;
}

/** A tick: the time for 6-hour ticks (the date at midnight), else the date. */
export function tickLabel(ms: number, from: number, to: number, lang: string): string {
  const d = new Date(ms);
  if (to - from <= 1.5 * DAY && (d.getHours() !== 0 || d.getMinutes() !== 0)) return time(ms, lang);
  return date(ms, lang);
}

/** A share (0..1) in percent; above 0 but below 1 % reads „< 1 %“, below 100 but above 99 % „> 99 %“, so a few
 *  minutes never round away. */
export function sharePct(v: number | null, lang: string): string {
  if (v === null || !Number.isFinite(v)) return "–";
  const p = v * 100, n = (x: number) => x.toLocaleString(lang, { maximumFractionDigits: 0 });
  if (p > 0 && p < 1) return `< ${n(1)} %`;
  if (p < 100 && p > 99) return `> ${n(99)} %`;
  return `${n(Math.round(p))} %`;
}

/** How much of the span the recording covers, from the heatmap's slices (a slice with any value counts), in ms. */
export function covered(slices: (number | null)[] | undefined, from: number, to: number): number {
  if (!slices?.length) return 0;
  return Math.round((slices.filter((v) => v !== null).length / slices.length) * (to - from));
}

/** A duration in hours up to two days, else in days (one decimal), locale-aware. */
export function durationText(ms: number, lang: string, t: (k: string, v: Record<string, unknown>) => string): string {
  const one = (x: number) => x.toLocaleString(lang, { maximumFractionDigits: 1 });
  return ms <= 2 * DAY ? t("cell.hours", { n: one(ms / HOUR) }) : t("cell.days", { n: one(ms / DAY) });
}
