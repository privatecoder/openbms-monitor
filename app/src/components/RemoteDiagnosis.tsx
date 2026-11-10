import { useEffect, useRef, useState } from "react";
import { actLocked, decideUi } from "../locks";
import { useTranslation } from "react-i18next";
import { Microscope } from "lucide-react";
import { api, type CalPageRead, type SocStoreRead } from "../api";
import { CAP } from "../access";
import { errorText } from "../control";
import { useReadCaps } from "../imageFlags";
import { cn } from "../lib/utils";
import { calRows, PAGES, SOC_DELTA_WARN, socView } from "../remoteDiag";
import { Button } from "./ui/button";
import { SectionHead, StatusLine } from "./Section";

const has = (caps: number | null, bit: number) => caps !== null && (caps & (1 << bit)) !== 0;

/** „Ferndiagnose“ (MNT format 5): the calibration (0x64/05) and the stored state with the energy counters (0x64/06), each
 *  asked on demand and only where the pack's own 64/03 announces it (read-capability bits). Changes nothing; an answer
 *  belongs to the pack and link it was asked for. */
export function RemoteDiagnosis({ address, known, connected, locked, linkKey }: { address: number | null; known: number | null | undefined; connected: boolean; locked?: boolean; linkKey: string }) {
  const { t } = useTranslation();
  const caps = useReadCaps(connected ? address : null, known, linkKey);
  const [cal, setCal] = useState<CalPageRead[] | { error: string } | null>(null);
  const [soc, setSoc] = useState<SocStoreRead | { error: string } | null>(null);
  const [busy, setBusy] = useState<"cal" | "soc" | null>(null);
  const gen = useRef(0);
  useEffect(() => { gen.current += 1; setCal(null); setSoc(null); setBusy(null); }, [address, linkKey]);
  const calOk = has(caps, CAP.calibration), socOk = has(caps, CAP.soc_store) || has(caps, CAP.energy);
  if (address === null || !connected || (!calOk && !socOk)) return null;
  const run = async <T,>(what: "cal" | "soc", ask: () => Promise<T>, set: (v: T | { error: string } | null) => void) => {
    const mine = ++gen.current;
    setBusy(what); set(null);
    try { const r = await ask(); if (gen.current === mine) set(r); }
    catch (e) { if (gen.current === mine) set({ error: errorText(e, t) }); }
    finally { if (gen.current === mine) setBusy(null); }
  };
  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={Microscope} title={t("rdiag.title")} help="topic.remoteDiagnosis" />
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        {calOk && <Button size="sm" disabled={actLocked(busy !== null, !!locked)} onClick={() => void run("cal", () => api.packCalibration(address), setCal)}>{busy === "cal" ? t("rdiag.reading") : t("rdiag.cal.read")}</Button>}
        {socOk && <Button size="sm" disabled={actLocked(busy !== null, !!locked)} onClick={() => void run("soc", () => api.packSocStore(address), setSoc)}>{busy === "soc" ? t("rdiag.reading") : t("rdiag.soc.read")}</Button>}
      </div>
      {cal && ("error" in cal ? <StatusLine tone="error">{cal.error}</StatusLine> : <CalTable pages={cal} />)}
      {soc && ("error" in soc ? <StatusLine tone="error">{soc.error}</StatusLine> : <SocLines read={soc} full />)}
      <StatusLine>{t("rdiag.hint")}</StatusLine>
    </div>
  );
}

