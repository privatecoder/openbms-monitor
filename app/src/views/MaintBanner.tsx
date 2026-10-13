import { useTranslation } from "react-i18next";
import { TriangleAlert, Wrench } from "lucide-react";
import type { Endpoint, MaintOverview } from "../api";
import { failingText, type WorkLine } from "../maint";
import { Button } from "../components/ui/button";
import { cn } from "../lib/utils";

/** „14:02“ today, else with the date: a block can be days old. */
function when(at: number | null): string {
  if (at === null) return "?";
  const d = new Date(at);
  const today = new Date().toDateString() === d.toDateString();
  return today ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : d.toLocaleString([], { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
}

/** One line per installation with open work (`openWorkLines`), above every view. „Anzeigen“ always; „Beenden“ only
 *  where `line.end` allows it, otherwise the line says what must be cleared first. */
export function MaintBanner({ lines, failed, overview, unassigned, other, onAssign, onSwitch, onShow, onEnd, switchLocked }: {
  /** why „Anzeigen“ cannot switch now (an action, a flash or a switch runs); null: it can */
  switchLocked: string | null;
  lines: WorkLine[];
  /** the last load of the open work failed: the last known lines stay, with a prominent note */
  failed: boolean;
  overview: MaintOverview | null;
  unassigned: Endpoint | null;
  other: { id: string; name: string } | null;
  onAssign: () => void;
  onSwitch: (id: string) => void;
  /** switch to the installation (if not shown) and open its panel */
  onShow: (id: string) => void;
  onEnd: () => void;
}) {
  const { t } = useTranslation();
  const assign = overview && (unassigned || other) && (
    <div role="status" className="mb-6 flex items-center gap-3 rounded-md border border-discharge p-3 text-sm">
      {unassigned && <>
        <p className="flex-1">{t("maint.banner.unassigned", { endpoint: unassigned.kind === "tcp" ? unassigned.addr : unassigned.path, name: overview.installation.name })}</p>
        <Button variant="secondary" size="sm" onClick={onAssign}>{t("maint.banner.assign")}</Button>
      </>}
      {other && <>
        <p className="flex-1">{t("maint.banner.other", { other: other.name, name: overview.installation.name })}</p>
        <Button variant="secondary" size="sm" onClick={() => onSwitch(other.id)}>{t("maint.banner.switch", { other: other.name })}</Button>
      </>}
    </div>
  );
  if (!lines.length && !failed) return assign || null;
  const text = (l: WorkLine) => {
    const p = { name: l.name, since: when(l.at) };
    if (l.kind === "block" && l.block) return t(`maint.openWork.block.${l.block.reason}`, { ...p, pack: l.block.pack });
    if (l.kind === "session" && l.session) return t(l.unconfirmed ? "maint.openWork.unconfirmed" : "maint.openWork.session", { ...p, how: t(`maint.how.${l.session.kind}`) });
    return t(`maint.openWork.${l.kind}`, p);
  };
  return (
    <>
      {assign}
      <div className="mb-6 space-y-2">
        {failed && (
          <p role="alert" className="flex items-center gap-2 rounded-md border border-alarm p-3 text-sm font-medium text-alarm"><TriangleAlert className="h-4 w-4 shrink-0" />{t("maint.openWork.notLoaded")}</p>
        )}
        {lines.map((l) => {
          const alarm = l.kind === "block" || l.kind === "unexplained";
          return (
            <div key={l.id} role="status" className={cn("flex items-start gap-3 rounded-md border p-3 text-sm", alarm ? "border-alarm text-alarm" : "border-discharge")}>
              {alarm ? <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" /> : <Wrench className="mt-0.5 h-4 w-4 shrink-0" />}
              <div className="flex-1 space-y-1">
                <p className="font-medium">{text(l)}</p>
                {l.why && <p className="text-muted">{t(l.why)}</p>}
                {l.kind === "session" && l.shown && <p className="text-muted">{t("maint.banner.hint")}</p>}
                {l.session?.failing && <p className="text-alarm">{t("maint.banner.failing", { what: failingText(l.session.failing, t) })}</p>}
              </div>
              <div className="flex shrink-0 gap-2">
                {/* the shown installation needs no switch: its „Anzeigen“ only opens the panel */}
                <Button variant="secondary" size="sm" disabled={!l.shown && !!switchLocked} title={!l.shown && switchLocked ? switchLocked : undefined} onClick={() => onShow(l.id)}>{t("maint.openWork.show")}</Button>
                {l.end && <Button variant="secondary" size="sm" onClick={onEnd}>{t("maint.end")}</Button>}
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
}
