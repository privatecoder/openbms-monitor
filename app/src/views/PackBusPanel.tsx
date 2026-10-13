import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, Lock, Play, TriangleAlert } from "lucide-react";
import { api, type EndResult, type EndStep, type MaintOverview } from "../api";
import { errorKey, failingText, packsDerived, packsLine, parsePacks, slavesOf, workUnchanged, type Flow, type FlowStep, type Monitor, type Reason } from "../maint";
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

export function PackBusPanel({ maint, flow, port, portPicker, monitor, focusEnd, onFocusDone, busy, setBusy, inverter, setInverter, measuredAt, setMeasuredAt, setEndNote, noControl, setNoControl, singlePack, setSinglePack }: {
  maint: ReturnType<typeof useMaint>;
  flow: Flow;
  port: string;
  /** the pack port selection (lives in the firmware view, shown in step 1) */
  portPicker: ReactNode;
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
  /** the result of End lives in the parent: it must outlast the session */
  setEndNote: (v: string | null) => void;
  noControl: boolean;
  setNoControl: (v: boolean) => void;
  singlePack: boolean;
  setSinglePack: (v: boolean) => void;
}) {
  const { t } = useTranslation();
  const o = maint.overview;
  const [error, setError] = useState<string | null>(null);
  // packs on the bus, master included: no default, the user types it (an empty or out-of-range value keeps the
  // buttons locked); the backend gets N = packs − 1
  const [nText, setNText] = useState("");
  const packs = parsePacks(nText);
  const n = packs === null ? null : slavesOf(packs);
  const [editSlaves, setEditSlaves] = useState(false);
  // the N editor belongs to one session; a new (or closed) session starts with it closed
  const sessionId = o?.session?.id ?? null;
  useEffect(() => { setEditSlaves(false); setNText(""); }, [sessionId]);
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

  /** `still()` tells an action, after an await, that the installation and session it began with are still in use;
   *  if not, it must not write (overview, measurement, inverter, note). */
  const run = async (f: (still: () => boolean) => Promise<MaintOverview | void>) => {
    const start = { installation: o?.installation.id ?? "", session: o?.session?.id ?? null };
    const still = () => workUnchanged(start, maint.current());
    setBusy(true); setError(null);
    try { const r = await f(still); if (r && still()) maint.setOverview(r); }
    catch (e) { if (still()) { const k = errorKey(e); setError(t(k.key, k.params)); } await maint.refresh(); }
    finally { setBusy(false); }
  };

  if (!o) return <section ref={panelRef} className={cn("space-y-2 rounded-md border p-4 text-sm text-muted transition-colors", highlight ? "border-charge ring-2 ring-charge" : "border-line")}>{t("maint.panel.noInstallation")}{portPicker}</section>;
  const st = flow.steps;
  const s = flow.state;
  const stored = o.state?.state;
  const idle = flow.idle;
  const may = (step: FlowStep) => idle && step.state === "current";
  // harmless repeats: available on the current and on a done step
  const repeat = (step: FlowStep) => idle && step.state !== "locked";
  const sequential = flow.replug || stepwise;
  const reasonText = (r: Reason | undefined) => (r ? t(r.key, r.params) : "");

  const endNoteOf = (r: EndResult) => r.ok ? t(o.session?.expected_slaves === 0 ? "maint.panel.endOkZero" : "maint.panel.endOk") : r.missing.length ? t("maint.panel.endMissing", { list: r.missing.join(", ") }) : t("maint.panel.endFailed", { what: failingText(r.failing ?? "", t) });
  const packsField = <input type="number" min={1} max={16} className={input} value={nText} onChange={(e) => setNText(e.target.value)} aria-label={t("maint.panel.packs")} />;
  // the derived slave count right under the input, before a valid entry nothing
  const derived = packs !== null && <p className="font-medium">{packsDerived(packs, t)}</p>;
  const packsInput = <div className="space-y-1"><label className="flex items-center gap-2">{t("maint.panel.packs")}{packsField}</label>{derived}</div>;

  const check = () => run(async (still) => {
    let r: MaintOverview | null = null;
    try { r = await maint.check(o.installation.id, true); }
    finally {
      // a failed check takes the inverter statement with it; so does an answer that leaves the state unknown
      if (still() && (!r || r.state?.state === "unknown")) setInverter(false);
    }
  });
  const scan = () => run(async (still) => {
    const slave = await api.maintScanSlaves(o.installation.id, port);
    if (still()) maint.setScan(o.installation.id, port, slave);
  });
  const endStep = (step: EndStep) => run(async (still) => {
    setEndNote(null);
    let r: EndResult;
    try { r = await api.maintEndStep(o.installation.id, port, step); }
    catch (e) { if (still()) setEndFailed(true); throw e; }
    if (still()) {
      setEndFailed(!r.ok);
      if (step === "values") { setInverter(false); setMeasuredAt(null); setEndNote(endNoteOf(r)); }
      else if (!r.ok) setEndNote(endNoteOf(r));
    }
    return api.maintOverview(o.installation.id);
  });
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
      <Button variant={startKey === "maint.panel.startDip" ? "primary" : "secondary"} size="sm" disabled={!may(st.start) || !dipConfirmed || n === null} onClick={() => n !== null && run(() => api.maintStartDip(o.installation.id, n, port))}>{t(startKey)}</Button>
    </div>
  );

  const start = (() => {
    switch (flow.branch) {
      case "activate":
        return (
          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-3">
              <Button variant="primary" size="sm" disabled={!may(st.start) || !inverter} onClick={() => run(() => api.maintActivate(o.installation.id, inverter))}>{t("maint.panel.activate")}</Button>
              <span className="text-muted">{t("maint.flow.recommended")}</span>
            </div>
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
            <Button variant="primary" size="sm" disabled={!may(st.start) || !port} onClick={scan}>{t("maint.flow.scan")}</Button>
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
            <div className="flex flex-wrap items-center gap-3">
              <span>{packsLine(o.session.expected_slaves + 1, t)}</span>
              {editSlaves ? (
                <>
                  <label className="flex items-center gap-2">{t("maint.panel.packs")}{packsField}</label>
                  <Button variant="secondary" size="sm" disabled={!idle || n === null} onClick={() => n !== null && run(async (still) => {
                    const r = await api.maintSetSlaves(o.installation.id, n, port);
                    if (still()) setEditSlaves(false);
                    return r;
                  })}>{t("maint.panel.save")}</Button>
                </>
              ) : (
                <Button variant="secondary" size="sm" disabled={!idle} onClick={() => { setNText(o.session ? String(o.session.expected_slaves + 1) : ""); setEditSlaves(true); }}>{t("maint.panel.changeSlaves")}</Button>
              )}
            </div>
            {editSlaves && derived}
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
              <Button variant="secondary" size="sm" disabled={!may(st.start) || n === null || n === 0} onClick={() => n !== null && n > 0 && run(() => api.maintAccountMarker(o.installation.id, n))}>{t("maint.panel.accountMarker")}</Button>
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
        <Button variant="secondary" size="sm" onClick={() => setSetup(!setup)}>{t("maint.panel.setup")}</Button>
      </div>
      {/* the pack port in use, visible before every bus action */}
      <p className="font-medium">{port ? t("maint.flow.packPort", { port }) : t("maint.flow.noPackPort")}</p>
      {monitor !== "connected" && <p className="text-muted">{t(`maint.flow.monitor.${monitor}`)}</p>}
      {setup && <MaintSetup key={o.installation.id} overview={o} onSaved={(x) => { maint.setOverview(x); setSetup(false); }} />}

      {/* unclear updates: listed above the steps; they lock measure, flash and End */}
      {o.blocks.map((b) => (
        <div key={b.id} className="space-y-1 rounded-md border border-alarm p-2 text-alarm">
          <p className="flex gap-2"><TriangleAlert className="h-4 w-4 shrink-0" />{t(`maint.panel.block.${b.reason}`, { pack: b.pack })}</p>
          <div className="flex gap-2">
            <Button variant="secondary" size="sm" disabled={!idle || !port} onClick={() => run(() => api.maintClearBlock(o.installation.id, b.id, "answered", port))}>{t("maint.panel.blockCheck")}</Button>
            <Button variant="secondary" size="sm" disabled={!idle} onClick={() => run(() => api.maintClearBlock(o.installation.id, b.id, "isolated", null))}>{t("maint.panel.blockIsolated")}</Button>
          </div>
        </div>
      ))}

      <ol className="space-y-2">
        <Item n={1} title={t("maint.flow.step.connections")} step={st.connections}>
          <p>{o.installation.control ? t("maint.flow.control", { endpoint: o.installation.control.kind === "tcp" ? o.installation.control.addr : o.installation.control.path }) : t("maint.flow.noControl")}</p>
          {!o.installation.control && (
            <label className="flex items-center gap-2"><input type="checkbox" checked={noControl} disabled={!idle} onChange={(e) => setNoControl(e.target.checked)} />{t("maint.flow.withoutControl")}</label>
          )}
          {portPicker}
          {flow.replug && <p className="text-discharge">{t("maint.flow.oneAdapter")}</p>}
        </Item>

        <Item n={2} title={t("maint.flow.step.check")} step={st.check}>
          <div className="flex flex-wrap items-center gap-3">
            <Button variant={st.check.state === "current" ? "primary" : "secondary"} size="sm" disabled={!repeat(st.check) || !o.installation.control} onClick={check}>{t("maint.panel.check")}</Button>
            {flow.replug && <span className="text-discharge">{t("maint.panel.replugControl")}</span>}
          </div>
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
            <Button variant={st.measure.state === "current" ? "primary" : "secondary"} size="sm" disabled={!repeat(st.measure) || !port} onClick={() => run(async (still) => { setMeasuredAt(null); const at = await api.maintMeasure(o.installation.id, port); if (still()) setMeasuredAt(at); })}>{t("maint.panel.measure")}</Button>
            {flow.replug && <span className="text-discharge">{t("maint.panel.replugPack")}</span>}
            {measuredAt && <span className="flex items-center gap-1 text-ok"><CircleCheck className="h-4 w-4" />{t("maint.panel.quietAt", { time: new Date(measuredAt).toLocaleTimeString() })}</span>}
          </div>
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
          <p>{packsLine(o.session.expected_slaves + 1, t)}</p>
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
                          <Button variant="primary" size="sm" disabled={!idle || !port} onClick={() => endStep(step)}>{t("maint.flow.runStep")}</Button>
                        </div>
                      )}
                    </Item>
                  );
                })}
              </ol>
            </div>
          ) : (
            <Button variant="primary" size="sm" disabled={!idle || flow.end.state === "locked"} onClick={() => run(async (still) => {
              setEndNote(null);
              let r: EndResult;
              try { r = await api.maintEnd(o.installation.id, port); }
              catch (e) {
                if (errorKey(e).key === "maint.err.refused.end-sequential") { if (still()) { setStepwise(true); setError(null); } return; }
                throw e;
              }
              if (still()) { setInverter(false); setMeasuredAt(null); setEndNote(endNoteOf(r)); }
              return api.maintOverview(o.installation.id);
            })}>{t("maint.end")}</Button>
          )}
          {o.session.kind === "dip" && <p className="text-muted">{t("maint.panel.dipBack")}</p>}
        </section>
      )}
      <p className="text-muted">{t("maint.panel.residual")}</p>
      {error && <p className="text-alarm">{error}</p>}
    </section>
  );
}
