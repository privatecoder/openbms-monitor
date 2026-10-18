import { useSyncExternalStore } from "react";
import type { Gates, HardStopSeen, MasterView, PackPatch, ProfileView } from "./api";

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
export function profileLine(p: ProfileView | null, t: T): { tone: "ok" | "warn" | "muted"; text: string } | null {
  if (!p || p.bus !== "can") return null;
  const m = p.master;
  switch (m.kind) {
    case "patched": {
      const marker = m.marker === "on" ? t("profile.markerOn") : m.marker === "off" ? t("profile.markerOff") : m.marker === "foreign" ? t("profile.markerForeign", { word: m.marker_word }) : t("profile.markerUnread");
      return { tone: m.marker === "off" ? "ok" : "warn", text: `${patchName(m, t)} · ${marker}` };
    }
    case "unknown-patch": return { tone: "warn", text: t("profile.unknownPatch", { version: m.version ?? "" }) };
    case "no-0x64": return { tone: "muted", text: t("profile.no0x64") };
    default: return { tone: "warn", text: t("profile.unknown") };
  }
}

/** „Hard-Stop: …“ of one pack: the image's own flag wins (a format-3 master, `flag` true/false); else a sighting in the
 *  pack's behaviour (`seen`: waiting for recharge with the charge MOSFET off, with its date); else unknown. Never from a
 *  version number or a flash, and never „not seen“. */
export function hardStopLine(flag: boolean | null | undefined, seen: HardStopSeen | undefined, t: T, lang = "de"): string {
  const at = (ms: number) => new Date(ms).toLocaleDateString(lang);
  const state = flag === true ? t("profile.hardStopOn") : flag === false ? t("profile.hardStopOff")
    : seen ? t("profile.hardStopSeen", { at: at(seen.at) })
    : t("profile.hardStopUnknown");
  return t("profile.hardStop", { state });
}

/** The master's release bits as a short line („Schreiben über CAN-Buchse: erlaubt · Wartungsstart: gesperrt“). */
export function gatesLine(g: Gates | null, t: T, stale = false): string | null {
  if (!g) return stale ? t("profile.gatesStale") : null;
  const part = (v: boolean | null, key: string) => (v === null ? null : `${t(`profile.gate.${key}`)}: ${t(v ? "profile.allowed" : "profile.locked")}`);
  const line = [part(g.write, "write"), part(g.start, "start")].filter(Boolean).join(" · ");
  return line ? (stale ? `${line} ${t("profile.staleSuffix")}` : line) : null;
}

/** „Erneut prüfen“ is offered for every CAN socket profile: also a recognised patch may have changed its release bits. */
export const recheckable = (p: ProfileView | null): boolean => p?.bus === "can";

/** How the release bits of Group0 appear in a parameter column: named and editable where the pack's patch is proven and
 *  the bus allows it, else unnamed and fixed. `uart1`: the column was read over the CAN socket. */
export function gateBits(patch: PackPatch | null | undefined, uart1: boolean): { named: boolean; writeEditable: boolean; startEditable: boolean } {
  if (!patch) return { named: false, writeEditable: false, startEditable: false };
  // RS485-1/2 is the way to change both; over the CAN socket bit 2 never changes (the pack refuses the whole write)
  return { named: true, writeEditable: patch.caps.write_gate, startEditable: !uart1 && patch.caps.start_gate };
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
  return uart1 && !!patch?.caps.write_gate && group0 !== undefined && (group0 & 1) === 0;
}
