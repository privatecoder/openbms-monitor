/** BMS parameters (0x47): order, grouping, BatteryMonitor XML import/export and the comparison between sources. */
import type { PackPatch } from "./api";
import CATALOG from "./generated/access.json";
import PARAM_NAMES from "./generated/param_names.json";

export interface Parameter { index: number; key: string; raw: number; value: number; unit: string }
/** `bms_serial`: over the CAN socket the String 2 read with the block, when it identifies the board; a
 *  write there must find the same. */
export interface Parameters { address: number; parameters: Parameter[]; function_switches: number[]; device_name: string; bms_serial?: string | null;
  /** what proves the pack's patch version (the release bits of Group0 are named only then) */
  patch?: PackPatch | null }

/** Parameter keys in BMS order (P0..P86) and their display units, from the generated catalog
 *  (`openbms_proto::params::DEFINITIONS`). */
export const KEYS: readonly string[] = CATALOG.params.map((p) => p.key);

/** Display units, as the backend sends them. */
export const UNITS: string[] = CATALOG.params.map((p) => p.unit);

/** The names BatteryMonitor shows (machine translation, typos included), only as a hint in the parameter table for users
 *  who know them from there; never written to or compared in a file. */
export const BM_NAMES = [
  "Monomer high voltage alarm", "Monomer high pressure recovery", "Monomer low pressure alarm", "Monomer low pressure recovery",
  "Monomer overvoltage protection", "Monomer overvoltage recovery", "Monomer undervoltage protection", "Monomer undervoltage recovery",
  "Equalization opening voltage", "Battery low voltage forbidden charging",
  "Total pressure high pressure alarm", "Total pressure and high pressure recovery", "Total pressure low pressure alarm", "Total pressure and low pressure recovery",
  "Total_voltage overvoltage protection", "Total pressure overpressure recovery", "Total_voltage undervoltage protection", "Total pressure undervoltage recovery",
  "Harging overvoltage protection", "Charging overvoltage recovery",
  "Charging high temperature warning", "Charging high temperature recovery", "Charging low temperature warning", "Charging low temperature recovery",
  "Charging over temperature protection", "Charging over temperature recovery", "Charging under-temperature protection", "Charging under temperature recovery",
  "Discharge high temperature warning", "Discharge high temperature recovery", "Discharge low temperature warning", "Discharge low temperature recovery",
  "Discharge over temperature protection", "Discharge over temperature recovery", "Discharge under-temperature protection", "Discharge under temperature recovery",
  "Cell low temperature heating", "Cell heating recovery",
  "Ambient high temperature alarm", "Ambient high temperature recovery", "Ambient low temperature alarm", "Ambient low temperature recovery",
  "Environmental over-temperature protection", "Environmental overtemperature recovery", "Environmental under-temperature protection", "Environmental undertemperature recovery",
  "Power high temperature alarm", "Power high temperature recovery", "Power over temperature protection", "Power over temperature recovery",
  "Charging overcurrent warning", "Charging overcurrent recovery", "Discharge overcurrent warning", "Discharge overcurrent recovery",
  "Charge overcurrent protection", "Discharge overcurrent protection", "Transient overcurrent protection", "Output soft start delay",
  "Battery rated capacity", "SOC",
  "Cell invalidation differential pressure", "Cell invalidation recovery", "Equalization opening pressure difference", "Equalization closing pressure difference",
  "Static equilibrium time", "Battery number in series", "Charge overcurrent delay", "Discharge overcurrent delay",
  "Transient overcurrent delay", "Overcurrent delay recovery", "Overcurrent recovery times", "Charge current limit delay",
  "Charge activation delay", "Charging activation interval", "Charge activation times", "Work record interval",
  "Standby recording interval", "Standby shutdown delay", "Remaining capacity alarm", "Remaining capacity protection",
  "Interval charge capacity", "Cycle cumulative capacity", "Connection fault impedance", "Compensation point 1 position",
  "Compensation point 1 impedance", "Ompensation point 2 position", "Compensation point 2 impedance",
];
/** The names in the app's parameter files: the English help titles (generated from `tools/param_help.py`), so a file
 *  says what a parameter means. Identity in the file is the technical key, not the name. */
