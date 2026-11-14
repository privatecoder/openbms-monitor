/** Flashing several packs one after another with one confirmation (the flash series): the order, what each pack's
 *  flash came to, and the run that stops at the first failure. Pure; the pack bus panel supplies the flash (the backend's
 *  `firmware_start` with every check of a single flash) and waits for its end. */

import type { FirmwareDone, FirmwareImage } from "./api";
import { fitsImage } from "./firmware";

/** The order of a series: the slaves by address, the master (address 0) last (backend `series_order`). */
export const seriesOrder = (addresses: number[]): number[] =>
  [...new Set(addresses)].sort((a, b) => (a === 0 ? 1 : b === 0 ? -1 : a - b));

/** What one pack's flash came to. `refused`: a check before the flash refused, nothing written; `failed`: the flash
 *  broke off; `pending`: the pack may hang in its bootloader; `deviates`: the flash finished but the pack did not answer
 *  cleanly with the new image (the after-check missing or incomplete, something changed, another version reported);
 *  `notReached`: the series stopped before it. */
export type PackEnd =
  | { kind: "flashed"; to: string | null }
  | { kind: "refused"; error: unknown }
  | { kind: "failed"; done: FirmwareDone }
  | { kind: "pending"; done: FirmwareDone }
  | { kind: "deviates"; done: FirmwareDone; why: "missing" | "incomplete" | "differs" | "version" }
  | { kind: "notReached" };

/** A finished flash's end event as the series sees it: only a pack that answers cleanly with the new image lets the
 *  next one start (the backend applies the same rule before it admits the next pack). */
export function judge(done: FirmwareDone, image: FirmwareImage): PackEnd {
  if (done.pending) return { kind: "pending", done };
  if (!done.ok) return { kind: "failed", done };
  const to = done.after?.firmware_version ?? null;
  if (!done.comparison || !to) return { kind: "deviates", done, why: "missing" };
  if (done.comparison.verdict === "differs") return { kind: "deviates", done, why: "differs" };
  if (done.comparison.verdict !== "same") return { kind: "deviates", done, why: "incomplete" };
  // an unknown custom image has no version to compare with
  if (image.kind !== "custom" && !fitsImage(to, image)) return { kind: "deviates", done, why: "version" };
  return { kind: "flashed", to };
}

/** How a series ended: every pack flashed, stopped at a pack (its row says why), cancelled between packs, the link
 *  (installation or port) changed, or a pending bootloader record showed before the next pack. */
export type FlashSeriesEnd = "done" | "stopped" | "cancelled" | "link" | "pending";

/** The run's callbacks. `link`: the installation and port now; `begun`: the ones the series was confirmed for. */
export interface SeriesRun {
  link: () => string; cancelled: () => boolean;
  /** a fresh look at the open bootloader records (any, on any port) */
  pending: () => Promise<boolean>;
  /** starts the flash and resolves with its end event; throws a refusal of its checks */
  flash: (address: number) => Promise<FirmwareDone>;
  judge: (done: FirmwareDone) => PackEnd;
  report: (address: number, end: PackEnd) => void;
  start?: (address: number) => void;
}

/** Flashes `order` one pack after another. Right before each pack, also after the wait for the bootloader records,
 *  it stops when the link is no longer `begun`, the user cancelled or a record is open; a running flash is never
 *  interrupted. The next pack starts only after the previous one was `flashed`; any other end stops the series. Packs
 *  not reached are reported as such. */
export async function runFlashSeries(order: number[], o: SeriesRun & { begun: string }): Promise<FlashSeriesEnd> {
  const rest = (from: number) => { for (const a of order.slice(from)) o.report(a, { kind: "notReached" }); };
  const stopNow = (): FlashSeriesEnd | null => (o.link() !== o.begun ? "link" : o.cancelled() ? "cancelled" : null);
  for (const [n, a] of order.entries()) {
    let stop = stopNow();
    if (!stop && await o.pending()) stop = "pending";
    // the look at the records takes a moment: a cancel or a link change meanwhile still stops before the flash
    stop ??= stopNow();
    if (stop) { rest(n); return stop; }
    o.start?.(a);
    let end: PackEnd;
    try { end = o.judge(await o.flash(a)); } catch (error) { end = { kind: "refused", error }; }
    o.report(a, end);
    if (end.kind !== "flashed") { rest(n + 1); return "stopped"; }
  }
  return "done";
}

/** A whole series from its confirmation: the link is taken before the confirmation is sent, so a change while the
 *  backend confirms ends the series before its first pack (all packs not flashed). `begin` returns the order;
 *  `end` releases the backend's grant, after the series in any case (also after a refused begin). */
export async function startFlashSeries(o: SeriesRun & { begin: () => Promise<number[]>; end: () => void; onOrder: (order: number[]) => void }): Promise<FlashSeriesEnd> {
  const begun = o.link();
  try {
    const order = await o.begin();
    o.onOrder(order);
    return await runFlashSeries(order, { ...o, begun });
  } finally { o.end(); }
}

/** One series as shown: its rows and its image's label, set together once the backend confirmed it (a refused begin
 *  leaves the shown series, rows and label, as it was). */
export interface SeriesView { label: string; rows: { address: number; from: string | null; end?: PackEnd }[] }

export const seriesView = (order: number[], from: Map<number, string | null>, label: string): SeriesView =>
  ({ label, rows: order.map((address) => ({ address, from: from.get(address) ?? null })) });

/** One pack's end into the shown series. */
export const withEnd = (v: SeriesView | null, address: number, end: PackEnd): SeriesView | null =>
  v && { ...v, rows: v.rows.map((r) => (r.address === address ? { ...r, end } : r)) };

type T = (key: string, params?: Record<string, unknown>) => string;

/** A result row's text. `label`: the image of this run (not the one chosen since); `current`: the pack flashing now. */
export function seriesRowText(address: number, end: PackEnd | undefined, current: number | null, label: string, t: T, errText: (e: unknown) => string): string {
  if (!end) return t(current === address ? "maint.series.flashing" : "maint.series.waiting");
  switch (end.kind) {
    case "flashed": return t("maint.series.res.flashed", { to: label, v: end.to ?? "?" });
    case "refused": return t("maint.series.res.refused", { why: errText(end.error) });
    case "failed": return t(end.done.written ? "maint.series.res.failedAfter" : "maint.series.res.failed", { why: end.done.error ?? "?" });
    case "pending": return t("maint.series.res.pending");
    case "deviates": return t(`maint.series.res.deviates.${end.why}`, { v: end.done.after?.firmware_version ?? "?" });
    case "notReached": return t("maint.series.res.notReached");
  }
}

/** The counts above the result list. */
export interface FlashSeriesSummary { flashed: number; failed: number; notReached: number }

export function summarizeSeries(rows: { end?: PackEnd }[]): FlashSeriesSummary {
  const s: FlashSeriesSummary = { flashed: 0, failed: 0, notReached: 0 };
  for (const r of rows) {
    if (!r.end) continue;
    if (r.end.kind === "flashed") s.flashed++;
    else if (r.end.kind === "notReached") s.notReached++;
    else s.failed++;
  }
  return s;
}
