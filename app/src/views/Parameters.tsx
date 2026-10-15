import { Fragment, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Archive, Download, FileUp, FolderOpen, History, PenLine, SlidersHorizontal, TriangleAlert, X } from "lucide-react";
import { api, isDemo, type Bus, type Installation } from "../api";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { InfoIcon, helpTitle, useHelpEntry } from "../help";
import { cn } from "../lib/utils";
import { BM_NAMES, GROUPS, KEYS, SWITCH_BITS, UNITS, decimals, deviating, exportName, fmtParam, fromPack, fromStored, parseExport, toExport, type Parameters, type Source } from "../params";
import { blockers, changes, fromRaw, notTaken, rawsOf, sameBlock, switchDiff, takeOver, toWire, SERIES_INDEX } from "../paramWrite";
import { EditBar, EditCell, EditSwitch, RefusedPanel, badInput, type Edit, type Full, type Note, type Refused } from "../components/ParamEdit";
import { SerialLine } from "../components/SerialWrite";
import { useUnchecked } from "../bank";
import { useCellDb } from "../cells/store";
import { useAssignments, type Assignment } from "../cells/packCheck";
import { SERIES, paramFindings, paramLimits, type ParamLevel, type ParamLimit } from "../cells/paramCheck";
import { Segmented } from "../components/PackPicker";
import { ClockLine, usePackChoice } from "../components/BmsClock";
import { CalibrationSection, CanProtocolLine } from "../components/Calibration";
import { PackControlSection } from "../components/PackControl";
import { errorText, packBusy, writeError } from "../control";
import { cellName, config } from "./CellAssign";

const packNo = (a: number) => String(a).padStart(2, "0");

// kept while the app runs, so switching views does not drop what was read; `keptLink` is the site and connection it
// was read on: after a change the pack columns stay for comparison but can no longer be edited
let kept: Source[] = [];
let keptLink = "";
// refusals whose switches are still live, per site and address (see Refused); they outlive a view switch
const keptRefused = new Map<string, Refused>();
/** Pack columns of another connection to the same installation: values stay, nothing to edit or undo from. */
const unlink = (s: Source[]) => s.map((x) => (x.kind === "pack" ? { ...x, read: undefined, previous: undefined, lastWritten: undefined, notTaken: undefined } : x));
// the datasheet chosen for the check; null = follow the packs' cell type
let keptRef: Assignment | "none" | null = null;
// the installations view's link to the stored state: the view opens with those columns
let wantStored = false;
export function showStoredParameters() { wantStored = true; }

/** The cell type most packs have, as the default datasheet for the check. */
function commonAssignment(assigned: Record<number, Assignment>): Assignment | undefined {
  const count = new Map<string, { a: Assignment; n: number }>();
  for (const a of Object.values(assigned)) {
    const k = `${a.cell}|${a.parallel}`;
    count.set(k, { a, n: (count.get(k)?.n ?? 0) + 1 });
  }
  return [...count.values()].sort((x, y) => y.n - x.n)[0]?.a;
}

/** Read the packs' parameters (RS485-1/2), show the stored state of the chosen installation, or open BatteryMonitor
 *  exports, and compare them side by side. Editing needs a connection with that installation. */
