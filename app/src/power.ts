/** Ein/aus, restart and the clock over the CAN socket (MAINT 1.5 with image flag bit 1): what the Parameters view offers
 *  for one pack. Pure logic; the backend checks identity, flag and lock again before every command (`power.rs`). */
import type { Bus, PackProbeView, ProfileView } from "./api";

/** Group0 bit 3: 1 locks ein/aus, restart and setting the clock over the CAN socket. */
export const POWER_LOCK_BIT = 0x08;

/** The image flag of `address` as this connection proved it: the master by the profile, a slave by its own 64/03. A
 *  stored proof never counts, and nothing is ever sent to find out. */
export function powerRtc(address: number | null, profile: ProfileView | null, probe: PackProbeView | undefined): boolean {
  if (address === null || profile?.bus !== "can") return false;
  if (address === 0) return profile.master.kind === "patched" && profile.master.caps?.power_rtc === true;
  return probe?.gen === profile.gen && probe.state === "proven" && probe.power_rtc === true;
}

/** Group0 bit 3 in the last state seen: the pack's column read on this connection, else the master's release bits of the
 *  profile; null: not known (a command still reads it fresh first). */
export function powerLocked(address: number | null, group0: number | undefined, profile: ProfileView | null): boolean | null {
  if (group0 !== undefined) return (group0 & POWER_LOCK_BIT) !== 0;
  if (address === 0 && profile?.bus === "can" && typeof profile.gates?.power === "boolean") return !profile.gates.power;
  return null;
}

type Where = { connected: boolean; bus: Bus | null; demo: boolean; installation: boolean };

/** What „Steuern“ offers: everything on RS485-1/2 with an installation (or the demo); over the CAN socket only restart and
 *  ein/aus of a pack with the flag, with an installation; else nothing. */
export function controlMode(w: Where & { power: boolean }): "full" | "power" | null {
  if (!w.connected) return null;
  if (w.demo || (w.bus === "pack" && w.installation)) return "full";
  return w.bus === "can" && w.installation && w.power ? "power" : null;
}

/** The clock line: read on RS485-1/2 (and the demo) and over the CAN socket with the flag; set with an installation too,
 *  over the CAN socket not while the lock is known to be on. `unsupported`: the CAN socket hint for a pack without it. */
export function clockAccess(w: Where & { power: boolean; locked: boolean | null }): { read: boolean; write: boolean; unsupported: boolean } {
  const can = w.connected && w.bus === "can" && !w.demo;
  const read = w.connected && (w.bus === "pack" || w.demo || (can && w.power));
  const write = read && (w.demo || w.installation) && !(can && w.locked === true);
  return { read, write, unsupported: can && !w.power };
}

/** „Alle Packs stellen“: over the CAN socket the packs without the flag are skipped and named. */
export function clockTargets(packs: number[], canSet: (a: number) => boolean): { targets: number[]; skipped: number[] } {
  return { targets: packs.filter(canSet), skipped: packs.filter((a) => !canSet(a)) };
}

/** Over the CAN socket a pack without the service gets one line where „Steuern“ would be, instead of nothing. */
export const controlHint = (w: Where & { power: boolean }) => w.connected && w.bus === "can" && !w.demo && !w.power;

/** After ein/aus at the master over the CAN socket: its acknowledgement covers only the master, its broadcast to the
 *  slaves is not confirmed. */
export const masterBroadcast = (mode: "full" | "power" | null, address: number) => mode === "power" && address === 0;
