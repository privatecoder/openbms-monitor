/** Writing parameters (0xA1): raw values, the firmware's plausibility rules, the change list and the checks before
 *  the write button. Pure helpers for the Parameters view. */

import { KEYS, type Parameters, type Source } from "./params";

/** Raw values P0..P86 as numbers; currents (P50..P56) signed, everything else as stored. */
export type Raws = number[];
/** What a write sends and what the backend compares: raws as stored (u16, currents two's complement) and 8 switch bytes. */
export interface Wire { raws: number[]; switches: number[] }

/** Raw unit per index, as in openbms-proto/src/params.rs: value = (raw − offset) / scale. */
const unit = (i: number): { scale: number; offset: number } =>
  i <= 9 || i === 62 || i === 63 ? { scale: 1000, offset: 0 } // mV
  : i <= 19 || i === 60 || i === 61 ? { scale: 100, offset: 0 } // 10 mV
  : i <= 49 ? { scale: 10, offset: 2731 } // 0.1 K
  : i <= 56 || i === 58 || i === 59 ? { scale: 100, offset: 0 } // 10 mA (signed), 10 mAh
  : i === 82 || i === 84 || i === 86 ? { scale: 10, offset: 0 } // 0.1 mΩ
  : { scale: 1, offset: 0 };

export const isSigned = (i: number) => i >= 50 && i <= 56;

/** Raw range per index: u16 for P0..P59 (signed for currents), u8 from P60 on. */
export const rawRange = (i: number): [number, number] => (isSigned(i) ? [-32768, 32767] : i < 60 ? [0, 0xffff] : [0, 0xff]);

/** Display value → raw, rounded to the raw resolution. */
export const toRaw = (i: number, v: number) => Math.round(v * unit(i).scale + unit(i).offset);
/** Raw → display value. */
export const fromRaw = (i: number, raw: number) => (raw - unit(i).offset) / unit(i).scale;

/** Signed raws of a read (the backend sends currents as u16). */
export const rawsOf = (p: Parameters): Raws => p.parameters.map((x, i) => (isSigned(i) && x.raw >= 0x8000 ? x.raw - 0x10000 : x.raw));

/** Raws as the backend wants them: currents back to two's complement u16. */
export const toWire = (raws: Raws, switches: number[]): Wire => ({ raws: raws.map((r) => (r < 0 ? r + 0x10000 : r)), switches: [...switches] });

/** Indices whose raw value does not fit the stored width. */
export const outOfRange = (raws: Raws) => raws.flatMap((r, i) => {
  const [lo, hi] = rawRange(i);
  return Number.isInteger(r) && r >= lo && r <= hi ? [] : [i];
});

/** One rule of the firmware's check: `a ≤ b`, where each side is a parameter index or a raw constant. */
export type Rule = { lo: number; hi: number } | { lo: number; max: number } | { min: number; hi: number };

/** One broken rule, named by the parameter it is about: `index` must be ≤ (or ≥) the other parameter or the bound. */
export type Violation = { index: number; op: "le" | "ge"; other: number } | { index: number; op: "le" | "ge"; bound: number };

/** a ≥ b ≥ c …: each one at most the one before. */
const desc = (...ix: number[]): Rule[] => ix.slice(1).map((lo, k) => ({ lo, hi: ix[k] }));
const between = (min: number, i: number, max: number): Rule[] => [{ min, hi: i }, { lo: i, max }];

/** The plausibility check of 0xA1 (firmware 16.06.13, function 0x4C3C), on raw values with signed currents. Equality
 *  is allowed. A block that breaks one of them is refused with RTN 06, and its switches are live but not stored. */
