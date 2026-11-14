import type { TFunction } from "i18next";
import type { AccessDecision, AccessReason, Channel } from "./api";
import CATALOG from "./generated/access.json";
import ACCESS_TEXT from "./i18n.access.json";

export const CHANNELS: Channel[] = ["uart0", "uart1", "uart2"];

/** The matrix as rows: one per action, in the backend's order, with its ways per channel (none where the firmware has
 *  none; the CAN socket may have two, e.g. the binary serial read and the master's Modbus String 1). */
export function accessRows(decisions: AccessDecision[]): { action: string; cells: Partial<Record<Channel, AccessDecision[]>> }[] {
  const rows: { action: string; cells: Partial<Record<Channel, AccessDecision[]>> }[] = [];
  for (const d of decisions) {
    let row = rows.find((r) => r.action === d.route.action);
    if (!row) rows.push((row = { action: d.route.action, cells: {} }));
    (row.cells[d.route.channel] ??= []).push(d);
  }
  return rows;
}

const rank = { ready: 0, unknown: 1, blocked: 2, "not-implemented": 3 } as const;
/** The state a cell shows: its best way. */
export const bestState = (ds: AccessDecision[]) => bestDecision(ds).state;
/** The way a cell shows: its best one (the first of equals), whose firmware need the cell names. */
export const bestDecision = (ds: AccessDecision[]): AccessDecision => ds.reduce((b, d) => (rank[d.state] < rank[b.state] ? d : b), ds[0]);

/** One reason as text (`acc.reason.*`). */
export function reasonText(t: TFunction, r: AccessReason): string {
  switch (r.id) {
    case "firmware": return r.need.kind === "patch-from"
      ? t(`acc.fw.patch.${r.known}`, { v: extName(LINE_FROM[r.need.version.join(".")] ?? r.need.version) })
      : r.need.kind === "read-cap"
      ? t(`acc.fw.${capKind(r.need.mask) === "feature" ? "capFeature" : "cap"}.${r.known}`, { name: capNames(t, r.need.mask) })
      : t(`acc.fw.flag.${r.known}`, { bit: r.need.bit, name: t(`acc.fw.flagName.b${r.need.bit}`, { defaultValue: t("acc.fw.flagName.other") }) });
    case "bit": {
      const name = t(`acc.bitName.g${r.group}b${r.bit}`, { defaultValue: "" });
      return t(r.known === null ? "acc.bitUnread" : "acc.bitWrong", { g: r.group, b: r.bit, want: r.want ? 1 : 0, now: r.known ? 1 : 0, name: name ? ` (${name})` : "" });
    }
    case "with-slaves": return t(r.known === null ? "acc.slavesUnknown" : "acc.slavesNone");
    case "marker": return t("acc.marker", { want: t(`acc.markerState.${r.on ? "on" : "off"}`), now: t(`acc.markerState.${r.known ?? "unread"}`) });
    case "base": return t(r.known ? "acc.baseLow" : "acc.baseUnknown", { from: r.from.map((n) => String(n).padStart(2, "0")).join("."), now: r.known?.map((n) => String(n).padStart(2, "0")).join(".") });
    default: return t(`acc.reason.${r.id}`);
  }
}

/** The tooltip of a cell: per way its command and state, then every reason on its own line. */
export const cellTitle = (t: TFunction, ds: AccessDecision[]) =>
  ds.map((d) => [`${d.route.wire}: ${t(`acc.state.${d.state}`)}`, ...d.reasons.map((r) => `· ${reasonText(t, r)}`)].join("\n")).join("\n\n");

// ---- The catalog of `openbms-proto::access`, evaluated here for what the UI offers (the backend decides again before
// sending). Generated from Rust; `access.test.ts` checks this evaluation against the Rust vectors.


export type Tri = "yes" | "no" | "unknown";
export type Firmware = { kind: "patch-from"; version: [number, number] } | { kind: "image-flag"; bit: number } | { kind: "read-cap"; mask: number };
export type Need =
  | { kind: "bit-set" | "bit-clear"; group: number; bit: number }
  | { kind: "master-only" | "quiet-bus" | "local-board" | "with-slaves" }
  | { kind: "marker"; on: boolean }
  | { kind: "base-from"; base: [number, number, number] };
