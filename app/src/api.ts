import { invoke } from "@tauri-apps/api/core";

export type Bus = "pack" | "can";
export type Endpoint = { kind: "serial"; path: string; bus: Bus } | { kind: "tcp"; addr: string; bus: Bus };

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
  idle_current_ma: number | null;
  energy_charged_kwh: number | null;
  energy_discharged_kwh: number | null;
}

export const api = {
  listPorts: () => invoke<string[]>("list_ports"),
  connect: (endpoint: Endpoint) => invoke<void>("connect", { endpoint }),
  disconnect: () => invoke<void>("disconnect"),
  scan: () => invoke<[number, DeviceInfo][]>("scan"),
  telemetry: (address: number) => invoke<Telemetry>("telemetry", { address }),
};
