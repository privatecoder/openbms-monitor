import { Fragment, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, FileUp, FolderOpen, SlidersHorizontal, TriangleAlert, X } from "lucide-react";
import { api, isDemo, type Bus } from "../api";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { InfoIcon, useHelpEntry } from "../help";
import { cn } from "../lib/utils";
import { BM_NAMES, GROUPS, KEYS, SWITCH_BITS, UNITS, decimals, deviating, exportName, fmtParam, fromPack, parseExport, toExport, type Source } from "../params";

const packNo = (a: number) => String(a).padStart(2, "0");

// kept while the app runs, so switching views does not drop what was read
let kept: Source[] = [];

/** Read the packs' parameters (RS485-1/2) or open BatteryMonitor exports, and compare them side by side. */
export function ParametersView({ connected, bus, packs }: { connected: boolean; bus: Bus | null; packs: number[] }) {
  const { t, i18n } = useTranslation();
  const [sources, setSourcesState] = useState<Source[]>(kept);
  const setSources = (f: (s: Source[]) => Source[]) => setSourcesState((s) => (kept = f(s)));
  const [reading, setReading] = useState<{ address: number; n: number; of: number } | null>(null);
  const [onlyDiff, setOnlyDiff] = useState(false);
  const [saved, setSaved] = useState<{ path: string } | { error: string } | null>(null);
  const file = useRef<HTMLInputElement>(null);

  const canRead = connected && (bus === "pack" || isDemo) && packs.length > 0 && !reading;

  const read = async () => {
    setSaved(null);
    const ids = new Set(packs.map((a) => `pack-${a}`));
    // columns appear at once, in pack order before the opened files, and fill as the answers come in
    setSources((s) => [...packs.map((a) => ({ id: `pack-${a}`, kind: "pack" as const, label: packNo(a) })), ...s.filter((x) => !ids.has(x.id))]);
    for (const [n, address] of packs.entries()) {
      setReading({ address, n: n + 1, of: packs.length });
      let next: Source;
      try { next = fromPack(await api.parameters(address)); }
      catch (e) { next = { id: `pack-${address}`, kind: "pack", label: packNo(address), error: String(e) }; }
      setSources((s) => s.map((x) => (x.id === next.id ? next : x)));
    }
    setReading(null);
  };

  const open = async (files: FileList | null) => {
    for (const f of Array.from(files ?? [])) {
      const src = parseExport(await f.text(), f.name);
      setSources((s) => [...s.filter((x) => x.id !== src.id), src]);
    }
    if (file.current) file.current.value = "";
  };

  const save = async (s: Source) => {
    if (!s.read) return;
    try { setSaved({ path: await api.saveParameters(exportName(s.read.address), toExport(s.read)) }); }
    catch (e) { setSaved({ error: String(e) }); }
  };

  const nf = (i: number) => new Intl.NumberFormat(i18n.language, { minimumFractionDigits: decimals(i), maximumFractionDigits: decimals(i) });
  const cols = [...sources.filter((c) => c.kind === "file"), ...sources.filter((c) => c.kind === "pack")];
  const paramKeys = (i: number) => cols.map((c) => (c.values ? fmtParam(i, c.values[i]) : undefined));
  const switchKeys = (g: number) => cols.map((c) => (c.switches ? String(c.switches[g]) : undefined));
  const diffParams = KEYS.map((_, i) => deviating(paramKeys(i)));
  const diffSwitches = Array.from({ length: 8 }, (_, g) => deviating(switchKeys(g)));
  const nDiff = diffParams.filter((d) => d.size).length;
  const nDiffSwitches = diffSwitches.filter((d) => d.size).length;
  const withValues = cols.filter((c) => c.values).length;
  const suspect = cols.filter((c) => c.suspectFrom !== undefined && c.suspectFrom !== null);
  const broken = cols.filter((c) => c.kind === "file" && c.error);

  return (
    <div className="view-in mx-auto max-w-7xl space-y-6">
      <header>
        <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight">
          <SlidersHorizontal className="h-8 w-8" strokeWidth={1.75} aria-hidden />{t("par.title")}<InfoIcon id="topic.params" />
        </h1>
        <p className="mt-1 max-w-3xl text-muted">{t("par.intro")}</p>
      </header>

      <Panel className="space-y-3 p-5">
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="primary" disabled={!canRead} onClick={read}><Download className="h-4 w-4" />{t("par.read")}</Button>
          <Button onClick={() => file.current?.click()}><FileUp className="h-4 w-4" />{t("par.open")}</Button>
          <input ref={file} type="file" accept=".xml,application/xml,text/xml" multiple className="hidden" onChange={(e) => open(e.target.files)} />
          {cols.length > 0 && <Button variant="ghost" disabled={!!reading} onClick={() => { setSources(() => []); setSaved(null); }}>{t("par.clear")}</Button>}
          <label className="ml-auto flex cursor-pointer items-center gap-2 text-sm">
            <input type="checkbox" className="h-4 w-4 accent-[var(--charge)]" checked={onlyDiff} onChange={(e) => setOnlyDiff(e.target.checked)} disabled={withValues < 2} />
            <span className={cn(withValues < 2 && "text-muted")}>{t("par.onlyDiff")}</span>
          </label>
        </div>
        <p className={cn("text-sm", bus === "can" && connected && !isDemo ? "text-discharge" : "text-muted")}>
          {reading ? t("par.reading", { pack: packNo(reading.address), n: reading.n, of: reading.of })
            : !connected ? t("par.needConnection")
            : bus === "can" && !isDemo ? t("par.canBus")
            : t("par.packBus")}
        </p>
        {saved && (
          "path" in saved ? (
            <p className="flex flex-wrap items-center gap-2 text-sm text-ok">
              {t("par.saved", { name: saved.path.split(/[\\/]/).pop() })}
              {!isDemo && <Button size="sm" variant="ghost" onClick={() => api.revealParameters(saved.path)}><FolderOpen className="h-4 w-4" />{t("par.reveal")}</Button>}
            </p>
          ) : <p className="text-sm text-alarm">{t("par.saveError", { err: saved.error })}</p>
        )}
      </Panel>

      {(suspect.length > 0 || broken.length > 0) && (
        <div className="space-y-2">
          {suspect.map((c) => (
            <p key={c.id} className="flex items-start gap-2 rounded-md border border-discharge/40 bg-discharge/10 px-4 py-3 text-sm">
              <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-discharge" aria-hidden />
              <span>{t("par.suspect", { file: c.label, p: c.suspectFrom })}</span>
            </p>
          ))}
          {broken.map((c) => (
            <p key={c.id} className="flex items-start gap-2 rounded-md border border-alarm/40 bg-alarm/10 px-4 py-3 text-sm">
              <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-alarm" aria-hidden />
              <span>{t("par.notExport", { file: c.label })}</span>
            </p>
          ))}
        </div>
      )}

      {cols.length === 0 ? (
        <div className="flex max-w-md flex-col gap-3 py-10 text-lg text-muted">
          <SlidersHorizontal className="h-8 w-8" strokeWidth={1.5} aria-hidden />{t("par.empty")}
        </div>
      ) : (
        <>
          {withValues >= 2 && (
            <p className={cn("text-sm", nDiff + nDiffSwitches ? "text-discharge" : "text-ok")}>
              {nDiff + nDiffSwitches ? t("par.diffCount", { p: nDiff, s: nDiffSwitches }) : t("par.allEqual", { n: withValues })}
            </p>
          )}
          <Panel className="overflow-x-auto">
            <table className="w-full border-separate border-spacing-0 text-sm tabular-nums">
              <thead>
                <tr>
                  <th className="sticky left-0 z-10 min-w-80 border-b border-line bg-surface px-4 py-3 text-left font-normal text-muted">{t("par.parameter")}</th>
                  {cols.map((c) => <ColumnHead key={c.id} c={c} busy={!!reading} onSave={() => save(c)} onRemove={() => setSources((s) => s.filter((x) => x.id !== c.id))} />)}
                </tr>
              </thead>
              <tbody>
                {GROUPS.map((g) => {
                  const rows = g.indices.filter((i) => !onlyDiff || diffParams[i].size);
                  if (!rows.length) return null;
                  return (
                    <Fragment key={g.id}>
                      <GroupRow span={cols.length + 1} title={t(`par.group.${g.id}`)} />
                      {rows.map((i) => (
                        <tr key={i} className="group">
                          <NameCell id={`param.${KEYS[i]}`} sub={`P${i}${UNITS[i] ? ` · ${UNITS[i]}` : ""}`} original={BM_NAMES[i]} />
                          {cols.map((c, ci) => (
                            <ValueCell key={c.id} c={c} off={diffParams[i].has(ci)}>{c.values ? nf(i).format(c.values[i]) : null}</ValueCell>
                          ))}
                        </tr>
                      ))}
                    </Fragment>
                  );
                })}
                {Array.from({ length: 8 }, (_, g) => g)
                  .filter((g) => !onlyDiff || diffSwitches[g].size)
                  .map((g, n) => (
                    <Fragment key={`bg${g}`}>
                      <GroupRow span={cols.length + 1} title={t(`par.switchGroup.${g}`, { g })} help={n === 0 ? "topic.switches" : undefined} first={n === 0} />
                      <tr>
                        <td className="sticky left-0 z-10 border-b border-line bg-surface px-4 py-1.5 text-muted">{t("par.byte", { g })}</td>
                        {cols.map((c, ci) => (
                          <ValueCell key={c.id} c={c} off={diffSwitches[g].has(ci)}>{c.switches ? c.switches[g].toString(16).toUpperCase().padStart(2, "0") : null}</ValueCell>
                        ))}
                      </tr>
                      {SWITCH_BITS[g]
                        .filter((b) => !onlyDiff || deviating(cols.map((c) => (c.switches ? String((c.switches[g] >> b) & 1) : undefined))).size)
                        .map((b) => {
                          const off = deviating(cols.map((c) => (c.switches ? String((c.switches[g] >> b) & 1) : undefined)));
                          return (
                            <tr key={b}>
                              <NameCell id={`switch.${g}.${b}`} sub={t("par.bit", { b })} />
                              {cols.map((c, ci) => (
                                <ValueCell key={c.id} c={c} off={off.has(ci)}>{c.switches ? <OnOff on={((c.switches[g] >> b) & 1) === 1} /> : null}</ValueCell>
                              ))}
                            </tr>
                          );
                        })}
                    </Fragment>
                  ))}
              </tbody>
            </table>
          </Panel>
        </>
      )}
    </div>
  );
}