export interface Route { action: string; channel: Channel; wire: string; firmware: Firmware[]; needs: Need[]; implemented: boolean; yields?: string }
export type Marker = { kind: "off" | "on" } | { kind: "foreign"; word: number };
export type Patch = { kind: "unknown" | "none" } | { kind: "version"; version: [number, number] };
export interface Facts {
  patch: Patch; flags: number | null; caps: number | null; group0: number | null; group7: number | null; address: number; installation: boolean;
  marker: Marker | null; base: [number, number, number] | null; slaves: number | null;
}
export interface Proof { write_gate: boolean; start_gate: boolean; power: Tri; cutoff: Tri }
export interface Group0Plan { keep: number; refuse_change: number; need_set: number }
type State = AccessDecision["state"];

export const ROUTES = CATALOG.routes as Route[];
/** the highest DIP address (`openbms_proto::address::DIP_MAX`): packs 0..=DIP_MAX */
export const DIP_MAX = CATALOG.dip_max;
export const EXT_MIN = CATALOG.ext_min as [number, number];
export const CUTOFF_MIN = CATALOG.cutoff_min as [number, number];
const B = CATALOG.bits;
/** Image flag bits ways depend on today (`access::FLAG_*`). */
export const FLAG_POWER = CATALOG.flags.power;
/** Image flag bit 3: the Group0 bits 3/4 enable at 1 (every EMU1101-EXT): bit 3 at 1 allows power and
 *  clock over the CAN socket, bit 4 at 1 switches the extended off on. */
export const FLAG_POSITIVE = CATALOG.flags.positive;
/** Whether the image flag byte announces the positive bits 3/4 (null: not known). */
export const positiveOf = (flags: number | null | undefined): boolean => flags != null && (flags & (1 << FLAG_POSITIVE)) !== 0;
/** `access::positive_build`: bit 3 at 1 allows power and clock: image flag bit 3 (every EMU1101-EXT); without known
 *  flags (a stored proof) every supported build. TEMP(maint-1.6): the first 1.6 (flags 06) keeps bit 3 at 0. */
export const positiveBuild = (patch: Patch, flags: number | null | undefined): boolean =>
  positiveOf(flags) || (flags == null && patch.kind === "version");
/** `access::inverted`: Group0 bit 4 always, bit 3 in a positive build act at 1 where the routes are written for 0. */
export const inverted = (group: number, bit: number, patch: Patch, flags: number | null | undefined): boolean =>
  group === 0 && (bit === B.g0_extended_off || (bit === B.g0_power_lock && positiveBuild(patch, flags)));
export const FLAG_CUTOFF = CATALOG.flags.cutoff;
/** Group0 bits of the patched firmware as masks. */
export const G0 = { write: 1 << B.g0_write, start: 1 << B.g0_start, powerLock: 1 << B.g0_power_lock, extendedOff: 1 << B.g0_extended_off, lockState: 1 << B.g0_lock_state,
  hardStop: 1 << B.g0_hard_stop, hardStopInherit: 1 << B.g0_hard_stop_inherit };

/** EMU1101-EXT Full (image flag bit 7): the hard stop at a recharge pause is chosen by Group0 bits 6/7. */
export const hardStopConfigurable = (flags: number | null | undefined): boolean => flags != null && (flags & (1 << CATALOG.flags.hard_stop)) !== 0;
/** The hard stop a Group0 chooses: bit 7 at 1 takes the build's default (image flag bit 0), else bit 6 (explicit). */
export function hardStopChoice(group0: number, flags: number): { enabled: boolean; inherited: boolean } {
  const inherited = (group0 & G0.hardStopInherit) !== 0;
  return { enabled: inherited ? (flags & 1) !== 0 : (group0 & G0.hardStop) !== 0, inherited };
}
/** Group0 with an explicit choice: bits 6/7 together, every lower bit as it was. */
export const withHardStop = (group0: number, enabled: boolean): number => (group0 & ~(G0.hardStop | G0.hardStopInherit) & 0xff) | (enabled ? G0.hardStop : 0);
const NEEDS_INSTALLATION = new Set(CATALOG.needs_installation);

type V3 = [number, number, number];
/** The first supported build of a patch line (1.5 → Light 1.5.4, 1.6 → Full 1.6.4) and the first one carrying an image flag, from
 *  the catalog (`maint::SUPPORTED_15/16`, `access::FLAG_SINCE`). */
