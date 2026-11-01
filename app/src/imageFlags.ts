import { useEffect, useState } from "react";
import { api, type ImageBits } from "./api";

/** The image flags and read capabilities of a pack from its own 0x64/03 on the current link, asked once per link and
 *  address when nothing on this connection knows them yet (on RS485-1/2 the firmware profile has none). Shared by the
 *  lines that need them. */
const asked = new Map<string, Promise<ImageBits | null>>();

function useBits<T>(address: number | null, known: T | null | undefined, linkKey: string, pick: (b: ImageBits) => T): T | null {
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
  }, [address, known, linkKey]);
  return value;
}

export const useImageFlags = (address: number | null, known: number | null | undefined, linkKey: string): number | null =>
  useBits(address, known, linkKey, (b) => b.flags);

/** The read-capability mask (MNT format 5: history over the CAN socket, calibration, stored SOC, energy; 0 before). */
export const useReadCaps = (address: number | null, known: number | null | undefined, linkKey: string): number | null =>
  useBits(address, known, linkKey, (b) => b.read_caps);
