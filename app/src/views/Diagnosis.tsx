import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Gauge, RotateCcw, Trash2 } from "lucide-react";
import { effectiveCurrent } from "../api";
import type { PackEntry } from "../store";
import { useDiag, type Refs } from "../diag/store";
import type { RefPoint } from "../diag/calc";
import { MIN_CURRENT_A, REST_CURRENT_A, flagHigh } from "../diag/calc";
import { checkPairs, usePairs, validPairs } from "../pairs";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";

const input = "h-9 w-28 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";
const packNo = (a: number) => String(a).padStart(2, "0");
const num = (s: string) => (s.trim() === "" ? undefined : Number(s.replace(",", ".")));

/** Resistance diagnosis: zero at rest, live inner resistance, outer resistance against the busbar, pair currents. */
export function Diagnosis({ packs, site }: { packs: Record<number, PackEntry>; site: string }) {
  const { t, i18n } = useTranslation();
  const { pairs: allPairs } = usePairs(site);
  const [point, setPoint] = useState<RefPoint>("bus");
  const [restIn, setRestIn] = useState<Record<string, string>>({});
  const [nowIn, setNowIn] = useState<Record<string, string>>({});
  const list = Object.values(packs).filter((p) => p.telemetry).sort((a, b) => a.address - b.address);
  const pairs = validPairs(allPairs, list.map((p) => p.address));
  const d = useDiag(site, packs, pairs);
  // input keys: "bus" or the plus pack of a pair
  const refsOf = (vals: Record<string, string>): Refs => ({ bus: num(vals.bus ?? ""), perPair: Object.fromEntries(pairs.map((p) => [p.plus, num(vals[p.plus] ?? "")])) });
  const restRefs = refsOf(restIn), nowRefs = refsOf(nowIn);
  const nowComplete = point === "bus" ? nowRefs.bus !== undefined : pairs.some((p) => nowRefs.perPair[p.plus] !== undefined);
  // a plain function, not a component: a component defined here would remount on every keystroke and drop focus
  const refInputs = (vals: Record<string, string>, set: (f: (v: Record<string, string>) => Record<string, string>) => void, placeholder: string) =>
    point === "bus" ? (
      <label className="flex flex-wrap items-center gap-2 text-sm">
        <span className="text-muted">{t("diag.refBus")}</span>
        <input className={input} inputMode="decimal" value={vals.bus ?? ""} onChange={(e) => set((v) => ({ ...v, bus: e.target.value }))} placeholder={placeholder} />
        <span className="text-muted">V</span>
      </label>
    ) : (
      <div className="grid grid-cols-[auto_7rem_auto] items-center justify-start gap-x-2 gap-y-1.5 text-sm">
        {pairs.map((p) => (
          <label key={p.plus} className="contents">
            <span className="text-muted">{t("diag.refSwitch", { a: packNo(p.plus), b: packNo(p.minus) })}</span>
            <input className={input} inputMode="decimal" value={vals[p.plus] ?? ""} onChange={(e) => set((v) => ({ ...v, [p.plus]: e.target.value }))} placeholder={placeholder} />
            <span className="text-muted">V</span>
          </label>
        ))}
        {pairs.length === 0 && <span className="col-span-3 text-discharge">{t("diag.needPairs")}</span>}
      </div>
    );
  const maxAbs = Math.max(0, ...list.map((p) => Math.abs(p.telemetry!.current)));
  const atRest = list.length > 0 && maxAbs < REST_CURRENT_A;
  const loaded = list.filter((p) => Math.abs(p.telemetry!.current) >= MIN_CURRENT_A).length;
  const last = d.snapshots[0];
  const innerFlags = flagHigh(d.inner);
  const outerFlags = flagHigh(Object.fromEntries(Object.entries(last?.rows ?? {}).map(([a, r]) => [a, r.outer])));
  const time = (ms: number) => new Date(ms).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" });
  const pairRows = checkPairs(pairs, (a) => (packs[a]?.telemetry ? effectiveCurrent(packs[a].telemetry!) : undefined));
  const mOhm = (v: number | undefined) => (v === undefined ? "–" : fmt(v, 1, "mΩ"));

  return (
    <div className="view-in mx-auto max-w-6xl space-y-6">
      <header>
        <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight"><Gauge className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("diag.title")}<InfoIcon id="topic.diagnosis" /></h1>
        <p className="mt-1 max-w-3xl text-muted">{t("diag.intro")}</p>
        <div className="mt-3 flex flex-wrap items-center gap-2 text-sm">
          <span className="text-muted">{t("diag.refPoint")}</span>
          <div className="flex rounded-md bg-sunken p-0.5" role="group">
            {(["bus", "switch"] as const).map((k) => (
              <button key={k} aria-pressed={point === k} onClick={() => setPoint(k)}
                className={cn("rounded px-3 py-1", point === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>{t(`diag.point.${k}`)}</button>
            ))}
          </div>
        </div>
      </header>

      <div className="grid gap-5 md:grid-cols-2">
        <Panel className="space-y-3 p-5">
          <h2 className="font-display text-xl font-semibold">{t("diag.step1")}</h2>
          <p className="text-sm text-muted">{t("diag.step1Text")}</p>
          {refInputs(restIn, setRestIn, "53,20")}
          <div className="flex flex-wrap items-center gap-3">
            <Button variant="primary" disabled={!atRest} onClick={() => d.zero(restRefs)}><RotateCcw className="h-4 w-4" />{t("diag.zero")}</Button>
            <span className={cn("text-sm", atRest ? "text-ok" : "text-muted")}>
              {atRest ? t("diag.atRest") : t("diag.notAtRest", { a: fmt(maxAbs, 1, "A"), max: fmt(REST_CURRENT_A, 1, "A") })}
            </span>
          </div>
          <p className="text-xs text-muted">
            {d.zeroAt ? t("diag.zeroDone", { at: time(d.zeroAt), refs: [d.hasOuter("bus") && t("diag.point.bus"), d.hasOuter("switch") && t("diag.point.switch")].filter(Boolean).join(", ") || t("diag.noBus") }) : t("diag.zeroMissing")}
          </p>
        </Panel>

        <Panel className="space-y-3 p-5">
          <h2 className="font-display text-xl font-semibold">{t("diag.step2")}</h2>
          <p className="text-sm text-muted">{t("diag.step2Text", { a: MIN_CURRENT_A })}</p>
          {refInputs(nowIn, setNowIn, "53,30")}
          <div className="flex flex-wrap items-center gap-3">
            <Button variant="primary" disabled={!d.zeroAt || !d.hasOuter(point) || !nowComplete || loaded === 0}
              onClick={() => { d.measure(point, nowRefs); setNowIn({}); }}>{t("diag.measure")}</Button>
            <span className="text-sm text-muted">{t("diag.loaded", { n: loaded, total: list.length })}</span>
          </div>
          {d.zeroAt && !d.hasOuter(point) && <p className="text-xs text-discharge">{t("diag.needBusRest")}</p>}
        </Panel>
      </div>

      <Panel className="overflow-x-auto p-5">
        <table className="w-full text-sm tabular-nums">
          <thead className="text-left text-muted">
            <tr className="border-b border-line">
              <th className="py-2 pr-4 font-normal">{t("dash.pack")}</th>
              <th className="py-2 pr-4 text-right font-normal">{t("dash.current")}</th>
              <th className="py-2 pr-4 text-right font-normal"><span className="inline-flex items-center gap-1.5">{t("diag.inner")}<InfoIcon id="topic.diagnosis" /></span></th>
              <th className="py-2 pr-4 text-right font-normal">{t("diag.outer")}{last && <span className="block text-xs">{t(`diag.point.${last.point}`)}, {time(last.at)}</span>}</th>
              <th className="py-2 font-normal">{t("diag.rating")}</th>
            </tr>
          </thead>
          <tbody>
            {list.map((p) => {
              const out = last?.rows[p.address]?.outer;
              const flags = [innerFlags.has(p.address) && t("diag.flagInner"), outerFlags.has(p.address) && t("diag.flagOuter")].filter(Boolean);
              return (
                <tr key={p.address} className="border-b border-line last:border-b-0">
                  <td className="py-2 pr-4 font-display text-lg font-semibold">{packNo(p.address)}</td>
                  <td className="py-2 pr-4 text-right">{fmt(effectiveCurrent(p.telemetry!), 2, "A")}</td>
                  <td className={cn("py-2 pr-4 text-right", innerFlags.has(p.address) && "font-medium text-alarm")}>{d.zeroAt ? mOhm(d.inner[p.address]) : "–"}</td>
                  <td className={cn("py-2 pr-4 text-right", outerFlags.has(p.address) && "font-medium text-alarm")}>{mOhm(out)}</td>
                  <td className={cn("py-2", flags.length ? "text-alarm" : "text-muted")}>{flags.length ? flags.join(", ") : d.zeroAt ? t("diag.ok") : ""}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <p className="mt-3 text-xs text-muted">{t("diag.ruleNote")}</p>
      </Panel>

      {pairRows.length > 0 && (
        <Panel className="overflow-x-auto p-5">
          <h2 className="mb-2 flex items-center gap-1.5 font-display text-xl font-semibold">{t("diag.pairs")}<InfoIcon id="topic.pairs" /></h2>
          <table className="w-full text-sm tabular-nums">
            <thead className="text-left text-muted">
              <tr className="border-b border-line">
                <th className="py-2 pr-4 font-normal">{t("diag.pair")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("diag.currents")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("diag.sum")}</th>
                <th className="py-2 font-normal">{t("diag.rating")}</th>
              </tr>
            </thead>
            <tbody>
              {pairRows.map((r) => {
                const cur = (a: number) => fmt(effectiveCurrent(packs[a].telemetry!), 1, "A");
                const notes = [r.lowTotal && t("diag.pairLow"), r.weak !== undefined && t("diag.pairWeak", { p: packNo(r.weak), bridge: r.weak === r.pair.minus ? "+" : "−" })].filter(Boolean);
                return (
                  <tr key={r.pair.plus} className="border-b border-line last:border-b-0">
                    <td className="py-2 pr-4 font-medium">{packNo(r.pair.plus)} / {packNo(r.pair.minus)}</td>
                    <td className="py-2 pr-4 text-right">{cur(r.pair.plus)} / {cur(r.pair.minus)}</td>
                    <td className={cn("py-2 pr-4 text-right", r.lowTotal && "font-medium text-alarm")}>{fmt(r.total, 1, "A")}</td>
                    <td className={cn("py-2", notes.length ? "text-alarm" : "text-muted")}>{notes.length ? notes.join("; ") : t("diag.ok")}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Panel>
      )}
      {pairRows.length === 0 && pairs.length === 0 && <p className="text-sm text-muted">{t("diag.noPairs")}</p>}

      {d.snapshots.length > 0 && (
        <Panel className="overflow-x-auto p-5">
          <div className="mb-2 flex items-center justify-between">
            <h2 className="font-display text-xl font-semibold">{t("diag.history")}</h2>
            <Button size="sm" variant="ghost" onClick={d.clearHistory}><Trash2 className="h-4 w-4" />{t("diag.clear")}</Button>
          </div>
          <table className="w-full text-xs tabular-nums">
            <thead className="text-muted">
              <tr className="border-b border-line">
                <th className="py-1.5 pr-3 text-left font-normal">{t("diag.when")}</th>
                <th className="py-1.5 pr-3 text-left font-normal">{t("diag.refPoint")}</th>
                {list.map((p) => <th key={p.address} className="py-1.5 pr-3 text-right font-normal">{packNo(p.address)}</th>)}
              </tr>
            </thead>
            <tbody>
              {d.snapshots.map((s) => {
                const f = flagHigh(Object.fromEntries(Object.entries(s.rows).map(([a, r]) => [a, r.outer])));
                return (
                  <tr key={s.at} className="border-b border-line last:border-b-0">
                    <td className="py-1.5 pr-3">{time(s.at)}</td>
                    <td className="py-1.5 pr-3">{t(`diag.point.${s.point}`)}</td>
                    {list.map((p) => <td key={p.address} className={cn("py-1.5 pr-3 text-right", f.has(p.address) && "font-medium text-alarm")}>{s.rows[p.address]?.outer === undefined ? "–" : fmt(s.rows[p.address].outer!, 1)}</td>)}
                  </tr>
                );
              })}
            </tbody>
          </table>
          <p className="mt-2 text-xs text-muted">{t("diag.historyNote")}</p>
        </Panel>
      )}
    </div>
  );
}