const LINE_FROM = CATALOG.lines as unknown as Record<string, V3>;
const FLAG_FROM = CATALOG.flag_since as unknown as Record<string, V3>;
const CAP_FROM = CATALOG.cap_since as unknown as Record<string, V3>;
/** Read-capability bits of the EXT05 identity (`maint::CAP_*`). */
export const CAP = CATALOG.caps;
/** The first build carrying any bit of a read-capability mask. */
const capFrom = (mask: number): V3 | undefined => Object.entries(CAP_FROM).find(([bit]) => (mask & (1 << Number(bit))) !== 0)?.[1];

/** The firmware a way needs at least, for the matrix: the first supported EMU1101-EXT build (`patch`), the OEM base (`base`).
 *  Null where the original firmware is enough. */
export interface RouteNeed { patch: V3 | null; base: V3 | null }
export function routeNeed(r: Pick<Route, "firmware" | "needs">): RouteNeed | null {
  let patch: V3 | null = null;
  const at = (v: V3) => { if (!patch || cmp3(v, patch) > 0) patch = v; };
  for (const f of r.firmware) {
    at(f.kind === "patch-from" ? LINE_FROM[f.version.join(".")] ?? [f.version[0], f.version[1], 0]
      : f.kind === "read-cap" ? capFrom(f.mask) ?? LINE_FROM["1.5"]
      : FLAG_FROM[f.bit] ?? LINE_FROM["1.5"]);
  }
  const base = r.needs.reduce<V3 | null>((b, n) => (n.kind === "base-from" && (!b || cmp3(n.base, b) > 0) ? n.base : b), null);
  return patch || base ? { patch, base } : null;
}

/** „Light 1.5.4“, „Full 1.6.4“ (another line: „1.7.0“): an EMU1101-EXT version with its line. */
export function extName(v: readonly number[]): string {
  const line = v[0] === 1 && v[1] === 5 ? "Light " : v[0] === 1 && v[1] === 6 ? "Full " : "";
  return line + v.join(".");
}

/** „ab EMU1101-EXT Light 1.5.4“, „ab OEM 16.06.04“ (texts from `acc.need.*`); "" where nothing is needed. */
export function needText(t: TFunction, n: RouteNeed | null): string {
  if (!n) return "";
  return [
    n.patch && t("acc.need.patch", { v: extName(n.patch) }),
    n.base && t("acc.need.base", { v: n.base.map((x) => String(x).padStart(2, "0")).join(".") }),
  ].filter(Boolean).join(" · ");
}

/** Detection queries (`Action::probe`): safe to send to any pack, but only a patch answers; their `ANY` is no OEM function. */
export const PROBES = new Set<string>(CATALOG.probe_actions);
/** The Group0 bits whose direction depends on the build (`access::BUILD_POLARITY`): texts give them no value. */
const POLARITY = new Set(CATALOG.build_polarity.map(([g, b]) => `${g}.${b}`));
/** A translate function: i18next's `t`, or the plain one the error texts get. */
type Tr = (key: string, options?: Record<string, unknown>) => string;
const maskBits = (mask: number) => Array.from({ length: 16 }, (_, b) => b).filter((b) => (mask & (1 << b)) !== 0);
/** The read capabilities of a mask, any of which is enough („Historie“, „gespeicherter Stand oder Energiezähler“). */
/** How the capability bits of a mask are worded (`acc.fw.capKind`, shared with `tools/access_help.py`): "read" (a read
 *  capability, the default) or "feature" (a bit that gates more than a read, the CCL decrease step). One mask never mixes
 *  them. */
export function capKind(mask: number): "read" | "feature" {
  const kinds = new Set(maskBits(mask).map((b) => (ACCESS_TEXT.de.fw.capKind as Record<string, string>)[`c${b}`] ?? "read"));
  if (kinds.size !== 1) throw new Error(`access: capability mask ${mask} mixes the kinds ${[...kinds].join(", ")}`);
  return [...kinds][0] as "read" | "feature";
}
const capNames = (t: Tr, mask: number) => maskBits(mask).map((b) => t(`acc.fw.capName.c${b}`, { defaultValue: t("acc.fw.capName.other") })).join(t("acc.doc.or"));

/** One condition of a way in the hint form (`tools/access_help.py`, `_need` without detail): names, no bit numbers or
 *  values of the bits whose direction depends on the build; null for a note (the quiet bus), which the hint leaves out. */
