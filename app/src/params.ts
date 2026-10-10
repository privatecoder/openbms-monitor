/** BMS parameters (0x47): order, grouping, BatteryMonitor XML import/export and the comparison between sources. */

export interface Parameter { index: number; key: string; raw: number; value: number; unit: string }
export interface Parameters { address: number; parameters: Parameter[]; function_switches: number[]; device_name: string }

/** Parameter keys in BMS order (P0..P86), as in openbms-proto/src/params.rs. */
export const KEYS = [
  "cell_high_voltage_alarm", "cell_high_voltage_recovery", "cell_low_voltage_alarm", "cell_low_voltage_recovery",
  "cell_overvoltage_protection", "cell_overvoltage_recovery", "cell_undervoltage_protection", "cell_undervoltage_recovery",
  "balancing_start_voltage", "cell_low_voltage_charging_forbidden",
  "pack_high_voltage_alarm", "pack_high_voltage_recovery", "pack_low_voltage_alarm", "pack_low_voltage_recovery",
  "pack_overvoltage_protection", "pack_overvoltage_recovery", "pack_undervoltage_protection", "pack_undervoltage_recovery",
  "charger_overvoltage_protection", "charger_overvoltage_recovery",
  "charging_high_temperature_alarm", "charging_high_temperature_recovery", "charging_low_temperature_alarm", "charging_low_temperature_recovery",
  "charging_over_temperature_protection", "charging_over_temperature_recovery", "charging_under_temperature_protection", "charging_under_temperature_recovery",
  "discharging_high_temperature_alarm", "discharging_high_temperature_recovery", "discharging_low_temperature_alarm", "discharging_low_temperature_recovery",
  "discharging_over_temperature_protection", "discharging_over_temperature_recovery", "discharging_under_temperature_protection", "discharging_under_temperature_recovery",
  "cell_heating_on", "cell_heating_off",
  "ambient_high_temperature_alarm", "ambient_high_temperature_recovery", "ambient_low_temperature_alarm", "ambient_low_temperature_recovery",
  "ambient_over_temperature_protection", "ambient_over_temperature_recovery", "ambient_under_temperature_protection", "ambient_under_temperature_recovery",
  "power_high_temperature_alarm", "power_high_temperature_recovery", "power_over_temperature_protection", "power_over_temperature_recovery",
  "charging_overcurrent_alarm", "charging_overcurrent_recovery", "discharging_overcurrent_alarm", "discharging_overcurrent_recovery",
  "charging_overcurrent_protection", "discharging_overcurrent_protection", "transient_overcurrent_protection", "output_soft_start_delay",
  "rated_capacity", "remaining_capacity_setting",
  "cell_difference_fault", "cell_difference_fault_recovery", "balancing_start_difference", "balancing_stop_difference",
  "static_balancing_time", "cells_in_series", "charging_overcurrent_delay", "discharging_overcurrent_delay",
  "transient_overcurrent_delay", "overcurrent_recovery_delay", "overcurrent_lock_events", "charge_current_limit_duration",
  "charge_activation_window", "charge_activation_interval", "charge_activation_attempts", "work_record_interval",
  "standby_record_interval", "standby_shutdown_delay", "remaining_capacity_alarm", "remaining_capacity_protection",
  "intermittent_recharge_below", "cycle_cumulative_capacity", "connection_fault_impedance", "compensation_1_cell",
  "compensation_1_resistance", "compensation_2_cell", "compensation_2_resistance",
] as const;

/** Display units, as the backend sends them. */
export const UNITS: string[] = KEYS.map((_, i) =>
  i <= 19 || (i >= 60 && i <= 63) ? "V" : i <= 49 ? "°C" : i <= 56 ? "A" : i === 57 || i === 68 ? "ms" : i <= 59 ? "Ah"
  : i === 64 || i === 73 || i === 77 ? "h" : i === 66 || i === 67 || i === 69 ? "s" : i === 71 || i === 72 || i === 75 || i === 76 ? "min"
  : i >= 78 && i <= 81 ? "%" : i === 82 || i === 84 || i === 86 ? "mΩ" : "");

