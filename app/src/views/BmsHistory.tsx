import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, History as HistoryIcon, X } from "lucide-react";
import { api, isDemo, type BmsHistoryRead, type Bus } from "../api";
import { CLOCK_FAR_S, csvName, currentRun, fmtBmsTime, historyCsv, lastRead, noteProgress, startHistoryRead, summary, type HistoryRun } from "../bmsMemory";
import { DiffText, readDiff, useBmsClock } from "../components/BmsClock";
import { errorText } from "../control";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { SevIcon } from "../components/widgets";
import { InfoIcon, helpTitle } from "../help";
import { cn, fmt } from "../lib/utils";

const no = (a: number) => String(a).padStart(2, "0");

// the chosen pack, kept while the app runs (the running read and the last result live in bmsMemory)
let keptAddress: number | null = null;

/** The pack's internal history (0x4B), read on demand over RS485-1/2: table newest first, CSV export. */
export function BmsHistory({ connected, bus, packs }: { connected: boolean; bus: Bus | null; packs: number[] }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [address, setAddressState] = useState<number | null>(currentRun()?.address ?? keptAddress ?? packs[0] ?? null);
  const setAddress = (a: number | null) => { keptAddress = a; setAddressState(a); };
  useEffect(() => { if (address === null || !packs.includes(address)) setAddress(packs[0] ?? null); }, [packs.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps
  const [result, setResult] = useState<BmsHistoryRead | null>(lastRead);
  const [progress, setProgress] = useState<number | null>(() => currentRun()?.n ?? null);
  const [saved, setSaved] = useState<{ name: string } | { error: string } | null>(null);
  const usable = connected && (bus === "pack" || isDemo);
  const { read: clock } = useBmsClock(address, usable);
  const diff = readDiff(clock);

  // follow a read until it ends; on mount also one started before this view was left
  const alive = useRef(true);
  const attach = (r: HistoryRun) => {
    setProgress(r.n);
    void r.promise.then((res) => { if (alive.current) { setResult(res); setProgress(null); } });
  };
  useEffect(() => {
    alive.current = true;
    const r = currentRun();
    if (r) attach(r);
    const un = api.onBmsHistoryProgress((p) => { noteProgress(p.n); if (currentRun()) setProgress(p.n); });
    return () => { alive.current = false; un.then((f) => f()); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const start = () => {
    if (address === null || currentRun()) return;
    setSaved(null); setResult(null);
    attach(startHistoryRead(address, api.bmsHistory));
  };

  const save = async () => {
    if (!result) return;
    setSaved(null);
    const content = historyCsv(result.address, result.records), name = csvName(result.address);
    try {
      if (isDemo) {
        const a = document.createElement("a");
        a.href = URL.createObjectURL(new Blob([content], { type: "text/csv" }));
        a.download = name; a.click(); URL.revokeObjectURL(a.href);
        setSaved({ name });
        return;
      }
      const { save: pick } = await import("@tauri-apps/plugin-dialog");
      const [, exports] = await api.historyDirs();
      const path = await pick({ defaultPath: `${exports}/${name}`, filters: [{ name: "CSV", extensions: ["csv"] }] });
      if (!path) return;
      await api.saveBmsHistory(path, content);
      setSaved({ name: path.split(/[\\/]/).pop() ?? path });
    } catch (e) { setSaved({ error: String(e) }); }
  };

  const reading = progress !== null || currentRun() !== null;
  const count = result?.records.length ?? 0;
  const endText = !result ? null
    : result.end === "empty" ? (count ? t("bmsh.end.emptyAfter", { count }) : t("bmsh.end.empty"))
    : result.end === "error" ? t("bmsh.end.error", { count, err: errorText(result.error ?? "", t) })
    : t(`bmsh.end.${result.end}`, { count });
  const rows = result ? [...result.records].reverse() : [];

  return (
    <div className="space-y-6">
      <Panel className="space-y-3 p-5">
        <p className="max-w-3xl text-muted">{t("bmsh.intro")}<InfoIcon id="topic.bmsHistory" className="ml-1.5 align-middle" /></p>
        <div className="flex flex-wrap items-center gap-2">
          {packs.length > 0 && (
            <select value={address ?? ""} onChange={(e) => { setAddress(Number(e.target.value)); setSaved(null); }} disabled={reading} aria-label={t("bmsh.pack")}
              className="h-9 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge">
              {packs.map((a) => <option key={a} value={a}>{t("bmsh.pack")} {no(a)}</option>)}
            </select>
          )}
          <Button variant="primary" disabled={!usable || address === null || reading} onClick={start}><HistoryIcon className="h-4 w-4" />{t("bmsh.read")}</Button>
          {reading && <Button variant="ghost" onClick={() => void api.bmsHistoryCancel()}><X className="h-4 w-4" />{t("bmsh.cancel")}</Button>}
          {count > 0 && !reading && <Button className="ml-auto" onClick={save}><Download className="h-4 w-4" />{t("bmsh.save")}</Button>}
        </div>
        <p className={cn("text-sm", bus === "can" && connected && !isDemo ? "text-discharge" : "text-muted")}>
          {reading ? t("bmsh.reading", { n: (progress ?? 0) + 1 })
            : !connected ? t("bmsh.needConnection")
            : bus === "can" && !isDemo ? t("bmsh.canBus")
            : t("bmsh.packBus")}
        </p>
        {usable && clock && (
          <p className="text-sm">
            {"error" in clock ? <span className="text-alarm">{t("clock.readError", { err: clock.error })}</span> : <DiffText diff={diff!} />}
            <span className="text-muted"> · {t("clock.pointer")}</span>
          </p>
        )}
        {diff !== null && Math.abs(diff) > CLOCK_FAR_S && count > 0 && <p className="text-sm text-discharge">{t("bmsh.farOff")}</p>}
        {endText && !reading && <p className={cn("text-sm", result?.end === "error" || result?.end === "cap" ? "text-alarm" : "text-muted")}>{endText}</p>}
        {saved && ("name" in saved ? <p className="text-sm text-ok">{t("bmsh.saved", { name: saved.name })}</p> : <p className="text-sm text-alarm">{t("bmsh.saveError", { err: saved.error })}</p>)}
      </Panel>

      {rows.length === 0 ? (
        !reading && <p className="text-muted">{result ? "" : t("bmsh.empty")}</p>
      ) : (
        <Panel className="overflow-x-auto p-5">
          <table className="w-full text-sm tabular-nums">
            <thead className="text-left text-muted">
              <tr className="border-b border-line">
                <th className="py-2 pr-4 font-normal">{t("bmsh.col.time")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("bmsh.col.soc")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("bmsh.col.current")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("bmsh.col.voltage")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("bmsh.col.min")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("bmsh.col.max")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("bmsh.col.spread")}</th>
                <th className="py-2 pr-4 text-right font-normal">{t("bmsh.col.temp")}</th>
                <th className="py-2 font-normal">{t("bmsh.col.alarms")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => {
                const s = summary(r);
                return (
                  <tr key={i} className="border-b border-line/60 align-top">
                    <td className="whitespace-nowrap py-1.5 pr-4">{fmtBmsTime(r.time, lang)}</td>
                    <td className="whitespace-nowrap py-1.5 pr-4 text-right">{fmt(r.soc, 1, "%")}</td>
                    <td className="whitespace-nowrap py-1.5 pr-4 text-right">{fmt(r.current, 2, "A")}</td>
                    <td className="whitespace-nowrap py-1.5 pr-4 text-right">{fmt(r.pack_voltage, 2, "V")}</td>
                    <td className="whitespace-nowrap py-1.5 pr-4 text-right">{fmt(s.min, 3, "V")} <span className="text-muted">{t("bmsh.cell", { n: s.minCell })}</span></td>
                    <td className="whitespace-nowrap py-1.5 pr-4 text-right">{fmt(s.max, 3, "V")} <span className="text-muted">{t("bmsh.cell", { n: s.maxCell })}</span></td>
                    <td className="whitespace-nowrap py-1.5 pr-4 text-right">{fmt(s.spreadMv, 0, "mV")}</td>
                    <td className="whitespace-nowrap py-1.5 pr-4 text-right">{fmt(s.maxTemp, 1, "°C")}</td>
                    <td className="py-1.5">
                      {r.alarms.length === 0 ? <span className="text-muted">–</span> : (
                        <ul className="space-y-0.5">
                          {r.alarms.map((a) => <li key={a.id} className="flex items-center gap-1.5 whitespace-nowrap"><SevIcon severity={a.severity} className="h-3.5 w-3.5 shrink-0" />{helpTitle(a.id, lang)}</li>)}
                        </ul>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Panel>
      )}
    </div>
  );
}
