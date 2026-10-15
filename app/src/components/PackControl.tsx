import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Power } from "lucide-react";
import { api, type PackLive } from "../api";
import { refusal } from "../calibration";
import { belowSocProtection, blockingAlarms, chargeFree, commandedOff, dischargeFree, energyText, errorText, heaterOn, noAnswer, offReasons, remainingAllowed, remainingRaw, socOf, type RestartResult } from "../control";
import { helpTitle } from "../help";
import { cn } from "../lib/utils";
import { useCurrent } from "./Calibration";
import { SectionHead, StatusLine } from "./Section";
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
export function PackControlSection({ address, usable, locked, onBusy, onChanged, p79: p79Shown, linkKey }: {
  address: number | null; usable: boolean; locked?: boolean; onBusy?: (busy: boolean) => void;
  /** after a restart: the pack's parameters column (if shown) is read again */
  onChanged?: (address: number) => void;
  /** P79 (SOC protection, %) from the pack's read or stored column; null: read once when the field is used */
  p79?: number | null;
  /** installation and connection: results of an action begun on another one are dropped */
  linkKey: string;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [open, setOpen] = useState(false);
  const [live, setLive] = useState<PackLive | { error: string } | null>(null);
  const [p79Read, setP79] = useState<number | null>(null);
  const [busy, setBusy] = useState<Action | null>(null);
  const [note, setNote] = useState<Note>(null);
  // where the result line shows: under the control that started the action
  const [noteFor, setNoteFor] = useState<Action | null>(null);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [ah, setAh] = useState("");
  const gen = useRef(0);
  const current = useCurrent(address);
  const linkNow = useCurrent(linkKey);
  // still the pack and the connection an action or read began on
  const same = (a: number, lk: string) => current.current === a && linkNow.current === lk;
  const p79Asked = useRef(new Set<number>());
  useEffect(() => onBusy?.(busy !== null), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => forced.clear(), []);

  const [reading, setReading] = useState(false);
  const [readAt, setReadAt] = useState<Date | null>(null);
  const reload = async (a: number) => {
    const mine = ++gen.current, lk = linkNow.current;
    setReading(true);
    try {
      const l = await api.packLive(a);
      if (gen.current !== mine || !same(a, lk)) return null;
      setReadAt(new Date());
      // the field starts at the pack's current value, so a correction only changes what is wrong
      setAh((v) => (v === "" ? new Intl.NumberFormat(lang, { minimumFractionDigits: 2, maximumFractionDigits: 2, useGrouping: false }).format(l.telemetry.remaining_capacity_ah) : v));
      // the forced counter is not readable: an output seen off ends the „forced“ state shown
      if (!heaterOn(l.status)) forced.delete(a);
      setLive(l);
      return l;
    } catch (e) { if (gen.current === mine && same(a, lk)) setLive({ error: errorText(e, t) }); return null; }
    finally { if (gen.current === mine) setReading(false); }
  };
  useEffect(() => {
    setLive(null); setNote(null); setAh(""); setP79(null); setAsk(null); p79Asked.current.clear();
    if (!open || !usable || address === null) return;
    void reload(address);
  }, [open, usable, address, linkKey]); // eslint-disable-line react-hooks/exhaustive-deps
  // P79 only when no column shows it and the field is used: one 0x47 per pack; a pack that does not answer gets the
  // general hint
  const p79 = p79Shown ?? p79Read;
  useEffect(() => {
    if (!open || !usable || address === null || p79 !== null || !ah.trim() || p79Asked.current.has(address)) return;
    p79Asked.current.add(address);
    const a = address, lk = linkNow.current;
    api.parameters(a).then((p) => { if (same(a, lk)) setP79(p.parameters[P79]?.value ?? null); }, () => {});
  }, [open, usable, address, ah, p79]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!usable || address === null) return null;

  const pack = no(address);
  const nf = (d: number) => new Intl.NumberFormat(lang, { minimumFractionDigits: d, maximumFractionDigits: d });
  const tel = live && "telemetry" in live ? live.telemetry : null;
  const st = live && "status" in live ? live.status : null;
  const names = (ids: string[]) => ids.map((id) => helpTitle(id, lang)).join(", ");
  const failed = (e: unknown, refused = t("ctl.refused")) => {
    const r = refusal(e);
    // a silent pack: most likely a master that polls its slaves
    const text = r === "refused" ? refused : r === "invalid" ? t("ctl.invalid") : noAnswer(e) ? t("ctl.silent") : t("ctl.error", { err: errorText(e, t) });
    return { text, tone: "error" as const };
  };

  /** One action on pack `a`; a reply after a switch of pack, installation or connection is dropped. */
  const run = async (k: Action, a: number, f: (lk: string) => Promise<Note>) => {
    const lk = linkNow.current;
    setBusy(k); setNote(null); setNoteFor(k);
    try { const n = await f(lk); if (same(a, lk)) setNote(n); }
    catch (e) { if (same(a, lk)) setNote(failed(e, k === "remaining" ? t("ctl.remaining.refused") : undefined)); }
    finally { setBusy(null); }
  };

  const restart = (a: number) => run("restart", a, async (lk) => {
    const r: RestartResult = await api.packRestart(a);
    if (!same(a, lk)) return null;
    await reload(a);
    if (r.kind !== "not_reachable") { forced.delete(a); if (same(a, lk)) onChanged?.(a); }
    return r.kind === "back" ? { text: t("ctl.restart.back", { pack: no(a), s: nf(1).format(r.after_ms / 1000) }), tone: "ok" }
      : r.kind === "no_interruption" ? { text: t("ctl.restart.noInterruption"), tone: "warn" }
      : r.kind === "no_answer" ? { text: t("ctl.restart.noAnswer", { pack: no(a) }), tone: "error" }
      : r.kind === "sent_link_lost" ? { text: t("ctl.restart.linkLost", { err: errorText(r.error, t) }), tone: "error" }
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
    return on
      ? { text: heaterOn(l.status) ? t("ctl.heater.onDone") : t("ctl.heater.onNoOutput"), tone: heaterOn(l.status) ? "ok" : "warn" }
      : { text: heaterOn(l.status) ? t("ctl.heater.offStill") : t("ctl.heater.offDone"), tone: heaterOn(l.status) ? "warn" : "ok" };
  });

  const setRemaining = (a: number, raw: number) => run("remaining", a, async (lk) => {
    await api.packSetRemaining(a, raw);
    if (!same(a, lk)) return null;
    setNote({ text: t("ctl.remaining.taken"), tone: "ok" });
    // 0x42 shows the new value only after the next 1-s evaluation
    await wait(1500);
    const l = await reload(a);
    if (same(a, lk)) setAh("");
    return l ? { text: t("ctl.remaining.done", { soc: nf(1).format(l.telemetry.soc), ah: nf(2).format(l.telemetry.remaining_capacity_ah) }), tone: "ok" }
      : { text: t("ctl.sentUnread"), tone: "warn" };
  });

  const clear = (a: number) => run("clear", a, async () => {
    await api.packClearHistory(a);
    const l = await reload(a);
    return { text: l ? t("ctl.clear.done", { cycles: t("ctl.state.cycles", { count: l.telemetry.cycles }) }) : t("ctl.sentUnread"), tone: l ? "ok" : "warn" };
  });

  // what the clear dialog names is read right before it opens
  const askClear = async (a: number) => {
    const lk = linkNow.current;
    setBusy("clear");
    const l = await reload(a);
    setBusy(null);
    if (same(a, lk)) setAsk({ kind: "clear", live: l });
  };

  const off = st ? commandedOff(st) : false;
  const heaterForced = forced.has(address);
  const raw = remainingRaw(ah);
  const full = tel?.full_capacity_ah;
  const soc = raw !== null && full ? socOf(raw, full) : null;
  const under = soc !== null ? belowSocProtection(soc, p79) : null;
  const disabled = busy !== null || locked;
  const yesNo = (free: boolean) => <span className={free ? "text-ok" : "text-discharge"}>{t(free ? "ctl.state.free" : "ctl.state.blocked")}</span>;
  const blockers = st && !dischargeFree(st) ? blockingAlarms(st, "discharge") : [];
  const chargeBlockers = st && !chargeFree(st) ? blockingAlarms(st, "charge") : [];

  const noteLine = note && <StatusLine tone={note.tone}>{note.text}</StatusLine>;

  return (
    <div className="space-y-3 border-t border-line pt-3 text-sm">
      <SectionHead icon={Power} title={t("ctl.title")} help="topic.control" open={open} onToggle={() => setOpen(!open)} closedHint={t("ctl.closed")}
        onReload={open ? () => void reload(address) : undefined} reloading={reading} reloadDisabled={disabled} reloadLabel={t("ctl.reload")} readAt={readAt} />
      {open && (
        <div className="space-y-3">
          <div className="space-y-1 tabular-nums">
            {live === null ? <StatusLine>{t("ctl.reading")}</StatusLine>
              : "error" in live ? <StatusLine tone="error">{t("ctl.readError", { err: live.error })}</StatusLine>
              : <>
                <div>{t("ctl.state.command")} <span className={off ? "text-discharge" : "text-ok"}>{t(off ? "ctl.state.off" : "ctl.state.on")}</span>{!off && offReasons(live.status).length > 0 && <span className="text-discharge"> ({t("ctl.state.offSoon")})</span>}</div>
                <div>{t("ctl.state.discharge")} {yesNo(dischargeFree(live.status))}{!dischargeFree(live.status) && (off || blockers.length > 0) && <span className="text-discharge"> ({[...(off ? [t("ctl.state.offCommand")] : []), ...(blockers.length ? [names(blockers)] : [])].join(", ")})</span>}</div>
                <div>{t("ctl.state.charge")} {yesNo(chargeFree(live.status))}{chargeBlockers.length > 0 && <span className="text-discharge"> ({names(chargeBlockers)})</span>}</div>
                <div>{t("ctl.state.heater")} {t(heaterOn(live.status) ? "ctl.state.on" : "ctl.state.off")}</div>
                <div>{t("ctl.state.soc", { soc: nf(1).format(live.telemetry.soc), rem: nf(2).format(live.telemetry.remaining_capacity_ah), full: nf(2).format(live.telemetry.full_capacity_ah) })}</div>
                <div>{t("ctl.state.cycles", { count: live.telemetry.cycles })}</div>
                </>}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" disabled={disabled} onClick={() => setAsk({ kind: "restart" })} title={t("ctl.tip.restart")}>{busy === "restart" ? t("ctl.restart.running") : t("ctl.restart.button")}</Button>
            <Button size="sm" disabled={disabled || !st} onClick={() => (off ? void onOff(address, true) : setAsk({ kind: "off" }))} title={t(off ? "ctl.tip.on" : "ctl.tip.off")}>
              {busy === "on" || busy === "off" ? t("ctl.running") : t(off ? "ctl.onOff.on" : "ctl.onOff.off")}
            </Button>
            <Button size="sm" disabled={disabled} onClick={() => void heater(address, !heaterForced)} title={t(heaterForced ? "ctl.tip.heaterOff" : "ctl.tip.heaterOn")}>
              {busy === "heater" ? t("ctl.running") : t(heaterForced ? "ctl.heater.off" : "ctl.heater.on")}
            </Button>
          </div>
          {noteFor !== "remaining" && noteFor !== "clear" && noteLine}
          <p className="max-w-3xl text-muted">{t("ctl.heater.hint")}</p>

          <div className="space-y-1">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <label className="flex items-center gap-2">{t("ctl.remaining.field")}
                <input className={input} inputMode="decimal" value={ah} onChange={(e) => setAh(e.target.value)} placeholder="0,00" disabled={disabled} />
              </label>
              {soc !== null && full !== undefined && <span className="tabular-nums">{t("ctl.remaining.soc", { soc: nf(0).format(soc), full: nf(2).format(full) })}</span>}
              <Button size="sm" disabled={disabled || !remainingAllowed(raw, full)} onClick={() => raw !== null && void setRemaining(address, raw)} title={t("ctl.tip.remaining")}>
                {busy === "remaining" ? t("ctl.running") : t("ctl.remaining.button")}
              </Button>
              {raw !== null && full !== undefined && !remainingAllowed(raw, full) && <span className="text-discharge">{t("ctl.remaining.tooLarge")}</span>}
            </div>
            {under === true && <StatusLine tone="warn">{t("ctl.remaining.under", { p: nf(0).format(p79!) })}</StatusLine>}
            {under === null && soc !== null && <StatusLine>{t("ctl.remaining.underUnknown")}</StatusLine>}
            {noteFor === "remaining" && noteLine}
            <p className="max-w-3xl text-muted">{t("ctl.remaining.hint")}</p>
          </div>

          <div className="space-y-1">
            <Button size="sm" className="text-alarm" disabled={disabled} onClick={() => void askClear(address)} title={t("ctl.tip.clear")}>{busy === "clear" ? t("ctl.running") : t("ctl.clear.button")}</Button>
            {noteFor === "clear" && noteLine}
          </div>
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
                ? t("ctl.clear.what", { pack, cycles: t("ctl.state.cycles", { count: ask.live.telemetry.cycles }),
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