function ColumnHead({ c, busy, onSave, onRemove }: { c: Source; busy: boolean; onSave: () => void; onRemove: () => void }) {
  const { t } = useTranslation();
  const pending = c.kind === "pack" && !c.values && !c.error;
  return (
    <th className="min-w-24 border-b border-l border-line bg-surface px-3 py-2 text-right align-bottom font-normal">
      <div className="flex items-center justify-end gap-1">
        {c.suspectFrom !== undefined && c.suspectFrom !== null && <TriangleAlert className="h-3.5 w-3.5 text-discharge" aria-label={t("par.suspectShort")} />}
        <span className={cn("truncate font-medium text-ink", c.kind === "file" && "max-w-40")} title={c.label}>
          {c.kind === "pack" ? t("par.pack", { n: c.label }) : c.label}
        </span>
        {c.kind === "pack" && c.read && (
          <button onClick={onSave} disabled={busy} className="rounded p-1 text-muted hover:bg-sunken hover:text-ink disabled:opacity-40" title={t("par.save")} aria-label={t("par.save")}>
            <Download className="h-3.5 w-3.5" />
          </button>
        )}
        {(c.kind === "file" || c.error) && (
          <button onClick={onRemove} disabled={busy} className="rounded p-1 text-muted hover:bg-sunken hover:text-ink disabled:opacity-40" title={t("par.remove")} aria-label={t("par.remove")}>
            <X className="h-3.5 w-3.5" />
          </button>
        )}
      </div>
      <div className={cn("text-xs", c.error ? "text-alarm" : "text-muted")} title={c.error}>
        {c.error ? (c.kind === "pack" ? t("par.noAnswer") : t("par.unreadable")) : pending ? t("par.pending") : c.kind === "file" ? (c.device || t("par.file")) : c.device}
      </div>
    </th>
  );
}