export const FILE_NAMES: string[] = KEYS.map((k) => (PARAM_NAMES as Record<string, string>)[k]);

/** Decimals as BatteryMonitor shows them; also the precision two sources are compared at. */
export const decimals = (i: number) =>
  i <= 9 || i === 62 || i === 63 ? 3 : i <= 19 || (i >= 50 && i <= 56) || i === 58 || i === 59 || i === 60 || i === 61 ? 2
  : i <= 49 || i === 82 || i === 84 || i === 86 ? 1 : 0;

export const fmtParam = (i: number, v: number) => v.toFixed(decimals(i));

export const GROUPS: { id: string; indices: number[] }[] = [
  { id: "cellVoltage", indices: [0, 1, 2, 3, 4, 5, 6, 7, 9] },
  { id: "packVoltage", indices: [10, 11, 12, 13, 14, 15, 16, 17, 18, 19] },
  { id: "tempCharge", indices: [20, 21, 22, 23, 24, 25, 26, 27] },
  { id: "tempDischarge", indices: [28, 29, 30, 31, 32, 33, 34, 35] },
  { id: "tempBoard", indices: [38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49] },
  { id: "heating", indices: [36, 37] },
  { id: "current", indices: [50, 51, 52, 53, 54, 66, 55, 67, 56, 68, 69, 70, 71, 57] },
  { id: "capacity", indices: [58, 59, 78, 79, 80, 81] },
  { id: "balancing", indices: [8, 62, 63, 64, 60, 61] },
  { id: "activation", indices: [72, 73, 74, 75, 76, 77] },
  { id: "pack", indices: [65, 83, 84, 85, 86, 82] },
];

/** One column of the comparison: a pack read over the bus, an opened export, or the stored state of a place (the
 *  newest block the app kept for the installation). */
export interface Source {
  id: string;
  kind: "pack" | "file" | "stored";
  /** pack columns: the address the parameters were read from */
  address?: number;
  label: string;
  /** device name from the BMS or the export */
  device?: string;
  values?: number[];
  switches?: number[];
  error?: string;
  /** first parameter index from which an export does not fit the known order */
  suspectFrom?: number | null;
  /** the parameters as read, for saving and as the base of an edit */
  read?: Parameters;
  /** pack columns: the link (`linkNow()`) the read belongs to; only columns of the current one are write targets */
  link?: string;
  /** pack columns read over the CAN socket's parameter service (patched firmware): nothing of it is stored */
  path?: "uart1";
  /** last write of this pack at this installation (ms) and the parameters it replaced */
  lastWritten?: number | null;
  previous?: Parameters | null;
  /** after a write: values and switch bytes the read-back does not show as sent */
  notTaken?: { params: Set<number>; switches: Set<number> };
  /** pack columns: shown for a pack on the bus before anything was read from it (empty, „nicht gelesen“) */
  notRead?: boolean;
  /** pack columns: an ein/aus changed the pack and its re-read failed; bit 5 (the lock state) is shown unclear until a read */
  lockUnclear?: boolean;
  /** stored columns: when and how the block was kept, and whether it is of the place's current board */
  stored?: { at: number; kind: "read" | "written" | "refused"; board: "current" | "previous" | "unclear"; serial: string | null; unconfirmed?: boolean };
}

/** The stored state of a place as a column (compare, datasheet check and XML save; never edited). */
/** A stored block as a column; a read keeps the patch it was taken under (`patch`), so its release bits read as then. */
export const fromStored = (s: { address: number; at: number; kind: "read" | "written" | "refused"; board: "current" | "previous" | "unclear"; bms_serial: string | null; parameters: Parameters | null; switches_unconfirmed?: boolean; patch?: PackPatch | null }): Source => ({
  id: `stored-${s.address}`, kind: "stored", address: s.address, label: String(s.address).padStart(2, "0"),
  device: s.parameters?.device_name, values: s.parameters?.parameters.map((x) => x.value), switches: s.parameters?.function_switches,
  read: s.parameters ? { ...s.parameters, patch: s.patch ?? undefined } : undefined, error: s.parameters ? undefined : "unreadable",
  stored: { at: s.at, kind: s.kind, board: s.board, serial: s.bms_serial, unconfirmed: !!s.switches_unconfirmed },
});

