import type { BenchPack, BenchScan, FirmwareDone, FirmwareImage, FirmwareProgress } from "./api";

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

/** „Platine flashen“: the pack preselected after a search, only when exactly one answered. */
export function benchPick(packs: BenchPack[]): number | null {
  return packs.length === 1 ? packs[0].address : null;
}

/** „Platine flashen“: what the app can say about the chosen board's marker before the flash (null: nothing to say). Set
 *  and shown over RS485-1/2: cleared before an image without the maintenance functions. Not shown there (an older MAINT
 *  build or the original), at a master: the way over the CAN socket. */
export function benchMarkerKey(pack: BenchPack | null, image: FirmwareImage | null): string | null {
  if (!pack || image?.kind === "maintenance") return null;
  if (pack.marker === "on") return image ? "fw.bench.markerOn" : null;
  if (pack.marker === null && pack.address === 0) return "fw.bench.markerOlder";
  return null;
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
 *  power and clock over the CAN socket (`powerRtc`) also its lock (bit 3: 1 locks); with the stored pack cutoff
 *  (`packCutoff`) the extended off (bit 4: 0 on, FF leaves it off) and a bit 5 at 0, which the image takes for a lock. */
export function flashGateKeys(g0: number, powerRtc = false, packCutoff = false): string[] {
  const write = (g0 & 1) === 1, start = (g0 & 4) === 4;
  const keys = write && start ? ["fw.patch.ok"] : !write && !start ? ["fw.patch.bothLocked"] : [write ? "fw.patch.startLocked" : "fw.patch.writeLocked"];
  const power = powerRtc ? [(g0 & 8) === 8 ? "fw.patch.powerLocked" : "fw.patch.powerOpen"] : [];
  const cutoff = packCutoff ? [(g0 & 0x10) === 0 ? "fw.patch.cutoffOn" : "fw.patch.cutoffOff", ...((g0 & 0x20) === 0 ? ["fw.patch.lockKept"] : [])] : [];
  return [...keys, ...power, ...cutoff];
}

/** The image, „Eigene Firmware zulassen“, port and address are fixed while a flash runs (until its end event) or a pack
 *  check runs: a change then could not reach the running flash and would only mislead. */
export const firmwareInputsLocked = (running: boolean, busy: boolean) => running || busy;
