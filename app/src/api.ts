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
/** packs: addresses to record, empty = all; interval_s: minimum seconds between two lines per pack (0 = every poll). */
export interface RecordOptions { packs: number[]; system: boolean; interval_s: number }
export interface RecordingStatus {
  active: boolean; path: string | null; started_ms: number; lines: number; bytes: number;
  options: RecordOptions | null; error: string | null;
}

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
  startRecording: (options: RecordOptions) => invoke<RecordingStatus>("start_recording", { options }),
  stopRecording: () => invoke<RecordingStatus>("stop_recording"),
  recordingStatus: () => invoke<RecordingStatus>("recording_status"),
  revealRecording: (path: string | null) => invoke<void>("reveal_recording", { path }),
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
    // preview: a simulated recording that only counts, nothing is written
    startRecording: async (options) => {
      rec = { active: true, path: "~/Library/Application Support/OpenBMS Monitor/recordings/openbms_preview.jsonl", started_ms: Date.now(), lines: 1, bytes: 90, options, error: null };
      return rec;
    },
    stopRecording: async () => { rec = { ...rec, active: false }; return rec; },
    recordingStatus: async () => {
      if (rec.active) {
        const n = rec.options?.packs.length || 12, every = Math.max(10, rec.options?.interval_s ?? 0);
        const lines = 1 + Math.floor(((Date.now() - rec.started_ms) / 1000 / every) * n);
        rec = { ...rec, lines, bytes: lines * 1600 };
      }
      return rec;
    },
    revealRecording: async () => {},
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
let rec: RecordingStatus = { active: false, path: null, started_ms: 0, lines: 0, bytes: 0, options: null, error: null };

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
