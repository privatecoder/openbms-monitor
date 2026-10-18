import { useEffect, useState } from "react";
import { api, type PackProbeView } from "./api";
import { useProfile } from "./profile";

/** The packs' own 64/03 findings of the connection, by address: loaded per connection and followed as they come (the
 *  first check runs with the serial reads after connecting). */
export function usePackProbes(): Record<number, PackProbeView> {
  const profile = useProfile();
  const [found, setFound] = useState<Record<number, PackProbeView>>({});
  useEffect(() => {
    let alive = true;
    setFound({});
    if (profile) api.packProbes().then((l) => { if (alive) setFound(Object.fromEntries(l.map((p) => [p.address, p]))); }, () => {});
    const u = api.onPackProbe((p) => setFound((s) => ({ ...s, [p.address]: p })));
    return () => { alive = false; u.then((f) => f()); };
  }, [profile?.gen]); // eslint-disable-line react-hooks/exhaustive-deps
  return found;
}

type T = (key: string, params?: Record<string, unknown>) => string;

/** The pack view's line for a slave's own 64/03: „noch ungeprüft“ until it is known. */
export function packProbeLine(p: PackProbeView | undefined, t: T): string {
  if (!p) return t("probe.pending");
  if (p.state === "proven") return t("probe.proven", { version: p.listed ?? p.version ?? "", build: p.build_id ?? "" });
  if (p.state === "negative") return t("probe.negative", { version: p.version ?? "MNT" });
  return t("probe.undecided");
}
