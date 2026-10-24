import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { HardDrive } from "lucide-react";
import { api, type DiagCompare, type DiagRecord, type MemoryDiagnosis } from "../api";
import { errorText } from "../control";
import { Button } from "./ui/button";
import { SectionHead, StatusLine } from "./Section";

/** The image flags that carry the stored pack lock (bit 2) */
const CUTOFF = 0x04;

/** „Speicherdiagnose“ (0x64/04, MAINT 1.5.2/1.6.3): asked on demand, never in the background; changes nothing. The lock
 *  fields only with a build that stores a pack lock (`flags` bit 2), the cache only with its identifier (bit 2 of
 *  `cache`). */
export function DiagnosisLine({ address, usable, flags, locked }: { address: number | null; usable: boolean; flags: number | null | undefined; locked?: boolean }) {
  const { t } = useTranslation();
  const [d, setD] = useState<MemoryDiagnosis | { error: string } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => setD(null), [address]);
  if (address === null || !usable) return null;
  const ask = async () => {
    const a = address;
    setBusy(true);
    try { const r = await api.packDiagnosis(a); if (a === address) setD(r); }
    catch (e) { setD({ error: errorText(e, t) }); }
    finally { setBusy(false); }
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
          <StatusLine>{flags != null && (flags & CUTOFF) !== 0
            ? `${t("mdiag.lock", { a: lockText(d.lock[0]), b: lockText(d.lock[1]) })} ${(d.cache & 4) !== 0 ? t("mdiag.cache", { latch: t((d.cache & 1) ? "mdiag.latchOn" : "mdiag.latchOff") }) : t("mdiag.cacheUnconfirmed")}`
            : t("mdiag.noLock")}</StatusLine>
        </>
      )}
      <StatusLine>{t("mdiag.hint")}</StatusLine>
    </div>
  );
}
