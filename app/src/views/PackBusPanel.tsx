import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, TriangleAlert } from "lucide-react";
import { api, type EndResult, type EndStep, type MaintOverview } from "../api";
import { errorKey, needsReplug } from "../maint";
import type { useMaint } from "../maintStore";
import { Button } from "../components/ui/button";
import { MaintSetup } from "./MaintSetup";

const input = "h-9 w-20 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function PackBusPanel({ maint, port, inverter, setInverter, measuredAt, setMeasuredAt }: {
  maint: ReturnType<typeof useMaint>;
  port: string;
  inverter: boolean;
  setInverter: (v: boolean) => void;
  measuredAt: number | null;
  setMeasuredAt: (v: number | null) => void;
}) {
  const { t } = useTranslation();
  const o = maint.overview;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [n, setN] = useState(0);
  const [dipConfirmed, setDipConfirmed] = useState(false);
  const [setup, setSetup] = useState(false);
  const [endNote, setEndNote] = useState<string | null>(null);
  // set when maintEnd was refused with end-sequential (one adapter for both buses)
  const [stepwise, setStepwise] = useState(false);

  const run = async (f: () => Promise<MaintOverview | void>) => {
    setBusy(true); setError(null);
    try { const r = await f(); if (r) maint.setOverview(r); }
    catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); await maint.refresh(); }
    finally { setBusy(false); }
  };

  if (!o) return <section className="space-y-2 rounded-md border border-line p-4 text-sm text-muted">{t("maint.panel.noInstallation")}</section>;
  const s = o.state?.state;
  const replug = needsReplug(o.installation.control, port);
  const sequential = replug || stepwise;

  const endNoteOf = (r: EndResult) => r.ok ? t("maint.panel.endOk") : r.missing.length ? t("maint.panel.endMissing", { list: r.missing.join(", ") }) : t("maint.panel.endFailed", { what: r.failing });
  const endStep = (step: EndStep) => run(async () => {
    setEndNote(null);
    const r = await api.maintEndStep(o.installation.id, port, step);
    if (step === "values") { setInverter(false); setMeasuredAt(null); setEndNote(endNoteOf(r)); }
    else if (!r.ok) setEndNote(endNoteOf(r));
    return api.maintOverview(o.installation.id);
  });
  const endSteps: { step: EndStep; hint: string }[] = [
    ...(o.session?.kind === "marker" ? [{ step: "stop" as const, hint: "maint.panel.replugControl" }] : []),
    { step: "bus", hint: "maint.panel.replugPack" },
    { step: "values", hint: "maint.panel.replugControl" },
  ];

  return (
    <section className="space-y-3 rounded-md border border-line p-4 text-sm">
      <div className="flex items-center justify-between">
        <h2 className="font-display text-lg font-semibold">{t("maint.panel.title", { name: o.installation.name })}</h2>
        <Button variant="ghost" size="sm" onClick={() => setSetup(!setup)}>{t("maint.panel.setup")}</Button>
      </div>
      {setup && <MaintSetup key={o.installation.id} overview={o} onSaved={(x) => { maint.setOverview(x); setSetup(false); }} />}

      {/* 1. check master */}
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="sm" disabled={busy || !o.installation.control} onClick={() => run(() => api.maintCheck(o.installation.id))}>{t("maint.panel.check")}</Button>
        <span>{s ? t(`maint.state.${s}`, { slaves: o.state?.slaves ?? 0 }) : t("maint.state.none")}</span>
        {replug && <span className="text-discharge">{t("maint.panel.replugControl")}</span>}
      </div>

      {s === "patched-off" && (o.state?.slaves ?? 0) > 0 && !o.session && (
        <Button variant="primary" size="sm" disabled={busy || !inverter} onClick={() => run(() => api.maintActivate(o.installation.id, inverter))}>{t("maint.panel.activate")}</Button>
      )}
      {(s === "no-0x64" || s === "unknown") && !o.session && (
        <div className="space-y-2">
          <p>{t(s === "unknown" ? "maint.panel.unknownHint" : "maint.panel.dipHint")}</p>
          <label className="flex items-center gap-2"><input type="checkbox" checked={dipConfirmed} onChange={(e) => setDipConfirmed(e.target.checked)} />{t("maint.panel.dipConfirm")}</label>
          <label className="flex items-center gap-2">{t("maint.panel.slaves")}<input type="number" min={0} max={15} className={input} value={n} onChange={(e) => setN(Number(e.target.value))} /></label>
          <Button variant="ghost" size="sm" disabled={busy || !dipConfirmed} onClick={() => run(() => api.maintStartDip(o.installation.id, n))}>{t("maint.panel.startDip")}</Button>
        </div>
      )}
      {s === "marker-unexplained" && (
        <div className="space-y-2">
          <p className="text-alarm">{t("maint.banner.unexplainedHint")}</p>
          <label className="flex items-center gap-2">{t("maint.panel.slaves")}<input type="number" min={0} max={15} className={input} value={n} onChange={(e) => setN(Number(e.target.value))} /></label>
          {o.session?.kind !== "dip" && <Button variant="ghost" size="sm" disabled={busy} onClick={() => run(() => api.maintAccountMarker(o.installation.id, n))}>{t("maint.panel.accountMarker")}</Button>}
          {/* the recovery row: DIP session first, then the original image on address 0 (firmware view below) */}
          {!o.session && (
            <div className="space-y-2 border-t border-line pt-2">
              <p>{t("maint.panel.recoveryHint")}</p>
              <label className="flex items-center gap-2"><input type="checkbox" checked={dipConfirmed} onChange={(e) => setDipConfirmed(e.target.checked)} />{t("maint.panel.dipConfirm")}</label>
              <Button variant="ghost" size="sm" disabled={busy || !dipConfirmed} onClick={() => run(() => api.maintStartDip(o.installation.id, n))}>{t("maint.panel.startRecovery")}</Button>
            </div>
          )}
        </div>
      )}

      {/* 2. inverter: in memory only, cleared with every new work phase */}
      {(o.session || s === "patched-on" || (s === "patched-off" && (o.state?.slaves ?? 0) > 0)) && (
        <label className="flex items-start gap-2">
          <input type="checkbox" className="mt-1" checked={inverter} onChange={(e) => setInverter(e.target.checked)} />
          <span>{t("maint.panel.inverter")}<span className="block text-muted">{t("maint.panel.inverterHint")}</span></span>
        </label>
      )}

      {/* 3. measure */}
      {(o.session || s === "patched-on") && (
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="ghost" size="sm" disabled={busy || !port} onClick={() => run(async () => { setMeasuredAt(null); setMeasuredAt(await api.maintMeasure(o.installation.id, port)); })}>{t("maint.panel.measure")}</Button>
          {replug && <span className="text-discharge">{t("maint.panel.replugPack")}</span>}
          {measuredAt && <span className="flex items-center gap-1 text-ok"><CircleCheck className="h-4 w-4" />{t("maint.panel.quietAt", { time: new Date(measuredAt).toLocaleTimeString() })}</span>}
        </div>
      )}

      {/* unclear updates */}
      {o.blocks.map((b) => (
        <div key={b.id} className="space-y-1 rounded-md border border-alarm p-2 text-alarm">
          <p className="flex gap-2"><TriangleAlert className="h-4 w-4 shrink-0" />{t(`maint.panel.block.${b.reason}`, { pack: b.pack })}</p>
          <div className="flex gap-2">
            <Button variant="ghost" size="sm" disabled={busy || !port} onClick={() => run(() => api.maintClearBlock(o.installation.id, b.id, "answered", port))}>{t("maint.panel.blockCheck")}</Button>
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => run(() => api.maintClearBlock(o.installation.id, b.id, "isolated", null))}>{t("maint.panel.blockIsolated")}</Button>
          </div>
        </div>
      ))}

      {/* 5. end */}
      {o.session && s !== "marker-unexplained" && (
        <div className="space-y-1">
          {sequential ? (
            <div className="space-y-2">
              <p className="text-muted">{t("maint.panel.endSequential")}</p>
              {endSteps.map(({ step, hint }) => (
                <div key={step} className="flex flex-wrap items-center gap-3">
                  <span className="text-discharge">{t(hint)}</span>
                  <Button variant="primary" size="sm" disabled={busy || !port} onClick={() => endStep(step)}>{t(`maint.panel.endStep.${step}`)}</Button>
                </div>
              ))}
            </div>
          ) : (
            <Button variant="primary" size="sm" disabled={busy || !port} onClick={() => run(async () => {
              setEndNote(null);
              let r: EndResult;
              try { r = await api.maintEnd(o.installation.id, port); }
              catch (e) {
                if (errorKey(e).key === "maint.err.refused.end-sequential") { setStepwise(true); setError(null); return; }
                throw e;
              }
              setInverter(false); setMeasuredAt(null);
              setEndNote(endNoteOf(r));
              return api.maintOverview(o.installation.id);
            })}>{t("maint.end")}</Button>
          )}
          {o.session.kind === "dip" && <p className="text-muted">{t("maint.panel.dipBack")}</p>}
          {endNote && <p>{endNote}</p>}
        </div>
      )}
      <p className="text-muted">{t("maint.panel.residual")}</p>
      {error && <p className="text-alarm">{error}</p>}
    </section>
  );
}
