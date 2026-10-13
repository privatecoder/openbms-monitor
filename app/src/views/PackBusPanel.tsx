import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, LoaderCircle, Lock, Play, TriangleAlert } from "lucide-react";
import { api, type EndResult, type EndStep, type MaintOverview } from "../api";
import { activityStatus, errorKey, failingText, packsDerived, packsLine, parsePacks, slavesOf, workUnchanged, type Activity, type Flow, type FlowStep, type Monitor, type Reason } from "../maint";
import type { useMaint } from "../maintStore";
import { Button } from "../components/ui/button";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { MaintSetup } from "./MaintSetup";

const input = "h-9 w-20 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

/** One numbered step: ✓ done, ▶ current, 🔒 locked with the reason. */
function Item({ n, title, step, children }: { n: number | null; title: string; step: Pick<FlowStep, "state" | "reason" | "notNeeded">; children?: ReactNode }) {
  const { t } = useTranslation();
  const Icon = step.state === "done" ? CircleCheck : step.state === "current" ? Play : Lock;
  return (
    <li className={cn("space-y-2 rounded-md border p-3", step.state === "current" ? "border-charge" : "border-line", step.state === "locked" && "text-muted")}>
      <p className="flex flex-wrap items-center gap-2 font-medium">
        <Icon className={cn("h-4 w-4 shrink-0", step.state === "done" ? "text-ok" : step.state === "current" ? "text-charge" : "text-muted")} aria-label={t(`maint.flow.state.${step.state}`)} />
        {n !== null && <span className="text-muted">{n}.</span>}{title}
        {step.notNeeded && <span className="font-normal text-muted">· {t("maint.flow.notNeeded")}</span>}
      </p>
      {step.reason && <p className={cn("text-sm", step.state === "current" ? "text-discharge" : "text-muted")}>{t(step.reason.key, step.reason.params)}</p>}
      {children}
    </li>
  );
}

/** A button of a panel action: a spinner while its action runs (not greyed out then). */
function ActButton({ spinning, children, className, ...props }: Parameters<typeof Button>[0] & { spinning: boolean }) {
  return <Button {...props} className={cn(spinning && "disabled:opacity-100", className)}>{spinning && <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden />}{children}</Button>;
}

/** The status line under a button: the running phase, then the result of its last action. */
function StatusLine({ activity }: { activity: Activity }) {
  const { t } = useTranslation();
  const st = activityStatus(activity, t);
  if (!st) return null;
  const Icon = st.kind === "running" ? LoaderCircle : st.kind === "ok" ? CircleCheck : TriangleAlert;
  return (
    <p role="status" className={cn("flex items-start gap-1.5", st.kind === "running" ? "text-muted" : st.kind === "ok" ? "text-ok" : "text-alarm")}>
      <Icon className={cn("mt-0.5 h-4 w-4 shrink-0", st.kind === "running" && "animate-spin")} aria-hidden />{st.text}
    </p>
  );
}

