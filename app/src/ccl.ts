import type { ProfileView } from "./api";
import { CAP } from "./access";
import { packFirmware } from "./power";

/** „EXT — CCL-Abbaustufe (Master)“ (EMU1101-EXT Full 1.6.7, capability bit 4): pure rules of the section. The setting is
 *  the master's own, numeric, 1..20 A per online pack and aggregate update; never part of the parameter block. */
export const CCL_MIN = 1;
export const CCL_MAX = 20;

/** The text typed as a step: digits only, 1..20; null for anything else (a fraction, a sign, an exponent, empty, out of
 *  range). Nothing is rounded or clamped. */
export function parseStep(text: string): number | null {
  const v = text.trim();
  if (!/^\d{1,2}$/.test(v)) return null;
  const n = Number(v);
  return n >= CCL_MIN && n <= CCL_MAX ? n : null;
}

/** The master's capability bit 4 as this connection proved it (its own fresh 64/03 in the profile); a version alone never. */
export function masterOffers(profile: ProfileView | null): boolean {
  const caps = packFirmware(0, profile, undefined).caps;
  return caps != null && (caps & (1 << CAP.ccl_step)) !== 0;
}

/** What a shown value belongs to: the link, the profile read (generation, time) and the master's build. Any change drops
 *  the value and reads again, so a control never targets a former master. */
export function masterKey(linkKey: string, profile: ProfileView | null): string {
  const m = profile?.master;
  return `${linkKey}|${profile?.gen ?? ""}|${profile?.at ?? ""}|${m?.build_id ?? ""}|${m?.version ?? ""}`;
}

/** The master's write release (Group0 bit 0) as the profile last read it: true locked, false free, null not known (the
 *  save reads it fresh before sending). A write to the master since then leaves it open. */
export function writeLockedOf(profile: ProfileView | null): boolean | null {
  if (!profile?.gates || profile.gates_stale || profile.gates.write === null) return null;
  return !profile.gates.write;
}

/** The bank decrease per completed update with `packs` online at step `step` (A); not per second. */
export const bankStep = (packs: number, step: number): number => packs * step;
