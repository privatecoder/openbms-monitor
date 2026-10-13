import { useTranslation } from "react-i18next";
import { TriangleAlert, Wrench } from "lucide-react";
import type { Endpoint, MaintOverview } from "../api";
import { bannerOf } from "../maint";
import { Button } from "../components/ui/button";
import { cn } from "../lib/utils";

export function MaintBanner({ overview, unassigned, other, onAssign, onSwitch, onEnd }: {
  overview: MaintOverview | null;
  unassigned: Endpoint | null;
  other: { id: string; name: string } | null;
  onAssign: () => void;
  onSwitch: (id: string) => void;
  onEnd: () => void;
}) {
  const { t } = useTranslation();
  const b = bannerOf(overview);
  const assign = overview && (unassigned || other) && (
    <div role="status" className="mb-6 flex items-center gap-3 rounded-md border border-discharge p-3 text-sm">
      {unassigned && <>
        <p className="flex-1">{t("maint.banner.unassigned", { endpoint: unassigned.kind === "tcp" ? unassigned.addr : unassigned.path, name: overview.installation.name })}</p>
        <Button variant="ghost" size="sm" onClick={onAssign}>{t("maint.banner.assign")}</Button>
      </>}
      {other && <>
        <p className="flex-1">{t("maint.banner.other", { other: other.name, name: overview.installation.name })}</p>
        <Button variant="ghost" size="sm" onClick={() => onSwitch(other.id)}>{t("maint.banner.switch", { other: other.name })}</Button>
      </>}
    </div>
  );
  if (!b) return assign || null;
  const since = b.since ? new Date(b.since).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : null;
  return (
    <>
    {assign}
    <div role="status" className={cn("mb-6 flex items-start gap-3 rounded-md border p-3 text-sm", b.kind === "open" ? "border-discharge" : "border-alarm text-alarm")}>
      {b.kind === "open" ? <Wrench className="mt-0.5 h-4 w-4 shrink-0" /> : <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />}
      <div className="flex-1 space-y-1">
        <p className="font-medium">{t(`maint.banner.${b.kind}`, { since: since ?? "?", how: t(`maint.how.${b.how}`) })}</p>
        <p className="text-muted">{t(b.kind === "unexplained" ? "maint.banner.unexplainedHint" : "maint.banner.hint")}</p>
        {overview?.session?.failing && <p className="text-alarm">{t("maint.banner.failing", { what: overview.session.failing })}</p>}
      </div>
      {b.end && <Button variant="ghost" size="sm" onClick={onEnd}>{t("maint.end")}</Button>}
    </div>
    </>
  );
}
