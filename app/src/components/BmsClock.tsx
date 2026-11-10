import { useEffect, useState } from "react";
import { actLocked } from "../locks";
import { useTranslation } from "react-i18next";
import { Clock } from "lucide-react";
import { api, isDemo, linkNow, type BmsTime, type Bus } from "../api";
import { bankRefusal, errorText, writeError } from "../control";
import { CLOCK_OK_S, clockDiff, durationText, fmtBmsTime, fromBmsTime, toBmsTime, zoneLabel } from "../bmsMemory";
import { cn } from "../lib/utils";
import { clockAccess, clockTargets, type Firmware } from "../power";
import { SectionHead, StatusLine } from "./Section";
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

/** The clock line of the parameters view: BMS time, PC time, difference, and setting it to the PC's local time. Over the
 *  CAN socket only for a pack with the power and clock service (`power.ts`). */
export function ClockLine({ connected, bus, packs, address, setAddress, locked, onBusy, installation = true, firmware, powerLock }: {
  connected: boolean; bus: Bus | null; packs: number[]; address: number | null; setAddress: (a: number) => void;
  /** false with „Nur ansehen“: the clock is read, not set */
  installation?: boolean;
  /** CAN socket: the pack's image flag for the clock as this connection proved it, and its lock bit as last seen */
  /** each pack's firmware as this connection proved it (`power.ts` `packFirmware`) */
  firmware?: (a: number) => Firmware; powerLock?: (a: number) => boolean | null;
  /** another write (CAN protocol, calibration) runs: pack picker and clock buttons wait */
  locked?: boolean; onBusy?: (busy: boolean) => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const now = useNow();
  const can = bus === "can" && !isDemo;
  const fwOf = (a: number | null): Firmware => (a !== null && firmware?.(a)) || { patch: { kind: "unknown" }, flags: null, caps: null };
  const accessOf = (a: number | null, lock: boolean | null) => clockAccess({ connected, bus, demo: isDemo, installation, fw: fwOf(a), locked: lock });
  const access = accessOf(address, address !== null ? powerLock?.(address) ?? null : null);
  const usable = access.read;
  const { read, setRead, reload } = useBmsClock(address, usable);
  const [busy, setBusy] = useState<string | null>(null);
  useEffect(() => onBusy?.(busy !== null), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
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
    } catch (e) { setNote({ text: writeError(e, t, (err) => t("clock.setError", { err })), error: true }); }
    finally { setBusy(null); }
  };
  const setAll = async () => {
    setNote(null);
    const failed: string[] = [];
    let bank: unknown = null;
    // the series belongs to the installation and connection it began on: after a switch the rest is not set
    const link = linkNow();
    let set = 0;
    // over the CAN socket a pack without the service is skipped and named, nothing is sent to it
    // the packs the catalog lets set the clock over this channel (each lock is read fresh before its command)
    const { targets, skipped } = clockTargets(packs, (a) => accessOf(a, null).write);
    for (const [n, a] of targets.entries()) {
      if (linkNow() !== link) break;
      setBusy(t("clock.settingAll", { pack: no(a), n: n + 1, of: targets.length }));
      try {
        const bms = await api.setBmsClock(a, toBmsTime(new Date()));
        set += 1;
        if (a === address && linkNow() === link) setRead({ bms, at: Date.now() });
      } catch (e) { failed.push(no(a)); if (!bank && bankRefusal(e)) bank = e; }
    }
    setBusy(null);
    if (linkNow() !== link) { setNote({ text: t("clock.setAllStopped", { ok: set, of: targets.length }), error: true }); return; }
    const done = t("clock.setAllDone", { ok: targets.length - failed.length, of: targets.length });
    const why = bank ? ` ${errorText(bank, t)}` : "";
    const skip = skipped.length ? ` ${t("clock.setAllSkipped", { list: skipped.map(no).join(", ") })}` : "";
    setNote({ text: `${failed.length ? `${done} ${t("clock.setAllFailed", { list: failed.join(", ") })}${why}` : done}${skip}`, error: failed.length > 0 });
  };

  const diff = readDiff(read);
  const bmsNow = read && "bms" in read ? toBmsTime(new Date(fromBmsTime(read.bms).getTime() + (now - read.at))) : null;
  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={Clock} title={t("clock.title")} help="topic.bmsHistory" onReload={usable && address !== null ? reload : undefined}
        reloading={usable && address !== null && read === null} reloadDisabled={actLocked(!!busy, !!locked)} readAt={read && "bms" in read ? new Date(read.at) : null} />
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        {packs.length > 0 && (
          <select value={address ?? ""} onChange={(e) => setAddress(Number(e.target.value))} disabled={actLocked(!!busy, !!locked)} aria-label={t("bmsh.pack")}
            className="h-8 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge">
            {packs.map((a) => <option key={a} value={a}>{t("bmsh.pack")} {no(a)}</option>)}
          </select>
        )}
        {usable && read && "bms" in read && (
          <>
            <span className="tabular-nums">{t("clock.bms", { time: fmtBmsTime(bmsNow!, lang) })}</span>
            <span className="tabular-nums text-muted">{t("clock.pc", { time: fmtBmsTime(toBmsTime(pc), lang) })}</span>
            <DiffText diff={diff!} />
          </>
        )}
      </div>
      {!usable ? (
        <StatusLine tone={access.unsupported ? "warn" : "muted"}>{access.unsupported ? t("clock.canBus") : t("clock.needConnection")}</StatusLine>
      ) : read === null ? (
        <StatusLine>{t("clock.reading")}</StatusLine>
      ) : "error" in read && (
        <StatusLine tone="error">{t("clock.readError", { err: read.error })}</StatusLine>
      )}
      {usable && access.write && address !== null && (
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" disabled={actLocked(!!busy, !!locked)} onClick={setOne} className="tabular-nums">{label}</Button>
          {packs.length > 1 && <Button size="sm" variant="ghost" disabled={actLocked(!!busy, !!locked)} onClick={setAll}>{t("clock.setAll")}</Button>}
        </div>
      )}
      {usable && access.write && address !== null && <StatusLine>{busy ?? (can ? `${t("clock.hint")} ${t("clock.canHint")}` : t("clock.hint"))}</StatusLine>}
      {usable && installation && can && !access.write && <StatusLine tone="warn">{t("clock.locked")}</StatusLine>}
      {note && <StatusLine tone={note.error ? "error" : "ok"}>{note.text}</StatusLine>}
    </div>
  );
}
