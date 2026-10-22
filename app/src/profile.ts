import { useSyncExternalStore } from "react";
import type { HardStopSeen, MasterView, PackPatch, ProfileView } from "./api";
import { G0, group0Plan, positiveBuild, tri, type Proof } from "./access";

type T = (key: string, params?: Record<string, unknown>) => string;

/** The firmware profile of the monitor connection, as `connect` (or „Erneut prüfen“) read it on the connected endpoint;
 *  null while not connected. Every view configures itself from it. */
let current: ProfileView | null = null;
const listeners = new Set<() => void>();

export function setProfile(p: ProfileView | null) {
  current = p;
  for (const l of listeners) l();
}

export const getProfile = () => current;

export function useProfile(): ProfileView | null {
  return useSyncExternalStore((l) => { listeners.add(l); return () => { listeners.delete(l); }; }, getProfile, getProfile);
}

/** The master offers the patch functions (by its capabilities, never by a version number). */
export const patched = (m: MasterView | null | undefined): boolean => m?.kind === "patched" && !!m.caps;

/** The name of a recognised patch: the listed build, else „MAINT x.y (Basis a.b.c, Build nicht gelistet)“. */
export function patchName(m: MasterView, t: T): string {
  if (m.listed) return m.listed;
  return t("profile.unlisted", { patch: m.patch ?? "?", base: m.base ?? "?" });
}

/** The profile line of the sidebar: tone and text by the master's class. RS485-1/2 knows no 0x64: nothing to say. */
export function profileLine(p: ProfileView | null, t: T): { tone: "ok" | "warn" | "muted"; text: string; title?: string } | null {
  if (!p || p.bus !== "can") return null;
  const m = p.master;
  switch (m.kind) {
    // the version alone: the maintenance state has its own window
    // short: the version as 64/03 reports it; whether the build is listed, in the tooltip
    case "patched": return { tone: "ok", text: m.listed ?? m.version ?? patchName(m, t), title: m.listed ? undefined : `${patchName(m, t)}${m.build_id ? ` · Build ${m.build_id}` : ""}` };
    case "unknown-patch": return { tone: "warn", text: t("profile.unknownPatch", { version: m.version ?? "" }) };
    case "no-0x64": return { tone: "muted", text: t("profile.no0x64") };
    default: return { tone: "warn", text: t("profile.unknown") };
  }
}

/** „Hard-Stop: …“ of one pack: the image's own flag wins (a format-3 master, `flag` true/false); else a stored sighting
 *  in the pack's behaviour (`seen`: waiting for recharge with the charge MOSFET off, no other visible block) shows „an
 *  (beobachtet am …)“; else unknown. Never from a version number or a flash, and never „not seen“. */
export function hardStopLine(flag: boolean | null | undefined, seen: HardStopSeen | undefined, t: T, lang = "de"): string {
  const at = (ms: number) => new Date(ms).toLocaleDateString(lang);
  const state = flag === true ? t("profile.hardStopOn") : flag === false ? t("profile.hardStopOff")
    : seen ? t("profile.hardStopSeen", { at: at(seen.at) })
    : t("profile.hardStopUnknown");
  return t("profile.hardStop", { state });
}

/** The firmware a pack header names: the version its own 64/03 proves on this connection (`proven`, e.g. „16.06.13
 *  MAINT 1.5“) instead of the short one of 0x51 (`reported`, „16.06“) when that fits it; both when they differ. */
export function firmwareText(reported: string | null | undefined, proven: string | null | undefined): string | null {
  if (!proven) return reported || null;
  if (!reported || proven === reported || proven.startsWith(`${reported}.`) || proven.startsWith(`${reported} `)) return proven;
  return `${reported} (${proven})`;
}

/** „Erneut prüfen“ is offered for every CAN socket profile: also a recognised patch may have changed its release bits. */
export const recheckable = (p: ProfileView | null): boolean => p?.bus === "can";

/** The patch as the catalog needs it for a parameter write (`access::Proof`). */
export const proofOf = (patch: PackPatch): Proof =>
  ({ write_gate: patch.caps.write_gate, start_gate: patch.caps.start_gate, power: tri(patch.caps.power_rtc), cutoff: tri(patch.caps.pack_cutoff) });

/** The Group0 plan of a column (`access::group0_plan`): over RS485-1/2 with the column's proof the bits without function
 *  in that build stay; over the CAN socket what the patch lets that way change. */
const planOf = (patch: PackPatch | null | undefined, uart1: boolean) => group0Plan(uart1 ? "uart1" : "uart0", patch ? proofOf(patch) : null);
const fixedOf = (patch: PackPatch | null | undefined, uart1: boolean) => { const p = planOf(patch, uart1); return p.keep | p.refuse_change; };

/** How the release bits of Group0 appear in a parameter column: named and editable where the pack's patch is proven and
 *  the bus allows it, else unnamed and fixed. `uart1`: the column was read over the CAN socket. `power`: the pack's
 *  image flag for power and clock over the CAN socket (bit 3), null when not known. */
export function gateBits(patch: PackPatch | null | undefined, uart1: boolean): { named: boolean; writeEditable: boolean; startEditable: boolean; powerEditable: boolean; power: boolean | null } {
  if (!patch) return { named: false, writeEditable: false, startEditable: false, powerEditable: false, power: null };
  const fixed = fixedOf(patch, uart1);
  return { named: true, writeEditable: !(fixed & G0.write), startEditable: !(fixed & G0.start), powerEditable: !(fixed & G0.powerLock), power: patch.caps.power_rtc ?? null };
}

