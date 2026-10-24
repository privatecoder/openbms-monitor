import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { HardDrive } from "lucide-react";
import { api, type DiagCompare, type DiagRecord, type MemoryDiagnosis } from "../api";
import { errorText } from "../control";
import { useImageFlags } from "../imageFlags";
import { Button } from "./ui/button";
import { SectionHead, StatusLine } from "./Section";

/** The image flags of the stored pack lock (bit 2) and of the diagnosis (bit 5) */
const CUTOFF = 0x04;
const DIAGNOSIS = 0x20;

/** „Speicherdiagnose“ (0x64/04, MAINT 1.5.3/1.6.3): asked on demand, never in the background; changes nothing. Shown
 *  where the pack's image flags (this connection's, else its own 64/03) announce it (bit 5). The lock fields as
 *  supported where the flags say the build stores a pack lock (bit 2), „not supported“ where they say it does not; the
 *  cache only with its identifier (bit 2 of `cache`). An answer belongs to the pack and link it was asked for. */
export function DiagnosisLine({ address, known, connected, locked, linkKey }: { address: number | null; known: number | null | undefined; connected: boolean; locked?: boolean; linkKey: string }) {
  const { t } = useTranslation();
  const flags = useImageFlags(connected ? address : null, known, linkKey);
  const [d, setD] = useState<MemoryDiagnosis | { error: string } | null>(null);
  const [busy, setBusy] = useState(false);
  // the request generation: a new pack or link drops whatever answer is still on its way
  const gen = useRef(0);
  useEffect(() => { gen.current += 1; setD(null); setBusy(false); }, [address, linkKey]);
  if (address === null || !connected || flags === null || (flags & DIAGNOSIS) === 0) return null;
  const ask = async () => {
    const mine = ++gen.current;
    setBusy(true); setD(null);
    try { const r = await api.packDiagnosis(address); if (gen.current === mine) setD(r); }
    catch (e) { if (gen.current === mine) setD({ error: errorText(e, t) }); }
    finally { if (gen.current === mine) setBusy(false); }
  };
  const rec = (r: DiagRecord) => t(`mdiag.rec.${r}`);
  const cmp = (c: DiagCompare) => t(`mdiag.cmp.${c}`);
  const bad = (x: MemoryDiagnosis) => x.params.includes("invalid") || x.soc.includes("invalid") || x.params_compare === "different" || x.soc_compare === "different";
  const lockText = (l: boolean | null) => t(l === null ? "mdiag.lockUnknown" : l ? "mdiag.locked" : "mdiag.unlocked");
  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={HardDrive} title={t("mdiag.title")} help="topic.memoryDiagnosis" />
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <Button size="sm" disabled={busy || locked} onClick={ask}>{busy ? t("mdiag.reading") : t("mdiag.read")}</Button>
      </div>
      {d && "error" in d && <StatusLine tone="error">{d.error}</StatusLine>}
      {d && !("error" in d) && (
        <>
          <StatusLine tone={bad(d) ? "warn" : "ok"}>{t("mdiag.params", { a: rec(d.params[0]), b: rec(d.params[1]), cmp: cmp(d.params_compare) })}</StatusLine>
          <StatusLine tone={bad(d) ? "warn" : "ok"}>{t("mdiag.soc", { a: rec(d.soc[0]), b: rec(d.soc[1]), cmp: cmp(d.soc_compare) })}</StatusLine>
          <StatusLine>{(flags & CUTOFF) !== 0
            ? `${t("mdiag.lock", { a: lockText(d.lock[0]), b: lockText(d.lock[1]) })} ${(d.cache & 4) !== 0 ? t("mdiag.cache", { latch: t((d.cache & 1) ? "mdiag.latchOn" : "mdiag.latchOff") }) : t("mdiag.cacheUnconfirmed")}`
            : t("mdiag.noLock")}</StatusLine>
        </>
      )}
      <StatusLine>{t("mdiag.hint")}</StatusLine>
    </div>
  );
}