function needLine(t: Tr, n: Need): string | null {
  switch (n.kind) {
    case "bit-set": case "bit-clear": {
      const name = t(`acc.bitName.g${n.group}b${n.bit}`);
      if (POLARITY.has(`${n.group}.${n.bit}`)) return t("acc.doc.short.release", { name });
      return t(n.kind === "bit-set" ? "acc.doc.short.on" : "acc.doc.short.off", { name });
    }
    case "master-only": return t("acc.doc.short.masterOnly");
    case "marker": return t("acc.doc.marker", { state: t(`acc.markerState.${n.on ? "on" : "off"}`) });
    case "base-from": return t("acc.doc.short.base", { v: n.base.map((x) => String(x).padStart(2, "0")).join(".") });
    case "with-slaves": return t("acc.doc.withSlaves");
    case "local-board": return t("acc.doc.localBoard");
    case "quiet-bus": return null;
    default: throw new Error(`access: no text for the condition ${JSON.stringify(n)}`);
  }
}

/** The conditions of one way in the hint form, all of them needed: probe, firmware with its features (no version; the
 *  help and the matrix name it), the further conditions, what it returns. */
export function wayConditions(t: Tr, r: Route): string[] {
  const conds: string[] = [];
  const probe = PROBES.has(r.action);
  if (probe) conds.push(t("acc.doc.short.probe"));
  const features = r.firmware.flatMap((f) => {
    switch (f.kind) {
      case "patch-from": return [];
      case "image-flag": return [t(`acc.fw.flagName.b${f.bit}`)];
      case "read-cap": return [capNames(t, f.mask)];
      default: throw new Error(`access: no text for the firmware need ${JSON.stringify(f)}`);
    }
  });
  if (r.firmware.length) conds.push(features.length ? t("acc.doc.short.extWith", { features: features.join(", ") }) : t("acc.doc.short.ext"));
  else if (!probe && !r.needs.some((n) => n.kind === "base-from")) conds.push(t("acc.doc.short.oem"));
  // the firmware base first, with the firmware
  const needs = [...r.needs].sort((a, b) => Number(a.kind !== "base-from") - Number(b.kind !== "base-from"));
  for (const n of needs) { const c = needLine(t, n); if (c) conds.push(c); }
  if (r.yields) conds.push(t(`acc.doc.short.yields.${r.yields}`));
  return conds;
}

/** [(port, conditions)] → „A oder B (c)“: ports with the same conditions together, alternatives joined by „oder“. */
function joinPorts(t: Tr, ways: [string, string][]): string {
  const groups: { conds: string; ports: string[] }[] = [];
  for (const [port, conds] of ways) {
    const g = groups.find((x) => x.conds === conds);
    if (g) g.ports.push(port); else groups.push({ conds, ports: [port] });
  }
  const or = t("acc.doc.or");
  return groups.map((g) => `${g.ports.join(or)} (${g.conds})`).join(or);
}

/** The ways of `actions` the app uses, as one short line (`plain` in `tools/access_help.py`). Several actions over the
 *  same ports share their common conditions; otherwise one sentence per group of actions with the same ways. The action
 *  names only where the actions differ (the UI shows the action already). `routes`: the catalog (another one in tests). */
export function accessText(t: Tr, actions: readonly string[], routes: readonly Route[] = ROUTES): string {
  const per = actions.map((a) => {
    if (!ROUTES.some((r) => r.action === a)) throw new Error(`access: unknown action ${a}`);
    return { a, ways: routes.filter((r) => r.action === a && r.implemented).map((r) => ({ ch: r.channel, conds: wayConditions(t, r) })) };
  });
  const port = (ch: string) => t(`acc.doc.port.${ch}`);
  const act = (a: string) => t(`acc.act.${a}`);
  if (per.length > 1 && per.every((p) => p.ways.length) && new Set(per.map((p) => p.ways.map((w) => w.ch).join(","))).size === 1) {
    const merged: [string, string][] = [];
    for (const [i, w0] of per[0].ways.entries()) {
      const common = w0.conds.filter((c) => per.every((p) => p.ways[i].conds.includes(c)));
      if (!common.length) break;
      let text = common.join(", ");
      for (const p of per) {
        const extra = p.ways[i].conds.filter((c) => !common.includes(c));
        if (extra.length) text += `; ${act(p.a)}: ${extra.join(", ")}`;
      }
      merged.push([port(w0.ch), text]);
    }
    if (merged.length === per[0].ways.length) return `${joinPorts(t, merged)}.`;
  }
  const groups: { names: string[]; text: string }[] = [];
  for (const p of per) {
    const text = p.ways.length ? joinPorts(t, p.ways.map((w) => [port(w.ch), w.conds.join(", ")])) : t("acc.doc.notInApp");
    const g = groups.find((x) => x.text === text);
    if (g) g.names.push(p.a); else groups.push({ names: [p.a], text });
  }
  if (groups.length === 1) return `${groups[0].text}.`;
  return groups.map((g) => `${g.names.map(act).join(", ")}: ${g.text}.`).join(" ");
}