export const RULES: Rule[] = [
  // cell voltage
  ...desc(0, 1, 3, 2), { min: 2000, hi: 2 }, { lo: 0, max: 4300 },
  ...desc(4, 5, 7, 6), { min: 2000, hi: 6 }, { lo: 4, max: 4300 },
  { min: 3330, hi: 8 }, { lo: 8, hi: 4 },
  { min: 1000, hi: 9 }, { lo: 9, hi: 6 },
  // pack voltage
  ...desc(10, 11, 13, 12), { min: 1600, hi: 12 }, { lo: 10, max: 6000 },
  ...desc(14, 15, 17, 16), { min: 1600, hi: 16 }, { lo: 14, max: 6000 },
  { lo: 14, hi: 19 }, { lo: 19, hi: 18 }, { lo: 18, max: 6500 },
  // temperatures (0.1 K): alarm/protection high a, its recovery b, low c, its recovery d: a ≥ b ≥ d ≥ c
  ...[20, 24, 28, 32, 38, 42].flatMap((a) => [...desc(a, a + 1, a + 3, a + 2), { min: 2431, hi: a + 2 }, { lo: a, max: 3531 }]),
  { lo: 26, hi: 36 }, { lo: 36, hi: 37 }, { lo: 37, max: 2931 },
  { min: 3231, hi: 47 }, { lo: 47, hi: 46 }, { lo: 46, hi: 48 }, { lo: 48, max: 3931 }, { min: 3231, hi: 49 }, { lo: 49, hi: 48 },
  // currents (signed 0.01 A)
  { lo: 55, hi: 52 }, { lo: 52, hi: 53 }, { lo: 53, hi: 51 }, { lo: 51, hi: 50 }, { lo: 50, hi: 54 }, { lo: 54, max: 30000 },
  { min: -30000, hi: 56 }, { lo: 56, hi: 55 },
  ...between(500, 57, 5000),
  { lo: 59, hi: 58 }, { lo: 58, max: 65000 },
  { min: 10, hi: 61 }, { lo: 61, hi: 60 }, { lo: 60, max: 100 },
  { min: 10, hi: 63 }, { lo: 63, hi: 62 }, { lo: 62, max: 100 },
  // single-byte ranges
  ...([[64, 1, 18], [65, 7, 16], [66, 3, 180], [67, 3, 180], [68, 5, 250], [69, 10, 250], [70, 1, 30], [71, 1, 30], [72, 1, 30],
    [73, 1, 24], [74, 1, 100], [75, 1, 250], [76, 30, 250], [77, 1, 250]] as const).flatMap(([i, a, b]) => between(a, i, b)),
  { min: 1, hi: 79 }, { lo: 79, hi: 78 }, { lo: 78, max: 30 },
  ...([[80, 70, 99], [81, 10, 150], [82, 0, 100], [83, 1, 9], [84, 0, 50], [85, 10, 16], [86, 0, 50]] as const).flatMap(([i, a, b]) => between(a, i, b)),
];

/** Every broken rule, in rule order. A parameter compared with another is named by the lower side („P1 must be ≤ P0“). */
export function violations(raws: Raws): Violation[] {
  const out: Violation[] = [];
  for (const r of RULES) {
    if ("lo" in r && "hi" in r) { if (raws[r.lo] > raws[r.hi]) out.push({ index: r.lo, op: "le", other: r.hi }); }
    else if ("max" in r) { if (raws[r.lo] > r.max) out.push({ index: r.lo, op: "le", bound: r.max }); }
    else if (raws[r.hi] < r.min) out.push({ index: r.hi, op: "ge", bound: r.min });
  }
  return out;
}

/** P65: a wrong count makes every cell protection wrong, so it changes only when unlocked. */
export const SERIES_INDEX = 65;
/** P58 rated capacity (10 mAh). */
export const RATED_INDEX = 58;

/** After every successful 0xA1 the BMS sets the learned full capacity to P58 when they are 10.00 Ah or more apart. */
export const capacityResets = (fullAh: number | null | undefined, ratedRaw: number) =>
  fullAh !== null && fullAh !== undefined && Math.abs(Math.round(fullAh * 100) - ratedRaw) >= 1000;

/** The P58 raw that keeps the learned full capacity. */
export const ratedFor = (fullAh: number) => Math.round(fullAh * 100);

export type Change = { kind: "param"; index: number; from: number; to: number } | { kind: "switch"; group: number; bit: number; on: boolean };

/** What an edit changes against its base: parameters in index order, then switch bits. */
export function changes(base: Raws, raws: Raws, baseSwitches: number[], switches: number[]): Change[] {
  const out: Change[] = [];
  KEYS.forEach((_, i) => { if (raws[i] !== base[i]) out.push({ kind: "param", index: i, from: base[i], to: raws[i] }); });
  for (let g = 0; g < 8; g++)
    for (let b = 0; b < 8; b++)
      if (((switches[g] ^ baseSwitches[g]) >> b) & 1) out.push({ kind: "switch", group: g, bit: b, on: ((switches[g] >> b) & 1) === 1 });
  return out;
}

/** Switch bits that differ between two sets of switch bytes (e.g. what a refused block made live against the stored). */
export const switchDiff = (live: number[], stored: number[]) =>
  changes([], [], stored, live).filter((c): c is Extract<Change, { kind: "switch" }> => c.kind === "switch");

/** After a write: parameters and switch bytes the read-back does not show as sent („nicht übernommen“). */
export function notTaken(sent: Raws, sentSwitches: number[], read: Raws, readSwitches: number[]): { params: Set<number>; switches: Set<number> } {
  return {
    params: new Set(sent.flatMap((r, i) => (read[i] !== r ? [i] : []))),
    switches: new Set(sentSwitches.flatMap((s, g) => (readSwitches[g] !== s ? [g] : []))),
  };
}

/** Takes over another source's values (display values, e.g. an opened export) as raws; P65 stays unless unlocked. */
export function takeOver(own: Raws, values: number[], keepSeries: boolean): Raws {
  return values.map((v, i) => (keepSeries && i === SERIES_INDEX ? own[i] : toRaw(i, v)));
}

/** The write button's blockers, in the order the bar names them. */
export type Blocker = { kind: "input"; index: number } | { kind: "range"; index: number } | { kind: "series" } | { kind: "rule"; v: Violation };

/** Checks only against concrete harm: unreadable or unrepresentable input, a changed P65 that is not unlocked, and
 *  the firmware's own rules (a block the app lets through should not be refused, which would leave the switches live). */
