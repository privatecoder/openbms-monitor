import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Scale } from "lucide-react";
import { api, isDemo, type BalancerSlice, type BalancerTimeline as Timeline } from "../api";
import { InfoIcon } from "../help";
import { useSettings } from "../installData";
import { cn } from "../lib/utils";
import { bmsMark, extMark, shares, type LaneMark } from "../balancerTimeline";
import { sharePct } from "../timeAxis";
import { TimeAxis } from "./TimeAxis";

const DAY = 86_400_000;
const SLICES = 240;

const COLOR: Record<LaneMark["state"], string> = {
  active: "bg-charge", bal: "bg-charge", unknown: "bg-discharge", inactive: "bg-line", idle: "bg-line", none: "bg-sunken", noStatus: "bg-sunken", gap: "",
};

/** Balancing over time in the pack details: the external balancer (computed from the cells with its settings of the
 *  time, never measured while it has no connection profile) and the BMS's own balancing (reported bits). From the
 *  recording of the chosen installation (`balancer_timeline`), over the span the cell analysis above it shows
 *  (`CellAnalysisPanel` owns the period, so both line up). */
export function BalancerTimeline({ pack, scope, from, to }: { pack: number; scope: string | null; from: number; to: number }) {
  const { t, i18n } = useTranslation();
  const { settings, loaded } = useSettings(scope);
  const [data, setData] = useState<Timeline | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const site = scope ?? (isDemo ? "demo" : null);
  const devicesKey = JSON.stringify(settings.devices ?? null);

  useEffect(() => {
    if (!site || !loaded) return;
    let alive = true;
    api.balancerTimeline(site, pack, from, to, settings.devices ?? null, SLICES)
      .then((d) => { if (alive) { setData(d); setErr(null); } })
      .catch((e) => alive && setErr(String(e)));
    return () => { alive = false; };
  }, [site, pack, from, to, loaded, devicesKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const pct = (v: number | null) => sharePct(v, i18n.language);
  const when = (ms: number) => new Date(ms).toLocaleString(i18n.language, to - from <= DAY ? { timeStyle: "short" } : { dateStyle: "short", timeStyle: "short" });
  const total = data?.total;
  const recorded = !!total && total.active + total.inactive + total.unknown + total.unassigned > 0;
  const assigned = !!total && total.active + total.inactive + total.unknown > 0;
  const s = total ? shares(total) : null;

  return (
    <section>
      <h3 className="mb-3 flex items-center gap-1.5 font-display text-lg font-semibold"><Scale className="h-4 w-4 text-muted" aria-hidden />{t("dev.tl.title")}<InfoIcon id="topic.balancers" /></h3>
      {!site ? <p className="text-sm text-muted">{t("dev.tl.needInst")}</p>
        : err ? <p className="text-sm text-alarm">{err}</p>
        : !data ? <p className="text-sm text-muted">{t("hist.loading")}</p>
        : !recorded ? <p className="text-sm text-muted">{t("dev.tl.none")}</p> : (
          <div className="space-y-3">
            <Lane label={t("dev.tl.ext")} note={data.measured ? null : t("dev.tl.computed")} empty={!assigned && data.first_assigned === null ? t("dev.tl.noBalancer") : null}
              slices={data.slices} mark={extMark} title={(sl, m) => `${when(sl.from)}–${when(sl.to)}: ${t(`dev.tl.legend.${m.state}`)}${m.state === "active" ? ` ${pct(m.share)}` : ""}`} />
            <Lane label={t("dev.tl.bms")} note={null} empty={null}
              slices={data.slices} mark={bmsMark}
              title={(sl, m) => `${when(sl.from)}–${when(sl.to)}: ${t(`dev.tl.legend.${m.state}`)}${m.state === "bal" ? ` ${pct(m.share)}${sl.cells ? `, ${t("dev.tl.cells", { count: sl.cells })}` : ""}` : ""}`} />
            <TimeAxis from={data.from} to={data.to} />
            <ul className="space-y-0.5 text-sm">
              {data.devices.length > 0 && <li className="text-muted">{t("dev.tl.device", { list: data.devices.join(", ") })}</li>}
              {assigned && <li>{t("dev.tl.share", { pct: pct(s!.active) })}{s!.unknown ? `; ${t("dev.tl.unknownShare", { pct: pct(s!.unknown) })}` : ""}</li>}
              {s!.bal !== null && <li>{t("dev.tl.shareBms", { pct: pct(s!.bal) })}{total!.cells ? `, ${t("dev.tl.cells", { count: total!.cells })}` : ""}</li>}
              {total!.minutes && <li className="text-muted">{t("dev.tl.minutes")}</li>}
              {!!s!.noStatus && <li className="text-muted">{t("dev.tl.noStatusShare", { pct: pct(s!.noStatus) })}</li>}
            </ul>
            <Legend />
          </div>
        )}
    </section>
  );
}

function Lane({ label, note, empty, slices, mark, title }: {
  label: string; note: string | null; empty: string | null; slices: BalancerSlice[]; mark: (s: BalancerSlice) => LaneMark; title: (s: BalancerSlice, m: LaneMark) => string;
}) {
  return (
    <div className="space-y-1">
      <p className="flex flex-wrap items-baseline gap-x-2 text-sm"><span className="font-medium">{label}</span>{note && <span className="text-xs text-muted">{note}</span>}</p>
      {empty ? <p className="flex h-6 items-center rounded bg-sunken px-2 text-xs text-muted">{empty}</p> : (
        <div className="no-record flex h-6 overflow-hidden rounded border border-line">
          {slices.map((sl, i) => {
            const m = mark(sl);
            // activity by its share, so a short run looks lighter than a full one
            const opacity = m.state === "active" || m.state === "bal" ? 0.35 + 0.65 * m.share : 1;
            // a recorded slice covers the hatch of „nicht aufgezeichnet“ behind the lane
            return (
              <div key={i} className={cn("h-full flex-1", m.state !== "gap" && "bg-surface")} title={title(sl, m)}>
                <div className={cn("h-full", COLOR[m.state])} style={{ opacity }} />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function Legend() {
  const { t } = useTranslation();
  const items: [string, string][] = [["bg-charge", "active"], ["bg-discharge", "unknown"], ["bg-line", "inactive"], ["bg-sunken", "noStatus"]];
  return (
    <p className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted">
      {items.map(([c, k]) => <span key={k} className="inline-flex items-center gap-1.5"><span className={cn("h-2.5 w-2.5 rounded-sm", c)} />{k === "active" ? `${t("dev.tl.legend.active")} / ${t("dev.tl.legend.bal")}` : k === "noStatus" ? `${t("dev.tl.legend.none")} / ${t("dev.tl.legend.noStatus")}` : t(`dev.tl.legend.${k}`)}</span>)}
      <span className="inline-flex items-center gap-1.5"><span className="no-record h-2.5 w-2.5 rounded-sm border border-line" />{t("dev.tl.legend.gap")}</span>
    </p>
  );
}