/** Names and units exactly as BatteryMonitor writes them (machine translation, typos included). */
export const BM_NAMES = [
  "Monomer high voltage alarm", "Monomer high pressure recovery", "Monomer low pressure alarm", "Monomer low pressure recovery",
  "Monomer overvoltage protection", "Monomer overvoltage recovery", "Monomer undervoltage protection", "Monomer undervoltage recovery",
  "Equalization opening voltage", "Battery low voltage forbidden charging",
  "Total pressure high pressure alarm", "Total pressure and high pressure recovery", "Total pressure low pressure alarm", "Total pressure and low pressure recovery",
  "Total_voltage overvoltage protection", "Total pressure overpressure recovery", "Total_voltage undervoltage protection", "Total pressure undervoltage recovery",
  "Harging overvoltage protection", "Charging overvoltage recovery",
  "Charging high temperature warning", "Charging high temperature recovery", "Charging low temperature warning", "Charging low temperature recovery",
  "Charging over temperature protection", "Charging over temperature recovery", "Charging under-temperature protection", "Charging under temperature recovery",
  "Discharge high temperature warning", "Discharge high temperature recovery", "Discharge low temperature warning", "Discharge low temperature recovery",
  "Discharge over temperature protection", "Discharge over temperature recovery", "Discharge under-temperature protection", "Discharge under temperature recovery",
  "Cell low temperature heating", "Cell heating recovery",
  "Ambient high temperature alarm", "Ambient high temperature recovery", "Ambient low temperature alarm", "Ambient low temperature recovery",
  "Environmental over-temperature protection", "Environmental overtemperature recovery", "Environmental under-temperature protection", "Environmental undertemperature recovery",
  "Power high temperature alarm", "Power high temperature recovery", "Power over temperature protection", "Power over temperature recovery",
  "Charging overcurrent warning", "Charging overcurrent recovery", "Discharge overcurrent warning", "Discharge overcurrent recovery",
  "Charge overcurrent protection", "Discharge overcurrent protection", "Transient overcurrent protection", "Output soft start delay",
  "Battery rated capacity", "SOC",
  "Cell invalidation differential pressure", "Cell invalidation recovery", "Equalization opening pressure difference", "Equalization closing pressure difference",
  "Static equilibrium time", "Battery number in series", "Charge overcurrent delay", "Discharge overcurrent delay",
  "Transient overcurrent delay", "Overcurrent delay recovery", "Overcurrent recovery times", "Charge current limit delay",
  "Charge activation delay", "Charging activation interval", "Charge activation times", "Work record interval",
  "Standby recording interval", "Standby shutdown delay", "Remaining capacity alarm", "Remaining capacity protection",
  "Interval charge capacity", "Cycle cumulative capacity", "Connection fault impedance", "Compensation point 1 position",
  "Compensation point 1 impedance", "Ompensation point 2 position", "Compensation point 2 impedance",
];
const BM_UNIT: Record<string, string> = { "°C": "℃", ms: "mS", s: "S", min: "Minutes", h: "When", "": "" };
const bmUnit = (i: number) => (i === 65 || i === 83 || i === 85 ? "String" : i === 70 || i === 74 ? "times" : BM_UNIT[UNITS[i]] ?? UNITS[i]);

/** Decimals as BatteryMonitor shows them; also the precision two sources are compared at. */
export const decimals = (i: number) =>
  i <= 9 || i === 62 || i === 63 ? 3 : i <= 19 || (i >= 50 && i <= 56) || i === 58 || i === 59 || i === 60 || i === 61 ? 2
  : i <= 49 || i === 82 || i === 84 || i === 86 ? 1 : 0;

export const fmtParam = (i: number, v: number) => v.toFixed(decimals(i));

export const GROUPS: { id: string; indices: number[] }[] = [
  { id: "cellVoltage", indices: [0, 1, 2, 3, 4, 5, 6, 7, 9] },
  { id: "packVoltage", indices: [10, 11, 12, 13, 14, 15, 16, 17, 18, 19] },
  { id: "tempCharge", indices: [20, 21, 22, 23, 24, 25, 26, 27] },
  { id: "tempDischarge", indices: [28, 29, 30, 31, 32, 33, 34, 35] },
  { id: "tempBoard", indices: [38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49] },
  { id: "heating", indices: [36, 37] },
  { id: "current", indices: [50, 51, 52, 53, 54, 66, 55, 67, 56, 68, 69, 70, 71, 57] },
  { id: "capacity", indices: [58, 59, 78, 79, 80, 81] },
  { id: "balancing", indices: [8, 62, 63, 64, 60, 61] },
  { id: "activation", indices: [72, 73, 74, 75, 76, 77] },
  { id: "pack", indices: [65, 83, 84, 85, 86, 82] },
];

/** One column of the comparison: a pack read over the bus or an opened export. */
export interface Source {
  id: string;
  kind: "pack" | "file";
  label: string;
  /** device name from the BMS or the export */
  device?: string;
  values?: number[];
  switches?: number[];
  error?: string;
  /** first parameter index from which an export does not fit the known order */
  suspectFrom?: number | null;
  /** the parameters as read, for saving */
  read?: Parameters;
}

export const fromPack = (p: Parameters): Source => ({
  id: `pack-${p.address}`, kind: "pack", label: String(p.address).padStart(2, "0"), device: p.device_name,
  values: p.parameters.map((x) => x.value), switches: p.function_switches, read: p,
});

