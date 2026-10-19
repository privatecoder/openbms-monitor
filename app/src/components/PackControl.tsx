import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Power } from "lucide-react";
import { api, type PackLive, type PackPatch } from "../api";
import { refusal } from "../calibration";
import { writeError, belowSocProtection, blockingAlarms, chargeFree, commandedOff, cutoffOutcome, dischargeFree, energyText, errorText, heaterOn, lockState, noAnswer, offCutoffText, offExpect, offKind, offOutcome, offReasons, remainingAllowed, remainingRaw, socOf, cutoffError, shownOff, switchUnclear, relockable, relockAsk, type RestartResult } from "../control";
import { helpTitle } from "../help";
import { masterBroadcast } from "../power";
import { CUTOFF_BIT, getProfile } from "../profile";
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
/** „Aus“ carries what its confirmation was shown with: the kind and the extended off the backend checks again */
type Ask = { kind: "restart" } | { kind: "off"; lock: "cutoff" | "cutoffMaybe" | "soft"; expect?: boolean } | { kind: "clear"; live: PackLive | null };

// when the app forced the heater on, per connection and pack: the button reads „Heizung aus“ and the state line shows
// the expected end. The BMS's own counter (0x200000C8) is not readable over any command.
const forced = new Map<string, number>();
const HEATER_MS = 3600 * 1000;

/** „Steuern“: restart, on/off, heater, remaining capacity and clearing history and counters of the chosen pack; over the
 *  CAN socket (`mode` "power") only restart and on/off of a pack with the power and clock service. */
