import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Route } from "lucide-react";
import { api, type AccessView, type Channel } from "../api";
import { CHANNELS, accessRows, bestState, cellTitle, needText, routeNeed, routeOf } from "../access";
import { usePackProbes } from "../packProbes";
import { useProfile } from "../profile";
import { cn } from "../lib/utils";
import { SectionHead } from "./Section";

const tone = { ready: "text-ok", unknown: "text-discharge", blocked: "text-alarm", "not-implemented": "text-muted" } as const;

/** „Zugriffswege“: which action goes over which UART for the chosen pack (`openbms_proto::access`), as this connection
 *  knows the pack. A preview: every write still reads the pack and checks it before it sends. */
export function AccessMatrix({ address, group0, group7, storedProof, linkKey }: {
  address: number | null; group0?: number; group7?: number;
  /** this link's read of the pack carried a stored proof (bound to the String 2 read then) */
  storedProof: boolean; linkKey: string;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<AccessView | null>(null);
  // the connection's own proofs: the master profile and the packs' 64/03; a change of them asks again
  const profile = useProfile();
  const [probes] = usePackProbes();
  const probe = address !== null ? probes[address] : undefined;
  // the whole finding: two builds of one version differ in their image flags
  const proofKey = `${profile?.gen}|${JSON.stringify(profile?.master ?? null)}|${JSON.stringify(probe ?? null)}`;
  useEffect(() => {
    // another pack, link or proof: the old view goes at once, not after the answer
    setView(null);
    if (!open || address === null) return;
    let alive = true;
    api.accessView(address, group0 ?? null, group7 ?? null, storedProof).then((v) => { if (alive) setView(v); }, () => {});
    return () => { alive = false; };
  }, [open, address, group0, group7, storedProof, linkKey, proofKey]);
  const rows = view ? accessRows(view.decisions) : [];
  const head = (c: Channel) => (
    <th key={c} className={cn("px-2 py-1 text-left font-medium", view?.channel === c && "text-charge")} title={view?.channel === c ? t("acc.thisLink") : undefined}>
      {t(`acc.channel.${c}`)}
    </th>
  );
  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={Route} title={t("acc.title")} help="topic.access" open={open} onToggle={() => setOpen((o) => !o)} closedHint={t("acc.closed")} />
      {open && (address === null ? <p className="text-muted">{t("acc.noPack")}</p> : (
        <>
          <p className="max-w-3xl text-muted">{t("acc.intro")}</p>
          <p className="max-w-3xl text-muted">{t("acc.needLegend")}</p>
          <div className="overflow-x-auto">
            <table className="text-xs">
              <thead><tr><th className="px-2 py-1 text-left font-medium">{t("acc.action")}</th>{CHANNELS.map(head)}</tr></thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.action} className="border-t border-line">
                    <td className="px-2 py-1 whitespace-nowrap">{t(`acc.act.${r.action}`)}</td>
                    {CHANNELS.map((c) => {
                      const ds = r.cells[c];
                      const s = ds && bestState(ds);
                      // the firmware the cell's ways need at least (the first way's; several ways of one cell need the same)
                      const route = ds && routeOf(ds[0].route);
                      const need = route ? needText(t, routeNeed(route)) || t(`acc.needOf.${r.action}`, { defaultValue: "" }) : "";
                      return (
                        <td key={c} className="px-2 py-1 whitespace-nowrap" title={ds ? cellTitle(t, ds) : undefined}>
                          {s ? <span className={tone[s]}>{t(`acc.state.${s}`)}</span> : <span className="text-muted">–</span>}
                          {need && <div className="text-[11px] text-muted">{need}</div>}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      ))}
    </div>
  );
}
