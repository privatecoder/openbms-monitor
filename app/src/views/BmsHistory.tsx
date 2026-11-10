import { useEffect, useRef, useState } from "react";
import { decideUi, lockProps, type UiFacts } from "../locks";
import { useTranslation } from "react-i18next";
import { Download, History as HistoryIcon, X } from "lucide-react";
import { api, isDemo, linkNow, type ArchivePack, type ArchivedRecord, type BmsHistoryRecord, type Bus } from "../api";
import { CLOCK_FAR_S, csvName, currentRun, fmtBmsTime, historyCsv, lastRead, MAX_ROWS, noteProgress, sameInstallation, severalBoards, shownSource, startHistoryRead, summary, type HistoryRun, type LastRead } from "../bmsMemory";
import { DiffText, readDiff, useBmsClock } from "../components/BmsClock";
import { errorText } from "../control";
import { factsOf, offerable } from "../access";
import { channelOf } from "../power";
import { useReadCaps } from "../imageFlags";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { SevIcon } from "../components/widgets";
import { InfoIcon, helpTitle } from "../help";
import { cn, fmt } from "../lib/utils";

const no = (a: number) => String(a).padStart(2, "0");

// the chosen pack, kept while the app runs (the running read and the last result live in bmsMemory)
let keptAddress: number | null = null;

/** The pack's internal history (0x4B), read on demand over RS485-1/2, and over the CAN socket from a pack whose own 64/03
 *  announces it (read-capability bit 0): table newest first, CSV export. */
