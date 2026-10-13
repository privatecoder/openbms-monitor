import type { DeviceInfo, FirmwareDone, FirmwareImage, FirmwareProgress } from "./api";

/** Custom images get their own phrase, so consent to one kind never covers the other. */
export const confirmationPhrase = (address: number, custom = false) => custom ? `FLASH CUSTOM ${address}` : `FLASH ${address}`;

type T = (key: string, params?: Record<string, unknown>) => string;

/** The image's name: the release of an original, the project's known maintenance build (with `known`: "(known
 *  checksum)"), or "custom firmware". */
export function imageLabel(image: FirmwareImage, t: T, known = false): string {
  if (image.kind === "maintenance") return t(known ? "fw.maintKnown" : "fw.maintName", { release: image.release });
  return image.custom ? t("fw.customName") : image.release;
}

/** The warning box under the image: the maintenance build is known, a custom image is not. */
export function imageWarning(image: FirmwareImage): string | null {
  if (image.kind === "maintenance") return "fw.maintWarning";
  return image.custom ? "fw.customWarning" : null;
}

export interface FwSetup {
  running: boolean;
  /** a result is shown; continuing needs "New update" (or "Flash the next pack" in a maintenance session) */
  done: boolean;
  port: string;
  address: number;
  image: FirmwareImage | null;
  /** result of "check pack" and the port/address it was read from */
  target: DeviceInfo | null;
  checkedFor: { port: string; address: number } | null;
  typed: string;
  /** maintenance allows the flash (`canFlash`); true when no installation is in use */
  unlocked: boolean;
  /** the pack bus panel's steps 2–5 are done (`flowOf(...).steps.flash` is current) */
  ready: boolean;
  /** another action runs (pack check, a panel step): inputs and actions wait */
  busy: boolean;
}

/** Which steps are usable. A checked pack counts only for the port and address it was read from. */
export function steps(s: FwSetup) {
  // the confirmation is used up by a run: nothing is usable until the result is dismissed
  if (s.running || s.done || s.busy) return { image: false, inspect: false, confirm: false, start: false };
  const inspect = s.port !== "" && s.image !== null && s.ready;
  const confirm = inspect && s.target !== null && s.checkedFor?.port === s.port && s.checkedFor.address === s.address;
  return { image: true, inspect, confirm, start: confirm && s.typed.trim() === confirmationPhrase(s.address, s.image?.custom) && s.unlocked };
}

/** 0..1 over the data blocks; erasing counts as start, everything after the last block as done. */
export function progressFraction(p: FirmwareProgress | null): number {
  if (!p) return 0;
  if (p.stage === "block" && p.block !== null && p.total) return p.block / p.total;
  return ["finalize", "app_restart", "post_check"].includes(p.stage) ? 1 : 0;
}

/** The buttons under a result: an open maintenance session goes on with the next pack or its End (no way back to
 *  the monitor while it is open); without one a new update or back to the monitor. */
export const resultActions = (sessionOpen: boolean) => (sessionOpen ? ["next", "end"] : ["again", "back"]);

/** The master's post-flash check: only "as-expected" is a success ("unclear", "marker-unexplained",
 *  "unknown-image" are not). */
export const masterOk = (outcome: string) => outcome === "as-expected";

/** The version line of a result. 0x51 reports only major.minor ("16.06"), so "from" is what the pack reported
 *  (before the flash, else at "check pack") and "to" the flashed image's release in full ("16.06.13",
 *  "16.06.13 MAINT"). A pack that reports another version afterwards, or a custom image, says what it reports. */
export function resultVersions(done: Pick<FirmwareDone, "before" | "after">, checked: string | null, image: FirmwareImage | null, t: T): { from: string; to: string } {
  const from = done.before?.firmware_version ?? checked ?? "?";
  const after = done.after?.firmware_version ?? null;
  if (!image) return { from, to: after ?? "?" };
  if (image.kind === "custom") return { from, to: after ? `${t("fw.customName")} (${t("fw.reports", { v: after })})` : t("fw.customName") };
  // "16.06" fits "16.06.13" but not "16.06.1x" of another minor: compare whole parts
  const fits = !after || image.release === after || image.release.startsWith(`${after}.`) || image.release.startsWith(`${after} `);
  return { from, to: fits ? image.release : `${image.release} (${t("fw.reports", { v: after })})` };
}

/** The refusal under "start": not while a flash runs or its result is shown (the measurement it used is gone). */
export const gateHint = (gate: { ok: boolean; reason?: string }, s: { running: boolean; done: boolean }) =>
  !gate.ok && gate.reason && !s.running && !s.done ? gate.reason : null;
