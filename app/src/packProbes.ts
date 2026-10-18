import { useEffect, useState } from "react";
import { api, type PackProbeView } from "./api";
import { useProfile } from "./profile";

/** The packs' own 64/03 findings of the connection, by address: loaded per connection and followed as they come (the
 *  first check runs with the serial reads after connecting). Only findings of the current connection (`gen`) are taken;
 *  `put` takes a finding directly (the answer of „Erneut prüfen“). */
export function usePackProbes(): [Record<number, PackProbeView>, (p: PackProbeView) => void] {
  const profile = useProfile();
  const gen = profile?.gen;
  const [found, setFound] = useState<Record<number, PackProbeView>>({});
  const put = (p: PackProbeView) => { if (probeForLink(p, gen)) setFound((s) => ({ ...s, [p.address]: p })); };
  useEffect(() => {
    let alive = true;
    setFound({});
    const take = (p: PackProbeView) => { if (alive && probeForLink(p, gen)) setFound((s) => ({ ...s, [p.address]: p })); };
    if (gen !== undefined) api.packProbes().then((l) => l.forEach(take), () => {});
    const u = api.onPackProbe(take);
    return () => { alive = false; u.then((f) => f()); };
  }, [gen]);
  return [found, put];
}

/** Whether a finding may be shown on the connection `gen` (an answer of an earlier connection never). */
export const probeForLink = (p: PackProbeView, gen: number | undefined) => gen !== undefined && p.gen === gen;

type T = (key: string, params?: Record<string, unknown>) => string;

/** The pack view's line for a slave's own 64/03: „noch ungeprüft“ until it is known. */
export function packProbeLine(p: PackProbeView | undefined, t: T): string {
  if (!p) return t("probe.pending");
  if (p.state === "proven") return t("probe.proven", { version: p.listed ?? p.version ?? "", build: p.build_id ?? "" });
  if (p.state === "negative") return t("probe.negative", { version: p.version ?? "MNT" });
  return t("probe.undecided");
}