function CalTable({ pages }: { pages: CalPageRead[] }) {
  const { t, i18n } = useTranslation();
  const n = new Intl.NumberFormat(i18n.language, { useGrouping: false });
  const rows = calRows(pages);
  const page = (p: (typeof PAGES)[number]) => pages.find((x) => x.page === p);
  const head = (p: (typeof PAGES)[number]) => {
    const r = page(p);
    if (!r?.value) return <span className="text-alarm">{t("rdiag.cal.missing")}</span>;
    if (r.value.kind === "active") return null;
    return <span className={r.value.class === "valid" ? "text-ok" : "text-discharge"}>{t(`rdiag.cls.${r.value.class}`)}{r.value.crc_ok ? "" : ` · ${t("rdiag.cal.crcNo")}`}</span>;
  };
  const active = page("active")?.value;
  const differs = rows.some((r) => r.differs);
  return (
    <div className="space-y-2">
      <div className="overflow-x-auto">
        <table className="text-sm tabular-nums">
          <thead className="text-left text-muted">
            <tr className="border-b border-line">
              <th className="py-1 pr-4 font-normal" />
              {PAGES.map((p) => <th key={p} className="py-1 pr-4 text-right font-normal">{t(`rdiag.cal.page.${p}`)}<div className="text-xs">{head(p)}</div></th>)}
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => (
              <tr key={i} className="border-b border-line/60">
                <td className="whitespace-nowrap py-1 pr-4">{t(`rdiag.cal.row.${r.key}`, { n: r.n })}</td>
                {r.values.map((v, j) => <td key={j} className={cn("py-1 pr-4 text-right", r.differs && v !== null && "text-discharge")}>{v === null ? "–" : n.format(v)}</td>)}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {pages.filter((p) => p.error).map((p) => <StatusLine key={p.page} tone="error">{t(`rdiag.cal.page.${p.page}`)}: {p.error}</StatusLine>)}
      {active?.kind === "active" && active.invalid_bits !== 0 && <StatusLine tone="warn">{t("rdiag.cal.invalid", { bits: `0x${active.invalid_bits.toString(16).toUpperCase().padStart(2, "0")}` })}</StatusLine>}
      {active?.kind === "active" && <StatusLine>{t("rdiag.cal.current", { a: n.format(active.current_ma[0]), b: n.format(active.current_ma[1]) })}</StatusLine>}
      {differs && <StatusLine tone="warn">{t("rdiag.cal.differs")}</StatusLine>}
      <StatusLine>{t("rdiag.cal.hint")}</StatusLine>
    </div>
  );
}

/** The stored state against the live one, and the energy counters; `full` adds the copies' details (the Parameters view),
 *  without it one line for a dialog. */
export function SocLines({ read, full }: { read: SocStoreRead; full?: boolean }) {
  const { t, i18n } = useTranslation();
  const f1 = new Intl.NumberFormat(i18n.language, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const v = socView(read);
  const s = read.store;
  const bootText = t(`rdiag.soc.boot.${v.boot}`);
  const storedLine = !read.records ? null
    : v.stored !== null && v.record
      ? t("rdiag.soc.stored", { soc: f1.format(v.stored), rem: f1.format(v.record.remaining_ah), full: f1.format(v.record.full_ah), copy: bootText, live: f1.format(v.live), delta: f1.format(Math.abs(v.delta!)), dir: t(v.delta! < 0 ? "rdiag.soc.lower" : "rdiag.soc.higher") })
      : t("rdiag.soc.noStored", { copy: bootText, live: f1.format(v.live) });
  return (
    <div className="space-y-1">
      {storedLine && <StatusLine tone={v.delta !== null && Math.abs(v.delta) >= SOC_DELTA_WARN ? "warn" : "muted"}>{storedLine}</StatusLine>}
      {read.records && read.checkpoint && <StatusLine>{t("rdiag.soc.checkpoint")}</StatusLine>}
      {full && read.records && (
        <StatusLine>{t("rdiag.soc.copies", { a: t(`rdiag.cls.${s.class[0]}`), b: t(`rdiag.cls.${s.class[1]}`), cmp: t(`mdiag.cmp.${s.compare}`) })}{v.record ? ` ${t("rdiag.soc.cycles", { count: v.record.cycles })}` : ""}</StatusLine>
      )}
      {read.energy && <StatusLine>{t("rdiag.soc.energy", { charged: f1.format(s.live_charged_kwh), discharged: f1.format(s.live_discharged_kwh) })}</StatusLine>}
      {full && <StatusLine>{t("rdiag.soc.noPromise")}</StatusLine>}
    </div>
  );
}

/** One pack row of the pack bus before a flash: its stored state, read on a click on that row's own address and port
 *  (`firmware_soc_store`, quiet bus). Information only. */
export function RowSocNote({ port, address, disabled }: { port: string; address: number; disabled: boolean }) {
  const { t } = useTranslation();
  const [read, setRead] = useState<SocStoreRead | null | undefined>(undefined);
  const [reading, setReading] = useState(false);
  useEffect(() => { setRead(undefined); }, [port, address]);
  const ask = async () => {
    setReading(true);
    try { setRead(await api.firmwareSocStore(port, address)); } catch { setRead(null); } finally { setReading(false); }
  };
  return (
    <div className="space-y-1">
      <Button variant="ghost" size="sm" disabled={decideUi("firmware.readTarget", { source: null, chosen: null, portHeld: disabled, busy: reading, port: !!port }).locked} onClick={() => void ask()}>{reading ? t("rdiag.reading") : t("fw.soc.read")}</Button>
      {read === null && <StatusLine>{t("fw.soc.unavailable")}</StatusLine>}
      {read && <SocLines read={read} />}
    </div>
  );
}

/** For a confirmation dialog (restart): reads 0x64/06 once when shown, only where the pack announces it; shows nothing
 *  otherwise. Only information, never a condition. */
export function SocStoreHint({ address, known, linkKey }: { address: number; known: number | null | undefined; linkKey: string }) {
  const caps = useReadCaps(address, known, linkKey);
  const [read, setRead] = useState<SocStoreRead | null>(null);
  const ok = has(caps, CAP.soc_store);
  useEffect(() => {
    if (!ok) return;
    let alive = true;
    api.packSocStore(address).then((r) => { if (alive) setRead(r); }).catch(() => {});
    return () => { alive = false; };
  }, [address, ok, linkKey]);
  return read ? <SocLines read={read} /> : null;
}
