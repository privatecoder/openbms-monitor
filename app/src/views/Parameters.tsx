import { AccessMatrix } from "../components/AccessMatrix";
import { Fragment, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Archive, Download, FileUp, FolderOpen, History, PenLine, RotateCw, SlidersHorizontal, TriangleAlert, X } from "lucide-react";
import { api, isDemo, linkNow, type Bus, type Installation, type ParamWrite } from "../api";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { InfoIcon, helpTitle, useHelpEntry } from "../help";
import { cn } from "../lib/utils";
import { BM_NAMES, GROUPS, KEYS, SWITCH_BITS, UNITS, decimals, deviating, exportName, fmtParam, fromPack, fromStored, parseExport, toExport, withHistory, withPlaceholders, type Parameters, type Source } from "../params";
import { afterUart1Write, blockers, keepProof, capacityEffects, changeOf, changes, fromRaw, fullRequests, notTaken, planTarget, rawsOf, sameBlock, switchDiff, takeOver, toWire, SERIES_INDEX, type Raws, type TargetPlan, type Wire } from "../paramWrite";
import { failure, outcomeOf, runSeries, type Outcome, type SeriesEnd } from "../multiWrite";
import { EditBar, EditCell, EditSwitch, RefusedPanel, badInput, type Edit, type Full, type Note, type Refused } from "../components/ParamEdit";
import { MultiConfirm, SeriesPanel, TargetPicker, type Row } from "../components/MultiWrite";
import { SerialLine } from "../components/SerialWrite";
import { useUnchecked } from "../bank";
import { useCellDb } from "../cells/store";
import { useAssignments, type Assignment } from "../cells/packCheck";
import { SERIES, paramFindings, paramLimits, type ParamLevel, type ParamLimit } from "../cells/paramCheck";
import { Segmented } from "../components/PackPicker";
import { ClockLine, usePackChoice } from "../components/BmsClock";
import { CalibrationSection, CanProtocolLine } from "../components/Calibration";
import { PackControlSection } from "../components/PackControl";
import { errorText, packBusy, uart1Error, writeError } from "../control";
import { latestOnly } from "../maint";
import { cutoffBits, cutoffRowLabel, gateBits, gateRowLabel, getProfile, invertedBit, keepMask, LOCK_BIT, lockChanged, packLockActive, selfLock, switchOn, useProfile, writeLocked } from "../profile";
import { usePackProbes } from "../packProbes";
import { controlHint, controlMode, packCutoff, powerLocked, powerRtc } from "../power";
import { StatusLine } from "../components/Section";
import { cellName, config } from "./CellAssign";

const packNo = (a: number) => String(a).padStart(2, "0");

// kept while the app runs, so switching views does not drop what was read; `keptLink` is the site and connection it
// was read on: after a change the pack columns stay for comparison but can no longer be edited
let kept: Source[] = [];
let keptLink = "";
// refusals whose switches are still live, per site and address (see Refused); they outlive a view switch
const keptRefused = new Map<string, Refused>();
/** Pack columns of another connection to the same installation: values stay, nothing to edit or undo from. */
// a pack column without an answer says nothing about the next connection's packs: it goes
const unlink = (s: Source[]) => s.filter((x) => !(x.kind === "pack" && x.error && !x.values))
  .map((x) => (x.kind === "pack" ? { ...x, read: undefined, link: undefined, previous: undefined, lastWritten: undefined, notTaken: undefined } : x));
/** A pack column from a read, tagged with the link it was read on and, over the CAN socket, its path. */
// a stored or file column without a patch proof: its Group0 patch bits are shown raw, never with a function name
const rawGate = (c: Source) => c.kind !== "pack" && !c.read?.patch;
const packCol = (p: Parameters, address: number, link: string, path?: "uart1"): Source => ({ ...fromPack(p, address), link, ...(path ? { path } : {}) });
// the datasheet chosen for the check; null = follow the packs' cell type
let keptRef: Assignment | "none" | null = null;
// the installations view's link to the stored state: the view opens with those columns
let wantStored = false;
// the newest „Letzter Stand“ load; an older one's answer is dropped
const storedLoads = latestOnly();
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

/** Read the packs' parameters (RS485-1/2, or the CAN socket of a pack with the patched parameter service), show the stored state of the chosen installation, or open BatteryMonitor
 *  exports, and compare them side by side. Editing needs a connection with that installation. */
