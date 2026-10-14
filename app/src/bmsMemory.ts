import type { BmsHistoryRecord, BmsTime } from "./api";

const p2 = (n: number) => String(n).padStart(2, "0");

/** The PC's local calendar time as the BMS stores it (no zone). */
export const toBmsTime = (d: Date): BmsTime => ({
  year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate(), hour: d.getHours(), minute: d.getMinutes(), second: d.getSeconds(),
});

/** A BMS time read as PC local time. */
export const fromBmsTime = (t: BmsTime) => new Date(t.year, t.month - 1, t.day, t.hour, t.minute, t.second);

/** „14.10.2026 11:32:05“ (DE) or „2026-10-14 11:32:05“ (EN): every field shown, no locale surprises. */
export function fmtBmsTime(t: BmsTime, lang: string): string {
  const time = `${p2(t.hour)}:${p2(t.minute)}:${p2(t.second)}`;
  return lang === "de" ? `${p2(t.day)}.${p2(t.month)}.${t.year} ${time}` : `${t.year}-${p2(t.month)}-${p2(t.day)} ${time}`;
}

/** „UTC+2“, „UTC−3:30“ for the PC's offset at `d`. */
export function utcOffset(d: Date): string {
  const m = -d.getTimezoneOffset();
  const a = Math.abs(m);
  return `UTC${m < 0 ? "−" : "+"}${Math.floor(a / 60)}${a % 60 ? `:${p2(a % 60)}` : ""}`;
}

/** The PC's time zone as the user knows it: „MESZ, UTC+2“; only the offset if the zone has no short name of its own. */
export function zoneLabel(d: Date, lang: string, timeZone?: string): string {
  const name = new Intl.DateTimeFormat(lang, { timeZoneName: "short", timeZone }).formatToParts(d).find((x) => x.type === "timeZoneName")?.value ?? "";
  const offset = utcOffset(d);
  return !name || /^(GMT|UTC)([+\-−]|$)/.test(name) ? offset : `${name}, ${offset}`;
}

/** BMS clock minus PC clock in whole seconds (+ = the BMS is ahead). */
export const clockDiff = (bms: BmsTime, pc: Date) => Math.round((fromBmsTime(bms).getTime() - pc.getTime()) / 1000);

/** A duration in at most two units: „3 min 12 s“, „2 h 5 min“, „4 d 3 h“. */
export function durationText(seconds: number): string {
  const s = Math.abs(Math.round(seconds));
  const units: [number, string][] = [[86_400, "d"], [3600, "h"], [60, "min"], [1, "s"]];
  const parts: string[] = [];
  let rest = s;
  for (const [n, u] of units) {
    if (rest >= n || (u === "s" && !parts.length)) { parts.push(`${Math.floor(rest / n)} ${u}`); rest %= n; }
    if (parts.length === 2) break;
  }
  return parts.join(" ");
}

/** Within this the clock counts as right (the read itself takes a moment). */
export const CLOCK_OK_S = 2;
/** Beyond this the history's timestamps are worth a warning. */
export const CLOCK_FAR_S = 300;

/** Lowest/highest cell (1-based number), spread and warmest cell sensor of a record. */
export function summary(r: BmsHistoryRecord) {
  const v = r.cell_voltages;
  let lo = 0, hi = 0;
  v.forEach((x, i) => { if (x < v[lo]) lo = i; if (x > v[hi]) hi = i; });
  return {
    min: v[lo], minCell: lo + 1, max: v[hi], maxCell: hi + 1,
    spreadMv: Math.round((v[hi] - v[lo]) * 1000),
    maxTemp: Math.max(...r.cell_temperatures),
  };
}

const csvCell = (v: string | number) => (typeof v === "number" ? String(v) : /[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);

/** All fields of the records, oldest first, as CSV (comma, decimal point, alarms as keys). */
export function historyCsv(address: number, records: BmsHistoryRecord[]): string {
  const cells = Math.max(16, ...records.map((r) => r.cell_voltages.length));
  const temps = Math.max(4, ...records.map((r) => r.cell_temperatures.length));
  const head = ["pack", "bms_time", "system_status", ...Array.from({ length: 8 }, (_, i) => `event${i + 1}`), "alarms",
    ...Array.from({ length: cells }, (_, i) => `cell${i + 1}_v`), ...Array.from({ length: temps }, (_, i) => `cell_temp${i + 1}_c`),
    "ambient_temp_c", "power_temp_c", "current_a", "pack_voltage_v", "remaining_capacity_ah", "full_capacity_ah", "soc_pct",
    "rated_capacity_ah", "cycles", "soh_pct", "port_voltage_v"];
  const iso = (t: BmsTime) => fmtBmsTime(t, "en");
  const rows = records.map((r) => [
    address, iso(r.time), r.system_status, ...r.events, r.alarms.map((a) => a.key).join(" "),
    ...Array.from({ length: cells }, (_, i) => r.cell_voltages[i] ?? ""), ...Array.from({ length: temps }, (_, i) => r.cell_temperatures[i] ?? ""),
    r.ambient_temperature, r.power_temperature, r.current, r.pack_voltage, r.remaining_capacity_ah, r.full_capacity_ah, r.soc,
    r.rated_capacity_ah, r.cycles, r.soh, r.port_voltage,
  ]);
  return [head, ...rows].map((row) => row.map(csvCell).join(",")).join("\n") + "\n";
}

export const csvName = (address: number, d = new Date()) =>
  `bms-history-pack${p2(address)}-${d.getFullYear()}${p2(d.getMonth() + 1)}${p2(d.getDate())}-${p2(d.getHours())}${p2(d.getMinutes())}.csv`;

// While a history read runs, polling pauses and no pack data arrives: the sidebar's data timeout must not count
// that time as a lost connection. `readMark` is "now" during a read and the end of the last read afterwards.
let reading = false;
let endedAt = 0;
export const setHistoryReading = (on: boolean) => { if (reading && !on) endedAt = Date.now(); reading = on; };
export const readMark = () => (reading ? Date.now() : endedAt);