/** Hints that restate catalog facts: their text takes `{{access}}`, the line `accessText` gives for these actions. */
export const ACCESS_HINTS = {
  "clock.needConnection": ["set-clock"],
  "clock.canBus": ["set-clock"],
  "bmsh.needConnection": ["read-history"],
  "bmsh.canBus": ["read-history"],
  "par.needConnection": ["read-parameters"],
  "par.canBus": ["read-parameters", "write-parameters"],
  "ctl.canUnsupported": ["restart", "power-on-off"],
  "ctl.canHint": ["restart", "power-on-off"],
  "rdiag.hint": ["read-calibration", "read-soc-store"],
  "rdiag.noCap": ["read-calibration", "read-soc-store"],
  "mdiag.hint": ["read-diagnosis"],
  "hs.hint": ["hard-stop"],
  "ccl.hint": ["read-ccl-step", "write-ccl-step"],
  "maint.setup.controlHint": ["maint-start"],
  "fw.patch.update": ["flash"],
  "fw.soc.hint": ["read-soc-store"],
  "mk.noUart0": ["maint-status"],
  "mk.unknownPack": ["maint-status"],
} as const satisfies Record<string, readonly string[]>;
export type AccessHint = keyof typeof ACCESS_HINTS;
/** A hint of `ACCESS_HINTS` with its access line filled in; `vars` are the hint's own variables (a version …). */
export const accessHint = (t: Tr, key: AccessHint, routes: readonly Route[] = ROUTES, vars: Record<string, unknown> = {}): string =>
  t(key, { ...vars, access: accessText(t, ACCESS_HINTS[key], routes) });

/** The first supported build of each patch line („1.5.4“, „1.6.4“), for texts naming the supported range. */
export const supportedLines = () => ({ light: LINE_FROM["1.5"].join("."), full: LINE_FROM["1.6"].join(".") });

/** The catalog route of a decision (same action, channel and command). */
export const routeOf = (d: AccessDecision["route"]) => ROUTES.find((r) => r.action === d.action && r.channel === d.channel && r.wire === d.wire);

const cmp3 = (a: number[], b: number[]) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
const cmp2 = (a: number[], b: number[]) => a[0] - b[0] || a[1] - b[1];

/** `Facts::firmware`: a pack without a patch identity has none; a version and the image flags as reported. */
export function firmwareKnown(need: Firmware, f: Facts): Tri {
  if (f.patch.kind === "none") return "no";
  if (need.kind === "patch-from") return f.patch.kind === "version" ? (cmp2(f.patch.version, need.version) >= 0 ? "yes" : "no") : "unknown";
  if (need.kind === "read-cap") return f.caps === null ? "unknown" : (f.caps & need.mask) !== 0 ? "yes" : "no";
  return f.flags === null ? "unknown" : (f.flags & (1 << need.bit)) !== 0 ? "yes" : "no";
}