export function ParametersView({ connected, bus, packs, installation, versionOf }: {
  connected: boolean; bus: Bus | null; packs: number[]; installation: Installation | null;
  /** the firmware version a pack reported (0x51), for the access paths */
  versionOf?: (address: number) => string | undefined;
}) {
  const { t, i18n } = useTranslation();
  const db = useCellDb();
  const site = installation?.id ?? "";
  const { assigned } = useAssignments(installation?.id ?? null);
  const [refChoice, setRefChoiceState] = useState(keptRef);
  const setRef = (r: Assignment | "none" | null) => { keptRef = r; setRefChoiceState(r); };
  const common = commonAssignment(assigned);
  const ref = refChoice === "none" ? undefined : refChoice ?? common;
  const refCell = ref ? db.entries.find((e) => e.cell.id === ref.cell)?.cell : undefined;
  // a reconnect to the same installation is another connection as well
  const link = `${site}|${connected ? linkNow() : "-"}`;
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
  // the CAN socket's parameter service reads from MAINT 1.2 on and writes from 1.3 on (the backend checks the board's
  // identity before every write and names why it refuses); the other writes stay on RS485-1/2
  const path = bus === "can" && !isDemo ? "uart1" as const : undefined;
  const paramWritable = writable || (connected && bus === "can" && !!installation);
  // ein/aus, restart and the clock over the CAN socket: per pack, as this connection proved the image flag (the master by
  // the profile, a slave by its own 64/03), with the lock bit of the newest state seen
  const profile = useProfile();
  const [probes] = usePackProbes();
  const powerOf = (a: number) => powerRtc(a, profile, probes[a]);
  const lockOf = (a: number) => powerLocked(a, targetRead(a)?.function_switches[0], profile);
  const ctlMode = controlMode({ connected, bus, demo: isDemo, installation: !!installation, power: chosen !== null && powerOf(chosen) });
  // a CAN socket write that found the same board still listed at another address it could not check there: the
  // explicit, confirmed „no longer there“ for that entry
  // `gen`: the connection the finding came from (the action is refused on another one, and dropped on a link change)
  const [gone, setGone] = useState<{ address: number; board: number; at: number | null; gen: number; armed: boolean } | null>(null);
  // one write at a time; the pack picker stays put while one runs, so no answer lands on another pack
  // `multi`: a parameter write to several packs, from its confirmation to its last pack (also while it waits)
  const [writing, setWriting] = useState({ clock: false, can: false, cal: false, params: false, serial: false, ctl: false, multi: false });
  const busyFor = (k: keyof typeof writing) => (b: boolean) => setWriting((w) => (w[k] === b ? w : { ...w, [k]: b }));
  const lockedBut = (...ks: (keyof typeof writing)[]) => Object.entries(writing).some(([o, b]) => !ks.includes(o as keyof typeof writing) && b);
  const paramsBusy = writing.params || writing.multi;

  // one pack column is edited at a time; its write runs under the same lock as the other writes
  const [edit, setEdit] = useState<Edit | null>(null);
  const [full, setFull] = useState<Full>(null);
  // the full-capacity reads of the editor: only the newest one, for the pack still edited, may set `full`
  const fullReqs = useRef(fullRequests());
  const editing = useRef<number | undefined>(undefined);
  editing.current = edit?.address;
  const [note, setNote] = useState<{ address: number; note: Note } | null>(null);
  // packs a write went to without a serial to check the bank with (this connection)
  const unchecked = useUnchecked();
  const canRead = connected && (bus === "pack" || bus === "can" || isDemo) && packs.length > 0 && !reading && !edit && !paramsBusy && !writing.ctl;
  // the edit to further packs: the packs picked, the run's confirmation and its progress and result
  const [picking, setPicking] = useState(false);
  const [targets, setTargets] = useState<number[]>([]);
  const [confirm, setConfirm] = useState<{ plans: TargetPlan[]; skipped: Row[]; capacity: { resets: number[]; unknown: number[] } | null; reading: number | null } | null>(null);
  const [runState, setRunState] = useState<{ rows: Row[]; current: number | null; paused: number | null; end: SeriesEnd | null } | null>(null);
  const prepTicket = useRef(0);
  const cancelled = useRef(false);
  const resume = useRef<((goOn: boolean) => void) | null>(null);
  // leaving the view ends a run before its next pack (a waiting one too), so nothing writes on unseen
  useEffect(() => () => { cancelled.current = true; resume.current?.(false); prepTicket.current++; }, []);
  // another installation or a new connection: what was read no longer belongs to the packs on the bus
  useEffect(() => {
    if (keptLink === link) return;
    // another installation: its read and stored columns go (they describe its packs); opened exports stay
    if (keptLink.split("|")[0] !== site) { setSources((s) => s.filter((x) => x.kind === "file")); setStoredNote(null); }
    keptLink = link;
    setSources(unlink); setEdit(null); setNote(null); setGone(null);
    // a run waiting at a refusal ends; one being prepared is dropped
    resume.current?.(false); if (confirm) cancelPrepare(); setPicking(false); setTargets([]);
  }, [link]); // eslint-disable-line react-hooks/exhaustive-deps

  // the stored state of every place (newest block of its current board), after the stored columns of before
  const [storedNote, setStoredNote] = useState<string | null>(null);
  const loadStored = async () => {
    if (!installation) return;
    const ticket = storedLoads.begin(), id = installation.id;
    // a late answer for another installation (or a superseded load) must not put its columns into this one
    const current = () => storedLoads.accepts(ticket) && keptLink.split("|")[0] === id;
    setStoredNote(null);
    try {
      const rows = await api.installationSnapshots(id);
      if (!current()) return;
      setSources((s) => [...s.filter((x) => x.kind !== "stored"), ...rows.map(fromStored)]);
      // said also when the columns were there already, so a click visibly did something
      const newest = rows.reduce((m, r) => Math.max(m, r.at), 0);
      setStoredNote(rows.length
        ? t("par.storedLoaded", { count: rows.length, at: new Date(newest).toLocaleString(i18n.language, { dateStyle: "short", timeStyle: "short" }) })
        : t("par.storedNone"));
    } catch (e) { if (current()) setStoredNote(String(e)); }
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

  const replace = (next: Source) => setSources((s) => s.map((x) => (x.id === next.id ? { ...keepProof(x, next), lastWritten: x.lastWritten, previous: x.previous } : x)));
  // last write and the parameters it replaced, kept per installation and pack in the history database
  // the answer belongs to the link it was asked on (`withHistory`): a late one after a change is dropped
  const loadHistory = (address: number) => {
    const l = linkNow();
    return api.paramHistory(address).then(
      (h) => { if (linkNow() === l) setSources((s) => withHistory(s, address, l, h)); },
      () => {},
    );
  };

  // the packs on the bus show as empty columns („nicht gelesen“) before a read, so one can be read alone; they go with
  // the connection
  const readsBus = connected && (bus === "pack" || bus === "can" || isDemo);
  useEffect(() => {
    setSources((s) => withPlaceholders(s, packs, readsBus));
  }, [link, readsBus, packs.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps

  /** One pack's read on link `l`: its column, or one with the error; null once the link changed (the answer is dropped). */
  const readPack = async (address: number, l: string): Promise<Source | null> => {
    try {
      const p = await api.parameters(address);
      if (linkNow() !== l) return null;
      // over the CAN socket only a block of an identified board is stored (MAINT 1.3), so only then is there a last write
      const next = packCol(p, address, l, path); seen(address, p); if (!path || p.bms_serial) void loadHistory(address);
      return next;
    } catch (e) {
      if (linkNow() !== l) return null;
      return { id: `pack-${address}`, kind: "pack", address, label: packNo(address), error: String(e) };
    }
  };

  const read = async () => {
    setSaved(null); setRunState(null);
    const ids = new Set(packs.map((a) => `pack-${a}`));
    // columns appear at once, in pack order before the opened files, and fill as the answers come in
    setSources((s) => [...packs.map((a) => ({ id: `pack-${a}`, kind: "pack" as const, address: a, label: packNo(a) })), ...s.filter((x) => !ids.has(x.id))]);
    // the read belongs to the link it began on: an answer after a change is dropped, the rest not read
    const l = linkNow();
    for (const [n, address] of packs.entries()) {
      setReading({ address, n: n + 1, of: packs.length });
      const next = await readPack(address, l);
      if (!next) break;
      setSources((s) => s.map((x) => (x.id === next.id ? next : x)));
    }
    setReading(null);
  };

  /** „Neu lesen“ of one column: that pack alone, its column replaced in place (also one of an earlier read or error). */
  const readOne = async (address: number) => {
    setSaved(null);
    const id = `pack-${address}`, l = linkNow();
    setSources((s) => s.map((x) => (x.id === id ? { id, kind: "pack" as const, address, label: packNo(address) } : x)));
    setReading({ address, n: 1, of: 1 });
    const next = await readPack(address, l);
    if (next) setSources((s) => s.map((x) => (x.id === id ? next : x)));
    setReading(null);
  };

  /** After a restart (a refused write's live switches are gone then) or ein/aus (the lock state, Group0 bit 5): the pack's column, if shown, is read again. */
  const rereadPack = (address: number) => {
    if (!sources.some((x) => x.id === `pack-${address}`)) return;
    const l = linkNow();
    // an open edit of this pack moves onto the state now (bit 5 after ein/aus), the user's changes stay on top of it
    api.parameters(address).then((p) => { if (linkNow() === l) { replace(packCol(p, address, l, path)); seen(address, p); rebaseEdit(address, p); } },
      (e) => {
        // the column (and an open edit of it) shows the lock state before the action: marked unclear until a read
        if (linkNow() === l) setSources((s) => s.map((x) => (x.id === `pack-${address}` ? { ...x, lockUnclear: true } : x)));
        setNote({ address, note: { text: t("ctl.reread", { pack: packNo(address), err: errorText(e, t) }), error: true } });
      });
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

  // „Dieses Pack hat die Patch-Funktionen“ for a CAN socket column, bound to String 2 read now; and taking a proof back
  const withPatch = (c: Source, patch: Parameters["patch"]) =>
    setSources((s) => s.map((x) => (x.id === c.id && x.read ? { ...x, read: { ...x.read, patch } } : x)));
  const confirmPatch = async (c: Source) => {
    const gen = getProfile()?.gen;
    if (!installation || c.address === undefined || !c.read?.bms_serial || gen === undefined) return;
    try { withPatch(c, await api.patchProofConfirm(installation.id, c.address, c.read.bms_serial, gen)); }
    catch (e) { setNote({ address: c.address, note: { text: errorText(e, t), error: true } }); }
  };
  const revokePatch = async (c: Source) => {
    if (!installation || c.address === undefined) return;
    try { await api.patchProofRevoke(installation.id, c.address); withPatch(c, null); }
    catch (e) { setNote({ address: c.address, note: { text: errorText(e, t), error: true } }); }
  };

  const startEdit = (c: Source, from?: Parameters | null) => {
    if (!c.read || c.address === undefined) return;
    const a = c.address;
    const base = rawsOf(c.read), baseSwitches = [...c.read.function_switches];
    // „Vorherigen Stand“: that block, with P65 as now (it changes only when unlocked)
    const raws = from ? rawsOf(from).map((r, i) => (i === SERIES_INDEX ? base[i] : r)) : [...base];
    // the patch bits of Group0 that this path does not change stay as the pack has them now (also from „Vorherigen
    // Stand“), the lock state (bit 5) always
    const keep = keepMask(c.read.patch, c.path === "uart1", baseSwitches[0]);
    const switches = (from ? [...from.function_switches] : [...baseSwitches]).map((v, g) => (g === 0 ? (v & ~keep) | (baseSwitches[0] & keep) : v));
    setEdit({ address: a, base, baseSwitches, raws, switches, text: {}, seriesUnlocked: false });
    editing.current = a;
    setNote(null); setPicking(false); setTargets([]); setRunState(null);
    setSources((s) => s.map((x) => (x.id === c.id ? { ...x, notTaken: undefined } : x)));
    // the learned full capacity (0x42): every successful write sets it to P58 when they are 10 Ah apart
    setFull(null);
    // asked on the link of the column; an answer after a change of connection or installation is dropped
    const l = linkNow();
    const fits = fullReqs.current.begin(a);
    if (c.link !== l) { setFull({ address: a, error: t("pw.full.otherLink") }); return; }
    const ok = () => linkNow() === l && fits(editing.current);
    api.packFullCapacity(a).then((ah) => { if (ok()) setFull({ address: a, ah }); },
      (e) => { if (ok()) setFull({ address: a, error: String(e) }); });
  };

  const takeOverFrom = (id: string) => {
    const src = sources.find((x) => x.id === id);
    if (!src?.values || !src.switches) return;
    // Group0: the lock state (bit 5) and the patch bits this path does not change stay as the pack has them, as at the start
    setEdit((e) => {
      if (!e) return e;
      const own = sources.find((x) => x.id === `pack-${e.address}`);
      const keep = keepMask(own?.read?.patch, own?.path === "uart1", e.baseSwitches[0]);
      const switches = src.switches!.map((v, g) => (g === 0 ? (v & ~keep) | (e.baseSwitches[0] & keep) : v));
      return { ...e, raws: takeOver(e.raws, src.values!, !e.seriesUnlocked), switches, text: {} };
    });
  };

  /** One block to one pack (`base`: what the change was made against), then the pack's column as after every
   *  write: the current state when stale, the pending refusal, the read-back with what was not taken. A column of
   *  another link is left alone. */
  const send = async (a: number, base: Wire, raws: Raws, switches: number[], restore: boolean, l: string): Promise<{ r?: ParamWrite; error?: unknown; out: Outcome }> => {
    let r: ParamWrite;
    // the board the column was read from: a write over the CAN socket must find the same one
    const serial = sources.find((x) => x.id === `pack-${a}`)?.read?.bms_serial ?? null;
    try { r = await api.writeParameters(a, base, toWire(raws, switches), restore, serial); }
    catch (error) { return { error, out: failure(error) }; }
    // the pending refusal is kept per site and pack and holds whatever the link: the switches are live on that pack
    if (r.kind === "refused") {
      // only the first refusal knows the stored block: later reads show the live switches (the CAN socket's E2 makes
      // none live)
      if (r.refusal === "refused" && r.switches_live !== false && !refusedOf(a) && switchDiff(switches, r.stored.function_switches).length) setRefused(a, { stored: r.stored, live: [...switches] });
    } else if (r.kind === "written" && restore) setRefused(a, null);
    if (linkNow() === l) {
      const col = (p: Parameters) => ({ ...packCol(p, a, l, path), notTaken: notTaken(raws, switches, rawsOf(p), p.function_switches) });
      if (r.kind === "stale") { replace(packCol(r.fresh, a, l, path)); seen(a, r.fresh); }
      else if ((r.kind === "written" || r.kind === "unclear") && r.before) {
        // CAN socket: „Vorherigen Stand“ is the block before this write, for the session; only a confirmed write sets it
        const before = r.before, read = r.read_back, written = r.kind === "written";
        setSources((s) => s.map((x) => (x.id === `pack-${a}` ? afterUart1Write(x, read ? keepProof(x, col(read)) : null, written, before, Date.now()) : x)));
      } else if (r.kind === "written") {
        void loadHistory(a);
        if (r.read_back) replace(col(r.read_back));
      }
    }
    return { r, out: outcomeOf(r, raws, switches) };
  };

  /** A stale write: the edit of `a` moves onto the pack's current state, the user's changes stay on top of it. */
  const rebaseEdit = (a: number, p: Parameters) => {
    const fresh = rawsOf(p), fs = p.function_switches;
    setEdit((e) => (e && e.address === a ? {
      ...e, base: fresh, baseSwitches: [...fs],
      raws: e.raws.map((x, i) => (x !== e.base[i] ? x : fresh[i])),
      // the lock state (Group0 bit 5) is never the user's: always the pack's
      switches: e.switches.map((x, g) => { const touched = (x ^ e.baseSwitches[g]) & (g === 0 ? ~LOCK_BIT : 0xff); return (x & touched) | (fs[g] & ~touched & 0xff); }),
    } : e));
  };

  /** Write the edit of pack `a`, or with `restore` the stored block after a refusal (its switches are live but not
   *  stored then); a restore needs no open edit. */
  const write = async (a: number, restore?: Parameters) => {
    const e0 = edit?.address === a ? edit : null;
    if (!restore && (!e0 || refusedOf(a))) return;
    const pack = packNo(a);
    const sent = restore ? rawsOf(restore) : e0!.raws, sentSwitches = restore ? restore.function_switches : e0!.switches;
    const base = restore ? toWire(sent, sentSwitches) : toWire(e0!.base, e0!.baseSwitches);
    busyFor("params")(true); setNote(null); setGone(null);
    const say = (n: Note) => setNote({ address: a, note: n });
    try {
      const { r, error, out } = await send(a, base, sent, sentSwitches, !!restore, linkNow());
      if (!r) {
        say({ text: writeError(error, t, (err) => t("pw.error", { err })), error: true });
        const u = uart1Error(error);
        if (u?.error === "elsewhere" && u.address !== undefined && u.board !== undefined && u.gen !== undefined) setGone({ address: u.address, board: u.board, at: u.at ?? null, gen: u.gen, armed: false });
      }
      else if (r.kind === "stale") {
        // nothing written: the column shows the pack now, the user's changes stay on top of it
        rebaseEdit(a, r.fresh);
        say({ text: t(r.lock_changed ? "pw.lockChanged" : "pw.stale", { pack }), error: true });
      } else if (r.kind === "refused" && r.refusal === "locked") {
        // E3: the pack changed since the read (a protected bit, the write release); its state now, the edit on top of it
        say({ text: t("pw.locked"), error: true });
        const l = linkNow(), before = e0?.baseSwitches[0] ?? sentSwitches[0];
        try {
          const p = await api.parameters(a);
          if (linkNow() === l) {
            replace(packCol(p, a, l, path)); seen(a, p);
            if (!restore) rebaseEdit(a, p);
            say({ text: `${t("pw.lockedRead")}${lockChanged(before, p.function_switches[0]) ? ` ${t("pw.lockChanged", { pack })}` : ""}`, error: true });
          }
        } catch { /* the hint to read again stays */ }
      } else if (r.kind === "refused") {
        say({ text: t(r.switches_live === false ? "pw.refusedNothing" : r.refusal === "refused" ? (restore ? "pw.restoreRefused" : "pw.refusedShort") : "pw.invalid"), error: true });
      } else if (r.kind === "unclear") {
        // CAN socket: acknowledged, but the read-back failed
        const unread = !!r.before && !r.read_back && !uart1Error(r.error);
        say({ text: unread ? t("pw.uart1Unread", { err: r.error }) : uart1Error(r.error) ? errorText(r.error, t) : t("pw.unclear", { err: r.error }), error: true });
      } else {
        if (r.read_back) {
          const n = out.kind === "notTaken" ? out.params.length + out.switches.length : 0;
          say(n ? { text: t("pw.notTakenCount", { count: n }), error: true } : { text: t(restore ? "pw.restored" : "pw.done") + (r.unanswered ? ` ${t("pw.unanswered")}` : ""), error: false });
          // after restoring, the edit stays open on the stored state, so the values can be corrected
          if (restore) setEdit((e) => (e && e.address === a ? { ...e, base: rawsOf(r.read_back!), baseSwitches: [...r.read_back!.function_switches] } : e));
        } else say({ text: t("pw.unread", { err: r.read_error ?? "" }), error: true });
        if (!restore) setEdit(null);
      }
    } finally { busyFor("params")(false); }
  };

  /** Further packs that can take the edit: a read column of this link, not the edited pack. */
  const targetRead = (a: number) => {
    const c = sources.find((x) => x.id === `pack-${a}`);
    return c?.kind === "pack" && c.read && !c.error && c.link === linkNow() ? c.read : undefined;
  };

  /** Before a run over several packs: the change applied to every picked pack and checked there (skipped with the
   *  reason otherwise), then each written pack's learned full capacity, read now (each write may reset it to P58). */
  const prepare = async () => {
    if (!edit) return;
    const e = edit, l = linkNow(), ticket = ++prepTicket.current;
    const c = changeOf(e.base, e.raws, e.baseSwitches, e.switches);
    const plans: TargetPlan[] = [], skipped: Row[] = [];
    for (const a of targets) {
      const read = targetRead(a);
      if (!read) continue;
      const p = planTarget(c, a, rawsOf(read), read.function_switches, e.seriesUnlocked);
      if (refusedOf(a)) skipped.push({ address: a, out: { kind: "skipped", why: "pending" } });
      else if (p.blockers.length) skipped.push({ address: a, out: { kind: "skipped", why: "blocked", blockers: p.blockers } });
      else if (!p.changes.length) skipped.push({ address: a, out: { kind: "skipped", why: "same" } });
      else plans.push(p);
    }
    busyFor("multi")(true);
    setConfirm({ plans, skipped, capacity: null, reading: null });
    const current = () => prepTicket.current === ticket && linkNow() === l;
    const full = new Map<number, number | null>();
    for (const a of [e.address, ...plans.map((p) => p.address)]) {
      if (!current()) break;
      setConfirm((x) => x && { ...x, reading: a });
      try { full.set(a, await api.packFullCapacity(a)); } catch { full.set(a, null); }
    }
    if (!current()) { if (prepTicket.current === ticket) { setConfirm(null); busyFor("multi")(false); } return; }
    setConfirm((x) => x && { ...x, reading: null, capacity: capacityEffects([{ address: e.address, raws: e.raws }, ...plans], full) });
  };
  const cancelPrepare = () => { prepTicket.current++; setConfirm(null); busyFor("multi")(false); };

  /** The run: the edited pack first, then the others by address, one after another, each as a single write of its
   *  own read with the change on top. It stops before the next pack after a link change or „Abbrechen“ and waits at
   *  a refusal that left switches live. */
  const run = async () => {
    if (!edit || !confirm?.capacity) return;
    const e = edit, cf = confirm, l = linkNow();
    setConfirm(null); setNote(null);
    cancelled.current = false;
    const plan = new Map(cf.plans.map((p) => [p.address, p]));
    const rows: Row[] = [{ address: e.address }, ...[...cf.plans.map((p) => ({ address: p.address })), ...cf.skipped].sort((x, y) => x.address - y.address)];
    setRunState({ rows, current: null, paused: null, end: null });
    const report = (a: number, out: Outcome) => setRunState((s) => s && { ...s, rows: s.rows.map((r) => (r.address === a ? { ...r, out } : r)) });
    let source: ParamWrite | undefined;
    const end = await runSeries([e.address, ...cf.plans.map((p) => p.address)], {
      link: linkNow, cancelled: () => cancelled.current, report,
      write: async (a) => {
        setRunState((s) => s && { ...s, current: a });
        if (a === e.address) { const w = await send(a, toWire(e.base, e.baseSwitches), e.raws, e.switches, false, l); source = w.r; return w.out; }
        const p = plan.get(a)!;
        return (await send(a, toWire(p.base, p.baseSwitches), p.raws, p.switches, false, l)).out;
      },
      pause: (a) => new Promise<boolean>((res) => { resume.current = res; setRunState((s) => s && { ...s, current: null, paused: a }); })
        .then((goOn) => { resume.current = null; setRunState((s) => s && { ...s, paused: null }); return goOn; }),
    });
    setRunState((s) => s && { ...s, current: null, paused: null, end });
    busyFor("multi")(false);
    if (linkNow() !== l) return;
    // the edited pack as after a single write: written closes the edit, stale keeps it on the pack's current state
    setPicking(false); setTargets([]);
    const r = source as ParamWrite | undefined;
    if (r?.kind === "written") setEdit(null);
    else if (r?.kind === "stale") rebaseEdit(e.address, r.fresh);
  };

  const nf = (i: number) => new Intl.NumberFormat(i18n.language, { minimumFractionDigits: decimals(i), maximumFractionDigits: decimals(i) });
  const editId = edit ? `pack-${edit.address}` : null;
  // the edited column shows the values as edited, so differences and the datasheet check follow the input
  const cols = [...sources.filter((c) => c.kind === "file"), ...sources.filter((c) => c.kind === "stored"), ...sources.filter((c) => c.kind === "pack")]
    .map((c) => (edit && c.id === editId ? { ...c, values: edit.raws.map((r, i) => fromRaw(i, r)), switches: edit.switches } : c));
  const paramKeys = (i: number) => cols.map((c) => (c.values ? fmtParam(i, c.values[i]) : undefined));
  const switchKeys = (g: number) => cols.map((c) => (c.switches ? String(c.switches[g]) : undefined));
  const diffParams = KEYS.map((_, i) => deviating(paramKeys(i)));
  // a column whose lock state is unclear (re-read after ein/aus failed) counts as a deviation of Group0 bit 5: the filter
  // never hides it or reports „no differences“
  const unclearCols = cols.flatMap((c, i) => (c.lockUnclear ? [i] : []));
  const diffSwitches = Array.from({ length: 8 }, (_, g) => (g === 0 && unclearCols.length ? new Set([...deviating(switchKeys(g)), ...unclearCols]) : deviating(switchKeys(g))));
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
  // the release bits of Group0 (bits 0, 2 and 3): shown once a column comes from the CAN socket's service or has a proven
  // patch; named only when a column's patch is proven (before, they meant nothing). Bit 3 only where a proven pack may
  // have the power and clock service (its image flag set or not known), bits 4 and 5 (extended off, the lock state) only
  // where one may have the stored pack cutoff
  // a stored or file column without a proof does not make the rows neutral: it shows its patch bits raw (rawGate)
  const labelCols = cols.filter((c) => !rawGate(c)).map((c) => ({ patch: c.read?.patch, switches: !!c.switches }));
  const gateLabel = gateRowLabel(labelCols);
  // bits 4 and 5 are named only where every column may have the pack cutoff (a proven 1.5 pack makes them neutral)
  const cutoffLabel = cutoffRowLabel(labelCols);
  const rowLabel = (b: number) => (b >= 4 ? cutoffLabel : gateLabel);
  const gateRows = cols.some((c) => !!c.read?.patch || c.path === "uart1");
  const powerRow = cols.some((c) => c.read?.patch && c.read.patch.caps.power_rtc !== false);
  const cutoffRow = cols.some((c) => c.read?.patch && c.read.patch.caps.pack_cutoff !== false);
  const bitsOf0 = (g: number) => (g === 0 && gateRows ? [0, ...SWITCH_BITS[0], 2, ...(powerRow ? [3] : []), ...(cutoffRow ? [4, 5] : [])] : SWITCH_BITS[g]);
  // an unclear lock state shows bit 5 also without a proof (unnamed there), with its „neu lesen“
  const bitsOf = (g: number) => { const b = bitsOf0(g); return g === 0 && unclearCols.length && !b.includes(5) ? [...b, 5] : b; };
  const bitsOff = (g: number) => bitsOf(g).filter((b) => (g === 0 && b === 5 && unclearCols.length > 0) || deviating(cols.map((c) => (c.switches ? String((c.switches[g] >> b) & 1) : undefined))).size);
  const gateBit = (g: number, b: number) => g === 0 && b !== 1 && b <= 5;
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
  // the edited column's patch: switching its CAN socket write release off explains the way back first
  const editSource = cols.find((c) => c.id === editId);
  const editPatch = editSource?.read?.patch;
  const editWarning = !edit ? null
    : writeLocked(editPatch, editSource?.path === "uart1", edit.baseSwitches[0]) ? t("pw.gate.locked")
    : selfLock(editPatch, edit.baseSwitches[0], edit.switches[0])
      ? `${t("pw.gate.selfLock")}${(edit.address === 0 ? (edit.switches[0] & 4) === 0 : getProfile()?.gates?.start === false) ? ` ${t("pw.gate.selfLockBoth")}` : ""}`
    : null;
  const editable = (c: Source) => paramWritable && c.kind === "pack" && !!c.read && !edit && !reading && !lockedBut("params") && !writing.params;
  const candidates = edit ? packs.filter((a) => a !== edit.address).map((a) => ({ address: a, readable: !!targetRead(a) })) : [];
  const chosenTargets = targets.filter((a) => candidates.some((c) => c.address === a && c.readable));
  // at a pause the paused pack's restore is free; everything else waits for the run
  const restoreLocked = (a: number) => (runState?.paused === a ? lockedBut("params", "multi") : lockedBut("params"));
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
          {cols.some((c) => !c.notRead) && <Button variant="ghost" disabled={!!reading || !!edit || paramsBusy} onClick={() => { setSources(() => withPlaceholders([], packs, readsBus)); setSaved(null); }} title={t("par.clearHint")}>{t("par.clear")}</Button>}
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
        <ClockLine connected={connected} bus={bus} packs={packs} address={chosen} setAddress={setChosen} locked={lockedBut("clock")} onBusy={busyFor("clock")}
          installation={!!installation} power={powerOf} powerLock={lockOf} />
        <CanProtocolLine address={chosen} usable={writable} locked={lockedBut("can")} onBusy={busyFor("can")} />
        <SerialLine address={chosen} usable={writable} locked={lockedBut("serial")} onBusy={busyFor("serial")} linkKey={link} />
        <CalibrationSection address={chosen} usable={writable} locked={lockedBut("cal")} onBusy={busyFor("cal")} linkKey={link} />
        <PackControlSection address={chosen} usable={ctlMode !== null} mode={ctlMode ?? "full"} powerLocked={chosen !== null ? lockOf(chosen) : null}
          cutoff={chosen !== null && packCutoff(chosen, profile, probes[chosen])} group0={chosen !== null ? targetRead(chosen)?.function_switches[0] : undefined}
          locked={lockedBut("ctl")} onBusy={busyFor("ctl")} onChanged={rereadPack}
          p79={p79Of(chosen)} linkKey={link} />
        {chosen !== null && controlHint({ connected, bus, demo: isDemo, installation: !!installation, power: powerOf(chosen) }) && (
          <div className="border-t border-line pt-3 text-sm"><StatusLine>{t("ctl.canUnsupported")}</StatusLine></div>
        )}
        {connected && <AccessMatrix address={chosen} group0={chosen !== null ? targetRead(chosen)?.function_switches[0] : undefined}
          group7={chosen !== null ? targetRead(chosen)?.function_switches[7] : undefined} version={chosen !== null ? versionOf?.(chosen) : undefined}
          storedProof={chosen !== null && ["flash", "user"].includes(targetRead(chosen)?.patch?.source ?? "")} linkKey={link} />}
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
              note={note?.address === edit.address ? note.note : null} refused={refusedOf(edit.address)} busy={writing.params} locked={restoreLocked(edit.address)} held={writing.multi}
              sources={takeOverSources}
              writeLabel={chosenTargets.length ? t("pw.multi.write", { pack: packNo(edit.address), count: chosenTargets.length }) : undefined}
              more={candidates.length > 0 && (
                <Button variant="ghost" size="sm" disabled={paramsBusy} onClick={() => setPicking((p) => !p)} aria-expanded={picking}>{t("pw.multi.more")}</Button>
              )}
              extra={<>
                {picking && <TargetPicker candidates={candidates} chosen={chosenTargets} disabled={paramsBusy} onChange={setTargets} />}
                {confirm && <MultiConfirm changes={editChanges} packs={[edit.address, ...confirm.plans.map((p) => p.address)]} skipped={confirm.skipped}
                  capacity={confirm.capacity} reading={confirm.reading} onConfirm={run} onCancel={cancelPrepare} />}
              </>}
              warning={editWarning}
              onChange={(f) => setEdit((e) => e && f(e))} onTakeOver={takeOverFrom} onWrite={() => (chosenTargets.length ? prepare() : write(edit.address))}
              onRestore={(stored) => write(edit.address, stored)}
              onDiscard={() => { setEdit(null); setNote(null); setPicking(false); setTargets([]); }} />
          )}
          {runState && <SeriesPanel rows={runState.rows} current={runState.current} paused={runState.paused} end={runState.end}
            held={writing.params}
            onCancel={() => { cancelled.current = true; resume.current?.(false); }} onGoOn={() => resume.current?.(true)} onClose={() => setRunState(null)} />}
          {cols.flatMap((c) => {
            const r = c.kind === "pack" && c.address !== undefined && c.address !== edit?.address ? refusedOf(c.address) : null;
            return r ? [<RefusedPanel key={c.id} refused={r} fullAh={full && "ah" in full && full.address === c.address ? full.ah : null} pack={c.label}
              busy={writing.params} locked={restoreLocked(c.address!)} onRestore={(stored) => write(c.address!, stored)} />] : [];
          })}
          {gone && (
            <div className="flex flex-wrap items-center gap-2 text-sm">
              {!gone.armed
                ? <Button size="sm" variant="ghost" onClick={() => setGone({ ...gone, armed: true })}>{t("uart1.markGone", { pack: packNo(gone.address) })}</Button>
                : <>
                    <span className="text-discharge">{t("uart1.markGoneConfirm", { pack: packNo(gone.address) })}</span>
                    <Button size="sm" onClick={() => {
                      const g = gone; setGone(null);
                      api.bankMarkGone(g.address, g.board, g.at, g.gen).then(
                        (done) => setNote({ address: g.address, note: { text: t(done ? "uart1.markedGone" : "uart1.markGoneChanged", { pack: packNo(g.address) }), error: !done } }),
                        (e) => setNote({ address: g.address, note: { text: errorText(e, t), error: true } }));
                    }}>{t("uart1.markGoneYes")}</Button>
                    <Button size="sm" variant="ghost" onClick={() => setGone(null)}>{t("uart1.markGoneNo")}</Button>
                  </>}
            </div>
          )}
          {note && note.address !== edit?.address && (
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
                    <ColumnHead key={c.id} c={c} onRead={c.kind === "pack" && c.address !== undefined && canRead && packs.includes(c.address) ? () => void readOne(c.address!) : undefined} twin={c.kind === "pack" && !!c.read && !c.lockUnclear && c.link === linkNow() && cols.some((x) => x.kind === "stored" && x.address === c.address)} busy={!!reading || paramsBusy} onSave={() => save(c)} onRemove={() => setSources((s) => s.filter((x) => x.id !== c.id))}
                      onConfirmPatch={c.kind === "pack" && c.path === "uart1" && !c.read?.patch && c.read?.bms_serial && installation && c.link === linkNow() ? () => void confirmPatch(c) : undefined}
                      onRevokePatch={c.kind === "pack" && installation && (c.read?.patch?.source === "flash" || c.read?.patch?.source === "user") ? () => void revokePatch(c) : undefined}
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
                            <EditCell key={c.id} e={edit} i={i} level={findings[ci].get(i)} disabled={paramsBusy || (i === SERIES_INDEX && !edit.seriesUnlocked)}
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
                      {(filtered ? bitsOff(g) : bitsOf(g))
                        .map((b) => {
                          const off = deviating(cols.map((c) => (c.switches ? String((c.switches[g] >> b) & 1) : undefined)));
                          return (
                            <tr key={b}>
                              {gateBit(g, b) && rowLabel(b) !== "named" ? <NameCell id="topic.patchGates" label={t(rowLabel(b) === "neutral" ? "par.gateNeutral" : "par.gateUnnamed", { b })} sub={t("par.bit", { b })} />
                                : <NameCell id={`switch.${g}.${b}`} sub={t("par.bit", { b })} />}
                              {refCell && <td className="border-b border-l border-line bg-sunken/40" />}
                              {cols.map((c, ci) => {
                                const gate = gateBit(g, b) ? gateBits(c.read?.patch, c.path === "uart1") : null;
                                const cut = gate && b >= 4 ? cutoffBits(c.read?.patch, c.path === "uart1", c.read?.function_switches[0]) : null;
                                // bit 5 is status only, never editable
                                const fixed = gate && !(b === 0 ? gate.writeEditable : b === 2 ? gate.startEditable : b === 3 ? gate.powerEditable : b === 4 ? cut!.cutoffEditable : false);
                                // bits 4 and 5 of a proven column read inverted: on = bit 0
                                // (not where the build has no cutoff: there the raw bit, without function)
                                const inverted = invertedBit(g, b, !!gate?.named && cut?.cutoff !== false);
                                // the meaning only in proven columns; the shared row stays neutral in a mixed table
                                const meaning = gate?.named && (b !== 3 || gate.power !== false) && (b < 4 || cut!.cutoff !== false) ? helpTitle(`switch.0.${b}`, lang) : undefined;
                                // bit 3 acts only in a build with the power and clock service, bits 4 and 5 with the pack cutoff
                                const power = b !== 3 || !gate?.named ? null : gate.power === false ? t("par.gatePowerAbsent") : gate.power === null ? t("par.gatePowerUnknown") : null;
                                const cutoff = !cut || !gate?.named ? null : cut.cutoff === false ? t("par.gateCutoffAbsent") : cut.cutoff === null ? t("par.gateCutoffUnknown") : null;
                                // on a slave, bit 2 is the master's start release, stored here: it acts only at address 0
                                const slaveStart = b === 2 && gate?.named && c.address !== undefined && c.address !== 0 ? t("par.gateStartMasterOnly") : null;
                                const fixedWhy = !fixed ? null : b === 2 ? t("par.gateStartFixed") : b === 3 && gate.power !== false ? t("par.gatePowerFixed")
                                  : b === 4 && cut!.cutoff !== false ? t("par.gateCutoffFixed") : b === 5 ? t("par.gateLockFixed") : null;
                                const why = !gate ? undefined : rawGate(c) ? t("par.gateRaw") : !gate.named ? t("par.gateUnproven") : [meaning, slaveStart, power, cutoff, inverted ? t("par.gateInverted") : null, fixedWhy].filter(Boolean).join(" · ");
                                // bit 5 after an ein/aus whose re-read failed: the shown state may be old
                                // „neu lesen“ reads this column alone, also during an edit (which then moves onto it)
                                if (b === 5 && c.lockUnclear) return (
                                  <ValueCell key={c.id} c={c} off={false} title={t("par.lockUnclearTip")}>
                                    <span className="text-discharge">{t("par.lockUnclear")}</span>{" "}
                                    {c.address !== undefined && c.link === linkNow() && (
                                      <button className="text-xs underline disabled:opacity-40" disabled={paramsBusy || !!reading} onClick={() => rereadPack(c.address!)}>{t("par.lockReread")}</button>
                                    )}
                                  </ValueCell>
                                );
                                // bits 4 and 5 of an unproven column: raw, also in the edit (never an on/off box)
                                if (c.id === editId && edit && b >= 4 && gate && !gate.named) return <ValueCell key={c.id} c={c} off={false} title={why}><span className="text-muted">{t("par.rawBit", { v: (edit.switches[g] >> b) & 1 })}</span></ValueCell>;
                                return c.id === editId && edit ? (
                                  <EditSwitch key={c.id} e={edit} g={g} b={b} inverted={inverted} disabled={paramsBusy || !!fixed} title={why} onChange={(f) => setEdit((e) => e && f(e))} />
                                ) : (
                                  <ValueCell key={c.id} c={c} off={off.has(ci)} missed={c.notTaken?.switches.has(g)} title={why}>{!c.switches ? null : cut?.cutoff === false || (gate && rawGate(c)) || (b >= 4 && gate && !gate.named)
                                    // a build without the cutoff, a stored or file column without a proof, bits 4/5 unproven: the raw bit, no on/off meaning
                                    ? <span className="text-muted">{t("par.rawBit", { v: (c.switches[g] >> b) & 1 })}</span>
                                    : <OnOff on={switchOn(c.switches[g], b, inverted)} />}</ValueCell>
                                );
                              })}
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

function ColumnHead({ c, twin, busy, onSave, onRemove, onRead, onEdit, onPrevious, onConfirmPatch, onRevokePatch }: {
  c: Source; busy: boolean; onSave: () => void; onRemove: () => void;
  /** set when this pack can be read alone now (connected, on the bus, nothing else running) */
  onRead?: () => void;
  /** a live pack column next to the stored state of the same pack: labelled „aktuell“ */
  twin?: boolean;
  /** set when this pack column can be edited now; „Vorherigen Stand“ only after a write of this pack */
  onEdit?: () => void; onPrevious?: () => void;
  /** a CAN socket column with a board identity and no proof: „Patch-Funktionen bestätigen“; a stored proof: take it back */
  onConfirmPatch?: () => void; onRevokePatch?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const pending = c.kind === "pack" && !c.values && !c.error && !c.notRead;
  return (
    <th className="min-w-24 border-b border-l border-line bg-surface px-3 py-2 text-right align-bottom font-normal">
      <div className="flex items-center justify-end gap-1">
        {c.suspectFrom !== undefined && c.suspectFrom !== null && <TriangleAlert className="h-3.5 w-3.5 text-discharge" aria-label={t("par.suspectShort")} />}
        <span className={cn("truncate font-medium text-ink", c.kind === "file" && "max-w-40")} title={c.label}>
          {c.kind === "pack" ? t(twin ? "par.packNow" : "par.pack", { n: c.label }) : c.kind === "stored" ? t("par.storedCol", { n: c.label }) : c.label}
        </span>
        {c.path === "uart1" && <span className="rounded bg-sunken px-1 text-xs text-muted" title={t("par.uart1ColHint")}>{t("par.uart1Col")}</span>}
        {onRead && (
          <button onClick={onRead} className="rounded p-1 text-muted hover:bg-sunken hover:text-ink" title={t("par.readOne")} aria-label={t("par.readOne")}>
            <RotateCw className="h-3.5 w-3.5" />
          </button>
        )}
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
        {c.error ? (c.kind === "pack" ? (packBusy(c.error) ? t("par.busyShort") : t("par.noAnswer")) : t("par.unreadable")) : pending ? t("par.pending") : c.notRead ? t("par.notRead") : c.kind === "file" ? (c.device || t("par.file")) : c.device}
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
      {c.read?.patch && (
        <div className="flex items-center justify-end gap-1 text-xs whitespace-nowrap text-muted" title={t(`par.patch.source.${c.read.patch.source}`)}>
          {c.read.patch.release}
          {onRevokePatch && <button className="underline" disabled={busy} onClick={onRevokePatch}>{t("par.patch.revoke")}</button>}
        </div>
      )}
      {writeLocked(c.read?.patch, c.path === "uart1", c.switches?.[0]) && (
        <div className="text-xs whitespace-nowrap text-alarm" title={t("par.writeLockedHint")}>{t("par.writeLocked")}</div>
      )}
      {!c.lockUnclear && packLockActive(c.read?.patch, c.read?.function_switches[0]) && (
        <div className="text-xs font-semibold whitespace-nowrap text-alarm" title={t("par.packLockHint")}>{t("par.packLock")}</div>
      )}
      {onConfirmPatch && (
        <button className="text-xs whitespace-nowrap text-muted underline" disabled={busy} title={t("par.patch.confirmHint")} onClick={onConfirmPatch}>{t("par.patch.confirm")}</button>
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

function NameCell({ id, sub, original, label }: { id: string; sub: string; original?: string; label?: string }) {
  const e = useHelpEntry(id);
  return (
    <td className="sticky left-0 z-10 border-b border-line bg-surface px-4 py-1.5 group-hover:bg-sunken" title={original}>
      <div className="flex items-baseline gap-1.5">
        <span className="whitespace-nowrap">{label ?? e?.title ?? id}</span><InfoIcon id={id} className="self-center" />
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
