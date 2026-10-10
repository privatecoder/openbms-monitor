/** Cell database entry, see data/schema/cells.schema.json. */
export type Basis = "C" | "P";
export interface Rate { value: number; basis: Basis }
export interface DeratingPoint {
  temp_c?: number;
  temp_min_c?: number;
  temp_max_c?: number;
  soc_min_pct?: number;
  soc_max_pct?: number;
  value: number;
}
export interface Derating { basis: Basis; interpolation?: "linear"; points: DeratingPoint[] }
export interface Provenance { verified: boolean; source?: string; page?: number; table?: string; note?: string }
export interface Cell {
  schema_version: 1;
  id: string;
  entry_version: number;
  updated: string;
  manufacturer: string;
  model: string;
  variant?: string;
  chemistry: "LFP" | "NMC" | "LTO" | "NA-ION" | "OTHER";
  form_factor?: "prismatic" | "cylindrical" | "pouch";
  datasheet: { title: string; source_type: "manufacturer_pdf" | "manufacturer_web" | "distributor_pdf"; url: string; revision?: string; document_date?: string; sha256?: string };
  capacity?: { nominal_ah?: number; minimum_ah?: number; nominal_energy_wh?: number };
  voltage?: { nominal_v?: number; charge_cutoff_v?: number; discharge_cutoff_v?: number; discharge_cutoff_low_temp_v?: number; operating_min_v?: number; operating_max_v?: number; storage_v?: number };
  current?: { standard_charge?: Rate; standard_discharge?: Rate; max_continuous_charge_a?: number; max_continuous_discharge_a?: number; pulse_charge?: Rate; pulse_discharge?: Rate };
  temperature?: { charge_min_c?: number; charge_max_c?: number; discharge_min_c?: number; discharge_max_c?: number; storage_min_c?: number; storage_max_c?: number };
  charge_derating?: Derating;
  provenance?: Record<string, Provenance>;
  notes?: string;
}

/** Values every check needs; a bundled entry must have all of them verified. */
export const REQUIRED_FOR_CHECKS = [
  "capacity.nominal_ah", "voltage.nominal_v", "voltage.charge_cutoff_v", "voltage.discharge_cutoff_v",
  "current.standard_charge", "current.max_continuous_charge_a", "current.max_continuous_discharge_a",
  "temperature.charge_min_c", "temperature.charge_max_c", "temperature.discharge_min_c", "temperature.discharge_max_c",
  "charge_derating",
] as const;
