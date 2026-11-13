/** BMS parameters (0x47): order, grouping, the parameter file (TOML) import/export and the comparison between sources. */
import type { PackPatch } from "./api";
import CATALOG from "./generated/access.json";
import PARAM_FILE from "./generated/param_file.json";
import { parse as parseToml } from "smol-toml";

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
/** What the app's parameter files say about each value, generated from `tools/param_help.py` with the help pages:
 *  English title, short text and longer explanation per parameter, explanation per switch group, name per switch bit. */
type FileText = { params: Record<string, { name: string; short: string; detail: string }>; groups: Record<string, string>;
  switches: Record<string, { name: string; short: string }> };
const TEXT = PARAM_FILE as FileText;
/** The readable name of each parameter in a file (comments only; identity is the key). */
export const FILE_NAMES: string[] = KEYS.map((k) => TEXT.params[k].name);

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

/** The stored state of a place as a column (compare, datasheet check and file save; never edited). */
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

/** The kind and version a parameter file names in its first data lines. */
export const FILE_FORMAT = "openbms-parameters";
export const FILE_VERSION = 1;

/** Parse a parameter file of this app (`toExport`, TOML). Values go by their technical key, wherever they stand in the
 *  file: exactly the 87 parameters of `KEYS` and the eight groups `group0`…`group7`, nothing missing, nothing unknown.
 *  Parameters must be finite numbers, groups integers 0…255 (TOML's own hex `0xDF`). Comments are not data. Anything
 *  else, a BatteryMonitor export or a file of an earlier app version among it, is refused ("foreign", "structure",
 *  "value"): the user saves a fresh file with this app. */
export function parseExport(text: string, label: string): Source {
  const id = `file-${label}-${text.length}`;
  const fail = (error: string): Source => ({ id, kind: "file", label, error });
  let doc: Record<string, unknown>;
  try { doc = parseToml(text.replace(/^\uFEFF/, "")) as Record<string, unknown>; }
  catch { return fail("foreign"); }
  if (doc.format !== FILE_FORMAT || doc.version !== FILE_VERSION) return fail("foreign");
  const params = doc.parameters, groups = doc.function_switches;
  if (!params || typeof params !== "object" || !groups || typeof groups !== "object") return fail("structure");
  const p = params as Record<string, unknown>, g = groups as Record<string, unknown>;
  const groupKeys = Array.from({ length: 8 }, (_, n) => `group${n}`);
  if (Object.keys(p).length !== KEYS.length || KEYS.some((k) => !(k in p))
    || Object.keys(g).length !== 8 || groupKeys.some((k) => !(k in g))) return fail("structure");
  const values = KEYS.map((k) => p[k]);
  const switches = groupKeys.map((k) => g[k]);
  if (values.some((v) => typeof v !== "number" || !Number.isFinite(v))
    || switches.some((v) => typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 255)) return fail("value");
  const bad = KEYS.map((_, i) => i).filter((i) => !plausible(i, values[i] as number));
  return {
    id, kind: "file", label, values: values as number[], switches: switches as number[], suspectFrom: bad.length ? bad[0] : null,
    device: typeof doc.device === "string" ? doc.device.trim() : "",
  };
}

/** A TOML basic string. */
const tomlString = (v: string) => JSON.stringify(v);

/** Text as `#` comment lines of at most `width` characters. */
function comment(text: string, indent = "", width = 110): string[] {
  const out: string[] = [];
  for (const para of text.split("\n").map((x) => x.trim()).filter(Boolean)) {
    let line = "";
    for (const word of para.split(/\s+/)) {
      if (line && indent.length + 2 + line.length + 1 + word.length > width) { out.push(`${indent}# ${line}`); line = word; }
      else line = line ? `${line} ${word}` : word;
    }
    if (line) out.push(`${indent}# ${line}`);
  }
  return out;
}

const HEADER = [
  "OpenBMS Monitor parameter file: the parameters (0x47) and function switches of one pack.",
  "Values are read by their key; the order and every comment are ignored when the file is opened again. Units are fixed per parameter (shown in the comments). Numbers use a decimal point.",
  "Warning = reports and may change limits (e.g. the charge limit), but does not open a MOSFET by itself. Protection = the MOSFET opens.",
  "Function switches: a bit at 1 is on. Group 0 differs between firmwares: the original firmware uses only bit 1; the patched firmware EMU1101-EXT also uses bits 0 and 2-7 (releases, pack lock, hard stop). Taking a file over sets only bit 1 of group 0; the other bits stay as the target pack has them, so a file fits original firmware and EMU1101-EXT alike.",
];

/** The app's parameter file (TOML, UTF-8, LF): head (format, version, device; the proven firmware as a comment), then
 *  every parameter by its technical key with name, number, unit and explanation as comments, then the eight switch bytes
 *  with the meaning of each bit. `parseExport` reads it back. */
export function toExport(p: Parameters): string {
  const l: string[] = [...comment(HEADER.join("\n")), "",
    `format = ${tomlString(FILE_FORMAT)}`, `version = ${FILE_VERSION}`, `device = ${tomlString(p.device_name.trim())}`,
    ""];
  if (p.patch) l.splice(l.length - 1, 0, `# firmware: ${p.patch.release}`);
  l.push("[parameters]");
  p.parameters.forEach((x, i) => {
    const t = TEXT.params[KEYS[i]];
    l.push("", `# P${i} · ${t.name}${UNITS[i] ? ` · ${UNITS[i]}` : ""}`, ...comment(t.short), ...comment(t.detail), `${KEYS[i]} = ${fmtParam(i, x.value)}`);
  });
  l.push("", "[function_switches]");
  p.function_switches.forEach((b, g) => {
    l.push("", `# Group ${g}`, ...comment(TEXT.groups[String(g)] ?? ""));
    for (let bit = 0; bit < 8; bit++) {
      const sw = TEXT.switches[`${g}.${bit}`];
      l.push(`#   bit ${bit} = ${(b >> bit) & 1}: ${sw ? sw.name : "no function"}`);
    }
    l.push(`group${g} = 0x${b.toString(16).toUpperCase().padStart(2, "0")}`);
  });
  return l.join("\n") + "\n";
}

export const exportName = (address: number, now = new Date()) => {
  const z = (n: number) => String(n).padStart(2, "0");
  return `Parameter_pack${z(address)}_${now.getFullYear()}-${z(now.getMonth() + 1)}-${z(now.getDate())}_${z(now.getHours())}-${z(now.getMinutes())}.toml`;
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