export function PackBusPanel({ maint, flow, port, ports, onPortsChanged, monitor, focusEnd, onFocusDone, busy, setBusy, inverter, setInverter, measuredAt, setMeasuredAt, activity, setActivity, noControl, setNoControl, singlePack, setSinglePack }: {
  maint: ReturnType<typeof useMaint>;
  flow: Flow;
  /** the RS485-1/2 port of the installation ("" = not set up); chosen only in the setup form */
  port: string;
  /** the serial ports plugged in now */
  ports: string[];
  /** the setup was saved: the ports may have changed */
  onPortsChanged: () => void;
  monitor: Monitor;
  /** "End" in the banner was pressed: scroll to the End section (or the panel) and highlight it briefly */
  focusEnd: number;
  onFocusDone: () => void;
  busy: boolean;
  setBusy: (v: boolean) => void;
  inverter: boolean;
  setInverter: (v: boolean) => void;
  measuredAt: number | null;
  setMeasuredAt: (v: number | null) => void;
  /** the running or last panel action with its phase and result; lives in the parent, so it outlasts the
   *  session (End closes it, the panel is keyed by session) and goes with a view change */
  activity: Activity | null;
  setActivity: (f: (a: Activity | null) => Activity | null) => void;
  noControl: boolean;
  setNoControl: (v: boolean) => void;
  singlePack: boolean;
  setSinglePack: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  const o = maint.overview;
  // the port in use right now (not the render value of an action that began earlier)
  const portRef = useRef(port);
  portRef.current = port;
  // packs on the bus, master included: no default, the user types it (an empty or out-of-range value keeps the
  // buttons locked); the backend gets N = packs − 1
  const [nText, setNText] = useState("");
  const packs = parsePacks(nText);
  const n = packs === null ? null : slavesOf(packs);
  // the N editor (step 4 or the End section) belongs to one session; a new (or closed) session starts with it closed
  const [editing, setEditing] = useState<"start" | "end" | null>(null);
  const sessionId = o?.session?.id ?? null;
  useEffect(() => { setEditing(null); setNText(""); }, [sessionId]);
  const [dipConfirmed, setDipConfirmed] = useState(false);
  // patched-off with N > 0: the manual DIP way instead of activating
  const [dipInstead, setDipInstead] = useState(false);
  const [setup, setSetup] = useState(false);
  // set when maintEnd was refused with end-sequential (one adapter for both buses)
  const [stepwise, setStepwise] = useState(false);
  // a sub-step of End failed: the overview's end_next is the restart point
  const [endFailed, setEndFailed] = useState(false);
  useEffect(() => { setEndFailed(false); }, [sessionId]);
  const panelRef = useRef<HTMLElement>(null);
  const endRef = useRef<HTMLElement>(null);
  const [highlight, setHighlight] = useState<"end" | "panel" | null>(null);
  const highlightTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(highlightTimer.current), []);
  useEffect(() => {
    if (!focusEnd) return;
    const target = endRef.current ? "end" : "panel";
    (endRef.current ?? panelRef.current)?.scrollIntoView({ behavior: "smooth", block: "start" });
    setHighlight(target);
    onFocusDone();
    clearTimeout(highlightTimer.current);
    highlightTimer.current = setTimeout(() => setHighlight(null), 2000);
  }, [focusEnd]); // eslint-disable-line react-hooks/exhaustive-deps

  /** Run a panel action: `key` names its button (spinner and status line there), `action` the backend command its
   *  progress events name. `f` returns the new overview and the result text (null: no result line). `still()` tells
   *  an action, after an await, that the installation and session it began with are still in use; if not, it must
   *  not write (overview, measurement, inverter). The overview is reloaded after every action. */
  type Done = { overview?: MaintOverview; ok?: boolean; text?: string } | null;
  const run = async (key: string, action: string, f: (still: () => boolean) => Promise<Done>) => {
    // the RS485-1/2 port belongs to the work too: a result measured on another port must not be kept
    const installation = o?.installation.id ?? "";
    const start = { installation, session: o?.session?.id ?? null, port };
    const still = () => workUnchanged(start, { ...maint.current(), port: portRef.current });
    setBusy(true);
    setActivity(() => ({ installation, key, action, running: true }));
    let result: Activity["result"];
    try {
      const r = await f(still);
      if (r?.overview && still()) maint.setOverview(r.overview);
      if (r) result = { ok: r.ok ?? true, text: r.text ?? t("maint.result.done") };
    } catch (e) {
      const k = errorKey(e); result = { ok: false, text: t(k.key, k.params) };
    } finally {
      await maint.refresh(installation);
      setActivity((a) => (a && a.key === key && a.installation === installation ? { installation, key, action, running: false, result } : a));
      setBusy(false);
    }
  };
  const mine = (key: string) => !!o && activity?.key === key && activity.installation === o.installation.id;
  const spinning = (key: string) => mine(key) && !!activity?.running;
  const status = (key: string) => (mine(key) && activity ? <StatusLine activity={activity} /> : null);

  if (!o) return <section ref={panelRef} className={cn("space-y-2 rounded-md border p-4 text-sm text-muted transition-colors", highlight ? "border-charge ring-2 ring-charge" : "border-line")}>{t("maint.panel.noInstallation")}</section>;
  const st = flow.steps;
  const s = flow.state;
  const stored = o.state?.state;
  const idle = flow.idle;
  const may = (step: FlowStep) => idle && step.state === "current";
  // harmless repeats: available on the current and on a done step
  const repeat = (step: FlowStep) => idle && step.state !== "locked";
  const sequential = flow.replug || stepwise;
  const reasonText = (r: Reason | undefined) => (r ? t(r.key, r.params) : "");
  const control = o.installation.control;
  // a serial port that is not in the list of available ports: "… (not plugged in)"
  const plugged = (name: string, serial: boolean) => (serial && !ports.includes(name) ? `${name} (${t("maint.flow.notPlugged")})` : name);

  const endNoteOf = (r: EndResult) => r.ok ? t(o.session?.expected_slaves === 0 ? "maint.panel.endOkZero" : "maint.panel.endOk") : r.missing.length ? t("maint.panel.endMissing", { list: r.missing.join(", ") }) : t("maint.panel.endFailed", { what: failingText(r.failing ?? "", t) });
  const packsField = <input type="number" min={1} max={16} className={input} value={nText} onChange={(e) => setNText(e.target.value)} aria-label={t("maint.panel.packs")} />;
  // the derived slave count right under the input, before a valid entry nothing
  const derived = packs !== null && <p className="font-medium">{packsDerived(packs, t)}</p>;
  const packsInput = <div className="space-y-1"><label className="flex items-center gap-2">{t("maint.panel.packs")}{packsField}</label>{derived}</div>;

  const check = () => run("check", "check", async (still) => {
    let r: MaintOverview | null = null;
    try { r = await maint.check(o.installation.id, true); }
    finally {
      // a failed check takes the inverter statement with it; so does an answer that leaves the state unknown
      if (still() && (!r || r.state?.state === "unknown")) setInverter(false);
    }
    const m = r?.state?.state ?? "unknown";
    return { text: t("maint.result.check", { state: t(`maint.state.${m}`, { slaves: r?.state?.slaves ?? 0 }) }) };
  });
  const scan = () => run("scan", "scan", async (still) => {
    const slave = await api.maintScanSlaves(o.installation.id, port);
    if (still()) maint.setScan(o.installation.id, port, slave);
    return { text: slave === null ? t("maint.result.noSlave") : t("maint.result.slave", { addr: slave }) };
  });
  const endStep = (step: EndStep) => run(`end-step:${step}`, "end-step", async (still) => {
    let r: EndResult;
    try { r = await api.maintEndStep(o.installation.id, port, step); }
    catch (e) { if (still()) setEndFailed(true); throw e; }
    if (still()) {
      setEndFailed(!r.ok);
      if (step === "values") { setInverter(false); setMeasuredAt(null); }
    }
    const text = step === "values" || !r.ok ? endNoteOf(r) : t("maint.result.step", { step: t(`maint.panel.endStep.${step}`) });
    return { ok: r.ok, text };
  });
  const end = () => run("end", "end", async (still) => {
    let r: EndResult;
    try { r = await api.maintEnd(o.installation.id, port); }
    catch (e) {
      if (errorKey(e).key === "maint.err.refused.end-sequential") { if (still()) setStepwise(true); return null; }
      throw e;
    }
    if (still()) { setInverter(false); setMeasuredAt(null); }
    return { ok: r.ok, text: endNoteOf(r) };
  });
  // correcting N: in step 4 (session branch) and in the End section, whenever a session is open
  const slavesEditor = (where: "start" | "end") => {
    if (!o.session) return null;
    const key = `set-slaves:${where}`;
    const current = o.session.expected_slaves + 1;
    return (
      <div className="space-y-1">
        <div className="flex flex-wrap items-center gap-3">
          <span>{packsLine(current, t)}</span>
          {editing === where ? (
            <>
              <label className="flex items-center gap-2">{t("maint.panel.packs")}{packsField}</label>
              <ActButton variant="secondary" size="sm" spinning={spinning(key)} disabled={!idle || n === null} onClick={() => n !== null && run(key, "set-slaves", async (still) => {
                const r = await api.maintSetSlaves(o.installation.id, n, port);
                if (still()) setEditing(null);
                return { overview: r, text: t("maint.result.set-slaves", { line: packsLine(n + 1, t) }) };
              })}>{t("maint.panel.save")}</ActButton>
            </>
          ) : (
            <Button variant="secondary" size="sm" disabled={!idle} onClick={() => { setNText(String(current)); setEditing(where); }}>{t("maint.panel.changeSlaves")}</Button>
          )}
        </div>
        {editing === where && derived}
        {status(key)}
      </div>
    );
  };
  const endSteps: { step: EndStep; hint: string }[] = [
    ...(o.session?.kind === "marker" ? [{ step: "stop" as const, hint: "maint.panel.replugControl" }] : []),
    { step: "bus", hint: "maint.panel.replugPack" },
    { step: "values", hint: "maint.panel.replugControl" },
  ];
  const endIndex = (x: EndStep | null) => endSteps.findIndex((e) => e.step === x);

  // the DIP way, for every branch that offers it
  const dipForm = (hint: string, startKey = "maint.panel.startDip") => (
    <div className="space-y-2">
      <p>{hint}</p>
      <label className="flex items-center gap-2"><input type="checkbox" checked={dipConfirmed} onChange={(e) => setDipConfirmed(e.target.checked)} />{t("maint.panel.dipConfirm")}</label>
      {packsInput}
      <ActButton variant={startKey === "maint.panel.startDip" ? "primary" : "secondary"} size="sm" spinning={spinning("start-dip")} disabled={!may(st.start) || !dipConfirmed || n === null}
        onClick={() => n !== null && run("start-dip", "start-dip", async () => ({ overview: await api.maintStartDip(o.installation.id, n, port), text: t("maint.result.start-dip") }))}>{t(startKey)}</ActButton>
      {status("start-dip")}
    </div>
  );

  const start = (() => {
    switch (flow.branch) {
      case "activate":
        return (
          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-3">
              <ActButton variant="primary" size="sm" spinning={spinning("activate")} disabled={!may(st.start) || !inverter}
                onClick={() => run("activate", "activate", async () => ({ overview: await api.maintActivate(o.installation.id, inverter), text: t("maint.result.activate") }))}>{t("maint.panel.activate")}</ActButton>
              <span className="text-muted">{t("maint.flow.recommended")}</span>
            </div>
            {status("activate")}
            <label className="flex items-center gap-2 text-muted"><input type="checkbox" checked={dipInstead} onChange={(e) => setDipInstead(e.target.checked)} disabled={!may(st.start)} />{t("maint.flow.dipInstead")}</label>
            {dipInstead && dipForm(t("maint.flow.dipInsteadHint"))}
          </div>
        );
      case "dip": {
        const hint = flow.packsUnclear ? t("maint.flow.packsUnclear")
          : s === "unknown" ? t(o.installation.control ? "maint.panel.unknownHint" : "maint.flow.manualHint")
          : s === "patched-off" ? t("maint.panel.dipZeroHint") : t("maint.panel.dipHint");
        return (
          <div className="space-y-2">
            {flow.singlePackOffered && (
              <label className="flex items-center gap-2"><input type="checkbox" checked={singlePack} disabled={!idle} onChange={(e) => setSinglePack(e.target.checked)} />{t("maint.flow.singlePackStated")}</label>
            )}
            {dipForm(hint)}
          </div>
        );
      }
      case "single-pack":
        if (flow.scanNeeded) return (
          <div className="space-y-2">
            <p>{t("maint.flow.scanHint")}</p>
            <ActButton variant="primary" size="sm" spinning={spinning("scan")} disabled={!may(st.start) || !port} onClick={scan}>{t("maint.flow.scan")}</ActButton>
            {status("scan")}
          </div>
        );
        return (
          <div className="space-y-2">
            <p>{t(o.installation.control ? "maint.flow.singlePack" : "maint.flow.singlePackManual")}</p>
            {flow.singlePackOffered && <label className="flex items-center gap-2"><input type="checkbox" checked={singlePack} disabled={!idle} onChange={(e) => setSinglePack(e.target.checked)} />{t("maint.flow.singlePackStated")}</label>}
          </div>
        );
      case "session":
        return o.session && (
          <div className="space-y-1">
            <p>{t("maint.flow.sessionOpen", { how: t(`maint.how.${o.session.kind}`) })}</p>
            {/* a wrong N could never pass End's bus step: the user corrects it here (End starts over) */}
            {slavesEditor("start")}
          </div>
        );
      case "recovery":
        return <p>{t("maint.flow.recoveryRunning")}</p>;
      case "unexplained":
        return (
          <div className="space-y-2">
            <p className="text-alarm">{t("maint.banner.unexplainedHint")}</p>
            {/* the recovery row: DIP session first, then the original image on address 0 (firmware view below) */}
            {!o.session && dipForm(t("maint.panel.recoveryHint"), "maint.panel.startRecovery")}
            {/* a set marker is never a single pack: 2 to 16 packs only (the backend refuses N = 0 here) */}
            <div className="space-y-2 border-t border-line pt-2">
              {o.session && packsInput}
              <ActButton variant="secondary" size="sm" spinning={spinning("account-marker")} disabled={!may(st.start) || n === null || n === 0}
                onClick={() => n !== null && n > 0 && run("account-marker", "account-marker", async () => ({ overview: await api.maintAccountMarker(o.installation.id, n), text: t("maint.result.account-marker") }))}>{t("maint.panel.accountMarker")}</ActButton>
              {status("account-marker")}
            </div>
          </div>
        );
      default:
        return null;
    }
  })();

  return (
    <section ref={panelRef} className={cn("space-y-3 rounded-md border p-4 text-sm transition-colors", highlight === "panel" ? "border-charge ring-2 ring-charge" : "border-line")}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="flex items-center gap-1.5 font-display text-lg font-semibold">{t("maint.panel.title", { name: o.installation.name })}<InfoIcon id="topic.maintenance" /></h2>
      </div>
      {/* the pack port in use, visible before every bus action */}
      <p className="font-medium">{port ? t("maint.flow.packPort", { port }) : t("maint.flow.noPackPort")}</p>
      {monitor !== "connected" && <p className="text-muted">{t(`maint.flow.monitor.${monitor}`)}</p>}

      {/* unclear updates: listed above the steps; they lock measure, flash and End */}
      {o.blocks.map((b) => {
        const clear = (how: "answered" | "isolated") => run(`block:${b.id}`, "clear-block", async () => ({ overview: await api.maintClearBlock(o.installation.id, b.id, how, how === "answered" ? port : null), text: t("maint.result.clear-block") }));
        return (
          <div key={b.id} className="space-y-1 rounded-md border border-alarm p-2 text-alarm">
            <p className="flex gap-2"><TriangleAlert className="h-4 w-4 shrink-0" />{t(`maint.panel.block.${b.reason}`, { pack: b.pack })}</p>
            <div className="flex gap-2">
              <ActButton variant="secondary" size="sm" spinning={spinning(`block:${b.id}`)} disabled={!idle || !port} onClick={() => clear("answered")}>{t("maint.panel.blockCheck")}</ActButton>
              <Button variant="secondary" size="sm" disabled={!idle} onClick={() => clear("isolated")}>{t("maint.panel.blockIsolated")}</Button>
            </div>
            {status(`block:${b.id}`)}
          </div>
        );
      })}
      {/* a cleared block takes its buttons along: its result stays here */}
      {activity?.key.startsWith("block:") && !o.blocks.some((b) => activity.key === `block:${b.id}`) && status(activity.key)}

      <ol className="space-y-2">
        <Item n={1} title={t("maint.flow.step.connections")} step={st.connections}>
          {/* read-only: the setup form is the single place to choose the two ports */}
          <p>{control ? t("maint.flow.control", { endpoint: plugged(control.kind === "tcp" ? control.addr : control.path, control.kind === "serial") }) : t("maint.flow.noControl")}</p>
          <p>{port ? t("maint.flow.packPort", { port: plugged(port, true) }) : t("maint.flow.noPackPort")}</p>
          {flow.replug && <p className="text-discharge">{t("maint.flow.oneAdapter")}</p>}
          <Button variant={!port ? "primary" : "secondary"} size="sm" disabled={!idle} onClick={() => setSetup(!setup)}>{t("maint.panel.setup")}</Button>
          {setup && <MaintSetup key={o.installation.id} overview={o} disabled={!idle} onSaved={(x) => { maint.setOverview(x); setSetup(false); onPortsChanged(); }} />}
          {!control && (
            <label className="flex items-center gap-2"><input type="checkbox" checked={noControl} disabled={!idle} onChange={(e) => setNoControl(e.target.checked)} />{t("maint.flow.withoutControl")}</label>
          )}
        </Item>

        <Item n={2} title={t("maint.flow.step.check")} step={st.check}>
          <div className="flex flex-wrap items-center gap-3">
            <ActButton variant={st.check.state === "current" ? "primary" : "secondary"} size="sm" spinning={spinning("check")} disabled={!repeat(st.check) || !o.installation.control} onClick={check}>{t("maint.panel.check")}</ActButton>
            {flow.replug && <span className="text-discharge">{t("maint.panel.replugControl")}</span>}
          </div>
          {status("check")}
          <p>{s ? t(`maint.state.${s}`, { slaves: o.state?.slaves ?? 0 }) : stored ? t("maint.flow.lastConfirmed", { state: t(`maint.state.${stored}`, { slaves: o.state?.slaves ?? 0 }) }) : t("maint.state.none")}</p>
        </Item>

        <Item n={3} title={t("maint.flow.step.inverter")} step={st.inverter}>
          {/* in memory only, cleared with every new work phase; unticking is always possible */}
          {!st.inverter.notNeeded && (
            <label className="flex items-start gap-2">
              <input type="checkbox" className="mt-1" checked={inverter} disabled={!idle || (st.inverter.state === "locked" && !inverter)} onChange={(e) => setInverter(e.target.checked)} />
              <span>{t("maint.panel.inverter")}<span className="block text-muted">{t("maint.panel.inverterHint")}</span></span>
            </label>
          )}
        </Item>

        <Item n={4} title={t("maint.flow.step.start")} step={st.start}>{st.start.state !== "locked" && start}</Item>

        <Item n={5} title={t("maint.flow.step.measure")} step={st.measure}>
          <div className="flex flex-wrap items-center gap-3">
            <ActButton variant={st.measure.state === "current" ? "primary" : "secondary"} size="sm" spinning={spinning("measure")} disabled={!repeat(st.measure) || !port}
              onClick={() => run("measure", "measure", async (still) => { setMeasuredAt(null); const at = await api.maintMeasure(o.installation.id, port); if (still()) setMeasuredAt(at); return null; })}>{t("maint.panel.measure")}</ActButton>
            {flow.replug && <span className="text-discharge">{t("maint.panel.replugPack")}</span>}
            {/* the success of a measurement is this line; it goes with the next state change */}
            {measuredAt && !spinning("measure") && <span className="flex items-center gap-1 text-ok"><CircleCheck className="h-4 w-4" />{t("maint.panel.quietAt", { time: new Date(measuredAt).toLocaleTimeString() })}</span>}
          </div>
          {status("measure")}
        </Item>

        <Item n={6} title={t("maint.flow.step.flash")} step={st.flash}>
          {st.flash.state === "current" && <p>{t("maint.flow.flashBelow")}</p>}
        </Item>
      </ol>

      {/* End, its own section: while a session is open */}
      {flow.end.shown && o.session && (
        <section ref={endRef} className={cn("space-y-2 rounded-md border p-3 transition-colors", highlight === "end" ? "border-charge ring-2 ring-charge" : "border-line")}>
          <h3 className="flex flex-wrap items-center gap-2 font-medium">
            {flow.end.state === "locked" ? <Lock className="h-4 w-4 text-muted" /> : <Play className="h-4 w-4 text-charge" />}{t("maint.flow.endTitle")}
          </h3>
          <p className="font-medium">{port ? t("maint.flow.packPort", { port }) : t("maint.flow.noPackPort")}</p>
          {slavesEditor("end")}
          {flow.end.reason && <p className="text-muted">{reasonText(flow.end.reason)}</p>}
          {sequential ? (
            <div className="space-y-2">
              <p className="text-muted">{t("maint.panel.endSequential")}</p>
              {endFailed && flow.end.next && <p className="text-discharge">{t("maint.flow.endRestart", { step: t(`maint.panel.endStep.${flow.end.next}`) })}</p>}
              <ol className="space-y-2">
                {endSteps.map(({ step, hint }, k) => {
                  const next = endIndex(flow.end.next);
                  const state: FlowStep["state"] = flow.end.state === "locked" ? "locked" : k < next ? "done" : k === next ? "current" : "locked";
                  return (
                    <Item key={step} n={null} title={t(`maint.panel.endStep.${step}`)} step={{ state, reason: state === "locked" && flow.end.state !== "locked" ? { key: "maint.flow.reason.end-first", params: { step: t(`maint.panel.endStep.${endSteps[Math.max(0, next)].step}`) } } : undefined }}>
                      {state === "current" && (
                        <div className="flex flex-wrap items-center gap-3">
                          <span className="text-discharge">{t(hint)}</span>
                          <ActButton variant="primary" size="sm" spinning={spinning(`end-step:${step}`)} disabled={!idle || !port} onClick={() => endStep(step)}>{t("maint.flow.runStep")}</ActButton>
                        </div>
                      )}
                      {status(`end-step:${step}`)}
                    </Item>
                  );
                })}
              </ol>
            </div>
          ) : (
            <div className="space-y-1">
              <ActButton variant="primary" size="sm" spinning={spinning("end")} disabled={!idle || flow.end.state === "locked"} onClick={end}>{t("maint.end")}</ActButton>
              {status("end")}
            </div>
          )}
          {o.session.kind === "dip" && <p className="text-muted">{t("maint.panel.dipBack")}</p>}
        </section>
      )}
      {/* End closed the session (or it was closed meanwhile): the section is gone, its result stays */}
      {!(flow.end.shown && o.session) && activity && (activity.key === "end" || activity.key.startsWith("end-step:")) && mine(activity.key) && (
        <section className="space-y-2 rounded-md border border-line p-3">
          <h3 className="font-medium">{t("maint.flow.endTitle")}</h3>
          {status(activity.key)}
        </section>
      )}
      <p className="text-muted">{t("maint.panel.residual")}</p>
    </section>
  );
}