/** A pack column; `address` is the address asked, not byte 0 of the answer, so a write goes where the column says. */
export const fromPack = (p: Parameters, address = p.address): Source => ({
  id: `pack-${address}`, kind: "pack", address, label: String(address).padStart(2, "0"), device: p.device_name,
  values: p.parameters.map((x) => x.value), switches: p.function_switches, read: p,
});

/** Values outside these ranges mean the export's order is shifted (e.g. a cell voltage of 56.8 V). */
const plausible = (i: number, v: number) =>
  i <= 9 || i === 62 || i === 63 ? v >= 0 && v <= 5 : i <= 19 ? v >= 5 && v <= 120 : i <= 49 ? v >= -50 && v <= 150
  : i <= 56 ? Math.abs(v) <= 1000 : i === 65 ? v >= 1 && v <= 32 : i >= 78 && i <= 81 ? v >= 0 && v <= 100 : true;

const unescape = (s: string) => s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, "&");

/** Parse a parameter file of this app (`toExport`). Every parameter carries its technical key; the keys must be all 87 in
 *  BMS order, so no value can land at another place. The names are only for reading and are not compared (a better
 *  title later does not break older files). BatteryMonitor's own exports (no keys, names that differ by version and
 *  language, decimal commas, partly wrong) are refused as "foreign": the user saves a fresh file with this app. */
