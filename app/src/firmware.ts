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

/** Red for an image with an unknown checksum; a known maintenance build is shown in orange. */
export function imageWarningIsAlarm(image: FirmwareImage): boolean {
  return image.kind !== "maintenance" && image.custom;
}

export interface FwSetup {
  running: boolean;
  /** a result is shown; continuing needs "New update" */
  done: boolean;
  port: string;
  address: number;
  image: FirmwareImage | null;
  /** result of "check pack" and the port/address (and installation) it was read for */
  target: DeviceInfo | null;
  checkedFor: { port: string; address: number; inst?: string | null } | null;
  /** the installation chosen now: a check made for another one does not count */
  inst?: string | null;
  typed: string;
  /** another action runs (pack check, a panel action): inputs and actions wait */
  busy: boolean;
}

/** Which steps are usable. A checked pack counts only for the port, address and installation it was read for. */
export function steps(s: FwSetup) {
  // the confirmation is used up by a run: nothing is usable until the result is dismissed
  if (s.running || s.done || s.busy) return { image: false, inspect: false, confirm: false, start: false };
  const inspect = s.port !== "" && s.image !== null;
  const confirm = inspect && s.target !== null && s.checkedFor?.port === s.port && s.checkedFor.address === s.address
    && (s.checkedFor.inst ?? null) === (s.inst ?? null);
  return { image: true, inspect, confirm, start: confirm && s.typed.trim() === confirmationPhrase(s.address, s.image?.custom) };
}

/** 0..1 over the data blocks; erasing counts as start, everything after the last block as done. */
export function progressFraction(p: FirmwareProgress | null): number {
  if (!p) return 0;
  if (p.stage === "block" && p.block !== null && p.total) return p.block / p.total;
  return ["finalize", "app_restart", "post_check"].includes(p.stage) ? 1 : 0;
}

/** The version line of a result. 0x51 reports only major.minor ("16.06"), so "from" is what the pack reported
 *  (before the flash, else at "check pack") and "to" the flashed image's release in full ("16.06.13",
 *  "16.06.13 MAINT 1.1"). A pack that reports another version afterwards, or a custom image, says what it reports. */
export function resultVersions(done: Pick<FirmwareDone, "before" | "after">, checked: string | null, image: FirmwareImage | null, t: T): { from: string; to: string } {
  const from = done.before?.firmware_version ?? checked ?? "?";
  const after = done.after?.firmware_version ?? null;
  if (!image) return { from, to: after ?? "?" };
  if (image.kind === "custom") return { from, to: after ? `${t("fw.customName")} (${t("fw.reports", { v: after })})` : t("fw.customName") };
  // "16.06" fits "16.06.13" but not "16.06.1x" of another minor: compare whole parts
  const fits = !after || image.release === after || image.release.startsWith(`${after}.`) || image.release.startsWith(`${after} `);
  return { from, to: fits ? image.release : `${image.release} (${t("fw.reports", { v: after })})` };
}

/** What the release bits of Group0 (`g0`) mean after a flash with the patch functions, which takes them over unchanged:
 *  writing over the CAN socket (bit 0) and the maintenance start (bit 2) locked, or both allowed; with an image with
 *  power and clock over the CAN socket (`powerRtc`) also its lock (bit 3: 1 locks). */
export function flashGateKeys(g0: number, powerRtc = false): string[] {
  const write = (g0 & 1) === 1, start = (g0 & 4) === 4;
  const keys = write && start ? ["fw.patch.ok"] : !write && !start ? ["fw.patch.bothLocked"] : [write ? "fw.patch.startLocked" : "fw.patch.writeLocked"];
  return powerRtc ? [...keys, (g0 & 8) === 8 ? "fw.patch.powerLocked" : "fw.patch.powerOpen"] : keys;
}

/** The image, „Eigene Firmware zulassen“, port and address are fixed while a flash runs (until its end event) or a pack
 *  check runs: a change then could not reach the running flash and would only mislead. */
export const firmwareInputsLocked = (running: boolean, busy: boolean) => running || busy;
