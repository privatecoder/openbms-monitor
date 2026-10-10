import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { KEYS, UNITS, type Parameters } from "./params";

export type Bus = "pack" | "can";
export type Endpoint = { kind: "serial"; path: string; bus: Bus } | { kind: "tcp"; addr: string; bus: Bus };
export type Severity = "info" | "warning" | "protection" | "fault";
export type LimitState = "normal" | "below_limit" | "above_limit" | { other: number };

export interface DeviceInfo { device_name: string; firmware_version: string; can_protocol: string }
export interface Telemetry {
  address: number;
  cell_voltages: number[];
  cell_temperatures: number[];
  ambient_temperature: number;
  power_temperature: number;
  current: number;
  pack_voltage: number;
  remaining_capacity_ah: number;
  full_capacity_ah: number;
  soc: number;
  rated_capacity_ah: number;
  cycles: number;
  port_voltage: number;
  current_offset_ma: number | null;
  idle_current_ma: number | null;
  energy_charged_kwh: number | null;
  energy_discharged_kwh: number | null;
}
export interface ActiveAlarm { id: string; key: string; severity: Severity }
export interface Status {
  address: number;
  cell_limits: LimitState[];
  events: number[];
  switch_state: number;
  balancing: number;
  system_status: number;
  alarms: ActiveAlarm[];
}
export interface SystemValues {
  state: number;
  charge_allowed: boolean;
  discharge_allowed: boolean;
  pack_communication_fault: boolean;
  voltage: number;
  current: number;
  soc: number;
  highest_cell_voltage: number;
  highest_cell_voltage_pack: number | null;
  lowest_cell_voltage: number;
  lowest_cell_voltage_pack: number | null;
  highest_cell_temperature: number;
  highest_cell_temperature_pack: number | null;
  lowest_cell_temperature: number;
  lowest_cell_temperature_pack: number | null;
  charge_voltage_limit: number;
  charge_current_limit: number;
  discharge_current_limit: number;
  discharge_voltage_limit: number;
  total_capacity_ah: number;
}
export interface PackUpdate { address: number; telemetry: Telemetry | null; status: Status | null; error: string | null }
export interface SystemUpdate { values: SystemValues | null; error: string | null }
/** Recording into the history database. Off by default; changes apply at once. */
export interface HistorySettings {
  enabled: boolean;
  /** addresses to record, empty = every polled pack */
  packs: number[];
  system: boolean;
  /** minimum seconds between two rows per pack (0 = every poll) */
  interval_s: number;
  /** days every answer is kept; older ones become minute values */
  keep_full_days: number;
  /** days minute values are kept after that, 0 = forever */
  keep_minutes_days: number;
}
export interface HistoryStatus { settings: HistorySettings; active: boolean; path: string | null; bytes: number; rows: number; error: string | null }
export interface SiteSpan { site: string; from: number | null; to: number | null; packs: number[]; has_system: boolean }
type Col = (number | null)[];
export interface PackCols { current: Col; soc: Col; voltage: Col; cell_max: Col; cell_min: Col; delta: Col; temp_max: Col; temp_min: Col }
export interface SystemCols { current: Col; soc: Col; voltage: Col; ccl: Col; dcl: Col; cvl: Col; cell_max: Col; cell_min: Col }
export interface HistoryEvent { pack: number; key: string; severity: Severity | ""; start: number; end: number | null }
export interface Series {
  from: number; to: number; step: number; x: number[];
  packs: Record<string, PackCols>; system: SystemCols | null; cells: Col[]; events: HistoryEvent[];
}
export interface ExportStats { rows: number; files: string[] }
export interface ImportStats { rows: number; skipped: number; from: number | null; to: number | null }

const inTauri = "__TAURI_INTERNALS__" in window;

const tauriApi = {
  listPorts: () => invoke<string[]>("list_ports"),
  connect: (endpoint: Endpoint) => invoke<void>("connect", { endpoint }),
  disconnect: () => invoke<void>("disconnect"),
  scan: () => invoke<[number, DeviceInfo][]>("scan"),
  startPolling: (addresses: number[], intervalMs: number) => invoke<void>("start_polling", { addresses, intervalMs }),
  stopPolling: () => invoke<void>("stop_polling"),
  onPack: (f: (u: PackUpdate) => void) => listen<PackUpdate>("pack", (e) => f(e.payload)),
  onSystem: (f: (u: SystemUpdate) => void) => listen<SystemUpdate>("system", (e) => f(e.payload)),
  loadUserCells: () => invoke<unknown[]>("load_user_cells"),
  saveUserCells: (cells: unknown[]) => invoke<void>("save_user_cells", { cells }),
  historyStatus: () => invoke<HistoryStatus>("history_status"),
  setHistorySettings: (settings: HistorySettings) => invoke<HistoryStatus>("history_settings", { settings }),
  historySites: () => invoke<SiteSpan[]>("history_sites"),
  historySeries: (site: string, from: number, to: number, packs: number[], cellsPack: number | null, maxPoints: number) =>
    invoke<Series>("history_series", { site, from, to, packs, cellsPack, maxPoints }),
  historyExport: (site: string, from: number, to: number, packs: number[], format: "jsonl" | "csv", path: string) =>
    invoke<ExportStats>("history_export", { site, from, to, packs, format, path }),
  historyImport: (path: string, site: string) => invoke<ImportStats>("history_import", { path, site }),
  historyClear: (site: string | null) => invoke<void>("history_clear", { site }),
  /** Folders the file dialogs start in: [old recordings, exports]. */
  historyDirs: () => invoke<[string, string]>("history_dirs"),
  revealHistory: (path: string | null) => invoke<void>("reveal_history", { path }),
  parameters: (address: number) => invoke<Parameters>("parameters", { address }),
  /** Saves into the app's parameters folder and returns the full path. */
  saveParameters: (name: string, content: string) => invoke<string>("save_parameters", { name, content }),
  revealParameters: (path: string | null) => invoke<void>("reveal_parameters", { path }),
};

