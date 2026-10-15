/** Right bank before writing (backend `bank.rs`): the sidebar line of a connection's unconfirmed packs, the hint for
 *  packs without a serial and the note of a write whose target could not be checked. */
import { useSyncExternalStore } from "react";
import type { IdentityView, OtherInstallation } from "./api";

// packs whose write went ahead without a serial to compare, on this connection (cleared at every connect)
let unchecked: ReadonlySet<number> = new Set();
const subs = new Set<() => void>();
const changed = () => subs.forEach((f) => f());
export const noteUnchecked = (address: number) => { if (!unchecked.has(address)) { unchecked = new Set([...unchecked, address]); changed(); } };
export const clearUnchecked = () => { if (unchecked.size) { unchecked = new Set(); changed(); } };
export const useUnchecked = () => useSyncExternalStore((f) => { subs.add(f); return () => { subs.delete(f); }; }, () => unchecked);

/** The identities of a connection by address: a later read replaces an earlier one. */
export const withIdentity = (all: IdentityView[], v: IdentityView) => [...all.filter((x) => x.address !== v.address), v].sort((a, b) => a.address - b.address);

/** What the sidebar says about a connection: packs of another installation (red: disconnect, connect with it, or take
 *  them over anyway), only new/moved ones (one question, one click for all), or nothing. `confirm`: the addresses one
 *  confirmation records (CAN hints have nothing to record). */
export type BankLine =
  | { kind: "foreign"; packs: number[]; other: OtherInstallation; hint: boolean; confirm: number[] }
  | { kind: "new"; packs: number[]; confirm: number[] }
  | null;

export function bankLine(ids: IdentityView[]): BankLine {
  const open = ids.filter((v) => v.kind === "unconfirmed");
  const confirm = open.filter((v) => !v.hint).map((v) => v.address);
  const foreign = open.filter((v) => v.why === "foreign" && v.installation);
  if (foreign.length) {
    const other = foreign[0].installation!;
    const theirs = foreign.filter((v) => v.installation!.id === other.id);
    return { kind: "foreign", packs: theirs.map((v) => v.address), other, hint: theirs.every((v) => v.hint), confirm };
  }
  return open.length ? { kind: "new", packs: open.map((v) => v.address), confirm } : null;
}

/** Packs whose BMS serial is empty or the placeholder: the app cannot tell their bank. */
export const noSerialPacks = (ids: IdentityView[]) => ids.filter((v) => v.kind === "unchecked").map((v) => v.address);

/** „00, 03“ */
export const packList = (packs: number[]) => packs.map((a) => String(a).padStart(2, "0")).join(", ");

/** The battery serial field filled from the BMS serial („BMS-Nummer übernehmen“): what fits the field (30 characters),
 *  whether it was cut, and whether the CAN socket would show only part of it (first 22 characters). */
export const SERIAL_MAX = 30;
export const CAN_SHOWN = 22;
export function batteryFromBms(bms: string): { text: string; cut: boolean; beyondCan: boolean } {
  const v = bms.trim();
  const text = v.slice(0, SERIAL_MAX);
  return { text, cut: v.length > SERIAL_MAX, beyondCan: text.length > CAN_SHOWN };
}
