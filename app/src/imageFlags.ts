import { useEffect, useState } from "react";
import { api, type ImageBits } from "./api";

/** The image flags and read capabilities of a pack from its own 0x64/03 on the current link, asked once per link and
 *  address when nothing on this connection knows them yet (on RS485-1/2 the firmware profile has none). Shared by the
 *  lines that need them. */
const asked = new Map<string, Promise<ImageBits | null>>();

/** Ask the pack's 64/03 again on a user's click (a pack that was silent the first time, e.g. a polling master on
 *  RS485-1/2): the cached answer of this link and address is replaced; nothing is written. */
export function askBitsAgain(address: number, linkKey: string): Promise<ImageBits | null> {
  const p = api.packImageFlags(address).catch(() => null);
  asked.set(`${linkKey}:${address}`, p);
  return p;
}

function useBits<T>(address: number | null, known: T | null | undefined, linkKey: string, pick: (b: ImageBits) => T, tick = 0): T | null {
  const [value, setValue] = useState<T | null>(known ?? null);
  useEffect(() => {
    if (address === null) { setValue(null); return; }
    if (known != null) { setValue(known); return; }
    setValue(null);
    const key = `${linkKey}:${address}`;
    let p = asked.get(key);
    if (!p) { p = api.packImageFlags(address).catch(() => null); asked.set(key, p); }
    let alive = true;
    p.then((b) => { if (alive) setValue(b ? pick(b) : null); });
    return () => { alive = false; };
    // `pick` is a fixed accessor per hook below
  }, [address, known, linkKey, tick]);
  return value;
}

export const useImageFlags = (address: number | null, known: number | null | undefined, linkKey: string): number | null =>
  useBits(address, known, linkKey, (b) => b.flags);

/** The read-capability mask (EXT05: history over the CAN socket, calibration, stored SOC, energy; 0: none). */
export const useReadCaps = (address: number | null, known: number | null | undefined, linkKey: string, tick = 0): number | null =>
  useBits(address, known, linkKey, (b) => b.read_caps, tick);