export const api = inTauri ? tauriApi : demoApi();
export const isDemo = !inTauri;

/** Preview only: ?balance=<pack> shows cells 3 and 7 of that pack as balancing. */
const balanceDemo = new URLSearchParams(location.search).get("balance");

/** Browser preview without Tauri: replays a recorded capture of a 12-pack system. */
function demoApi(): typeof tauriApi {
  const packL = new Set<(u: PackUpdate) => void>();
  const sysL = new Set<(u: SystemUpdate) => void>();
  let timer: number | undefined;
  const load = () => import("./demo/capture.json").then((m) => m.default as unknown as {
    scan: { address: number; device: DeviceInfo }[];
    packs: { address: number; telemetry: Telemetry; status: Status }[];
    system: SystemValues;
  });
  const sub = <T,>(set: Set<(u: T) => void>, f: (u: T) => void) => { set.add(f); return Promise.resolve(() => { set.delete(f); }); };
  return {
    listPorts: async () => ["/dev/cu.usbserial-demo"],
    connect: async () => {},
    disconnect: async () => { clearInterval(timer); },
    scan: async () => (await load()).scan.map((s) => [s.address, s.device] as [number, DeviceInfo]),
    startPolling: async (addresses, intervalMs) => {
      const c = await load();
      const tick = () => {
        sysL.forEach((f) => f({ values: c.system, error: null }));
        for (const p of c.packs.filter((p) => addresses.includes(p.address)))
          packL.forEach((f) => f({ address: p.address, telemetry: p.telemetry, error: null,
            status: String(p.address) === balanceDemo ? { ...p.status, balancing: 0b1000100 } : p.status }));
      };
      clearInterval(timer); tick(); timer = window.setInterval(tick, intervalMs);
    },
    stopPolling: async () => { clearInterval(timer); },
    onPack: (f) => sub(packL, f),
    onSystem: (f) => sub(sysL, f),
    // preview: keep own cells in browser storage
    // errors are passed on, so the cell store can refuse to report success or overwrite unreadable data
    loadUserCells: async () => JSON.parse(localStorage.getItem("cells.user") ?? "[]"),
    saveUserCells: async (cells) => { localStorage.setItem("cells.user", JSON.stringify(cells)); },
    // preview: settings in memory, a synthetic day of history for the packs of the capture
    historyStatus: async () => ({ ...demoHistory, active: demoHistory.settings.enabled }),
    setHistorySettings: async (settings) => { demoHistory = { ...demoHistory, settings }; return { ...demoHistory, active: settings.enabled }; },
    historySites: async () => {
      const c = await load();
      const now = Date.now();
      return [{ site: "demo:4196", from: now - 86_400_000, to: now, packs: c.packs.map((p) => p.address), has_system: true }];
    },
    historySeries: async (_site, from, to, packs, cellsPack, maxPoints) => demoSeries(from, to, packs, cellsPack, maxPoints),
    historyExport: async () => { throw new Error("preview"); },
    historyImport: async () => { throw new Error("preview"); },
    historyClear: async () => {},
    historyDirs: async () => ["", ""],
    revealHistory: async () => {},
    // preview: the same configuration in every pack, pack 05 with a different balancing start and switch byte
    parameters: async (address) => {
      await new Promise((r) => setTimeout(r, 250));
      if (address === 0) throw new Error("timeout after 1000 ms");
      const values = DEMO_PARAMS.map((v, i) => (address === 5 && i === 8 ? 3.45 : v));
      return {
        address, device_name: "1101-SP75",
        function_switches: address === 5 ? [0xff, 0xff, 0xff, 0x3f, 0xbf, 0x9f, 0xbf, 0x1f] : [0xff, 0xdf, 0xff, 0x3f, 0xbf, 0x9f, 0xbf, 0x1f],
        parameters: values.map((value, index) => ({ index, key: KEYS[index], raw: 0, value, unit: UNITS[index] })),
      };
    },
    saveParameters: async (name, content) => {
      const a = document.createElement("a");
      a.href = URL.createObjectURL(new Blob([content], { type: "application/xml" }));
      a.download = name; a.click(); URL.revokeObjectURL(a.href);
      return name;
    },
    revealParameters: async () => {},
  };
}
const DEMO_PARAMS = [3.45, 3.35, 2.9, 3.1, 3.65, 3.45, 2.7, 3.1, 3.4, 1.5, 55.2, 53.6, 46.4, 48, 56, 53.6, 43.2, 48, 63, 61,
  50, 47, 2, 5, 55, 50, -10, 0, 52, 47, -10, 3, 55, 50, -15, 0, 0, 10, 50, 47, 0, 3, 60, 55, -10, 0, 90, 85, 100, 85,
  150, 145, -155, -153, 160, -160, -300, 2000, 280, 75, 0.5, 0.3, 0.03, 0.02, 10, 16, 10, 10, 30, 60, 5, 5, 1, 10, 10,
  30, 240, 48, 15, 5, 96, 80, 10, 9, 0, 13, 0];