export function BmsHistory({ connected, bus, packs: livePacks, installation }: { connected: boolean; bus: Bus | null; packs: number[]; installation: string | null }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  // the installation's archive: which packs have one (also without a connection), and the chosen pack's records
  const [archived, setArchived] = useState<ArchivePack[]>([]);
  const [archive, setArchive] = useState<{ key: string; records: ArchivedRecord[]; skipped: number } | { key: string; error: string } | null>(null);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    setArchived([]);
    if (!installation) return;
    let alive = true;
    api.bmsArchivePacks(installation).then((p) => { if (alive) setArchived(p); }, () => {});
    return () => { alive = false; };
  }, [installation, reload]);
  const packs = [...new Set([...livePacks, ...archived.map((p) => p.address)])].sort((a, b) => a - b);
  const [address, setAddressState] = useState<number | null>(currentRun()?.address ?? keptAddress ?? packs[0] ?? null);
  const setAddress = (a: number | null) => { keptAddress = a; setAddressState(a); };
  useEffect(() => { if (address === null || !packs.includes(address)) setAddress(packs[0] ?? null); }, [packs.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps
  const [tagged, setResult] = useState<LastRead | null>(lastRead);
  // a result of another installation (same address, other bank) or of another pack than the chosen one is not this
  // one's history: not shown, not exported
  const result = tagged && sameInstallation(tagged.link, linkNow()) && tagged.read.address === address ? tagged.read : null;
  const archiveKey = `${installation ?? ""}|${address ?? ""}`;
  useEffect(() => {
    setArchive(null);
    if (!installation || address === null) return;
    let alive = true;
    api.bmsArchive(installation, address).then((a) => { if (alive) setArchive({ key: archiveKey, ...a }); }, (e) => { if (alive) setArchive({ key: archiveKey, error: String(e) }); });
    return () => { alive = false; };
  }, [archiveKey, reload]); // eslint-disable-line react-hooks/exhaustive-deps
  const arch = archive && archive.key === archiveKey ? archive : null;
  const [progress, setProgress] = useState<number | null>(() => currentRun()?.n ?? null);
  const [saved, setSaved] = useState<{ name: string } | { error: string } | null>(null);
  // the history read exists where the catalog has the app's way for it: RS485-1/2, and the CAN socket with the pack's
  // read-capability bit 0 (from its own 64/03 on this link)
  const channel = channelOf(bus);
  const caps = useReadCaps(connected && bus === "can" && !isDemo ? address : null, null, linkNow());
  const usable = connected && address !== null && livePacks.includes(address) && (isDemo || (!!channel && offerable("read-history", channel, factsOf({ caps }))));
  const canRead = bus === "can" && usable && !isDemo;
  const { read: clock } = useBmsClock(address, usable);
  const diff = readDiff(clock);

  // follow a read until it ends; on mount also one started before this view was left
  const alive = useRef(true);
  const attach = (r: HistoryRun) => {
    setProgress(r.n);
    void r.promise.then((res) => {
      if (!alive.current) return;
      setResult({ link: r.link, read: res }); setProgress(null);
      // the archive took records: show it with them
      if (res.archived != null) setReload((n) => n + 1);
    });
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
    attach(startHistoryRead(address, linkNow(), api.bmsHistory));
  };

  const source = shownSource(result, arch && "records" in arch ? arch.records : null);
  const all: (BmsHistoryRecord | ArchivedRecord)[] = source === "fresh" ? result!.records : source === "archive" && arch && "records" in arch ? arch.records : [];
  const save = async () => {
    if (!all.length || address === null) return;
    setSaved(null);
    try {
      const content = historyCsv(address, all), name = csvName(address);
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
    : result.end === "error" ? `${t("bmsh.end.error", { count, err: errorText(result.error ?? "", t) })}${result.error_kind === "eeprom" || result.error_kind === "silent" ? ` ${t(`bmsh.kind.${result.error_kind}`)}` : ""}`
    : t(`bmsh.end.${result.end}`, { count });
  const rows = [...all].reverse().slice(0, MAX_ROWS);
  const boardCol = source === "archive" && severalBoards(all as ArchivedRecord[]);
  const archNote = result && result.records.length > 0 && !reading
    ? result.archived != null ? t("bmsh.arch.saved", { count: result.archived }) : result.archive_note ? t(`bmsh.arch.${result.archive_note}`) : null
    : null;

  // what may be done now (`locks.ts`)
  const hf: UiFacts = { source: null, chosen: null, offered: usable && address !== null, reading: !!reading };
  return (
    <div className="space-y-6">
      <Panel className="space-y-3 p-5">
        <p className="max-w-3xl text-muted">{t("bmsh.intro")}<InfoIcon id="topic.bmsHistory" className="ml-1.5 align-middle" /></p>
        <div className="flex flex-wrap items-center gap-2">
          {packs.length > 0 && (
            <select value={address ?? ""} onChange={(e) => { setAddress(Number(e.target.value)); setSaved(null); }} {...lockProps(decideUi("bmsHistory.pick", hf), t)} aria-label={t("bmsh.pack")}
              className="h-9 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge">
              {packs.map((a) => <option key={a} value={a}>{t("bmsh.pack")} {no(a)}</option>)}
            </select>
          )}
          <Button variant="primary" {...lockProps(decideUi("bmsHistory.read", hf), t)} onClick={start}><HistoryIcon className="h-4 w-4" />{t("bmsh.read")}</Button>
          {reading && <Button variant="ghost" onClick={() => void api.bmsHistoryCancel()}><X className="h-4 w-4" />{t("bmsh.cancel")}</Button>}
          {all.length > 0 && !reading && <Button className="ml-auto" onClick={save}><Download className="h-4 w-4" />{t("bmsh.save")}</Button>}
        </div>
        <p className={cn("text-sm", bus === "can" && connected && !isDemo && !canRead ? "text-discharge" : "text-muted")}>
          {reading ? t(canRead ? "bmsh.readingCan" : "bmsh.reading", { n: (progress ?? 0) + 1 })
            : !connected ? t("bmsh.needConnection")
            : canRead ? t("bmsh.canRead")
            : bus === "can" && !isDemo ? t(caps === null ? "bmsh.canBusAsking" : "bmsh.canBus")
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
        {archNote && <p className={cn("text-sm", result?.archived != null ? "text-ok" : "text-discharge")}>{archNote}</p>}
        {installation && arch && ("error" in arch
          ? <p className="text-sm text-alarm">{t("bmsh.arch.loadError", { err: arch.error })}</p>
          : <p className="text-sm text-muted">{t(source === "fresh" ? "bmsh.arch.notShown" : "bmsh.arch.shown", { count: arch.records.length })}{arch.skipped ? ` ${t("bmsh.arch.skipped", { count: arch.skipped })}` : ""}</p>)}
        {all.length > MAX_ROWS && <p className="text-sm text-muted">{t("bmsh.arch.limited", { shown: MAX_ROWS, count: all.length })}</p>}
        {!installation && !isDemo && <p className="text-sm text-muted">{t("bmsh.arch.viewOnlyHint")}</p>}
        {saved && ("name" in saved ? <p className="text-sm text-ok">{t("bmsh.saved", { name: saved.name })}</p> : <p className="text-sm text-alarm">{t("bmsh.saveError", { err: saved.error })}</p>)}
      </Panel>

      {rows.length === 0 ? (
        !reading && <p className="text-muted">{result || arch ? "" : t("bmsh.empty")}</p>
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
                <th className="py-2 pr-4 font-normal">{t("bmsh.col.alarms")}</th>
                {boardCol && <th className="py-2 font-normal">{t("bmsh.col.board")}</th>}
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
                    {boardCol && <td className="whitespace-nowrap py-1.5 pl-4 text-muted">{(r as ArchivedRecord).bms_serial ?? "–"}</td>}
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
