import { useState } from "react";
import { useTranslation } from "react-i18next";
import { TriangleAlert, Wrench } from "lucide-react";
import { api, type Alerts, type Endpoint } from "../api";
import { errorKey } from "../maint";
import { Button } from "../components/ui/button";

/** „14:02“ today, else with the date: a record can be days old. */
function when(at: number): string {
  const d = new Date(at);
  const today = new Date().toDateString() === d.toDateString();
  return today ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : d.toLocaleString([], { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
}

const endpointText = (key: string) => key.replace(/^(serial|tcp):/, "");

/** Above every view: a master whose marker is on (until a read shows it off) and packs that may hang in their
 *  bootloader (until they answer again or the user confirms). Hidden during a flash: its own record is written
 *  before the restart. */
export function MaintBanner({ alerts, failed, flashing, unassigned, shownName, onAssign, onShow, onChanged }: {
  alerts: Alerts;
  /** the last load failed: the last known lines stay, with a note */
  failed: boolean;
  flashing: boolean;
  unassigned: Endpoint | null;
  /** the installation shown (for the assign offer) */
  shownName: string | null;
  onAssign: () => void;
  /** open the firmware view with this installation */
  onShow: (installation: string) => void;
  /** a release or confirmation changed the alerts */
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [error, setError] = useState<{ at: string; text: string } | null>(null);
  const act = async (at: string, f: () => Promise<unknown>) => {
    setBusy(at); setError(null);
    try { await f(); setConfirming(null); } catch (e) { const k = errorKey(e); setError({ at, text: t(k.key, k.params) }); }
    finally { setBusy(null); onChanged(); }
  };
  const err = (at: string) => error?.at === at && <p className="text-alarm">{error.text}</p>;
  const pending = flashing ? [] : alerts.pending;
  if (!alerts.markers.length && !pending.length && !failed && !(unassigned && shownName)) return null;
  return (
    <div className="mb-6 space-y-2">
      {unassigned && shownName && (
        <div role="status" className="flex items-center gap-3 rounded-md border border-discharge p-3 text-sm">
          <p className="flex-1">{t("maint.banner.unassigned", { endpoint: unassigned.kind === "tcp" ? unassigned.addr : unassigned.path, name: shownName })}</p>
          <Button variant="secondary" size="sm" onClick={onAssign}>{t("maint.banner.assign")}</Button>
        </div>
      )}
      {failed && <p role="alert" className="flex items-center gap-2 rounded-md border border-alarm p-3 text-sm font-medium text-alarm"><TriangleAlert className="h-4 w-4 shrink-0" />{t("maint.banner.notLoaded")}</p>}
      {pending.map((p) => (
        <div key={p.id} role="alert" className="flex items-start gap-3 rounded-md border border-alarm p-3 text-sm text-alarm">
          <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="flex-1 space-y-1">
            <p className="font-medium">{t("maint.banner.pending", { pack: p.pack, endpoint: endpointText(p.endpoint), since: when(p.since) })}</p>
            <p className="text-muted">{t("maint.banner.pendingHint", { pack: p.pack })}</p>
            {confirming === p.id && (
              <div className="flex flex-wrap gap-2">
                <Button variant="primary" size="sm" disabled={busy !== null} onClick={() => act(`p${p.id}`, () => api.maintClearPending(p.id))}>{t("maint.banner.pendingYes", { pack: p.pack })}</Button>
                <Button variant="secondary" size="sm" onClick={() => setConfirming(null)}>{t("maint.setup.cancel")}</Button>
              </div>
            )}
            {err(`p${p.id}`)}
          </div>
          {confirming !== p.id && <Button variant="secondary" size="sm" className="shrink-0" onClick={() => setConfirming(p.id)}>{t("maint.banner.pendingConfirm", { pack: p.pack })}</Button>}
        </div>
      ))}
      {alerts.markers.map((m) => (
        <div key={m.installation} role="status" className="flex items-start gap-3 rounded-md border border-discharge p-3 text-sm">
          <Wrench className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="flex-1 space-y-1">
            <p className="font-medium">{t("maint.banner.marker", { name: m.name, since: when(m.at) })}</p>
            <p className="text-muted">{t("maint.banner.markerHint")}</p>
            {err(`m${m.installation}`)}
          </div>
          <div className="flex shrink-0 gap-2">
            <Button variant="secondary" size="sm" onClick={() => onShow(m.installation)}>{t("maint.banner.show")}</Button>
            <Button variant="secondary" size="sm" disabled={busy !== null || flashing} onClick={() => act(`m${m.installation}`, () => api.maintRelease(m.installation))}>{t("maint.release")}</Button>
          </div>
        </div>
      ))}
    </div>
  );
}