/** Values outside these ranges mean the export's order is shifted (e.g. a cell voltage of 56.8 V). */
const plausible = (i: number, v: number) =>
  i <= 9 || i === 62 || i === 63 ? v >= 0 && v <= 5 : i <= 19 ? v >= 5 && v <= 120 : i <= 49 ? v >= -50 && v <= 150
  : i <= 56 ? Math.abs(v) <= 1000 : i === 65 ? v >= 1 && v <= 32 : i >= 78 && i <= 81 ? v >= 0 && v <= 100 : true;

const norm = (s: string) => s.toLowerCase().replace(/[\s_-]+/g, " ").trim();
const unescape = (s: string) => s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, "&");

/** Parse a BatteryMonitor parameter export. Values count by position, like BatteryMonitor itself does. */
export function parseExport(text: string, label: string): Source {
  const id = `file-${label}-${text.length}`;
  const params = [...text.matchAll(/<parameter>\s*<Name>([\s\S]*?)<\/Name>\s*<Value>([\s\S]*?)<\/Value>/g)];
  const groups = [...text.matchAll(/<param_bit_group>\s*<Name>([\s\S]*?)<\/Name>\s*<Value>([\s\S]*?)<\/Value>/g)];
  if (!/<paraGroup>/.test(text) || params.length !== KEYS.length || groups.length !== 8)
    return { id, kind: "file", label, error: `${params.length}/${KEYS.length}, ${groups.length}/8` };
  const values = params.map((m) => Number(m[2].trim()));
  const switches = groups.map((m) => parseInt(m[2].trim(), 16));
  if (values.some(Number.isNaN) || switches.some((s) => Number.isNaN(s) || s < 0 || s > 255))
    return { id, kind: "file", label, error: "value" };
  const bad = KEYS.map((_, i) => i).filter((i) => norm(unescape(params[i][1])) !== norm(BM_NAMES[i]) || !plausible(i, values[i]));
  return {
    id, kind: "file", label, values, switches, suspectFrom: bad.length ? bad[0] : null,
    device: unescape(text.match(/<modualName>([\s\S]*?)<\/modualName>/)?.[1] ?? "").trim(),
  };
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** BatteryMonitor XML (UTF-8 with BOM, CRLF), so the file can be opened there too. */
export function toExport(p: Parameters): string {
  const l = ['<?xml version="1.0" encoding="utf-8"?>', "<paraGroup>", "  <filetype>Parameter</filetype>",
    `  <modualName>${esc(p.device_name.padEnd(10, " "))}</modualName>`];
  p.parameters.forEach((x, i) => l.push("  <parameter>", `    <Name>${BM_NAMES[i]}</Name>`, `    <Value>${fmtParam(i, x.value)}</Value>`, `    <Unit>${bmUnit(i)}</Unit>`, "  </parameter>"));
  p.function_switches.forEach((b, g) => l.push("  <param_bit_group>", `    <Name>BitGroup${g}</Name>`, `    <Value>${b.toString(16).toUpperCase().padStart(2, "0")}</Value>`, "  </param_bit_group>"));
  l.push("</paraGroup>");
  return "﻿" + l.join("\r\n");
}

/** File name for a saved pack: Parameter_pack03_2026-10-10_18-05.xml (local time). */
export const exportName = (address: number, now = new Date()) => {
  const z = (n: number) => String(n).padStart(2, "0");
  return `Parameter_pack${z(address)}_${now.getFullYear()}-${z(now.getMonth() + 1)}-${z(now.getDate())}_${z(now.getHours())}-${z(now.getMinutes())}.xml`;
};

/**
 * Columns whose value differs from the others. With a clear majority only the outliers are marked;
 * without one (e.g. two sources that disagree) every column with a value is.
 */
export function deviating(values: (string | undefined)[]): Set<number> {
  const count = new Map<string, number>();
  for (const v of values) if (v !== undefined) count.set(v, (count.get(v) ?? 0) + 1);
  if (count.size <= 1) return new Set();
  const sorted = [...count.entries()].sort((a, b) => b[1] - a[1]);
  const clear = sorted[0][1] > sorted[1][1] && sorted[0][1] > 1;
  return new Set(values.flatMap((v, i) => (v !== undefined && (!clear || v !== sorted[0][0]) ? [i] : [])));
}

/** Bits that have a function (help entry switch.<group>.<bit>); the rest are shown greyed out. */
export const SWITCH_BITS: Record<number, number[]> = {
  0: [1], 1: [0, 1, 2, 3, 4, 5, 6, 7], 2: [0, 1, 2, 3, 4, 5, 6, 7], 3: [0, 1, 2, 3, 4, 5, 6, 7],
  4: [0, 1, 2, 3, 4, 5, 6, 7], 5: [0, 1, 2, 3, 4, 6, 7], 6: [0, 1, 2, 3, 4, 5, 6, 7], 7: [0, 1, 3, 4, 5, 7],
};
