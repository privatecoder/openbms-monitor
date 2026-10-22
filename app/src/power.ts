/** Ein/aus, restart and the clock over the CAN socket (MAINT 1.5 with image flag bit 1): what the Parameters view offers
 *  for one pack. Pure logic; the backend checks identity, flag and lock again before every command (`power.rs`). */
import type { Bus, Channel, PackProbeView, ProfileView } from "./api";
import { CUTOFF_MIN, factsOf, firmwareKnown, FLAG_CUTOFF, FLAG_POWER, G0, offerable, positiveBuild, type Facts } from "./access";

/** Group0 bit 3: 1 locks ein/aus, restart and setting the clock over the CAN socket (the positive build: 1 allows; read
 *  through `inverseG0`). */
export const POWER_LOCK_BIT = G0.powerLock;

/** The firmware of `address` as this connection proved it (`access::Facts` patch and image flags): over the CAN socket
 *  the master by the profile, a slave by its own 64/03. A stored proof never counts here (it carries no image flags), and
 *  nothing is ever sent to find out. Over RS485-1/2 nothing is proven: unknown. */
export function packFirmware(address: number | null, profile: ProfileView | null, probe: PackProbeView | undefined): Firmware {
  if (address === null || profile?.bus !== "can") return UNKNOWN;
  if (address === 0) {
    const m = profile.master;
    if (m.kind === "patched" && m.caps?.version) return { patch: { kind: "version", version: m.caps.version }, flags: m.caps.flags ?? null };
    // another or an older MNT, or no 64/03 at all: no supported patch
    return m.kind === "unknown-patch" || m.kind === "no-0x64" ? NONE : UNKNOWN;
  }
  if (probe?.gen !== profile.gen) return UNKNOWN;
  if (probe.state === "proven" && probe.patch_version) return { patch: { kind: "version", version: probe.patch_version }, flags: probe.flags ?? null };
  return probe.state === "negative" ? NONE : UNKNOWN;
}
export type Firmware = Pick<Facts, "patch" | "flags">;
const UNKNOWN: Firmware = { patch: { kind: "unknown" }, flags: null };
const NONE: Firmware = { patch: { kind: "none" }, flags: null };

/** The image flag of `address` as this connection proved it (power and clock over the CAN socket). */
export const powerRtc = (address: number | null, profile: ProfileView | null, probe: PackProbeView | undefined): boolean =>
  firmwareKnown({ kind: "image-flag", bit: FLAG_POWER }, factsOf(packFirmware(address, profile, probe))) === "yes";

/** The stored pack cutoff (patch from 1.6 with image flag bit 2) of `address` as this connection proved it. */
export function packCutoff(address: number | null, profile: ProfileView | null, probe: PackProbeView | undefined): boolean {
  const f = factsOf(packFirmware(address, profile, probe));
  return firmwareKnown({ kind: "patch-from", version: CUTOFF_MIN }, f) === "yes" && firmwareKnown({ kind: "image-flag", bit: FLAG_CUTOFF }, f) === "yes";
}

/** The power lock in the last state seen: the pack's column read on this connection (`group0` as the inverse builds read
 *  it, `inverseG0`), else the master's release bits of the profile; null: not known (a command still reads it fresh first). */
export function powerLocked(address: number | null, group0: number | undefined, profile: ProfileView | null): boolean | null {
  if (group0 !== undefined) return (group0 & POWER_LOCK_BIT) !== 0;
  if (address === 0 && profile?.bus === "can" && typeof profile.gates?.power === "boolean") return !profile.gates.power;
  return null;
}

type Where = { connected: boolean; bus: Bus | null; demo: boolean; installation: boolean };

/** The UART a bus reaches the packs on. */
export const channelOf = (bus: Bus | null): Channel | null => (bus === "pack" ? "uart0" : bus === "can" ? "uart1" : null);

/** The plain control actions of „Steuern“, in the order the section shows them. */
export const CONTROL_ACTIONS = ["restart", "power-on-off", "force-heater", "set-remaining", "set-learned-full", "clear-history"] as const;

/** What the catalog offers for one pack over this connection's channel (`fw`: its firmware as proven here). The lock
 *  bits are left out on purpose: a lock seen holds the buttons back in place (the line says why), and the backend reads
 *  them fresh before sending. */
export function controlOffers(w: Where & { fw: Firmware }): (action: string) => boolean {
  if (!w.connected) return () => false;
  if (w.demo) return () => true;
  const ch = channelOf(w.bus);
  const facts = factsOf({ ...w.fw, installation: w.installation, address: 1 });
  return (action) => !!ch && offerable(action, ch, facts);
}

/** What „Steuern“ shows: the section when the catalog offers one of its actions; "power" over the CAN socket (its
 *  texts), else "full". */
export function controlMode(w: Where & { fw: Firmware }): "full" | "power" | null {
  const offers = controlOffers(w);
  if (!CONTROL_ACTIONS.some(offers)) return null;
  return w.demo || w.bus !== "can" ? "full" : "power";
}

/** The clock line: read and set where the catalog offers it (over the CAN socket the power and clock build); set with an
 *  installation too, not while the lock is known to be on. `unsupported`: the CAN socket hint for a pack without it. */
export function clockAccess(w: Where & { fw: Firmware; locked: boolean | null }): { read: boolean; write: boolean; unsupported: boolean } {
  const can = w.connected && w.bus === "can" && !w.demo;
  const ch = channelOf(w.bus);
  // the lock as last seen, as the raw bit of this build (positive: 1 allows); not read: the backend reads it fresh first
  const facts = factsOf({ ...w.fw, installation: w.installation, address: 1, group0: w.locked === null ? null : w.locked !== positiveBuild(w.fw.patch, w.fw.flags) ? G0.powerLock : 0 });
  const read = w.connected && (w.demo || (!!ch && offerable("read-clock", ch, facts)));
  const write = read && (w.demo || (!!ch && offerable("set-clock", ch, facts)));
  return { read, write, unsupported: can && !read };
}

/** „Alle Packs stellen“: over the CAN socket the packs without the flag are skipped and named. */
export function clockTargets(packs: number[], canSet: (a: number) => boolean): { targets: number[]; skipped: number[] } {
  return { targets: packs.filter(canSet), skipped: packs.filter((a) => !canSet(a)) };
}

/** Over the CAN socket a pack the catalog offers no control action for gets one line where „Steuern“ would be. */
export const controlHint = (w: Where & { fw: Firmware }) => w.connected && w.bus === "can" && !w.demo && controlMode(w) === null;

/** After ein/aus at the master over the CAN socket: its acknowledgement covers only the master, its broadcast to the
 *  slaves is not confirmed. */
export const masterBroadcast = (mode: "full" | "power" | null, address: number) => mode === "power" && address === 0;