export function ParametersView({ connected, bus, packs, installation }: { connected: boolean; bus: Bus | null; packs: number[]; installation: Installation | null }) {
  const { t, i18n } = useTranslation();
  const db = useCellDb();
  const site = installation?.id ?? "";
  const { assigned } = useAssignments(installation?.id ?? null);
  const [refChoice, setRefChoiceState] = useState(keptRef);
  const setRef = (r: Assignment | "none" | null) => { keptRef = r; setRefChoiceState(r); };
  const common = commonAssignment(assigned);
  const ref = refChoice === "none" ? undefined : refChoice ?? common;
  const refCell = ref ? db.entries.find((e) => e.cell.id === ref.cell)?.cell : undefined;
  const link = `${site}|${connected}`;
  const [sources, setSourcesState] = useState<Source[]>(() => {
    if (keptLink !== link) {
      // another installation: its read and stored columns go (they describe its packs); opened exports stay
      if (keptLink.split("|")[0] !== site) kept = kept.filter((x) => x.kind === "file");
      kept = unlink(kept); keptLink = link;
    }
    return kept;
  });
  const setSources = (f: (s: Source[]) => Source[]) => setSourcesState((s) => (kept = f(s)));
  const [reading, setReading] = useState<{ address: number; n: number; of: number } | null>(null);
  const [onlyDiff, setOnlyDiff] = useState(false);
  const [saved, setSaved] = useState<{ path: string } | { error: string } | null>(null);
  const file = useRef<HTMLInputElement>(null);
  // one pack for the clock, the CAN protocol, the calibration and the control
  const [chosen, setChosen] = usePackChoice(packs);
  // „Nur ansehen“ writes nothing to a pack
  const writable = connected && (isDemo || (bus === "pack" && !!installation));
  // one write at a time; the pack picker stays put while one runs, so no answer lands on another pack
  const [writing, setWriting] = useState({ clock: false, can: false, cal: false, params: false, serial: false, ctl: false });
  const busyFor = (k: keyof typeof writing) => (b: boolean) => setWriting((w) => (w[k] === b ? w : { ...w, [k]: b }));
  const lockedBut = (k: keyof typeof writing) => Object.entries(writing).some(([o, b]) => o !== k && b);

  // one pack column is edited at a time; its write runs under the same lock as the other writes
  const [edit, setEdit] = useState<Edit | null>(null);
  const [full, setFull] = useState<Full>(null);
  const [note, setNote] = useState<{ address: number; note: Note } | null>(null);
  // packs a write went to without a serial to check the bank with (this connection)
  const unchecked = useUnchecked();
  const canRead = connected && (bus === "pack" || isDemo) && packs.length > 0 && !reading && !edit && !writing.params && !writing.ctl;
  // another installation or a new connection: what was read no longer belongs to the packs on the bus
  useEffect(() => {
    if (keptLink === link) return;
    // another installation: its read and stored columns go (they describe its packs); opened exports stay
    if (keptLink.split("|")[0] !== site) { setSources((s) => s.filter((x) => x.kind === "file")); setStoredNote(null); }
    keptLink = link;
    setSources(unlink); setEdit(null); setNote(null);
  }, [link]); // eslint-disable-line react-hooks/exhaustive-deps

  // the stored state of every place (newest block of its current board), after the stored columns of before
  const [storedNote, setStoredNote] = useState<string | null>(null);
  const loadStored = async () => {
    if (!installation) return;
    setStoredNote(null);
    try {
      const rows = await api.installationSnapshots(installation.id);
      setSources((s) => [...s.filter((x) => x.kind !== "stored"), ...rows.map(fromStored)]);
      // said also when the columns were there already, so a click visibly did something
      const newest = rows.reduce((m, r) => Math.max(m, r.at), 0);
      setStoredNote(rows.length
        ? t("par.storedLoaded", { count: rows.length, at: new Date(newest).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" }) })
        : t("par.storedNone"));
    } catch (e) { setStoredNote(String(e)); }
  };
  // from the installations view, or nothing shown yet without a connection
  useEffect(() => {
    if (installation && (wantStored || (!connected && !sources.length))) { wantStored = false; void loadStored(); }
  }, [installation?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const [, rerender] = useState(0);
  const refusedOf = (a: number) => keptRefused.get(`${site}|${a}`) ?? null;
  const setRefused = (a: number, r: Refused | null) => {
    if (r) keptRefused.set(`${site}|${a}`, r); else keptRefused.delete(`${site}|${a}`);
    rerender((n) => n + 1);
  };
  /** A read of a pack with a pending refusal: once it shows the stored block again (restart), the pack is free. */
  const seen = (a: number, p: Parameters) => { const r = refusedOf(a); if (r && sameBlock(p, r.stored)) setRefused(a, null); };

  const replace = (next: Source) => setSources((s) => s.map((x) => (x.id === next.id ? { ...next, lastWritten: x.lastWritten, previous: x.previous } : x)));
  // last write and the parameters it replaced, kept per installation and pack in the history database
  const loadHistory = (address: number) => api.paramHistory(address).then(
    (h) => setSources((s) => s.map((x) => (x.id === `pack-${address}` ? { ...x, lastWritten: h.last_written, previous: h.previous } : x))),
    () => {},
  );

  const read = async () => {
    setSaved(null);
    const ids = new Set(packs.map((a) => `pack-${a}`));
    // columns appear at once, in pack order before the opened files, and fill as the answers come in
    setSources((s) => [...packs.map((a) => ({ id: `pack-${a}`, kind: "pack" as const, label: packNo(a) })), ...s.filter((x) => !ids.has(x.id))]);
    for (const [n, address] of packs.entries()) {
      setReading({ address, n: n + 1, of: packs.length });
      let next: Source;
      try { const p = await api.parameters(address); next = fromPack(p, address); seen(address, p); void loadHistory(address); }
      catch (e) { next = { id: `pack-${address}`, kind: "pack", label: packNo(address), error: String(e) }; }
      setSources((s) => s.map((x) => (x.id === next.id ? next : x)));
    }
    setReading(null);
  };

  /** After a restart: the pack's column, if shown, is read again (a refused write's live switches are gone then). */
  const rereadPack = (address: number) => {
    if (!sources.some((x) => x.id === `pack-${address}`)) return;
    api.parameters(address).then((p) => { replace(fromPack(p, address)); seen(address, p); },
      (e) => setNote({ address, note: { text: t("ctl.reread", { pack: packNo(address), err: errorText(e, t) }), error: true } }));
  };
  // P79 (SOC protection) of the chosen pack, from its read or stored column, for the remaining capacity
  const p79Of = (a: number | null) => {
    const c = a === null ? undefined : sources.find((x) => x.id === `pack-${a}` && x.values) ?? sources.find((x) => x.id === `stored-${a}` && x.values);
    return c?.values?.[79] ?? null;
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

  const startEdit = (c: Source, from?: Parameters | null) => {
    if (!c.read || c.address === undefined) return;
    const a = c.address;
    const base = rawsOf(c.read), baseSwitches = [...c.read.function_switches];
    // „Vorherigen Stand“: that block, with P65 as now (it changes only when unlocked)
    const raws = from ? rawsOf(from).map((r, i) => (i === SERIES_INDEX ? base[i] : r)) : [...base];
    setEdit({ address: a, base, baseSwitches, raws, switches: from ? [...from.function_switches] : [...baseSwitches], text: {}, seriesUnlocked: false });
    setNote(null);
    setSources((s) => s.map((x) => (x.id === c.id ? { ...x, notTaken: undefined } : x)));
    // the learned full capacity (0x42): every successful write sets it to P58 when they are 10 Ah apart
    setFull(null);
    api.packLive(a).then((l) => setFull({ address: a, ah: l.telemetry.full_capacity_ah }), (e) => setFull({ address: a, error: String(e) }));
  };

  const takeOverFrom = (id: string) => {
    const src = sources.find((x) => x.id === id);
    if (!src?.values || !src.switches) return;
    setEdit((e) => e && { ...e, raws: takeOver(e.raws, src.values!, !e.seriesUnlocked), switches: [...src.switches!], text: {} });
  };

  /** Write the edit of pack `a`, or with `restore` the stored block after a refusal (its switches are live but not
   *  stored then); a restore needs no open edit. */
  const write = async (a: number, restore?: Parameters) => {
    const e0 = edit?.address === a ? edit : null;
    if (!restore && (!e0 || refusedOf(a))) return;
    const pack = packNo(a);
    const sent = restore ? rawsOf(restore) : e0!.raws, sentSwitches = restore ? restore.function_switches : e0!.switches;
    const base = restore ? toWire(sent, sentSwitches) : toWire(e0!.base, e0!.baseSwitches);
    busyFor("params")(true); setNote(null);
    const say = (n: Note) => setNote({ address: a, note: n });
    const mine = (f: (e: Edit) => Edit) => setEdit((e) => (e && e.address === a ? f(e) : e));
    try {
      const r = await api.writeParameters(a, base, toWire(sent, sentSwitches), !!restore);
      if (r.kind === "stale") {
        // nothing written: the column shows the pack now, the user's changes stay on top of it
        const fresh = rawsOf(r.fresh), fs = r.fresh.function_switches;
        mine((e) => ({
          ...e, base: fresh, baseSwitches: [...fs],
          raws: e.raws.map((x, i) => (x !== e.base[i] ? x : fresh[i])),
          switches: e.switches.map((x, g) => { const touched = x ^ e.baseSwitches[g]; return (x & touched) | (fs[g] & ~touched & 0xff); }),
        }));
        replace(fromPack(r.fresh, a));
        seen(a, r.fresh);
        say({ text: t("pw.stale", { pack }), error: true });
      } else if (r.kind === "refused") {
        // only the first refusal knows the stored block: later reads show the live switches
        if (r.refusal === "refused" && !refusedOf(a) && switchDiff(sentSwitches, r.stored.function_switches).length)
          setRefused(a, { stored: r.stored, live: [...sentSwitches] });
        say({ text: t(r.refusal === "refused" ? (restore ? "pw.restoreRefused" : "pw.refusedShort") : "pw.invalid"), error: true });
      } else {
        void loadHistory(a);
        if (restore) setRefused(a, null);
        if (r.read_back) {
          const missed = notTaken(sent, sentSwitches, rawsOf(r.read_back), r.read_back.function_switches);
          replace({ ...fromPack(r.read_back, a), notTaken: missed });
          const n = missed.params.size + missed.switches.size;
          say(n ? { text: t("pw.notTakenCount", { count: n }), error: true } : { text: t(restore ? "pw.restored" : "pw.done"), error: false });
          // after restoring, the edit stays open on the stored state, so the values can be corrected
          if (restore) mine((e) => ({ ...e, base: rawsOf(r.read_back!), baseSwitches: [...r.read_back!.function_switches] }));
        } else say({ text: t("pw.unread", { err: r.read_error ?? "" }), error: true });
        if (!restore) setEdit(null);
      }
    } catch (e) { say({ text: writeError(e, t, (err) => t("pw.error", { err })), error: true }); }
    finally { busyFor("params")(false); }
  };

  const nf = (i: number) => new Intl.NumberFormat(i18n.language, { minimumFractionDigits: decimals(i), maximumFractionDigits: decimals(i) });
  const editId = edit ? `pack-${edit.address}` : null;
  // the edited column shows the values as edited, so differences and the datasheet check follow the input
  const cols = [...sources.filter((c) => c.kind === "file"), ...sources.filter((c) => c.kind === "stored"), ...sources.filter((c) => c.kind === "pack")]
    .map((c) => (edit && c.id === editId ? { ...c, values: edit.raws.map((r, i) => fromRaw(i, r)), switches: edit.switches } : c));
  const paramKeys = (i: number) => cols.map((c) => (c.values ? fmtParam(i, c.values[i]) : undefined));
  const switchKeys = (g: number) => cols.map((c) => (c.switches ? String(c.switches[g]) : undefined));
  const diffParams = KEYS.map((_, i) => deviating(paramKeys(i)));
  const diffSwitches = Array.from({ length: 8 }, (_, g) => deviating(switchKeys(g)));
  const nDiff = diffParams.filter((d) => d.size).length;
  const nDiffSwitches = diffSwitches.filter((d) => d.size).length;
  const withValues = cols.filter((c) => c.values).length;
  const suspect = cols.filter((c) => c.suspectFrom !== undefined && c.suspectFrom !== null);
  const broken = cols.filter((c) => c.kind === "file" && c.error);

  // datasheet check: limits for the series count most sources have, findings per column
  const series = Math.round(mode(cols.flatMap((c) => (c.values ? [c.values[SERIES]] : []))) ?? 0);
  const limits = refCell && ref ? paramLimits(refCell, ref.parallel, series) : new Map<number, ParamLimit>();
  const findings = cols.map((c) => (refCell && ref && c.values ? paramFindings(refCell, ref.parallel, c.values) : new Map<number, ParamLevel>()));
  const outside = KEYS.map((_, i) => i).filter((i) => findings.some((f) => f.has(i)));
  // „Nur Abweichungen“ compares columns, or one column against the datasheet. Without either it does not filter
  // (the choice stays for later), so the table never empties because a column or the datasheet went away.
  const diffUsable = withValues >= 2 || (!!refCell && withValues >= 1);
  const filtered = onlyDiff && diffUsable;
  const bitsOff = (g: number) => SWITCH_BITS[g].filter((b) => deviating(cols.map((c) => (c.switches ? String((c.switches[g] >> b) & 1) : undefined))).size);
  const nothingLeft = filtered && GROUPS.every((g) => !g.indices.some((i) => diffParams[i].size || outside.includes(i))) && diffSwitches.every((d) => !d.size);
  const lang = i18n.language;
  const editCol = cols.findIndex((c) => c.id === editId);
  const editChanges = edit ? changes(edit.base, edit.raws, edit.baseSwitches, edit.switches) : [];
  const editBlockers = edit ? blockers(edit.base, edit.raws, badInput(edit), edit.seriesUnlocked) : [];
  const editSheet = edit && editCol >= 0 ? editChanges.flatMap((c) => {
    const level = c.kind === "param" ? findings[editCol].get(c.index) : undefined, l = c.kind === "param" ? limits.get(c.index) : undefined;
    return c.kind === "param" && level && l ? [{ index: c.index, level, limit: limitText(t, l, c.index, lang) }] : [];
  }) : [];
  const takeOverSources = cols.filter((c) => c.id !== editId && c.values && c.switches && !c.error && (c.suspectFrom === undefined || c.suspectFrom === null))
    .map((c) => ({ id: c.id, label: c.kind === "pack" ? t("par.pack", { n: c.label }) : c.label }));
  const editable = (c: Source) => writable && c.kind === "pack" && !!c.read && !edit && !reading && !lockedBut("params") && !writing.params;
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
          <Button disabled={!installation || isDemo} onClick={loadStored} title={t("par.storedHint")}><Archive className="h-4 w-4" />{t("par.stored")}</Button>
          <Button onClick={() => file.current?.click()}><FileUp className="h-4 w-4" />{t("par.open")}</Button>
          <input ref={file} type="file" accept=".xml,application/xml,text/xml" multiple className="hidden" onChange={(e) => open(e.target.files)} />
          {cols.length > 0 && <Button variant="ghost" disabled={!!reading || !!edit || writing.params} onClick={() => { setSources(() => []); setSaved(null); }} title={t("par.clearHint")}>{t("par.clear")}</Button>}
          <label className="ml-auto flex cursor-pointer items-center gap-2 text-sm" title={diffUsable ? t("par.onlyDiffHint") : t("par.onlyDiffNeeds")}>
            <input type="checkbox" className="h-4 w-4 accent-[var(--charge)]" checked={filtered} onChange={(e) => setOnlyDiff(e.target.checked)} disabled={!diffUsable} />
            <span className={cn(!diffUsable && "text-muted")}>{t("par.onlyDiff")}</span>
          </label>
        </div>
        <p className={cn("text-sm", bus === "can" && connected && !isDemo ? "text-discharge" : "text-muted")}>
          {reading ? t("par.reading", { pack: packNo(reading.address), n: reading.n, of: reading.of })
            : !connected ? t("par.needConnection")
            : bus === "can" && !isDemo ? t("par.canBus")
            : !installation && !isDemo ? t("par.viewOnly")
            : t("par.packBus")}
        </p>
        {storedNote && <p className="text-sm text-muted">{storedNote}</p>}
        {chosen !== null && unchecked.has(chosen) && <p className="flex items-center gap-1.5 text-sm text-discharge">{t("bank.unchecked", { pack: packNo(chosen) })}<InfoIcon id="topic.installations" /></p>}
        <ClockLine connected={connected} bus={bus} packs={packs} address={chosen} setAddress={setChosen} locked={lockedBut("clock")} onBusy={busyFor("clock")} canWrite={writable} />
        <CanProtocolLine address={chosen} usable={writable} locked={lockedBut("can")} onBusy={busyFor("can")} />
        <SerialLine address={chosen} usable={writable} locked={lockedBut("serial")} onBusy={busyFor("serial")} />
        <CalibrationSection address={chosen} usable={writable} locked={lockedBut("cal")} onBusy={busyFor("cal")} />
        <PackControlSection address={chosen} usable={writable} locked={lockedBut("ctl")} onBusy={busyFor("ctl")} onChanged={rereadPack}
          p79={p79Of(chosen)} linkKey={link} />
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-line pt-3 text-sm">
          <span className="flex items-center gap-1.5 font-medium">{t("par.check")}<InfoIcon id="topic.paramCheck" /></span>
          <select value={refChoice === "none" ? "" : ref?.cell ?? ""} disabled={db.loading}
            onChange={(e) => setRef(e.target.value ? { cell: e.target.value, parallel: ref?.parallel ?? common?.parallel ?? 1 } : "none")}
            className="h-8 min-w-60 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge">
            <option value="">{t("par.noCheck")}</option>
            {db.entries.map((e) => <option key={e.cell.id} value={e.cell.id}>{cellName(e.cell)}</option>)}
          </select>
          {ref && <Segmented value={String(ref.parallel)} onChange={(v) => setRef({ ...ref, parallel: Number(v) })} items={[1, 2, 3, 4].map((n) => [String(n), `${n}P`] as [string, string])} />}
          <span className="text-muted">
            {!ref ? (common ? "" : t("par.checkHint"))
              : refChoice === null ? t("par.fromAssignment")
              : common && (common.cell !== ref.cell || common.parallel !== ref.parallel) ? t("par.notAssignment") : ""}
          </span>
          {refChoice !== null && common && <Button size="sm" variant="ghost" onClick={() => setRef(null)}>{t("par.useAssignment")}</Button>}
        </div>
        {saved && (
          "path" in saved ? (
            <p className="flex flex-wrap items-center gap-2 text-sm text-ok">
              {t("par.saved", { name: saved.path.split(/[\\/]/).pop() })}
              {!isDemo && <Button size="sm" variant="ghost" onClick={() => api.revealParameters(saved.path)}><FolderOpen className="h-4 w-4" />{t("par.reveal")}</Button>}
            </p>
          ) : <p className="text-sm text-alarm">{t("par.saveError", { err: saved.error })}</p>
        )}
      </Panel>

      {refCell && ref && withValues > 0 && (
        outside.length ? (
          <div className="flex items-start gap-2 rounded-md border border-alarm/40 bg-alarm/10 px-4 py-3 text-sm">
            <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-alarm" aria-hidden />
            <div className="space-y-1">
              <p>{t("par.outside", { cell: `${cellName(refCell)} ${config(series || undefined, ref.parallel)}`, count: outside.length })}</p>
              <ul className="list-disc pl-5">
                {outside.map((i) => {
                  const n = findings.filter((f) => f.has(i)).length, l = limits.get(i);
                  return (
                    <li key={i}>
                      {helpTitle(`param.${KEYS[i]}`, lang)} (P{i}): {l && limitText(t, l, i, lang)}
                      {findings.some((f) => f.get(i) === "note") ? ` ${t("par.ratedNote")}` : ""}
                      <span className="text-muted"> · {n === withValues ? t("par.inAll", { count: n }) : t("par.inSome", { list: cols.filter((_, ci) => findings[ci].has(i)).map((c) => c.label).join(", ") })}</span>
                    </li>
                  );
                })}
              </ul>
            </div>
          </div>
        ) : <p className="text-sm text-ok">{t("par.inside", { cell: `${cellName(refCell)} ${config(series || undefined, ref.parallel)}` })}</p>
      )}

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
          {edit && editCol >= 0 && (
            <EditBar e={edit} pack={packNo(edit.address)} changes={editChanges} blockers={editBlockers} sheet={editSheet} full={full}
              note={note?.address === edit.address ? note.note : null} refused={refusedOf(edit.address)} busy={writing.params} locked={lockedBut("params")} sources={takeOverSources}
              onChange={(f) => setEdit((e) => e && f(e))} onTakeOver={takeOverFrom} onWrite={() => write(edit.address)} onRestore={(stored) => write(edit.address, stored)}
              onDiscard={() => { setEdit(null); setNote(null); }} />
          )}
          {cols.flatMap((c) => {
            const r = c.kind === "pack" && c.address !== undefined && c.address !== edit?.address ? refusedOf(c.address) : null;
            return r ? [<RefusedPanel key={c.id} refused={r} fullAh={full && "ah" in full && full.address === c.address ? full.ah : null} pack={c.label}
              busy={writing.params} locked={lockedBut("params")} onRestore={(stored) => write(c.address!, stored)} />] : [];
          })}
          {!edit && note && (
            <p className={cn("text-sm", note.note.error ? "text-alarm" : "text-ok")}>{t("par.pack", { n: packNo(note.address) })}: {note.note.text}</p>
          )}
          <Panel className="overflow-x-auto">
            <table className="w-full border-separate border-spacing-0 text-sm tabular-nums">
              <thead>
                <tr>
                  <th className="sticky left-0 z-10 min-w-80 border-b border-line bg-surface px-4 py-3 text-left font-normal text-muted">{t("par.parameter")}</th>
                  {refCell && (
                    <th className="min-w-24 border-b border-l border-line bg-sunken/40 px-3 py-2 text-right align-bottom font-normal">
                      <div className="font-medium text-ink">{t("par.datasheet")}</div>
                      <div className="max-w-36 truncate text-xs text-muted" title={cellName(refCell)}>{refCell.model} {config(series || undefined, ref!.parallel)}</div>
                    </th>
                  )}
                  {cols.map((c) => (
                    <ColumnHead key={c.id} c={c} busy={!!reading || writing.params} onSave={() => save(c)} onRemove={() => setSources((s) => s.filter((x) => x.id !== c.id))}
                      onEdit={editable(c) ? () => startEdit(c) : undefined} onPrevious={editable(c) && c.previous ? () => startEdit(c, c.previous) : undefined} />
                  ))}
                </tr>
              </thead>
              <tbody>
                {GROUPS.map((g) => {
                  const rows = g.indices.filter((i) => !filtered || diffParams[i].size || outside.includes(i));
                  if (!rows.length) return null;
                  return (
                    <Fragment key={g.id}>
                      <GroupRow span={cols.length + 1 + (refCell ? 1 : 0)} title={t(`par.group.${g.id}`)} />
                      {rows.map((i) => (
                        <tr key={i} className="group">
                          <NameCell id={`param.${KEYS[i]}`} sub={`P${i}${UNITS[i] ? ` · ${UNITS[i]}` : ""}`} original={BM_NAMES[i]} />
                          {refCell && (
                            <td className="border-b border-l border-line bg-sunken/40 px-3 py-1.5 text-right whitespace-nowrap text-muted">
                              {limits.has(i) ? limitText(t, limits.get(i)!, i, lang, true) : ""}
                            </td>
                          )}
                          {cols.map((c, ci) => c.id === editId && edit ? (
                            <EditCell key={c.id} e={edit} i={i} level={findings[ci].get(i)} disabled={writing.params || (i === SERIES_INDEX && !edit.seriesUnlocked)}
                              onChange={(f) => setEdit((e) => e && f(e))} />
                          ) : (
                            <ValueCell key={c.id} c={c} off={diffParams[i].has(ci)} level={findings[ci].get(i)} missed={c.notTaken?.params.has(i)}
                              title={findings[ci].has(i) && limits.has(i) ? `${t("par.datasheet")}: ${limitText(t, limits.get(i)!, i, lang)}` : undefined}>
                              {c.values ? nf(i).format(c.values[i]) : null}
                            </ValueCell>
                          ))}
                        </tr>
                      ))}
                    </Fragment>
                  );
                })}
                {nothingLeft && (
                  <tr><td colSpan={cols.length + 1 + (refCell ? 1 : 0)} className="px-4 py-3 text-ok">{t("par.noDiff")}</td></tr>
                )}
                {Array.from({ length: 8 }, (_, g) => g)
                  .filter((g) => !filtered || diffSwitches[g].size)
                  .map((g, n) => (
                    <Fragment key={`bg${g}`}>
                      <GroupRow span={cols.length + 1 + (refCell ? 1 : 0)} title={t(`par.switchGroup.${g}`, { g })} help={n === 0 ? "topic.switches" : undefined} first={n === 0} />
                      <tr>
                        <td className="sticky left-0 z-10 border-b border-line bg-surface px-4 py-1.5 text-muted">{t("par.byte", { g })}</td>
                        {refCell && <td className="border-b border-l border-line bg-sunken/40" />}
                        {cols.map((c, ci) => (
                          <ValueCell key={c.id} c={c} off={diffSwitches[g].has(ci)} missed={c.notTaken?.switches.has(g)}>{c.switches ? c.switches[g].toString(16).toUpperCase().padStart(2, "0") : null}</ValueCell>
                        ))}
                      </tr>
                      {(filtered ? bitsOff(g) : SWITCH_BITS[g])
                        .map((b) => {
                          const off = deviating(cols.map((c) => (c.switches ? String((c.switches[g] >> b) & 1) : undefined)));
                          return (
                            <tr key={b}>
                              <NameCell id={`switch.${g}.${b}`} sub={t("par.bit", { b })} />
                              {refCell && <td className="border-b border-l border-line bg-sunken/40" />}
                              {cols.map((c, ci) => c.id === editId && edit ? (
                                <EditSwitch key={c.id} e={edit} g={g} b={b} disabled={writing.params} onChange={(f) => setEdit((e) => e && f(e))} />
                              ) : (
                                <ValueCell key={c.id} c={c} off={off.has(ci)} missed={c.notTaken?.switches.has(g)}>{c.switches ? <OnOff on={((c.switches[g] >> b) & 1) === 1} /> : null}</ValueCell>
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

function ColumnHead({ c, busy, onSave, onRemove, onEdit, onPrevious }: {
  c: Source; busy: boolean; onSave: () => void; onRemove: () => void;
  /** set when this pack column can be edited now; „Vorherigen Stand“ only after a write of this pack */
  onEdit?: () => void; onPrevious?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const pending = c.kind === "pack" && !c.values && !c.error;
  return (
    <th className="min-w-24 border-b border-l border-line bg-surface px-3 py-2 text-right align-bottom font-normal">
      <div className="flex items-center justify-end gap-1">
        {c.suspectFrom !== undefined && c.suspectFrom !== null && <TriangleAlert className="h-3.5 w-3.5 text-discharge" aria-label={t("par.suspectShort")} />}
        <span className={cn("truncate font-medium text-ink", c.kind === "file" && "max-w-40")} title={c.label}>
          {c.kind === "pack" ? t("par.pack", { n: c.label }) : c.kind === "stored" ? t("par.storedCol", { n: c.label }) : c.label}
        </span>
        {onEdit && (
          <button onClick={onEdit} className="rounded p-1 text-muted hover:bg-sunken hover:text-ink" title={t("pw.edit")} aria-label={t("pw.edit")}>
            <PenLine className="h-3.5 w-3.5" />
          </button>
        )}
        {onPrevious && (
          <button onClick={onPrevious} className="rounded p-1 text-muted hover:bg-sunken hover:text-ink" title={t("pw.previous")} aria-label={t("pw.previous")}>
            <History className="h-3.5 w-3.5" />
          </button>
        )}
        {c.kind !== "file" && c.read && (
          <button onClick={onSave} disabled={busy} className="rounded p-1 text-muted hover:bg-sunken hover:text-ink disabled:opacity-40" title={t("par.save")} aria-label={t("par.save")}>
            <Download className="h-3.5 w-3.5" />
          </button>
        )}
        {(c.kind !== "pack" || c.error) && (
          <button onClick={onRemove} disabled={busy} className="rounded p-1 text-muted hover:bg-sunken hover:text-ink disabled:opacity-40" title={t("par.remove")} aria-label={t("par.remove")}>
            <X className="h-3.5 w-3.5" />
          </button>
        )}
      </div>
      <div className={cn("text-xs", c.error ? "text-alarm" : "text-muted")} title={c.error && errorText(c.error, t)}>
        {c.error ? (c.kind === "pack" ? (packBusy(c.error) ? t("par.busyShort") : t("par.noAnswer")) : t("par.unreadable")) : pending ? t("par.pending") : c.kind === "file" ? (c.device || t("par.file")) : c.device}
      </div>
      {c.stored && (
        <div className={cn("text-xs whitespace-nowrap", c.stored.board === "current" ? "text-muted" : "text-discharge")}
          title={c.stored.board === "previous" ? t("par.previousBoard", { serial: c.stored.serial ?? "" }) : c.stored.board === "unclear" ? t("par.boardUnclear") : undefined}>
          {t("par.storedAt", { at: new Date(c.stored.at).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" }) })} · {t(`par.kind.${c.stored.kind}`)}
          {c.stored.board === "previous" ? ` · ${t("par.previousBoardShort")}` : c.stored.board === "unclear" ? ` · ${t("par.boardUnclearShort")}` : ""}
        </div>
      )}
      {c.stored?.unconfirmed && (
        <div className="text-xs whitespace-nowrap text-discharge" title={t("par.switchesUnconfirmed")}>{t("par.switchesUnconfirmedShort")}</div>
      )}
      {c.lastWritten && (
        <div className="text-xs whitespace-nowrap text-muted">
          {t("pw.lastWritten", { at: new Date(c.lastWritten).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" }) })}
        </div>
      )}
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

function ValueCell({ c, off, level, missed, title, children }: { c: Source; off: boolean; level?: ParamLevel; missed?: boolean; title?: string; children: React.ReactNode }) {
  const { t } = useTranslation();
  return (
    <td title={missed ? t("pw.notTaken") : title} className={cn("border-b border-l border-line px-3 py-1.5 text-right whitespace-nowrap group-hover:bg-sunken/40",
      missed ? "bg-alarm/20 font-semibold text-alarm" : level === "over" ? "bg-alarm/12 font-semibold text-alarm" : level === "note" ? "font-semibold text-charge underline decoration-dotted underline-offset-4"
      : off ? "bg-discharge/12 font-semibold text-discharge" : children === null ? "text-muted/50" : "")}>
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

/** Most frequent value, undefined for none. */
function mode(xs: number[]): number | undefined {
  const n = new Map<number, number>();
  for (const x of xs) n.set(x, (n.get(x) ?? 0) + 1);
  return [...n.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
}

/** "≤ 3.650 V", "≥ 0.0 °C", "≈ 280.00 Ah"; short leaves out the unit (the row shows it). */
function limitText(t: (k: string) => string, l: ParamLimit, i: number, lang: string, short = false): string {
  const v = new Intl.NumberFormat(lang, { minimumFractionDigits: decimals(i), maximumFractionDigits: decimals(i) }).format(l.limit);
  const sign = l.kind === "min" ? "≥" : l.kind === "rated" ? "≈" : "≤";
  const abs = l.kind === "maxAbs" ? ` ${t("par.magnitude")}` : "";
  return `${sign} ${v}${short || !UNITS[i] ? "" : ` ${UNITS[i]}`}${short ? "" : abs}`;
}
