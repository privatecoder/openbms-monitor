import type { TFunction } from "i18next";
import type { AccessDecision, AccessReason, Channel } from "./api";
import CATALOG from "./generated/access.json";

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
export const bestState = (ds: AccessDecision[]) => ds.reduce((b, d) => (rank[d.state] < rank[b] ? d.state : b), ds[0].state);

/** One reason as text (`acc.reason.*`). */
export function reasonText(t: TFunction, r: AccessReason): string {
  switch (r.id) {
    case "firmware": return r.need.kind === "patch-from"
      ? t(`acc.fw.patch.${r.known}`, { v: r.need.version.join(".") })
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
export type Firmware = { kind: "patch-from"; version: [number, number] } | { kind: "image-flag"; bit: number };
export type Need =
  | { kind: "bit-set" | "bit-clear"; group: number; bit: number }
  | { kind: "master-only" | "quiet-bus" | "local-board" | "with-slaves" }
  | { kind: "marker"; on: boolean }
  | { kind: "base-from"; base: [number, number, number] };
export interface Route { action: string; channel: Channel; wire: string; firmware: Firmware[]; needs: Need[]; implemented: boolean }
export type Marker = { kind: "off" | "on" } | { kind: "foreign"; word: number };
export type Patch = { kind: "unknown" | "none" } | { kind: "version"; version: [number, number] };
export interface Facts {
  patch: Patch; flags: number | null; group0: number | null; group7: number | null; address: number; installation: boolean;
  marker: Marker | null; base: [number, number, number] | null; slaves: number | null;
}
export interface Proof { write_gate: boolean; start_gate: boolean; power: Tri; cutoff: Tri }
export interface Group0Plan { keep: number; refuse_change: number; need_set: number }
type State = AccessDecision["state"];

export const ROUTES = CATALOG.routes as Route[];
/** the highest DIP address (`openbms_proto::address::DIP_MAX`): packs 0..=DIP_MAX */
export const DIP_MAX = CATALOG.dip_max;
export const MAINT_MIN = CATALOG.maint_min as [number, number];
export const CUTOFF_MIN = CATALOG.cutoff_min as [number, number];
const B = CATALOG.bits;
/** Image flag bits ways depend on today (`access::FLAG_*`). */
export const FLAG_POWER = CATALOG.flags.power;
/** Image flag bit 3: Group0 bits 3/4 positive (MAINT 1.6.1 and the positive 1.6 build): bit 3 at 1 allows power and
 *  clock over the CAN socket, bit 4 at 1 switches the extended off on. */
export const FLAG_POSITIVE = CATALOG.flags.positive;
/** Whether the image flag byte announces the positive bits 3/4 (null: not known). */
export const positiveOf = (flags: number | null | undefined): boolean => flags != null && (flags & (1 << FLAG_POSITIVE)) !== 0;
/** `access::positive_build`: bit 3 at 1 allows power and clock: image flag bit 3 (MAINT 1.5.3, 1.6.3); without known
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

/** MAINT 1.6.3 (image flag bit 7): the hard stop at a recharge pause is chosen by Group0 bits 6/7. */
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
/** The first supported build of a patch line (1.5 → 1.5.3, 1.6 → 1.6.3) and the first one carrying an image flag, from
 *  the catalog (`maint::SUPPORTED_15/16`, `access::FLAG_SINCE`). */
const LINE_FROM = CATALOG.lines as unknown as Record<string, V3>;
const FLAG_FROM = CATALOG.flag_since as unknown as Record<string, V3>;

/** The firmware a way needs at least, for the matrix: the first supported MAINT build (`patch`), the OEM base (`base`).
 *  Null where the original firmware is enough. */
export interface RouteNeed { patch: V3 | null; base: V3 | null }
export function routeNeed(r: Pick<Route, "firmware" | "needs">): RouteNeed | null {
  let patch: V3 | null = null;
  const at = (v: V3) => { if (!patch || cmp3(v, patch) > 0) patch = v; };
  for (const f of r.firmware) at(f.kind === "patch-from" ? LINE_FROM[f.version.join(".")] ?? [f.version[0], f.version[1], 0] : FLAG_FROM[f.bit] ?? LINE_FROM["1.5"]);
  const base = r.needs.reduce<V3 | null>((b, n) => (n.kind === "base-from" && (!b || cmp3(n.base, b) > 0) ? n.base : b), null);
  return patch || base ? { patch, base } : null;
}

/** „ab MAINT 1.5.3“, „ab OEM 16.06.04“ (texts from `acc.need.*`); "" where nothing is needed. */
export function needText(t: TFunction, n: RouteNeed | null): string {
  if (!n) return "";
  return [
    n.patch && t("acc.need.patch", { v: n.patch.join(".") }),
    n.base && t("acc.need.base", { v: n.base.map((x) => String(x).padStart(2, "0")).join(".") }),
  ].filter(Boolean).join(" · ");
}

/** The catalog route of a decision (same action, channel and command). */
export const routeOf = (d: AccessDecision["route"]) => ROUTES.find((r) => r.action === d.action && r.channel === d.channel && r.wire === d.wire);

const cmp3 = (a: number[], b: number[]) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
const cmp2 = (a: number[], b: number[]) => a[0] - b[0] || a[1] - b[1];

/** `Facts::firmware`: a pack without a patch identity has none; a version and the image flags as reported. */
export function firmwareKnown(need: Firmware, f: Facts): Tri {
  if (f.patch.kind === "none") return "no";
  if (need.kind === "patch-from") return f.patch.kind === "version" ? (cmp2(f.patch.version, need.version) >= 0 ? "yes" : "no") : "unknown";
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
  patch: { kind: "unknown" }, flags: null, group0: null, group7: null, address: 0, installation: false, marker: null, base: null, slaves: null, ...f,
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