function GroupRow({ span, title, help, first }: { span: number; title: string; help?: string; first?: boolean }) {
  return (
    <tr>
      <td colSpan={span} className={cn("border-b border-line bg-sunken/60 px-4 py-2 font-display text-base font-semibold", first && "border-t-2")}>
        <span className="sticky left-4 inline-flex items-center gap-1.5">{title}{help && <InfoIcon id={help} />}</span>
      </td>
    </tr>
  );
}

function NameCell({ id, sub, original }: { id: string; sub: string; original?: string }) {
  const e = useHelpEntry(id);
  return (
    <td className="sticky left-0 z-10 border-b border-line bg-surface px-4 py-1.5 group-hover:bg-sunken" title={original}>
      <div className="flex items-baseline gap-1.5">
        <span className="whitespace-nowrap">{e?.title ?? id}</span><InfoIcon id={id} className="self-center" />
        <span className="ml-auto pl-3 text-xs whitespace-nowrap text-muted">{sub}</span>
      </div>
    </td>
  );
}

function ValueCell({ c, off, children }: { c: Source; off: boolean; children: React.ReactNode }) {
  return (
    <td className={cn("border-b border-l border-line px-3 py-1.5 text-right whitespace-nowrap group-hover:bg-sunken/40",
      off ? "bg-discharge/12 font-semibold text-discharge" : children === null ? "text-muted/50" : "")}>
      {children ?? (c.error ? "–" : "…")}
    </td>
  );
}

function OnOff({ on }: { on: boolean }) {
  const { t } = useTranslation();
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={cn("h-2 w-2 rounded-full", on ? "bg-ok" : "border border-muted")} aria-hidden />{on ? t("par.on") : t("par.off")}
    </span>
  );
}
