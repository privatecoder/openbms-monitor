import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, Power, RefreshCw } from "lucide-react";
import { api, type PackLive } from "../api";
import { refusal } from "../calibration";
import { belowSocProtection, blockingAlarms, chargeFree, commandedOff, dischargeFree, energyText, errorText, heaterOn, offReasons, remainingAllowed, remainingRaw, socOf, type RestartResult } from "../control";
import { InfoIcon, helpTitle } from "../help";
import { cn } from "../lib/utils";
import { useCurrent } from "./Calibration";
import { Button } from "./ui/button";

const input = "h-8 w-28 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge";
const no = (a: number) => String(a).padStart(2, "0");
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
/** P79: SOC protection in %. */
const P79 = 79;

type Action = "restart" | "on" | "off" | "heater" | "remaining" | "clear";
type Tone = "ok" | "warn" | "error";
type Note = { text: string; tone: Tone } | null;
type Ask = { kind: "restart" | "off" } | { kind: "clear"; live: PackLive | null };

// packs the app forced the heater on while the view is open: the button reads „Heizung aus“ for them
const forced = new Set<number>();

/** „Steuern“: restart, on/off, heater, remaining capacity and clearing history and counters of the chosen pack. */
export function PackControlSection({ address, usable, locked, onBusy, onChanged }: {
  address: number | null; usable: boolean; locked?: boolean; onBusy?: (busy: boolean) => void;
  /** after a restart: the pack's parameters column (if shown) is read again */
  onChanged?: (address: number) => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [open, setOpen] = useState(false);
  const [live, setLive] = useState<PackLive | { error: string } | null>(null);
  const [p79, setP79] = useState<number | null>(null);
  const [busy, setBusy] = useState<Action | null>(null);
  const [note, setNote] = useState<Note>(null);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [ah, setAh] = useState("");
  const gen = useRef(0);
  const current = useCurrent(address);
  useEffect(() => onBusy?.(busy !== null), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => forced.clear(), []);

  const reload = async (a: number) => {
    const mine = ++gen.current;
    try { const l = await api.packLive(a); if (gen.current === mine && current.current === a) setLive(l); return l; }
    catch (e) { if (gen.current === mine && current.current === a) setLive({ error: errorText(e, t) }); return null; }
  };
  useEffect(() => {
    setLive(null); setNote(null); setAh(""); setP79(null); setAsk(null);
    if (!open || !usable || address === null) return;
    void reload(address);
    // P79 for the warning under the remaining capacity; a pack that does not answer 0x47 gets the general hint
    api.parameters(address).then((p) => { if (current.current === address) setP79(p.parameters[P79]?.value ?? null); }, () => {});
  }, [open, usable, address]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!usable || address === null) return null;

  const pack = no(address);
  const nf = (d: number) => new Intl.NumberFormat(lang, { minimumFractionDigits: d, maximumFractionDigits: d });
  const tel = live && "telemetry" in live ? live.telemetry : null;
  const st = live && "status" in live ? live.status : null;
  const names = (ids: string[]) => ids.map((id) => helpTitle(id, lang)).join(", ");
  const failed = (e: unknown, refused = t("ctl.refused")) => {
    const r = refusal(e);
    return { text: r === "refused" ? refused : r === "invalid" ? t("ctl.invalid") : t("ctl.error", { err: errorText(e, t) }), tone: "error" as const };
  };

  /** One action on pack `a`; a reply after a pack switch is dropped. */
  const run = async (k: Action, a: number, f: () => Promise<Note>) => {
    setBusy(k); setNote(null);
    try { const n = await f(); if (current.current === a) setNote(n); }
    catch (e) { if (current.current === a) setNote(failed(e, k === "remaining" ? t("ctl.remaining.refused") : undefined)); }
    finally { setBusy(null); }
  };

  const restart = (a: number) => run("restart", a, async () => {
    const r: RestartResult = await api.packRestart(a);
    await reload(a);
    if (r.kind !== "not_reachable") { forced.delete(a); onChanged?.(a); }
    return r.kind === "back" ? { text: t("ctl.restart.back", { pack: no(a), s: nf(1).format(r.after_ms / 1000) }), tone: "ok" }
      : r.kind === "no_interruption" ? { text: t("ctl.restart.noInterruption"), tone: "warn" }
      : r.kind === "no_answer" ? { text: t("ctl.restart.noAnswer", { pack: no(a) }), tone: "error" }
      : { text: t("ctl.restart.notReachable"), tone: "error" };
  });

  const onOff = (a: number, on: boolean) => run(on ? "on" : "off", a, async () => {
    await api.packOnOff(a, on);
    // the output follows at the next 1-s evaluation of the BMS
    await wait(2000);
    const l = await reload(a);
    if (!l) return { text: t("ctl.sentUnread"), tone: "warn" };
    if (on && commandedOff(l.status)) {
      const why = offReasons(l.status);
      return { text: t("ctl.onOff.offAgain", { reason: why.length ? names(why) : t("ctl.onOff.protection") }), tone: "error" };
    }
    if (on) return dischargeFree(l.status) ? { text: t("ctl.onOff.onDone"), tone: "ok" } : { text: t("ctl.onOff.onBlocked"), tone: "warn" };
    return { text: t("ctl.onOff.offDone", { pack: no(a) }), tone: "ok" };
  });

  const heater = (a: number, on: boolean) => run("heater", a, async () => {
    await api.packHeater(a, on);
    if (on) forced.add(a); else forced.delete(a);
    await wait(1500);
    const l = await reload(a);
    if (!l) return { text: t("ctl.sentUnread"), tone: "warn" };
    // the forced counter is not readable: an output that stays off ends the „forced“ state shown
    if (!heaterOn(l.status)) forced.delete(a);
    return on
      ? { text: heaterOn(l.status) ? t("ctl.heater.onDone") : t("ctl.heater.onNoOutput"), tone: heaterOn(l.status) ? "ok" : "warn" }
      : { text: heaterOn(l.status) ? t("ctl.heater.offStill") : t("ctl.heater.offDone"), tone: heaterOn(l.status) ? "warn" : "ok" };
  });

  const setRemaining = (a: number, raw: number) => run("remaining", a, async () => {
    await api.packSetRemaining(a, raw);
    setNote({ text: t("ctl.remaining.taken"), tone: "ok" });
    // 0x42 shows the new value only after the next 1-s evaluation
    await wait(1500);
    const l = await reload(a);
    if (current.current === a) setAh("");
    return l ? { text: t("ctl.remaining.done", { soc: nf(1).format(l.telemetry.soc), ah: nf(2).format(l.telemetry.remaining_capacity_ah) }), tone: "ok" }
      : { text: t("ctl.sentUnread"), tone: "warn" };
  });

  const clear = (a: number) => run("clear", a, async () => {
    await api.packClearHistory(a);
    const l = await reload(a);
    return { text: l ? t("ctl.clear.done", { cycles: l.telemetry.cycles }) : t("ctl.sentUnread"), tone: l ? "ok" : "warn" };
  });

  // what the clear dialog names is read right before it opens
  const askClear = async (a: number) => {
    setBusy("clear");
    const l = await reload(a);
    setBusy(null);
    if (current.current === a) setAsk({ kind: "clear", live: l });
  };

  const off = st ? commandedOff(st) : false;
  const heaterForced = forced.has(address);
  const raw = remainingRaw(ah);
  const full = tel?.full_capacity_ah;
  const soc = raw !== null && full ? socOf(raw, full) : null;
  const under = soc !== null ? belowSocProtection(soc, p79) : null;
  const disabled = busy !== null || locked;
  const yesNo = (free: boolean) => <span className={free ? "text-ok" : "text-discharge"}>{t(free ? "ctl.state.free" : "ctl.state.blocked")}</span>;
  const blockers = st && !dischargeFree(st) && !off ? blockingAlarms(st) : [];

  return (
    <div className="space-y-3 border-t border-line pt-3 text-sm">
      <div className="flex items-center gap-1.5">
        <button onClick={() => setOpen(!open)} aria-expanded={open} className="flex items-center gap-1.5 font-medium hover:text-ink">
          {open ? <ChevronDown className="h-4 w-4" aria-hidden /> : <ChevronRight className="h-4 w-4" aria-hidden />}
          <Power className="h-4 w-4" aria-hidden />{t("ctl.title")}
        </button>
        <InfoIcon id="topic.control" />
        {!open && <span className="text-muted">{t("ctl.closed")}</span>}
      </div>
      {open && (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 tabular-nums">
            {live === null ? <span className="text-muted">{t("ctl.reading")}</span>
              : "error" in live ? <span className="text-alarm">{t("ctl.readError", { err: live.error })}</span>
              : <>
                <span>{t("ctl.state.command")} <span className={off ? "text-discharge" : "text-ok"}>{t(off ? "ctl.state.off" : "ctl.state.on")}</span></span>
                <span>{t("ctl.state.discharge")} {yesNo(dischargeFree(live.status))}{blockers.length > 0 && <span className="text-discharge"> ({names(blockers)})</span>}</span>
                <span>{t("ctl.state.charge")} {yesNo(chargeFree(live.status))}</span>
                <span>{t("ctl.state.heater")} {t(heaterOn(live.status) ? "ctl.state.on" : "ctl.state.off")}</span>
                <span>{t("ctl.state.soc", { soc: nf(1).format(live.telemetry.soc), rem: nf(2).format(live.telemetry.remaining_capacity_ah), full: nf(2).format(live.telemetry.full_capacity_ah) })}</span>
                <span>{t("ctl.state.cycles", { n: live.telemetry.cycles })}</span>
              </>}
            <Button variant="ghost" size="icon" className="-ml-2 h-8 w-8" disabled={disabled || live === null} onClick={() => void reload(address)}
              aria-label={t("ctl.reload")} title={t("ctl.reload")}><RefreshCw className="h-4 w-4" /></Button>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" disabled={disabled} onClick={() => setAsk({ kind: "restart" })}>{busy === "restart" ? t("ctl.restart.running") : t("ctl.restart.button")}</Button>
            <Button size="sm" disabled={disabled || !st} onClick={() => (off ? void onOff(address, true) : setAsk({ kind: "off" }))}>
              {busy === "on" || busy === "off" ? t("ctl.running") : t(off ? "ctl.onOff.on" : "ctl.onOff.off")}
            </Button>
            <Button size="sm" disabled={disabled} onClick={() => void heater(address, !heaterForced)}>
              {busy === "heater" ? t("ctl.running") : t(heaterForced ? "ctl.heater.off" : "ctl.heater.on")}
            </Button>
          </div>
          <p className="max-w-3xl text-muted">{t("ctl.heater.hint")}</p>

          <div className="space-y-1">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <label className="flex items-center gap-2">{t("ctl.remaining.field")}
                <input className={input} inputMode="decimal" value={ah} onChange={(e) => setAh(e.target.value)} placeholder="140,00" disabled={disabled} />
              </label>
              {soc !== null && full !== undefined && <span className="tabular-nums">{t("ctl.remaining.soc", { soc: nf(0).format(soc), full: nf(2).format(full) })}</span>}
              <Button size="sm" disabled={disabled || !remainingAllowed(raw, full)} onClick={() => raw !== null && void setRemaining(address, raw)}>
                {busy === "remaining" ? t("ctl.running") : t("ctl.remaining.button")}
              </Button>
              {raw !== null && full !== undefined && !remainingAllowed(raw, full) && <span className="text-discharge">{t("ctl.remaining.tooLarge")}</span>}
            </div>
            {under === true && <p className="text-discharge">{t("ctl.remaining.under", { p: nf(0).format(p79!) })}</p>}
            {under === null && soc !== null && <p className="text-muted">{t("ctl.remaining.underUnknown")}</p>}
            <p className="max-w-3xl text-muted">{t("ctl.remaining.hint")}</p>
          </div>

          <div>
            <Button size="sm" variant="ghost" disabled={disabled} onClick={() => void askClear(address)}>{busy === "clear" ? t("ctl.running") : t("ctl.clear.button")}</Button>
          </div>
          {note && <p className={note.tone === "ok" ? "text-ok" : note.tone === "warn" ? "text-discharge" : "text-alarm"}>{note.text}</p>}
        </div>
      )}
      {ask && (
        <Confirm
          title={t(`ctl.${ask.kind}.title`, { pack })}
          danger={ask.kind === "clear"}
          confirm={t(`ctl.${ask.kind}.confirm`)}
          onCancel={() => setAsk(null)}
          onConfirm={() => {
            setAsk(null);
            if (ask.kind === "restart") void restart(address);
            else if (ask.kind === "off") void onOff(address, false);
            else void clear(address);
          }}>
          {ask.kind === "clear" ? (
            <>
              <p>{ask.live
                ? t("ctl.clear.what", { pack, cycles: ask.live.telemetry.cycles,
                  charged: ask.live.telemetry.energy_charged_kwh !== null ? energyText(ask.live.telemetry.energy_charged_kwh, lang) : "–",
                  discharged: ask.live.telemetry.energy_discharged_kwh !== null ? energyText(ask.live.telemetry.energy_discharged_kwh, lang) : "–" })
                : t("ctl.clear.whatUnread", { pack })}</p>
              <p>{t("ctl.clear.text")}</p>
            </>
          ) : <p>{t(`ctl.${ask.kind}.text`, { pack })}</p>}
        </Confirm>
      )}
    </div>
  );
}

/** One confirmation before an action; `danger` makes the button red. */
function Confirm({ title, confirm, danger, onConfirm, onCancel, children }: {
  title: string; confirm: string; danger?: boolean; onConfirm: () => void; onCancel: () => void; children: React.ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-ink/20 p-4">
      <div role="dialog" aria-modal="true" aria-labelledby="ctl-dialog-title" className="w-full max-w-md space-y-3 rounded-md border border-line bg-surface p-5 text-sm">
        <h2 id="ctl-dialog-title" className="font-display text-lg font-semibold">{title}</h2>
        {children}
        <div className="flex justify-end gap-2">
          <Button variant="secondary" size="sm" onClick={onCancel} autoFocus>{t("ctl.cancel")}</Button>
          <Button variant="primary" size="sm" className={cn(danger && "bg-alarm text-white")} onClick={onConfirm}>{confirm}</Button>
        </div>
      </div>
    </div>
  );
}
