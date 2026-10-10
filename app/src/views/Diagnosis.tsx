import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Gauge, RotateCcw, Trash2 } from "lucide-react";
import { effectiveCurrent } from "../api";
import type { PackEntry } from "../store";
import { useDiag } from "../diag/store";
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
  const d = useDiag(site, packs);
  const { pairs } = usePairs(site);
  const [busRest, setBusRest] = useState("");
  const [busNow, setBusNow] = useState("");
  const list = Object.values(packs).filter((p) => p.telemetry).sort((a, b) => a.address - b.address);
  const maxAbs = Math.max(0, ...list.map((p) => Math.abs(p.telemetry!.current)));
  const atRest = list.length > 0 && maxAbs < REST_CURRENT_A;
  const loaded = list.filter((p) => Math.abs(p.telemetry!.current) >= MIN_CURRENT_A).length;
  const last = d.snapshots[0];
  const innerFlags = flagHigh(d.inner);
  const outerFlags = flagHigh(Object.fromEntries(Object.entries(last?.rows ?? {}).map(([a, r]) => [a, r.outer])));
  const time = (ms: number) => new Date(ms).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" });
  const pairRows = checkPairs(validPairs(pairs, list.map((p) => p.address)), (a) => (packs[a]?.telemetry ? effectiveCurrent(packs[a].telemetry!) : undefined));
  const mOhm = (v: number | undefined) => (v === undefined ? "–" : fmt(v, 1, "mΩ"));

  return (
    <div className="view-in mx-auto max-w-6xl space-y-6">
      <header>
        <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight"><Gauge className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("diag.title")}<InfoIcon id="topic.diagnosis" /></h1>
        <p className="mt-1 max-w-3xl text-muted">{t("diag.intro")}</p>
      </header>

      <div className="grid gap-5 md:grid-cols-2">
        <Panel className="space-y-3 p-5">
          <h2 className="font-display text-xl font-semibold">{t("diag.step1")}</h2>
          <p className="text-sm text-muted">{t("diag.step1Text")}</p>
          <label className="flex flex-wrap items-center gap-2 text-sm">
            <span className="text-muted">{t("diag.busRest")}</span>
            <input className={input} inputMode="decimal" value={busRest} onChange={(e) => setBusRest(e.target.value)} placeholder="53,20" />
            <span className="text-muted">V</span>
          </label>
          <div className="flex flex-wrap items-center gap-3">
            <Button variant="primary" disabled={!atRest} onClick={() => d.zero(num(busRest))}><RotateCcw className="h-4 w-4" />{t("diag.zero")}</Button>
            <span className={cn("text-sm", atRest ? "text-ok" : "text-muted")}>
              {atRest ? t("diag.atRest") : t("diag.notAtRest", { a: fmt(maxAbs, 1, "A"), max: fmt(REST_CURRENT_A, 1, "A") })}
            </span>
          </div>
          <p className="text-xs text-muted">
            {d.zeroAt ? t("diag.zeroDone", { at: time(d.zeroAt), bus: d.busRest !== undefined ? fmt(d.busRest, 3, "V") : t("diag.noBus") }) : t("diag.zeroMissing")}
          </p>
        </Panel>

        <Panel className="space-y-3 p-5">
          <h2 className="font-display text-xl font-semibold">{t("diag.step2")}</h2>
          <p className="text-sm text-muted">{t("diag.step2Text", { a: MIN_CURRENT_A })}</p>
          <label className="flex flex-wrap items-center gap-2 text-sm">
            <span className="text-muted">{t("diag.busNow")}</span>
            <input className={input} inputMode="decimal" value={busNow} onChange={(e) => setBusNow(e.target.value)} placeholder="53,30" />
            <span className="text-muted">V</span>
          </label>
          <div className="flex flex-wrap items-center gap-3">
            <Button variant="primary" disabled={!d.zeroAt || d.busRest === undefined || num(busNow) === undefined || loaded === 0}
              onClick={() => { d.measure(num(busNow)!); setBusNow(""); }}>{t("diag.measure")}</Button>
            <span className="text-sm text-muted">{t("diag.loaded", { n: loaded, total: list.length })}</span>
          </div>
          {d.zeroAt && d.busRest === undefined && <p className="text-xs text-discharge">{t("diag.needBusRest")}</p>}
        </Panel>
      </div>

      <Panel className="overflow-x-auto p-5">
        <table className="w-full text-sm tabular-nums">
          <thead className="text-left text-muted">
            <tr className="border-b border-line">
              <th className="py-2 pr-4 font-normal">{t("dash.pack")}</th>
              <th className="py-2 pr-4 text-right font-normal">{t("dash.current")}</th>
              <th className="py-2 pr-4 text-right font-normal"><span className="inline-flex items-center gap-1.5">{t("diag.inner")}<InfoIcon id="topic.diagnosis" /></span></th>
              <th className="py-2 pr-4 text-right font-normal">{t("diag.outer")}{last && <span className="block text-xs">{time(last.at)}</span>}</th>
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
                <th className="py-1.5 pr-3 text-right font-normal">{t("diag.bus")}</th>
                {list.map((p) => <th key={p.address} className="py-1.5 pr-3 text-right font-normal">{packNo(p.address)}</th>)}
              </tr>
            </thead>
            <tbody>
              {d.snapshots.map((s) => {
                const f = flagHigh(Object.fromEntries(Object.entries(s.rows).map(([a, r]) => [a, r.outer])));
                return (
                  <tr key={s.at} className="border-b border-line last:border-b-0">
                    <td className="py-1.5 pr-3">{time(s.at)}</td>
                    <td className="py-1.5 pr-3 text-right">{fmt(s.bus, 3, "V")}</td>
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
