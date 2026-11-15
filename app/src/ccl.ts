import type { CclStep, FeedState, ProfileView } from "./api";
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

/** The add-on's confirmed value is never older than this (its own HA rule): an older one is not shown as current. */
export const FEED_MAX_AGE_MS = 30 * 60_000;
/** A read time this far ahead of the add-on's own clock is implausible: not shown as current. */
export const FEED_FUTURE_MS = 60_000;

/** What the section shows over the feed: "current" the add-on's confirmed, fresh value; "none" the add-on reports no
 *  confirmed value (or only one past its own age limit); "unknown" whether its value is current cannot be told. */
export type FeedCclState = "current" | "none" | "unknown";
export interface FeedCcl { offered: boolean; state: FeedCclState; step: CclStep | null; at: number | null }

/** The section over the add-on's feed: whether the feed's master (address 0) reports capability bit 4 in its 64/03, and
 *  the step the add-on confirmed with its read time. A value counts as current only while the feed is live for the
 *  current boot (this boot's pack messages arrive, so the add-on's clock offset is known) and its read time lies within
 *  `FEED_MAX_AGE_MS` before and `FEED_FUTURE_MS` after the add-on's now (`nowMs` plus the offset). Without that (a
 *  retained meta alone, the add-on offline or switched off, no offset yet) nothing is shown as current. */
export function feedCcl(feed: FeedState | null, nowMs: number): FeedCcl {
  const m = feed?.packs.find((p) => p.address === 0);
  const offered = m?.read_caps != null && (m.read_caps & (1 << CAP.ccl_step)) !== 0;
  const no = (state: FeedCclState): FeedCcl => ({ offered, state, step: null, at: null });
  if (!offered || !feed) return no("unknown");
  if (feed.state !== "live" || feed.skew_ms === null) return no("unknown");
  const at = m?.ccl_at ?? null;
  if (!m?.ccl || at === null) return no("none");
  const age = nowMs + feed.skew_ms - at;
  if (age < -FEED_FUTURE_MS) return no("unknown");
  if (age > FEED_MAX_AGE_MS) return no("none");
  return { offered, state: "current", step: m.ccl, at };
}

type Tr = (key: string, params?: Record<string, unknown>) => string;
export interface Line { text: string; tone?: "ok" | "warn" }

/** The read-only lines over the feed: the same value lines as on a direct connection, then when the add-on read it; or
 *  that it has no confirmed value; or, neutral, that whether its value is current is not known now. `at` formats the
 *  add-on's read time. */
export function feedCclLines(v: Pick<FeedCcl, "state" | "step" | "at">, packs: number, t: Tr, at: (ms: number) => string): Line[] {
  if (v.state === "unknown") return [{ text: t("ccl.feedUnknown") }];
  if (v.state !== "current" || !v.step || v.at === null) return [{ text: t("ccl.feedUnread"), tone: "warn" }];
  const s = v.step;
  return [
    { text: `${t("ccl.value", { v: s.down })}${s.down === 1 ? ` ${t("ccl.isDefault")}` : ""}`, tone: "ok" },
    { text: t("ccl.up", { up: s.up, min: s.min, max: s.max }) },
    ...(packs > 0 ? [{ text: t("ccl.example", { n: packs, a: bankStep(packs, s.down) }) }] : []),
    { text: t("ccl.feedRead", { at: at(v.at) }) },
  ];
}
