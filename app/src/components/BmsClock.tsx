import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Clock } from "lucide-react";
import { api, isDemo, type BmsTime, type Bus } from "../api";
import { CLOCK_OK_S, clockDiff, durationText, fmtBmsTime, fromBmsTime, toBmsTime, zoneLabel } from "../bmsMemory";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";
import { useNow } from "./live";

const no = (a: number) => String(a).padStart(2, "0");

/** A clock read: the BMS time and the PC time of the read, so the BMS time can tick on. */
type Read = { bms: BmsTime; at: number } | { error: string } | null;

/** Reads the clock of `address` whenever it (or `enabled`) changes; `reload` reads again. */
export function useBmsClock(address: number | null, enabled: boolean) {
  const [read, setRead] = useState<Read>(null);
  const [gen, setGen] = useState(0);
  useEffect(() => {
    setRead(null);
    if (address === null || !enabled) return;
    let alive = true;
    api.bmsClock(address).then((bms) => alive && setRead({ bms, at: Date.now() }), (e) => alive && setRead({ error: String(e) }));
    return () => { alive = false; };
  }, [address, enabled, gen]);
  return { read, setRead, reload: () => setGen((g) => g + 1) };
}

/** BMS clock minus PC clock in s, or null without a read. */
export const readDiff = (read: Read) => (read && "bms" in read ? clockDiff(read.bms, new Date(read.at)) : null);

export function DiffText({ diff, className }: { diff: number; className?: string }) {
  const { t } = useTranslation();
  const ok = Math.abs(diff) < CLOCK_OK_S;
  return (
    <span className={cn(ok ? "text-ok" : "text-discharge", className)}>
      {ok ? t("clock.ok") : t(diff > 0 ? "clock.ahead" : "clock.behind", { d: durationText(diff) })}
    </span>
  );
}

/** The pack chosen in the clock line; the CAN protocol and the calibration below it act on the same pack. */
export function usePackChoice(packs: number[]) {
  const [address, setAddress] = useState<number | null>(packs[0] ?? null);
  useEffect(() => { if (address === null || !packs.includes(address)) setAddress(packs[0] ?? null); }, [packs.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps
  return [address, setAddress] as const;
}

/** The clock line of the parameters view: BMS time, PC time, difference, and setting it to the PC's local time. */
export function ClockLine({ connected, bus, packs, address, setAddress }: {
  connected: boolean; bus: Bus | null; packs: number[]; address: number | null; setAddress: (a: number) => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const now = useNow();
  const usable = connected && (bus === "pack" || isDemo);
  const { read, setRead } = useBmsClock(address, usable);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  useEffect(() => setNote(null), [address]);

  // the label names exactly what a click sends: this second's local time and the zone it belongs to
  const pc = new Date(now);
  const label = t("clock.set", { time: fmtBmsTime(toBmsTime(pc), lang), zone: zoneLabel(pc, lang) });

  const setOne = async () => {
    if (address === null) return;
    setBusy(t("clock.setting")); setNote(null);
    try {
      const bms = await api.setBmsClock(address, toBmsTime(new Date()));
      setRead({ bms, at: Date.now() });
    } catch (e) { setNote({ text: t("clock.setError", { err: String(e) }), error: true }); }
    finally { setBusy(null); }
  };
  const setAll = async () => {
    setNote(null);
    const failed: string[] = [];
    for (const [n, a] of packs.entries()) {
      setBusy(t("clock.settingAll", { pack: no(a), n: n + 1, of: packs.length }));
      try {
        const bms = await api.setBmsClock(a, toBmsTime(new Date()));
        if (a === address) setRead({ bms, at: Date.now() });
      } catch { failed.push(no(a)); }
    }
    setBusy(null);
    const done = t("clock.setAllDone", { ok: packs.length - failed.length, of: packs.length });
    setNote({ text: failed.length ? `${done} ${t("clock.setAllFailed", { list: failed.join(", ") })}` : done, error: failed.length > 0 });
  };

  const diff = readDiff(read);
  const bmsNow = read && "bms" in read ? toBmsTime(new Date(fromBmsTime(read.bms).getTime() + (now - read.at))) : null;
  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="flex items-center gap-1.5 font-medium"><Clock className="h-4 w-4" aria-hidden />{t("clock.title")}<InfoIcon id="topic.bmsHistory" /></span>
        {packs.length > 0 && (
          <select value={address ?? ""} onChange={(e) => setAddress(Number(e.target.value))} disabled={!!busy} aria-label={t("bmsh.pack")}
            className="h-8 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge">
            {packs.map((a) => <option key={a} value={a}>{t("bmsh.pack")} {no(a)}</option>)}
          </select>
        )}
        {!usable ? (
          <span className={cn(bus === "can" && connected ? "text-discharge" : "text-muted")}>{bus === "can" && connected ? t("clock.canBus") : t("par.needConnection")}</span>
        ) : read === null ? (
          <span className="text-muted">{t("clock.reading")}</span>
        ) : "error" in read ? (
          <span className="text-alarm">{t("clock.readError", { err: read.error })}</span>
        ) : (
          <>
            <span className="tabular-nums">{t("clock.bms", { time: fmtBmsTime(bmsNow!, lang) })}</span>
            <span className="tabular-nums text-muted">{t("clock.pc", { time: fmtBmsTime(toBmsTime(pc), lang) })}</span>
            <DiffText diff={diff!} />
          </>
        )}
      </div>
      {usable && address !== null && (
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" disabled={!!busy} onClick={setOne} className="tabular-nums">{label}</Button>
          {packs.length > 1 && <Button size="sm" variant="ghost" disabled={!!busy} onClick={setAll}>{t("clock.setAll")}</Button>}
          <span className="text-muted">{busy ?? t("clock.hint")}</span>
        </div>
      )}
      {note && <p className={note.error ? "text-alarm" : "text-ok"}>{note.text}</p>}
    </div>
  );
}
