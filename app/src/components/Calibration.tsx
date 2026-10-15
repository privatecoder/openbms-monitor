import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, Crosshair, Network, RefreshCw } from "lucide-react";
import { api, type DeviceInfo, type PackLive } from "../api";
import { CAN_PROTOCOLS, cellMv, cellStats, centiAmps, currentAllowed, flagAfter, notCalibrated, parseNumber, protocolOf, protocolSetResult, refusal, MIN_CURRENT_A, NOT_CALIBRATED_BIT, type Calibration, type FlagAfter } from "../calibration";
import { errorText } from "../control";
import { InfoIcon, helpTitle } from "../help";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";

const input = "h-8 w-28 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge";
type Note = { text: string; error: boolean } | null;

/** Props shared by the write lines of the parameters view: `locked` while another write runs, `onBusy` reports this one's. */
type WriteProps = { address: number | null; usable: boolean; locked?: boolean; onBusy?: (busy: boolean) => void };

/** The address as of the last render, so a reply that arrives after a pack switch can be dropped. */
export function useCurrent<T>(v: T) {
  const r = useRef(v);
  r.current = v;
  return r;
}

/** The 0x51 text as a name: offered protocols by their inverters, anything else (7 MUST, 8…20) as the pack sends it. */
const protocolName = (text: string) => protocolOf(text)?.name ?? text;

/** CAN inverter protocol of the chosen pack (0x51) and setting it (0xA6, 1…6). */
export function CanProtocolLine({ address, usable, locked, onBusy }: WriteProps) {
  const { t } = useTranslation();
  const [device, setDevice] = useState<DeviceInfo | { error: string } | null>(null);
  const [choice, setChoice] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<Note>(null);
  const [reload, setReload] = useState(0);
  const current = useCurrent(address);
  useEffect(() => onBusy?.(busy), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    setDevice(null); setNote(null); setChoice(null);
    if (address === null || !usable) return;
    let alive = true;
    api.scan([address]).then((r) => {
      if (!alive) return;
      const d = r[0]?.[1];
      setDevice(d ?? { error: t("can.noAnswer") });
      setChoice(protocolOf(d?.can_protocol)?.n ?? null);
    }, (e) => alive && setDevice({ error: String(e) }));
    return () => { alive = false; };
  }, [address, usable, reload]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!usable || address === null) return null;

  const set = async () => {
    if (choice === null) return;
    const a = address;
    setBusy(true); setNote(null);
    try {
      const r = await api.setCanProtocol(a, choice);
      if (current.current !== a) return; // another pack is shown now
      const d = r.device;
      if (d) setDevice(d);
      const res = protocolSetResult(choice, d?.can_protocol);
      setNote({
        text: res === "done" ? t("can.done", { name: protocolName(d!.can_protocol) })
          : res === "mismatch" ? t("can.mismatch", { text: d!.can_protocol })
          : t("can.unread", { err: r.read_error ?? "" }),
        error: res === "mismatch",
      });
    } catch (e) {
      if (current.current === a) setNote({ text: refusal(e) ? t("can.refused") : t("can.error", { err: errorText(e, t) }), error: true });
    } finally { setBusy(false); }
  };

  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="flex items-center gap-1.5 font-medium"><Network className="h-4 w-4" aria-hidden />{t("can.title")}<InfoIcon id="topic.calibration" /></span>
        {device === null ? <span className="text-muted">{t("can.reading")}</span>
          : "error" in device ? <span className="text-alarm">{t("can.readError", { err: device.error })}</span>
          : <span>{t("can.current", { name: protocolName(device.can_protocol) || "–" })}</span>}
        <Button variant="ghost" size="icon" className="-ml-2 h-8 w-8" disabled={busy || locked || device === null} onClick={() => setReload((n) => n + 1)}
          aria-label={t("can.reload")} title={t("can.reload")}><RefreshCw className="h-4 w-4" /></Button>
        <select value={choice ?? ""} onChange={(e) => setChoice(e.target.value ? Number(e.target.value) : null)} disabled={busy || locked} aria-label={t("can.choose")}
          className="h-8 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge">
          {choice === null && <option value="">{t("can.choose")}</option>}
          {CAN_PROTOCOLS.map((p) => <option key={p.n} value={p.n}>{p.n} · {p.name}</option>)}
        </select>
        <Button size="sm" disabled={busy || locked || choice === null} onClick={set}>{busy ? t("can.setting") : t("can.set")}</Button>
      </div>
      <p className="text-muted">{t("can.hint")}</p>
      {note && <p className={note.error ? "text-alarm" : "text-ok"}>{note.text}</p>}
    </div>
  );
}