/** How the shared table rows of Group0 bits 0 and 2 are labelled: by their function only when every column with switches
 *  has a proven patch ("named"); in a mixed table neutrally as „Bit 0.0/0.2“ ("neutral", the meaning shows per proven
 *  column); with no proven column as bits whose meaning needs a proof ("unnamed"). */
export function gateRowLabel(cols: { patch?: PackPatch | null; switches: boolean }[]): "named" | "neutral" | "unnamed" {
  const withSwitches = cols.filter((c) => c.switches);
  const proven = withSwitches.filter((c) => !!c.patch).length;
  return proven === 0 ? "unnamed" : proven === withSwitches.length ? "named" : "neutral";
}

/** A write that switches the CAN socket write release of a proven pack from on to off: the dialog explains the way back. */
export function selfLock(patch: PackPatch | null | undefined, before: number | undefined, after: number | undefined): boolean {
  return !!patch?.caps.write_gate && before !== undefined && after !== undefined && (before & 1) === 1 && (after & 1) === 0;
}

/** Over the CAN socket, a proven pack whose write release is off: no write goes out (the backend refuses it too). */
export function writeLocked(patch: PackPatch | null | undefined, uart1: boolean, group0: number | undefined): boolean {
  const need = planOf(patch, uart1).need_set;
  return uart1 && need !== 0 && group0 !== undefined && (group0 & need) !== need;
}

/** MAINT 1.6, Group0 bit 4: 0 switches the extended off with the stored pack cutoff on. */
export const CUTOFF_BIT = G0.extendedOff;
/** Group0 bit 3 at 1 allows power and clock in this proven build (`access::positive_build`: image flag bit 3 or a
 *  build from 1.6); not proven: as 1.5 (bit 3 at 0 allows). */
export const positivePatch = (patch: PackPatch | null | undefined): boolean =>
  !!patch && positiveBuild(patch.caps.version ? { kind: "version", version: patch.caps.version } : { kind: "unknown" }, patch.caps.flags);
/** Group0 as the helpers here read it (written for bits 3/4 acting at 0): bit 4 flipped always (the extended off acts at
 *  1, as in MAINT 1.6.1), bit 3 flipped in a positive build (`positive`). Never sent: the backend compares raw values. */
export const inverseG0 = (g0: number | undefined, positive: boolean): number | undefined =>
  g0 === undefined ? g0 : g0 ^ G0.extendedOff ^ (positive ? G0.powerLock : 0);
/** MAINT 1.6, Group0 bit 5: 0 shows a stored individual pack cutoff. Status only, never written. */
export const LOCK_BIT = G0.lockState;

/** Group0 bits 4 and 5 of a parameter column: `cutoff` the pack's proven pack cutoff (image flag bit 2 with 1.6; null not
 *  known, false none); bit 4 editable over RS485-1/2 unless the build has none, over the CAN socket only with the cutoff
 *  proven on this connection and writing released there (bit 0 of `group0`). Bit 5 is never editable. */
export function cutoffBits(patch: PackPatch | null | undefined, uart1: boolean, group0: number | undefined): { cutoff: boolean | null; cutoffEditable: boolean } {
  if (!patch) return { cutoff: null, cutoffEditable: false };
  return { cutoff: patch.caps.pack_cutoff ?? null, cutoffEditable: !(fixedOf(patch, uart1) & G0.extendedOff) && !(uart1 && !writeReleased(group0)) };
}

/** Over the CAN socket bit 4 is offered only while the last read shows writing released there (bit 0). */
const writeReleased = (group0: number | undefined) => group0 !== undefined && (group0 & G0.write) !== 0;

/** What a switch cell shows as „an“: its bit at 1, for every switch alike (the row names say what „an“ means). */
export const switchOn = (v: number, b: number) => ((v >> b) & 1) === 1;

/** The bits of Group0 an edit takes from the pack as it is now instead of the edited (or restored) block: bit 5 always,
 *  and over the CAN socket or in a proven column every patch bit this path does not change. */
export function keepMask(patch: PackPatch | null | undefined, uart1: boolean, group0: number | undefined): number {
  return fixedOf(patch, uart1) | (uart1 && !writeReleased(group0) ? G0.extendedOff : 0);
}

/** The label of the shared rows of bits 4 and 5: by their function only when every column with switches has a proof
 *  that may carry the cutoff (`pack_cutoff` true or not known); neutral „Bit 0.4/0.5“ when the columns differ (a 1.5
 *  pack, an original one); unnamed with no such column. */
export function cutoffRowLabel(cols: { patch?: PackPatch | null; switches: boolean }[]): "named" | "neutral" | "unnamed" {
  const withSwitches = cols.filter((c) => c.switches);
  const capable = withSwitches.filter((c) => !!c.patch && c.patch.caps.pack_cutoff !== false).length;
  return capable === 0 ? "unnamed" : capable === withSwitches.length ? "named" : "neutral";
}

/** A column whose pack proved the cutoff on this connection and whose Group0 shows the lock: „Packsperre aktiv“. */
export const packLockActive = (patch: PackPatch | null | undefined, group0: number | undefined) =>
  patch?.caps.pack_cutoff === true && group0 !== undefined && (group0 & LOCK_BIT) === 0;

/** The lock state differs between two Group0 values (the read an edit started from and the pack now). */
export const lockChanged = (before: number | undefined, now: number | undefined) =>
  before !== undefined && now !== undefined && ((before ^ now) & LOCK_BIT) !== 0;
