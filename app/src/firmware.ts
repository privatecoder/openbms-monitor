import type { DeviceInfo, FirmwareImage, FirmwareProgress } from "./api";

/** Custom images get their own phrase, so consent to one kind never covers the other. */
export const confirmationPhrase = (address: number, custom = false) => custom ? `FLASH CUSTOM ${address}` : `FLASH ${address}`;

export interface FwSetup {
  running: boolean;
  /** a result is shown; continuing needs "New update" */
  done: boolean;
  port: string;
  address: number;
  image: FirmwareImage | null;
  /** result of "check pack" and the port/address it was read from */
  target: DeviceInfo | null;
  checkedFor: { port: string; address: number } | null;
  typed: string;
}

/** Which steps are usable. A checked pack counts only for the port and address it was read from. */
export function steps(s: FwSetup) {
  // the confirmation is used up by a run: nothing is usable until the result is dismissed
  if (s.running || s.done) return { image: false, inspect: false, confirm: false, start: false };
  const inspect = s.port !== "" && s.image !== null;
  const confirm = inspect && s.target !== null && s.checkedFor?.port === s.port && s.checkedFor.address === s.address;
  return { image: true, inspect, confirm, start: confirm && s.typed.trim() === confirmationPhrase(s.address, s.image?.custom) };
}

/** 0..1 over the data blocks; erasing counts as start, everything after the last block as done. */
export function progressFraction(p: FirmwareProgress | null): number {
  if (!p) return 0;
  if (p.stage === "block" && p.block !== null && p.total) return p.block / p.total;
  return ["finalize", "app_restart", "post_check"].includes(p.stage) ? 1 : 0;
}