export function parseExport(text: string, label: string): Source {
  const id = `file-${label}-${text.length}`;
  if (!/<paraGroup>/.test(text)) return { id, kind: "file", label, error: "structure" };
  const params = [...text.matchAll(/<parameter>\s*<Name>[\s\S]*?<\/Name>\s*<Key>([\s\S]*?)<\/Key>\s*<Value>([\s\S]*?)<\/Value>/g)];
  const groups = [...text.matchAll(/<param_bit_group>\s*<Name>[\s\S]*?<\/Name>\s*<Value>([\s\S]*?)<\/Value>/g)];
  const anyParams = (text.match(/<parameter>/g) ?? []).length;
  // parameters without keys (BatteryMonitor, files of earlier app versions): foreign, not merely incomplete
  if (anyParams > 0 && params.length === 0) return { id, kind: "file", label, error: "foreign" };
  if (params.length !== KEYS.length || anyParams !== KEYS.length || groups.length !== 8)
    return { id, kind: "file", label, error: `${params.length}/${KEYS.length}, ${groups.length}/8` };
  const values = params.map((m) => Number(m[2].trim()));
  const switches = groups.map((m) => parseInt(m[1].trim(), 16));
  if (KEYS.some((k, i) => params[i][1].trim() !== k) || values.some((v, i) => Number.isNaN(v) || params[i][2].trim() === ""))
    return { id, kind: "file", label, error: "foreign" };
  if (switches.some((s) => Number.isNaN(s) || s < 0 || s > 255))
    return { id, kind: "file", label, error: "value" };
  const bad = KEYS.map((_, i) => i).filter((i) => !plausible(i, values[i]));
  return {
    id, kind: "file", label, values, switches, suspectFrom: bad.length ? bad[0] : null,
    device: unescape(text.match(/<modualName>([\s\S]*?)<\/modualName>/)?.[1] ?? "").trim(),
  };
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** The app's parameter file: BatteryMonitor's XML layout (UTF-8 with BOM, CRLF), but with the app's own names, the
 *  technical key of every parameter and the app's units, so the file says what each value means (`parseExport`). */
export function toExport(p: Parameters): string {
  const l = ['<?xml version="1.0" encoding="utf-8"?>', "<paraGroup>", "  <filetype>Parameter</filetype>",
    `  <modualName>${esc(p.device_name.padEnd(10, " "))}</modualName>`];
  p.parameters.forEach((x, i) => l.push("  <parameter>", `    <Name>${esc(FILE_NAMES[i])}</Name>`, `    <Key>${KEYS[i]}</Key>`, `    <Value>${fmtParam(i, x.value)}</Value>`, `    <Unit>${esc(UNITS[i])}</Unit>`, "  </parameter>"));
  p.function_switches.forEach((b, g) => l.push("  <param_bit_group>", `    <Name>Function switches ${g}</Name>`, `    <Value>${b.toString(16).toUpperCase().padStart(2, "0")}</Value>`, "  </param_bit_group>"));
  l.push("</paraGroup>");
  return "﻿" + l.join("\r\n");
}

/** File name for a saved pack: Parameter_pack03_2026-10-10_18-05.xml (local time). */
export const exportName = (address: number, now = new Date()) => {
  const z = (n: number) => String(n).padStart(2, "0");
  return `Parameter_pack${z(address)}_${now.getFullYear()}-${z(now.getMonth() + 1)}-${z(now.getDate())}_${z(now.getHours())}-${z(now.getMinutes())}.xml`;
};

/**
 * Columns whose value differs from the others. With a clear majority only the outliers are marked;
 * without one (e.g. two sources that disagree) every column with a value is.
 */
export function deviating(values: (string | undefined)[]): Set<number> {
  const count = new Map<string, number>();
  for (const v of values) if (v !== undefined) count.set(v, (count.get(v) ?? 0) + 1);
  if (count.size <= 1) return new Set();
  const sorted = [...count.entries()].sort((a, b) => b[1] - a[1]);
  const clear = sorted[0][1] > sorted[1][1] && sorted[0][1] > 1;
  return new Set(values.flatMap((v, i) => (v !== undefined && (!clear || v !== sorted[0][0]) ? [i] : [])));
}

/** Bits that have a function (help entry switch.<group>.<bit>); the rest are shown greyed out. */
export const SWITCH_BITS: Record<number, number[]> = {
  0: [1], 1: [0, 1, 2, 3, 4, 5, 6, 7], 2: [0, 1, 2, 3, 4, 5, 6, 7], 3: [0, 1, 2, 3, 4, 5, 6, 7],
  4: [0, 1, 2, 3, 4, 5, 6, 7], 5: [0, 1, 2, 3, 4, 6, 7], 6: [0, 1, 2, 3, 4, 5, 6, 7], 7: [0, 1, 3, 4, 5, 6, 7],
};

/** A parameter history answer (last write, the block it replaced) asked for pack `address` on `link`: only that pack's
 *  column of the same link takes it. A late answer after a change of connection or installation, whose column was read
 *  again on the new link meanwhile, changes nothing. */
export function withHistory(sources: Source[], address: number, link: string, h: { last_written: number | null; previous: Parameters | null }): Source[] {
  return sources.map((x) => (x.id === `pack-${address}` && x.link === link ? { ...x, lastWritten: h.last_written, previous: h.previous } : x));
}

/** The empty columns („nicht gelesen“) of the packs on the bus: one per pack without a column, in pack order before the
 *  other columns; without a readable bus none. Returns `s` itself when nothing changes. */
export function withPlaceholders(s: Source[], packs: number[], readsBus: boolean): Source[] {
  const rest = s.filter((x) => !x.notRead);
  if (!readsBus) return rest.length === s.length ? s : rest;
  const id = (a: number) => `pack-${a}`;
  const next = [
    ...packs.map((a) => rest.find((x) => x.id === id(a)) ?? s.find((x) => x.id === id(a)) ?? { id: id(a), kind: "pack" as const, address: a, label: String(a).padStart(2, "0"), notRead: true }),
    ...rest.filter((x) => !packs.some((a) => x.id === id(a))),
  ];
  return next.length === s.length && next.every((x, i) => x === s[i]) ? s : next;
}

/** A device name the block can hold: 1–10 printable ASCII characters without leading or trailing space. */
export const nameInputOk = (s: string) => /^[\x21-\x7e]([\x20-\x7e]{0,8}[\x21-\x7e])?$/.test(s);
