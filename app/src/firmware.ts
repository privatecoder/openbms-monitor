import type { DeviceInfo, FirmwareImage, FirmwareProgress } from "./api";

export const confirmationPhrase = (address: number) => `FLASH ${address}`;

export interface FwSetup {
  running: boolean;
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
  if (s.running) return { image: false, inspect: false, confirm: false, start: false };
  const inspect = s.port !== "" && s.image !== null;
  const confirm = inspect && s.target !== null && s.checkedFor?.port === s.port && s.checkedFor.address === s.address;
  return { image: true, inspect, confirm, start: confirm && s.typed.trim() === confirmationPhrase(s.address) };
}

/** 0..1 over the data blocks; erasing counts as start, everything after the last block as done. */
export function progressFraction(p: FirmwareProgress | null): number {
  if (!p) return 0;
  if (p.stage === "block" && p.block !== null && p.total) return p.block / p.total;
  return ["finalize", "app_restart", "post_check"].includes(p.stage) ? 1 : 0;
}
