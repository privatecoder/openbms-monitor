import type { TFunction } from "i18next";

/** An installation's notification settings (setting key `notifications`); the backend decides and sends. */
export interface NotifySettings { enabled: boolean; spread_mv: number; lost_minutes: number }

export const NOTIFY_DEFAULTS: NotifySettings = { enabled: false, spread_mv: 50, lost_minutes: 5 };
export const SPREAD_RANGE = [10, 1000] as const;
export const LOST_RANGE = [1, 1440] as const;

const clamp = (v: unknown, [lo, hi]: readonly [number, number], fallback: number) =>
  typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, Math.round(v))) : fallback;

/** The stored value with defaults (none or unreadable: off) and the backend's ranges. */
export function notifySettings(v: unknown): NotifySettings {
  const o = v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  return {
    enabled: o.enabled === true,
    spread_mv: clamp(o.spread_mv, SPREAD_RANGE, NOTIFY_DEFAULTS.spread_mv),
    lost_minutes: clamp(o.lost_minutes, LOST_RANGE, NOTIFY_DEFAULTS.lost_minutes),
  };
}

// every alarm bit the status knows (events 1..8, bits 0..7); the backend sends only protection and fault ids
const ALARM_IDS = Array.from({ length: 64 }, (_, i) => `alarm.ev${(i >> 3) + 1}.b${i & 7}`);
// the backend fills these itself; i18next must leave them as they are
const KEEP = Object.fromEntries(["name", "packs", "alarm", "mv", "limit", "min"].map((k) => [k, `{{${k}}}`]));

/** The wording for the backend (`notify_texts`) in the current language; `alarmName` gives an alarm id's name. */
export function notifyTexts(t: TFunction, alarmName: (id: string) => string) {
  const msg = (key: string, count?: number) => t(`notify.msg.${key}`, { ...KEEP, ...(count === undefined ? {} : { count }) });
  const alarms: Record<string, string> = {};
  for (const id of ALARM_IDS) {
    const name = alarmName(id);
    if (name !== id) alarms[id] = name;
  }
  return {
    title: msg("title"),
    alarm_one: msg("alarm", 1), alarm_other: msg("alarm", 2),
    silent_one: msg("silent", 1), silent_other: msg("silent", 2),
    spread_one: msg("spread", 1), spread_other: msg("spread", 2),
    lost: msg("lost"), test: msg("test"),
    alarms,
  };
}
