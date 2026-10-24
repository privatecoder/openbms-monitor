import { useEffect, useState } from "react";
import { api } from "./api";

/** The image flags of a pack from its own 0x64/03 on the current link, asked once per link and address when nothing on
 *  this connection knows them yet (on RS485-1/2 the firmware profile has none). Shared by the lines that need them. */
const asked = new Map<string, Promise<number | null>>();

export function useImageFlags(address: number | null, known: number | null | undefined, linkKey: string): number | null {
  const [flags, setFlags] = useState<number | null>(known ?? null);
  useEffect(() => {
    if (address === null) { setFlags(null); return; }
    if (known != null) { setFlags(known); return; }
    setFlags(null);
    const key = `${linkKey}:${address}`;
    let p = asked.get(key);
    if (!p) { p = api.packImageFlags(address).catch(() => null); asked.set(key, p); }
    let alive = true;
    p.then((f) => { if (alive) setFlags(f); });
    return () => { alive = false; };
  }, [address, known, linkKey]);
  return flags;
}