type Kind = Calibration["kind"];
const FLAG_TEXT: Record<FlagAfter, string> = { gone: "cal.flagGone", still: "cal.flagStill", unread: "cal.flagUnread" };
type Outcome = { ok: true; value: string; flag: FlagAfter } | { ok: false; text: string };

/** „Kalibrieren (Experten)“: zero point, current and voltage (0xA0) with the pack's live values, read every 2 s while open. */
export function CalibrationSection({ address, usable, locked, onBusy }: WriteProps) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [open, setOpen] = useState(false);
  const [live, setLive] = useState<PackLive | { error: string } | null>(null);
  const [busy, setBusy] = useState<Kind | null>(null);
  const [outcome, setOutcome] = useState<Partial<Record<Kind, Outcome>>>({});
  const [amps, setAmps] = useState("");
  const [volts, setVolts] = useState("");
  const gen = useRef(0);
  const current = useCurrent(address);
  useEffect(() => onBusy?.(busy !== null), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  // values measured on one pack must never be sent to another
  useEffect(() => { setAmps(""); setVolts(""); setOutcome({}); }, [address]);

  // one read after the other, 2 s apart, so a slow answer cannot pile up requests
  useEffect(() => {
    setLive(null); setOutcome({});
    if (!open || !usable || address === null) return;
    const mine = ++gen.current;
    let timer: number | undefined;
    const tick = async () => {
      try { const l = await api.packLive(address); if (gen.current === mine) setLive(l); }
      catch (e) { if (gen.current === mine) setLive({ error: String(e) }); }
      if (gen.current === mine) timer = window.setTimeout(tick, 2000);
    };
    void tick();
    return () => { gen.current++; window.clearTimeout(timer); };
  }, [open, usable, address]);
  if (!usable || address === null) return null;

  const nf = (d: number) => new Intl.NumberFormat(lang, { minimumFractionDigits: d, maximumFractionDigits: d });
  const tel = live && "telemetry" in live ? live.telemetry : null;
  const events = live && "status" in live ? live.status.events : undefined;
  const stats = tel ? cellStats(tel.cell_voltages) : null;
  const cells = tel?.cell_voltages.length ?? 0;
  const measured = parseNumber(amps);
  const packV = parseNumber(volts);
  const mv = cellMv(packV, cells);
  const flagName = (k: Kind) => helpTitle(`alarm.ev8.b${NOT_CALIBRATED_BIT[k]}`, lang);

  const run = async (cal: Calibration) => {
    const k = cal.kind;
    const a = address;
    setBusy(k); setOutcome((o) => ({ ...o, [k]: undefined }));
    try {
      await api.bmsCalibrate(a, cal);
      // read again at once: the new value and whether the flag is gone (RAM; stored shows after a restart)
      const l = await api.packLive(a).catch(() => null);
      if (current.current !== a) return; // another pack is shown now
      if (l) setLive(l);
      const value = !l ? "–" : k === "voltage" ? `${nf(3).format(cellStats(l.telemetry.cell_voltages)?.mean ?? 0)} V` : `${nf(2).format(l.telemetry.current)} A`;
      setOutcome((o) => ({ ...o, [k]: { ok: true, value, flag: flagAfter(l?.status.events, k) } }));
    } catch (e) {
      if (current.current !== a) return;
      const r = refusal(e);
      const text = r === "refused" ? t("cal.refused", { cond: t(`cal.cond.${k}`) }) : r === "invalid" ? t("cal.invalid") : t("cal.error", { err: errorText(e, t) });
      setOutcome((o) => ({ ...o, [k]: { ok: false, text } }));
    } finally { setBusy(null); }
  };

  const flagLine = (k: Kind) => events && (
    <span className={notCalibrated(events, k) ? "text-discharge" : "text-muted"}>
      {t(notCalibrated(events, k) ? "cal.flagOn" : "cal.flagOff", { name: flagName(k) })}
    </span>
  );
  const result = (k: Kind) => {
    const o = outcome[k];
    if (!o) return null;
    return o.ok
      ? <p className="text-ok">{t("cal.done", { value: o.value })} {t(FLAG_TEXT[o.flag], { name: flagName(k) })} <span className="text-muted">{t("cal.ram")}</span></p>
      : <p className="text-alarm">{o.text}</p>;
  };
  const diff = measured !== null && tel ? measured - tel.current : null;

  return (
    <div className="space-y-3 border-t border-line pt-3 text-sm">
      <div className="flex items-center gap-1.5">
        <button onClick={() => setOpen(!open)} aria-expanded={open} className="flex items-center gap-1.5 font-medium hover:text-ink">
          {open ? <ChevronDown className="h-4 w-4" aria-hidden /> : <ChevronRight className="h-4 w-4" aria-hidden />}
          <Crosshair className="h-4 w-4" aria-hidden />{t("cal.title")}
        </button>
        <InfoIcon id="topic.calibration" />
        {!open && <span className="text-muted">{t("cal.closed")}</span>}
      </div>
      {open && (
        <div className="space-y-3">
          <p className="max-w-3xl text-muted">{t("cal.intro")}</p>
          {live && "error" in live && <p className="text-alarm">{t("cal.liveError", { err: live.error })}</p>}

          <Block title={t("cal.zero.title")} guide={t("cal.zero.guide")}>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <Button size="sm" disabled={!!busy || locked} onClick={() => run({ kind: "zero_point" })}>{busy === "zero_point" ? t("cal.running") : t("cal.zero.button")}</Button>
              {tel && <span className="tabular-nums text-muted">{t("cal.bmsCurrent", { a: nf(2).format(tel.current) })}</span>}
              {flagLine("zero_point")}
            </div>
            <p className="text-muted">{t("cal.zero.limit")}</p>
            {result("zero_point")}
          </Block>

          <Block title={t("cal.current.title")} guide={t("cal.current.guide", { min: MIN_CURRENT_A })}>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <label className="flex items-center gap-2">{t("cal.current.field")}
                <input className={input} inputMode="decimal" value={amps} onChange={(e) => setAmps(e.target.value)} placeholder="25,0" />
              </label>
              <span className="tabular-nums">{tel ? t("cal.bmsCurrent", { a: nf(2).format(tel.current) }) : t("cal.reading")}</span>
              {diff !== null && <span className={cn("tabular-nums", Math.abs(diff) >= 3 ? "text-alarm" : "text-muted")}>{t("cal.current.diff", { d: nf(2).format(diff) })}</span>}
            </div>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <Button size="sm" disabled={!!busy || locked || !currentAllowed(measured, tel?.current)} onClick={() => measured !== null && run({ kind: "current", centi_amps: centiAmps(measured) })} className="tabular-nums">
                {busy === "current" ? t("cal.running") : measured !== null ? t("cal.current.button", { a: nf(2).format(measured) }) : t("cal.current.buttonEmpty")}
              </Button>
              {measured !== null && tel && !currentAllowed(measured, tel.current) && <span className="text-discharge">{t("cal.current.needs", { min: MIN_CURRENT_A })}</span>}
              {flagLine("current")}
            </div>
            <p className="text-muted">{t("cal.current.limit")}</p>
            {result("current")}
          </Block>

          <Block title={t("cal.voltage.title")} guide={t("cal.voltage.guide")}>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <label className="flex items-center gap-2">{t("cal.voltage.field")}
                <input className={input} inputMode="decimal" value={volts} onChange={(e) => setVolts(e.target.value)} placeholder="52,80" />
              </label>
              {mv !== null && <span className="tabular-nums">{t("cal.voltage.perCell", { v: nf(3).format(mv / 1000) })}</span>}
            </div>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2 tabular-nums">
              {tel && stats ? (
                <>
                  <span>{t("cal.voltage.mean", { v: nf(3).format(stats.mean) })}</span>
                  <span className={stats.spread * 1000 >= 20 ? "text-alarm" : "text-muted"}>{t("cal.voltage.spread", { mv: Math.round(stats.spread * 1000) })}</span>
                  <span className="text-muted">{t("cal.voltage.port", { v: nf(2).format(tel.port_voltage) })}</span>
                </>
              ) : <span className="text-muted">{t("cal.reading")}</span>}
            </div>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <Button size="sm" disabled={!!busy || locked || mv === null} onClick={() => mv !== null && run({ kind: "voltage", cell_mv: mv })} className="tabular-nums">
                {busy === "voltage" ? t("cal.running") : mv !== null ? t("cal.voltage.button", { v: nf(3).format(mv / 1000) }) : t("cal.voltage.buttonEmpty")}
              </Button>
              {flagLine("voltage")}
            </div>
            <p className="text-muted">{t("cal.voltage.limit")}</p>
            {result("voltage")}
          </Block>
        </div>
      )}
    </div>
  );
}

function Block({ title, guide, children }: { title: string; guide: string; children: ReactNode }) {
  return (
    <section className="space-y-2 rounded-md border border-line bg-sunken/30 p-3">
      <h3 className="font-medium">{title}</h3>
      <p className="max-w-3xl">{guide}</p>
      {children}
    </section>
  );
}