export function PackControlSection({ address, usable, mode = "full", powerLocked = null, cutoff = false, group0, locked, onBusy, onChanged, p79: p79Shown, linkKey }: {
  address: number | null; usable: boolean; locked?: boolean; onBusy?: (busy: boolean) => void;
  mode?: "full" | "power";
  /** CAN socket: Group0 bit 3 as last seen (true: locked, the buttons wait); null: not known, the backend reads it */
  powerLocked?: boolean | null;
  /** CAN socket: the stored pack cutoff (MAINT 1.6) proven for this pack on this connection */
  cutoff?: boolean;
  /** Group0 of the pack's parameters column read on this connection (bits 4/5: extended off, lock state) */
  group0?: number;
  /** after a restart, and after ein/aus where the pack may store a cutoff: the pack's parameters column (if shown) is read again */
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
  // Group0 and the patch of this section's own fresh 0x47 (before „Aus“, after ein/aus): newer than the column; `g0`
  // undefined: the read after an action failed, so neither it nor the column's older value says anything now
  const [fresh47, setFresh47] = useState<{ g0: number | undefined; patch: PackPatch | null } | null>(null);
  const gen = useRef(0);
  const current = useCurrent(address);
  const linkNow = useCurrent(linkKey);
  // still the pack and the connection an action or read began on
  const same = (a: number, lk: string) => current.current === a && linkNow.current === lk;
  const p79Asked = useRef(new Set<number>());
  useEffect(() => onBusy?.(busy !== null), [busy]); // eslint-disable-line react-hooks/exhaustive-deps

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
      if (!heaterOn(l.status)) forced.delete(`${lk}|${a}`);
      setLive(l);
      return l;
    } catch (e) { if (gen.current === mine && same(a, lk)) setLive({ error: errorText(e, t) }); return null; }
    finally { if (gen.current === mine) setReading(false); }
  };
  useEffect(() => {
    setLive(null); setNote(null); setAh(""); setP79(null); setAsk(null); setFresh47(null); p79Asked.current.clear();
    if (!open || !usable || address === null) return;
    void reload(address);
  }, [open, usable, address, linkKey]); // eslint-disable-line react-hooks/exhaustive-deps
  // a newer read of the column replaces this section's Group0 (also one marked unreadable)
  useEffect(() => setFresh47((f) => (f ? { ...f, g0: group0 } : f)), [group0]);
  // P79 only when no column shows it and the field is used: one 0x47 per pack; a pack that does not answer gets the
  // general hint
  const p79 = p79Shown ?? p79Read;
  useEffect(() => {
    if (!open || !usable || mode !== "full" || address === null || p79 !== null || !ah.trim() || p79Asked.current.has(address)) return;
    p79Asked.current.add(address);
    const a = address, lk = linkNow.current;
    api.parameters(a).then((p) => { if (same(a, lk)) setP79(p.parameters[P79]?.value ?? null); }, () => {});
  }, [open, usable, mode, address, ah, p79]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!usable || address === null) return null;
  const can = mode === "power";

  const pack = no(address);
  const nf = (d: number) => new Intl.NumberFormat(lang, { minimumFractionDigits: d, maximumFractionDigits: d });
  const tel = live && "telemetry" in live ? live.telemetry : null;
  const st = live && "status" in live ? live.status : null;
  const names = (ids: string[]) => ids.map((id) => helpTitle(id, lang)).join(", ");
  const failed = (e: unknown, refused = t("ctl.refused")) => {
    const r = refusal(e);
    // a silent pack: most likely a master that polls its slaves
    const text = r === "refused" ? refused : r === "invalid" ? t("ctl.invalid") : noAnswer(e) ? t("ctl.silent") : writeError(e, t, (err) => t("ctl.error", { err }));
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

  // after a restart over the CAN socket: the image (64/03; the master's profile) and String 2 are asked again, one after
  // the other; what they find reaches the views through their events
  const recheck = async (a: number) => {
    const g = getProfile()?.gen;
    if (g === undefined) return;
    try { await (a === 0 ? api.firmwareProfileCheck(g) : api.packProbe(a, g)); } catch { /* shown as before */ }
    try { await api.serialNumbers(a, true); } catch { /* the next read shows it */ }
  };

  const restart = (a: number) => run("restart", a, async (lk) => {
    const r: RestartResult = await api.packRestart(a);
    if (!same(a, lk)) return null;
    await reload(a);
    if (r.kind !== "not_reachable") { forced.delete(`${lk}|${a}`); if (same(a, lk)) onChanged?.(a); if (can) void recheck(a); }
    return r.kind === "back" ? { text: t("ctl.restart.back", { pack: no(a), s: nf(1).format(r.after_ms / 1000) }), tone: "ok" }
      : r.kind === "no_interruption" ? { text: t("ctl.restart.noInterruption"), tone: "warn" }
      : r.kind === "no_answer" ? { text: t("ctl.restart.noAnswer", { pack: no(a) }), tone: "error" }
      : r.kind === "sent_link_lost" ? { text: t("ctl.restart.linkLost", { err: errorText(r.error, t) }), tone: "error" }
      : { text: t("ctl.restart.notReachable"), tone: "error" };
  });

  // a fresh 0x47 of this pack (Group0 and the patch proof it names); undefined when not readable
  const read47 = async (a: number, lk: string) => {
    try {
      const p = await api.parameters(a);
      if (!same(a, lk)) return undefined;
      const f = { g0: p.function_switches[0], patch: p.patch ?? null };
      setFresh47(f);
      return f;
    } catch {
      if (same(a, lk)) setFresh47((f) => ({ g0: undefined, patch: f?.patch ?? null }));
      return undefined;
    }
  };
  // the pack cutoff matters here: proven over the CAN socket; on RS485-1/2 any pack may have MAINT 1.6, so every ein/aus
  // reads Group0 there (a polling master does not answer RS485-1/2 anyway)
  const aware = can ? cutoff : true;

  // „Aus“: where the pack may store a cutoff, Group0 is read fresh first and decides the confirmation; unreadable: no
  // command, read again (CAN socket without the cutoff: the soft off as before)
  // `relock`: the pack is off already (soft off) and gets the addressed off again to store the lock
  const askOff = async (a: number, relock = false) => {
    const lk = linkNow.current;
    if (!aware) { setAsk({ kind: "off", lock: "soft" }); return; }
    setBusy("off"); setNote(null); setNoteFor("off");
    const f = await read47(a, lk);
    setBusy(null);
    if (!same(a, lk)) return;
    const ask = offAsk(f);
    if (relock && ask) {
      const r = relockAsk(offKind(mode, cutoff, f?.patch, f?.g0), f?.g0);
      if (r === "locked") { setNote({ text: t("ctl.cutoff.relockLocked", { pack: no(a) }), tone: "ok" }); return; }
      if (r === "soft") { setNote({ text: t("ctl.cutoff.relockSoft"), tone: "warn" }); return; }
    }
    if (ask) setAsk(ask); else setNote({ text: t("ctl.cutoff.readFirst"), tone: "error" });
  };
  const offAsk = (f: { g0: number | undefined; patch: PackPatch | null } | undefined): Ask | null => {
    const lock = offKind(mode, cutoff, f?.patch, f?.g0);
    return lock === "unread" ? null : { kind: "off", lock, ...offExpect(f?.g0) };
  };

  const onOff = (a: number, on: boolean, expect?: boolean) => run(on ? "on" : "off", a, async (lk) => {
    try { await api.packOnOff(a, on, expect); }
    catch (e) {
      const err = cutoffError(e), unclear = switchUnclear(e), silent = unclear !== null;
      // after a refusal (RTN 08), a changed extended off or silence: the state and Group0 as they are now; no retry
      if (same(a, lk) && (aware || silent)) {
        await reload(a);
        const f = aware ? await read47(a, lk) : undefined;
        // the parameters column follows the pack (bits 4 and 5 of Group0)
        if (aware && same(a, lk)) onChanged?.(a);
        // the extended off changed since the confirmation: nothing was sent, ask again with the pack's state now
        if (err === "cutoff-changed" && same(a, lk)) {
          const ask = offAsk(f);
          if (ask) setAsk(ask);
          return { text: errorText(e, t), tone: "warn" };
        }
        if (silent && same(a, lk)) return { text: can ? errorText(e, t) : t(unclear === "link" ? "ctl.onOff.lostAnswer" : "ctl.onOff.noAnswer"), tone: "warn" };
      }
      throw e;
    }
    // the output follows at the next 1-s evaluation of the BMS
    await wait(2000);
    const l = await reload(a);
    // the lock state from that fresh 0x47, never from the ACK alone
    const f = aware ? await read47(a, lk) : undefined;
    // the parameters column follows the pack: bit 5 shows the lock state now
    if (aware && same(a, lk)) onChanged?.(a);
    const cut = aware ? cutoffOutcome(on, f?.g0, l?.status ?? null) : null;
    // the master's acknowledgement over the CAN socket covers only itself, not its broadcast to the slaves; with the pack
    // cutoff its lock stays its own and the master's charge limit follows only after the next polling
    const slaves = (n: NonNullable<Note>): Note => (masterBroadcast(mode, a)
      ? { text: `${n.text} ${t("ctl.onOff.slaves")}${cutoff ? ` ${t("ctl.cutoff.master")} ${t("ctl.cutoff.ccl")}` : ""}`, tone: n.tone } : n);
    // RS485-1/2: bit 5 only means a lock if the pack has MAINT 1.6; named where bit 4 or 5 is 0
    const maybe = (n: NonNullable<Note>): NonNullable<Note> => (!can && f?.g0 !== undefined && (f.g0 & 0x30) !== 0x30
      ? { text: `${n.text} ${t("ctl.cutoff.bit5", { v: (f.g0 >> 5) & 1, state: t((f.g0 & 0x20) === 0 ? "ctl.cutoff.bit5Locked" : "ctl.cutoff.bit5Free") })}`, tone: n.tone } : n);
    if (can && cut === "stillLocked") return slaves({ text: t("ctl.cutoff.stillLocked"), tone: "error" });
    // off: a stored lock (bit 5 at 0) or an unreadable Group0 never reads „Laden möglich“, on either way
    const offText = !on ? offCutoffText(cut, !can) : null;
    if (offText) return slaves({ text: t(offText, { pack: no(a) }), tone: cut === "locked" && l ? "ok" : "warn" });
    const unread = cut === "unread" ? ` ${t("ctl.cutoff.unread")}` : "";
    const withUnread = (n: NonNullable<Note>): NonNullable<Note> => (unread ? { text: `${n.text}${unread}`, tone: n.tone === "ok" ? "warn" : n.tone } : n);
    if (!l) return slaves(maybe(withUnread({ text: t("ctl.sentUnread"), tone: "warn" })));
    if (on && commandedOff(l.status)) {
      const why = offReasons(l.status);
      return slaves(maybe(withUnread({ text: t("ctl.onOff.offAgain", { reason: why.length ? names(why) : t("ctl.onOff.protection") }), tone: "error" })));
    }
    if (on && can && cut === "releasedHeld") return slaves({ text: t("ctl.cutoff.releasedHeld"), tone: "warn" });
    if (on) {
      const n = dischargeFree(l.status) ? { text: t("ctl.onOff.onDone"), tone: "ok" as const } : { text: t("ctl.onOff.onBlocked"), tone: "warn" as const };
      return slaves(maybe(withUnread(can && cut === "released" ? { text: `${t("ctl.cutoff.released")} ${n.text}`, tone: n.tone } : n)));
    }
    if (can && cut === "notLocked") return slaves({ text: t("ctl.cutoff.notLocked"), tone: "warn" });
    // „done“ only when the read shows it: charging current may have lifted „off“ meanwhile
    const out = offOutcome(l.status);
    return slaves(maybe(withUnread(out === "off" ? { text: t("ctl.onOff.offDone", { pack: no(a) }), tone: "ok" }
      : { text: t(out === "on" ? "ctl.onOff.offBackOn" : "ctl.onOff.offUnclear", { pack: no(a) }), tone: "warn" })));
  });

  const heater = (a: number, on: boolean) => run("heater", a, async (lk) => {
    await api.packHeater(a, on);
    if (on) forced.set(`${lk}|${a}`, Date.now()); else forced.delete(`${lk}|${a}`);
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
    if (same(a, lk)) setAh("");
    const l = await reload(a);
    if (!l) return { text: t("ctl.sentUnread"), tone: "warn" };
    const shown = { soc: nf(1).format(l.telemetry.soc), ah: nf(2).format(l.telemetry.remaining_capacity_ah) };
    // the SOC logic goes on from the value (current, full/empty rules): more than 1 % of full away means it was replaced
    const full = l.telemetry.full_capacity_ah;
    if (Math.abs(l.telemetry.remaining_capacity_ah * 100 - raw) > Math.max(100, full)) {
      const why = offReasons(l.status);
      return { text: t("ctl.remaining.replaced", { ...shown, reason: why.length ? names(why) : t("ctl.remaining.replacedUnknown") }), tone: "warn" };
    }
    return { text: t("ctl.remaining.done", shown), tone: "ok" };
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

  // the newest Group0 seen: this section's own read, else the parameters column
  const g0 = fresh47 ? fresh47.g0 : group0;
  const lock = lockState(can && cutoff, !can, g0);
  // a stored lock is released by „Ein“, whatever the off bit shows
  // „maybeActive“ (RS485-1/2, bit 5 only means a lock with 1.6) is an uncertainty line, never the command state
  const off = shownOff(st, lock);
  const extendedOff = can && cutoff && g0 !== undefined && (g0 & CUTOFF_BIT) === 0;
  const relock = relockable(off, lock, can, cutoff, g0);
  const forcedAt = forced.get(`${linkKey}|${address}`);
  const heaterForced = forcedAt !== undefined && Date.now() < forcedAt + HEATER_MS;
  const heaterEnd = heaterForced ? new Date(forcedAt! + HEATER_MS).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" }) : null;
  const raw = remainingRaw(ah);
  const full = tel?.full_capacity_ah;
  const soc = raw !== null && full ? socOf(raw, full) : null;
  const under = soc !== null ? belowSocProtection(soc, p79) : null;
  const disabled = busy !== null || locked;
  // over the CAN socket a lock seen in the last state holds restart and on/off back (Group0 bit 3)
  const powerHeld = can && powerLocked === true;
  const yesNo = (free: boolean) => <span className={free ? "text-ok" : "text-discharge"}>{t(free ? "ctl.state.free" : "ctl.state.blocked")}</span>;
  const blockers = st && !dischargeFree(st) ? blockingAlarms(st, "discharge") : [];
  const chargeBlockers = st && !chargeFree(st) ? blockingAlarms(st, "charge") : [];

  const noteLine = note && <StatusLine tone={note.tone}>{note.text}</StatusLine>;

  return (
    <div className="space-y-3 border-t border-line pt-3 text-sm">
      <SectionHead icon={Power} title={t("ctl.title")} help="topic.control" open={open} onToggle={() => setOpen(!open)} closedHint={t(can ? "ctl.closedCan" : "ctl.closed")}
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
                <div>{t("ctl.state.heater")} {t(heaterOn(live.status) ? "ctl.state.on" : "ctl.state.off")}{heaterOn(live.status) && heaterEnd && <span className="text-muted"> ({t("ctl.state.heaterUntil", { time: heaterEnd })})</span>}</div>
                <div>{t("ctl.state.soc", { soc: nf(1).format(live.telemetry.soc), rem: nf(2).format(live.telemetry.remaining_capacity_ah), full: nf(2).format(live.telemetry.full_capacity_ah) })}</div>
                <div>{t("ctl.state.cycles", { count: live.telemetry.cycles })}</div>
                {lock && <div>{t("ctl.cutoff.state")} <span className={lock === "none" ? "text-ok" : lock === "maybeActive" ? "text-discharge" : "font-semibold text-alarm"}>{t(`ctl.cutoff.${lock}`)}</span></div>}
                </>}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" disabled={disabled || powerHeld} onClick={() => setAsk({ kind: "restart" })} title={t("ctl.tip.restart")}>{busy === "restart" ? t("ctl.restart.running") : t("ctl.restart.button")}</Button>
            <Button size="sm" disabled={disabled || powerHeld || !st} onClick={() => (off ? void onOff(address, true) : void askOff(address))}
              title={t(off ? (lock === "active" ? "ctl.cutoff.tipOn" : "ctl.tip.on") : extendedOff ? "ctl.cutoff.tipOff" : masterBroadcast(mode, address) ? "ctl.tip.offMaster" : "ctl.tip.off")}>
              {busy === "on" || busy === "off" ? t("ctl.running") : t(off ? "ctl.onOff.on" : "ctl.onOff.off")}
            </Button>
            {relock && (
              <Button size="sm" disabled={disabled || powerHeld || !st} onClick={() => void askOff(address, true)} title={t("ctl.cutoff.tipRelock")}>
                {t("ctl.cutoff.offConfirm")}
              </Button>
            )}
            {!can && (
              <Button size="sm" disabled={disabled} onClick={() => void heater(address, !heaterForced)} title={t(heaterForced ? "ctl.tip.heaterOff" : "ctl.tip.heaterOn")}>
                {busy === "heater" ? t("ctl.running") : t(heaterForced ? "ctl.heater.off" : "ctl.heater.on")}
              </Button>
            )}
          </div>
          {powerHeld && <StatusLine tone="warn">{t("ctl.powerLocked")}</StatusLine>}
          {noteFor !== "remaining" && noteFor !== "clear" && noteLine}
          <p className="max-w-3xl text-muted">{t(can ? "ctl.canHint" : "ctl.heater.hint")}</p>
          {!can && <>
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
          </>}
        </div>
      )}
      {ask && (
        <Confirm
          title={ask.kind === "off" && ask.lock !== "soft" ? t("ctl.cutoff.offTitle", { pack }) : t(`ctl.${ask.kind}.title`, { pack })}
          danger={ask.kind === "clear" || (ask.kind === "off" && ask.lock === "cutoff")}
          confirm={ask.kind === "off" && ask.lock !== "soft" ? t("ctl.cutoff.offConfirm") : t(`ctl.${ask.kind}.confirm`)}
          onCancel={() => setAsk(null)}
          onConfirm={() => {
            setAsk(null);
            if (ask.kind === "restart") void restart(address);
            else if (ask.kind === "off") void onOff(address, false, ask.expect);
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
          ) : ask.kind === "off" && ask.lock !== "soft" ? (
            <>
              <p className="font-medium">{t("ctl.cutoff.offText")}{ask.lock === "cutoffMaybe" ? ` ${t("ctl.cutoff.maybe")}` : ""}</p>
              <p>{t("ctl.cutoff.offMore", { pack })}</p>
              {masterBroadcast(mode, address) && <p>{t("ctl.cutoff.master")} {t("ctl.cutoff.ccl")}</p>}
            </>
          ) : <p>{t(ask.kind === "off" && masterBroadcast(mode, address) ? "ctl.off.masterText" : `ctl.${ask.kind}.text`, { pack })}</p>}
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