export function blockers(base: Raws, raws: Raws, badInput: number[], seriesUnlocked: boolean): Blocker[] {
  return [
    ...badInput.map((index): Blocker => ({ kind: "input", index })),
    ...outOfRange(raws).filter((i) => !badInput.includes(i)).map((index): Blocker => ({ kind: "range", index })),
    ...(raws[SERIES_INDEX] !== base[SERIES_INDEX] && !seriesUnlocked ? [{ kind: "series" } as Blocker] : []),
    ...violations(raws).map((v): Blocker => ({ kind: "rule", v })),
  ];
}

/** Whether a read shows exactly a given block's values and switches (e.g. the stored block again after a restart). */
export const sameBlock = (a: Parameters, b: Parameters) =>
  a.parameters.length === b.parameters.length && a.parameters.every((x, i) => x.raw === b.parameters[i].raw)
  && a.function_switches.every((s, g) => s === b.function_switches[g]);

/** What an edit changes, to write onto other packs: the parameters set to new raws, and per switch byte the bits
 *  touched (`mask`) with their new state (`bits`). Everything else stays each pack's own. */
export interface ParamChange { params: Map<number, number>; mask: number[]; bits: number[] }

/** The change of an edit against the read it started from (the source pack's base). */
export function changeOf(base: Raws, raws: Raws, baseSwitches: number[], switches: number[]): ParamChange {
  const params = new Map<number, number>();
  raws.forEach((r, i) => { if (r !== base[i]) params.set(i, r); });
  const mask = switches.map((s, g) => (s ^ baseSwitches[g]) & 0xff);
  return { params, mask, bits: switches.map((s, g) => s & mask[g]) };
}

/** A pack's own read with exactly the change on top: byte 0, P65 and every untouched value or bit stay its own. */
export function applyChange(c: ParamChange, raws: Raws, switches: number[]): { raws: Raws; switches: number[] } {
  return {
    raws: raws.map((r, i) => c.params.get(i) ?? r),
    switches: switches.map((s, g) => (s & ~c.mask[g] & 0xff) | c.bits[g]),
  };
}

/** One further pack of a multi-pack write: its read (`base`), the change applied to it, and what the single write
 *  would say about that (changes left after applying, blockers). No changes left: nothing to write there. */
export interface TargetPlan { address: number; base: Raws; baseSwitches: number[]; raws: Raws; switches: number[]; changes: Change[]; blockers: Blocker[] }

export function planTarget(c: ParamChange, address: number, base: Raws, baseSwitches: number[], seriesUnlocked: boolean): TargetPlan {
  const { raws, switches } = applyChange(c, base, baseSwitches);
  return { address, base, baseSwitches, raws, switches, changes: changes(base, raws, baseSwitches, switches), blockers: blockers(base, raws, [], seriesUnlocked) };
}

/** Which packs of a run get their learned full capacity reset to P58, and for which that is unknown. `full`: the
 *  learned full capacity per address, null when it could not be read (a missing address counts as unknown, too). */
export function capacityEffects(packs: { address: number; raws: Raws }[], full: Map<number, number | null>): { resets: number[]; unknown: number[] } {
  const resets: number[] = [], unknown: number[] = [];
  for (const p of packs) {
    const f = full.get(p.address);
    if (f === null || f === undefined) unknown.push(p.address);
    else if (capacityResets(f, p.raws[RATED_INDEX])) resets.push(p.address);
  }
  return { resets, unknown };
}

/** The editor's full-capacity reads (0x42, `startEdit`): each opening of the editor is a new request with its ticket.
 *  An answer counts only for the newest request and while the editor still edits the pack it was asked for, so a late
 *  answer of an earlier editor never replaces the current one's. */
export function fullRequests() {
  let seq = 0;
  return {
    begin(address: number) {
      const ticket = ++seq;
      return (editing: number | undefined) => ticket === seq && editing === address;
    },
  };
}

/** A pack column re-read after a write (read-back, stale or refused state): the block the backend returns carries no
 *  patch proof and no board identity, those belong to the pack's read on this link and a write does not change them.
 *  They stay from the column as it was unless the new read brings its own. */
export function keepProof(prev: Source | undefined, next: Source): Source {
  if (!prev?.read || !next.read || prev.link !== next.link || prev.address !== next.address) return next;
  const read = { ...next.read };
  if (read.patch === undefined && prev.read.patch !== undefined) read.patch = prev.read.patch;
  if (read.bms_serial === undefined && prev.read.bms_serial !== undefined) read.bms_serial = prev.read.bms_serial;
  return { ...next, read };
}

/** A pack column after a write over the CAN socket (the answer carries the block before it): a read-back becomes the
 *  column's read (`readCol`); „zuletzt geschrieben“ and the block it replaced change only for a confirmed write. An
 *  unclear attempt keeps the earlier confirmed ones. */
export function afterUart1Write(col: Source, readCol: Source | null, written: boolean, before: Parameters, now: number): Source {
  const base = readCol ?? col;
  return written ? { ...base, previous: before, lastWritten: now } : { ...base, previous: col.previous, lastWritten: col.lastWritten };
}