/** `access::decide`: blocked wins over unknown; a way the app does not use is not-implemented with its conditions named. */
export function decide(route: Route, f: Facts): { state: State; reasons: AccessReason[] } {
  const reasons: AccessReason[] = [];
  let state: State = "ready";
  const worse = (s: State) => { if (s === "blocked" || (s === "unknown" && state === "ready")) state = s; };
  for (const need of route.firmware) {
    const known = firmwareKnown(need, f);
    if (known !== "yes") { worse(known === "no" ? "blocked" : "unknown"); reasons.push({ id: "firmware", need, known }); }
  }
  for (const n of route.needs) {
    switch (n.kind) {
      case "bit-set": case "bit-clear": {
        // the positive build reads bits 3/4 the other way round
        const want = (n.kind === "bit-set") !== inverted(n.group, n.bit, f.patch, f.flags);
        const g = n.group === 0 ? f.group0 : n.group === 7 ? f.group7 : null;
        const known = g === null ? null : (g & (1 << n.bit)) !== 0;
        if (known !== want) { worse(known === null ? "unknown" : "blocked"); reasons.push({ id: "bit", group: n.group, bit: n.bit, want, known }); }
        break;
      }
      case "master-only":
        if (f.address !== 0) { worse("blocked"); reasons.push({ id: "master-only" }); }
        break;
      case "quiet-bus": reasons.push({ id: "quiet-bus" }); break;
      case "local-board": reasons.push({ id: "local-board" }); break;
      case "marker": {
        const known = f.marker?.kind ?? null;
        if (known !== (n.on ? "on" : "off")) { worse(known === null ? "unknown" : "blocked"); reasons.push({ id: "marker", on: n.on, known }); }
        break;
      }
      case "with-slaves":
        if (f.slaves === null || f.slaves === 0) { worse(f.slaves === null ? "unknown" : "blocked"); reasons.push({ id: "with-slaves", known: f.slaves }); }
        break;
      case "base-from":
        if (f.base === null || cmp3(f.base, n.base) < 0) { worse(f.base === null ? "unknown" : "blocked"); reasons.push({ id: "base", from: n.base, known: f.base }); }
        break;
    }
  }
  if (route.implemented && NEEDS_INSTALLATION.has(route.action) && !f.installation) { worse("blocked"); reasons.push({ id: "view-only" }); }
  if (!route.implemented) { state = "not-implemented"; reasons.push({ id: "not-implemented" }); }
  return { state, reasons };
}

/** The ways of `action` over `channel` the app implements. */
export const implementedRoutes = (action: string, channel: Channel) => ROUTES.filter((r) => r.action === action && r.channel === channel && r.implemented);

/** `access::ready`: the first way of `action` over `channel` is ready for `facts`. */
export function ready(action: string, channel: Channel, facts: Facts): boolean {
  const r = ROUTES.find((x) => x.action === action && x.channel === channel);
  return !!r && decide(r, facts).state === "ready";
}

/** `access::group0_plan`: what a parameter write over `channel` does with Group0 for a pack with `proof`. */
export function group0Plan(channel: Channel, proof: Proof | null): Group0Plan {
  const { write: W, start: S, powerLock: P, extendedOff: C, lockState: L } = G0;
  const on = (b: boolean, m: number) => (b ? m : 0);
  if (channel === "uart1") {
    if (!proof) return { keep: L | W | S | P | C, refuse_change: 0, need_set: 0 };
    return { keep: L | P | on(proof.cutoff !== "yes", C) | on(!proof.write_gate, W) | on(!proof.start_gate, S), refuse_change: on(proof.start_gate, S), need_set: on(proof.write_gate, W) };
  }
  if (!proof) return { keep: L, refuse_change: 0, need_set: 0 };
  return { keep: L | on(!proof.write_gate, W) | on(!proof.start_gate, S) | on(proof.power === "no", P) | on(proof.cutoff === "no", C), refuse_change: 0, need_set: 0 };
}

/** The facts of a pack with everything not given unknown. */
export const factsOf = (f: Partial<Facts>): Facts => ({
  patch: { kind: "unknown" }, flags: null, caps: null, group0: null, group7: null, address: 0, installation: false, marker: null, base: null, slaves: null, ...f,
});

/** What a button offers: the app's way of `action` over `channel` is ready, or open only for a switch not read yet (the
 *  backend reads it fresh before sending). A capability or a condition known to fail offers nothing. */
export function offerable(action: string, channel: Channel, facts: Facts): boolean {
  return implementedRoutes(action, channel).some((r) => {
    const d = decide(r, facts);
    return d.state === "ready" || (d.state === "unknown" && d.reasons.every((x) => (x.id === "bit" ? x.known === null : x.id === "quiet-bus" || x.id === "local-board")));
  });
}

/** The tri-state of a capability: true, false, or not known. */
export const tri = (v: boolean | null | undefined): Tri => (v === true ? "yes" : v === false ? "no" : "unknown");
