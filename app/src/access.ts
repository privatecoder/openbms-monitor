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
    case "firmware": return t(`acc.fw.${r.need}.${r.known}`);
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
export type Firmware = "any" | "maint" | "power-flag" | "cutoff";
export type Need =
  | { kind: "bit-set" | "bit-clear"; group: number; bit: number }
  | { kind: "master-only" | "quiet-bus" | "local-board" | "with-slaves" }
  | { kind: "marker"; on: boolean }
  | { kind: "base-from"; base: [number, number, number] };
export interface Route { action: string; channel: Channel; wire: string; firmware: Firmware[]; needs: Need[]; implemented: boolean }
export type Marker = { kind: "off" | "on" } | { kind: "foreign"; word: number };
export interface Facts {
  maint: Tri; power: Tri; cutoff: Tri; group0: number | null; group7: number | null; address: number; installation: boolean;
  marker: Marker | null; base: [number, number, number] | null; slaves: number | null;
}
export interface Proof { write_gate: boolean; start_gate: boolean; power: Tri; cutoff: Tri }
export interface Group0Plan { keep: number; refuse_change: number; need_set: number }
type State = AccessDecision["state"];

export const ROUTES = CATALOG.routes as Route[];
export const MAINT_MIN = CATALOG.maint_min as [number, number];
export const CUTOFF_MIN = CATALOG.cutoff_min as [number, number];
const B = CATALOG.bits;
/** Group0 bits of the patched firmware as masks. */
export const G0 = { write: 1 << B.g0_write, start: 1 << B.g0_start, powerLock: 1 << B.g0_power_lock, extendedOff: 1 << B.g0_extended_off, lockState: 1 << B.g0_lock_state };
const NEEDS_INSTALLATION = new Set(CATALOG.needs_installation);

const cmp3 = (a: number[], b: number[]) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];

/** `access::decide`: blocked wins over unknown; a way the app does not use is not-implemented with its conditions named. */
export function decide(route: Route, f: Facts): { state: State; reasons: AccessReason[] } {
  const reasons: AccessReason[] = [];
  let state: State = "ready";
  const worse = (s: State) => { if (s === "blocked" || (s === "unknown" && state === "ready")) state = s; };
  for (const need of route.firmware) {
    const known: Tri = need === "any" ? "yes" : need === "maint" ? f.maint : need === "power-flag" ? f.power : f.cutoff;
    if (known !== "yes") { worse(known === "no" ? "blocked" : "unknown"); reasons.push({ id: "firmware", need, known }); }
  }
  for (const n of route.needs) {
    switch (n.kind) {
      case "bit-set": case "bit-clear": {
        const want = n.kind === "bit-set";
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
  maint: "unknown", power: "unknown", cutoff: "unknown", group0: null, group7: null, address: 0, installation: false, marker: null, base: null, slaves: null, ...f,
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