let demoHistory: HistoryStatus = {
  settings: { enabled: false, packs: [], system: true, interval_s: 0, keep_full_days: 30, keep_minutes_days: 0 },
  active: false, path: "~/Library/Application Support/io.github.privatecoder.openbms-monitor/history.sqlite", bytes: 48_300_000, rows: 0, error: null,
};

/** Preview: a day of a 12-pack bank (charging by day, discharging at night), pack 01 with a weak share. */
function demoSeries(from: number, to: number, packs: number[], cellsPack: number | null, maxPoints: number): Series {
  const step = Math.max((to - from) / maxPoints, 15_000), n = Math.max(1, Math.ceil((to - from) / step));
  const x = Array.from({ length: n }, (_, i) => from + i * step);
  const hour = (t: number) => ((t / 3_600_000) % 24 + 24) % 24;
  const total = (t: number) => { const h = hour(t); return h > 9 && h < 16 ? 160 * Math.sin(((h - 9) / 7) * Math.PI) : h > 18 || h < 6 ? -45 : 5; };
  const soc = (t: number) => { const h = hour(t); return h > 9 && h < 16 ? 30 + ((h - 9) / 7) * 65 : h >= 16 && h <= 18 ? 95 : 95 - (((h - 18 + 24) % 24) / 12) * 60; };
  const r = (a: number, i: number) => Math.sin(a * 12.9898 + i * 78.233) * 0.5;
  const out: Series["packs"] = {};
  for (const a of packs) {
    const share = a === 1 ? 0.55 : 1 + r(a, 0) * 0.08;
    const cur = x.map((t) => (total(t) / packs.length) * share);
    const cmax = x.map((t, i) => 3.3 + soc(t) / 1000 + (soc(t) > 90 ? (soc(t) - 90) / 40 : 0) + r(a, i) * 0.004);
    out[a] = {
      current: cur, soc: x.map((t) => soc(t) + r(a, 1) * 4), voltage: cmax.map((v) => v * 16 - 0.05),
      cell_max: cmax, cell_min: cmax.map((v, i) => v - 0.006 - Math.abs(r(a, i)) * 0.006), delta: cmax.map((_, i) => 6 + Math.abs(r(a, i)) * 6),
      temp_max: x.map((t) => 22 + Math.abs(total(t)) / 40 + r(a, 2)), temp_min: x.map((t) => 20 + Math.abs(total(t)) / 60 + r(a, 3)),
    };
  }
  const sys = { current: x.map(total), soc: x.map(soc), voltage: x.map((t) => 52.8 + soc(t) / 50), ccl: x.map((t) => (soc(t) > 95 ? 60 : 300)),
    dcl: x.map(() => 300), cvl: x.map(() => 56.8), cell_max: x.map((t) => 3.31 + soc(t) / 1000), cell_min: x.map((t) => 3.29 + soc(t) / 1000) };
  const cells = cellsPack === null ? [] : Array.from({ length: 16 }, (_, c) => x.map((t) => 3.3 + soc(t) / 1000 + r(c, 4) * 0.01));
  return { from, to, step, x, packs: out, system: sys, cells, events: [{ pack: 1, key: "alarm.ev3.b1", severity: "warning", start: from + (to - from) * 0.6, end: from + (to - from) * 0.63 }] };
}

/** Best available current: regular value, or the 1 mA resolution idle current while idle. */
export const effectiveCurrent = (t: Telemetry) => (t.current !== 0 ? t.current : (t.idle_current_ma ?? 0) / 1000);

export type PackState = "charging" | "discharging" | "standby" | "off" | "unknown";
export const packState = (systemStatus: number | undefined): PackState => {
  if (systemStatus === undefined) return "unknown";
  if (systemStatus & 0x20) return "off";
  if (systemStatus & 0x02) return "charging";
  if (systemStatus & 0x01) return "discharging";
  if (systemStatus & 0x10) return "standby";
  return "unknown";
};
