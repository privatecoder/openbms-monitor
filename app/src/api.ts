import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { KEYS, UNITS, type Parameters } from "./params";
import { CAN_PROTOCOLS, type Calibration } from "./calibration";
import type { RestartResult } from "./control";
import { fromRaw, isSigned, toRaw, violations, type Wire } from "./paramWrite";
import { noteUnchecked } from "./bank";
import { setProfile } from "./profile";

export type Bus = "pack" | "can";
/** what answered at the other bus's baud rate (`pack_at_9600`: RS485-1/2 of firmware 16.04.06 `_9600`, not the CAN socket) */
export type OtherBus = { answered: number | null; noise: boolean; intra: boolean; pack_at_9600: boolean };

export type Exported = { path: string; export_id: string; logs: number; logs_missing: number; recording: boolean; bytes: number };
export type Named = { id: string; name: string };
/** where an import goes (see the import dialog) */
export type ImportInto = { kind: "same" } | { kind: "merge-anyway" } | { kind: "new"; copy: boolean } | { kind: "merge"; id: string };
export type ImportPreview = {
  export_id: string; installation: string; name: string; created: number; master_serial: string | null; archived: boolean;
  places: number; boards: number; snapshots: number; logs: number; recording_bytes: number | null;
  same: Named | null; master_differs: boolean; master_matches: Named[]; completed: number | null; open_into: Named | null;
};
export type ImportApplied = {
  target: string; target_name: string; recording_rows: number; recording_events: number; logs_copied: number; already: boolean;
  report: { boards_added?: number; snapshots_added?: number; logs_added?: number; logs_missing?: number;
    not_taken?: { cell_types?: Record<string, string>; groups?: string | null; pairs?: string | null; diagnosis?: string | null; notifications?: string | null; devices?: string | null }; master_serial_not_taken?: string | null };
};
export type OpenImport = { export_id: string; installation: string; name: string; started: number };

export type Endpoint = { kind: "serial"; path: string; bus: Bus } | { kind: "tcp"; addr: string; bus: Bus };
export type Severity = "info" | "warning" | "protection" | "fault";
export type LimitState = "normal" | "below_limit" | "above_limit" | { other: number };

export interface DeviceInfo { device_name: string; firmware_version: string; can_protocol: string }
/** truncated: only the first 22 characters were readable (CAN socket bus); unavailable: not offered (RTN 04) or not
 *  reachable on this bus; no_answer: the pack stayed silent or the answer was malformed */
export type SerialValue =
  | { kind: "set"; value: string; truncated: boolean } | { kind: "placeholder"; value: string } | { kind: "empty" } | { kind: "unavailable" }
  | { kind: "no_answer" };
export interface SerialNumbers { battery: SerialValue; bms: SerialValue }
/** What a serial number read says about the board at its place: new (first identified there), same, other (another
 *  real String 2 than the newest board's: a new board row), unclear (String 2 not readable: nothing claimed). */
export type BoardSeen = { kind: "new" } | { kind: "same" } | { kind: "other"; previous: string } | { kind: "unclear" };
/** The other installation a board belongs to. */
export type OtherInstallation = { id: string; name: string; archived: boolean };
/** What a pack's BMS serial says about it against the connection's installation (`bank.rs`): known, unconfirmed (new:
 *  unknown everywhere, moved: this installation's at another address, foreign: another installation's; nothing is
 *  recorded until the user confirms), unchecked (no serial to compare), failed (no answer). `hint`: CAN socket bus,
 *  from the master's battery serial, nothing to confirm. */
export type IdentityView = {
  kind: "known" | "unconfirmed" | "unchecked" | "failed"; address: number; serial: string | null;
  why: "new" | "moved" | "foreign" | null; installation: OtherInstallation | null; hint: boolean;
};
/** A serial number read; `board` is null with „Nur ansehen“ (nothing stored), `identity` null with „Nur ansehen“. */
/** `uart1` (CAN socket): String 2 came over the binary service (true) or was refused with 0x81 (false); missing: not
 *  asked or no answer. */
export type SerialRead = SerialNumbers & { board?: BoardSeen | null; identity?: IdentityView | null; uart1?: boolean };
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
/** Calendar time of the BMS clock (no zone). */
export interface BmsTime { year: number; month: number; day: number; hour: number; minute: number; second: number }
/** One record of the pack's internal history (0x4B). */
export interface BmsHistoryRecord {
  time: BmsTime;
  system_status: number;
  events: number[];
  alarms: ActiveAlarm[];
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
  soh: number;
  port_voltage: number;
}
/** A history read, oldest record first. end: last = the pack flagged its last record; empty = no (more) records;
 *  cap = 520 records without a last flag; error = stopped by `error` after the records read so far. */
export interface BmsHistoryRead { address: number; records: BmsHistoryRecord[]; end: "last" | "empty" | "cancelled" | "cap" | "error"; error: string | null }
export interface PackUpdate { address: number; telemetry: Telemetry | null; status: Status | null; error: string | null }
export interface SystemUpdate { values: SystemValues | null; error: string | null }
/** Telemetry and status of one pack, read on demand (calibration). */
export interface PackLive { telemetry: Telemetry; status: Status }
/** A CAN protocol set the BMS accepted; `device` is null when the read-back of 0x51 failed. */
export interface CanProtocolSet { device: DeviceInfo | null; read_error: string | null }
/** Outcome of a parameter write. stale: the pack changed an untouched value since the edit's read, nothing written;
 *  written with `unanswered`: the answer to the write was missing, a fresh read showed the values sent; unclear: the
 *  answer was missing and a fresh read showed neither the values sent nor those before (or failed);
 *  refused: the BMS did not take the block, `stored` is the read right before the write (after RTN 06 the switches
 *  sent are live, these are the stored ones; `switches_live` false: the CAN socket's E2, nothing was taken).
 *  CAN socket: `before` is the read right before the write (nothing there is stored, so „Vorheriger Stand“ holds for
 *  the session), an unclear write carries what the pack holds now (`read_back`, null when that read failed). */
export type ParamWrite =
  | { kind: "stale"; fresh: Parameters }
  | { kind: "written"; read_back: Parameters | null; read_error: string | null; unanswered?: boolean; before?: Parameters }
  | { kind: "unclear"; error: string; read_back?: Parameters | null; before?: Parameters }
  | { kind: "refused"; refusal: "refused" | "invalid" | "locked"; stored: Parameters; switches_live?: boolean };
/** The last write of a pack at this installation and the parameters it replaced. */
export interface ParamHistory { last_written: number | null; previous: Parameters | null }
/** A serial number write the BMS accepted; `master_updated`: installations whose stored master serial followed;
 *  `master_unchecked`: master String 2, but the old or new value could not be read, so none was updated;
 *  `board_unchecked`: String 1 of a known board, but the database could not keep the new value for it. */
export interface SerialSet { value: SerialValue | null; read_error: string | null; master_updated: string[]; master_unchecked: boolean; board_unchecked: boolean }
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
/** `held`: connected, but recording waits until the bank is checked (right bank before writing) */
export interface HistoryStatus { settings: HistorySettings; active: boolean; path: string | null; bytes: number; rows: number; preparing?: boolean; held?: boolean;
  /** why it waits: no read decided yet, only boards this installation does not know yet (until „Übernehmen“), a pack of another installation */
  held_reason?: "pending" | "confirm" | "foreign" | null; error: string | null }
/** the frame capture under Diagnosis; `full`: it stopped itself at that limit */
export type CaptureStatus = { running: boolean; entries: number; bytes: number; timeouts: number; span_ms: number; full: "entries" | "bytes" | null };
export interface SiteSpan { site: string; from: number | null; to: number | null; packs: number[]; has_system: boolean }
type Col = (number | null)[];
export interface PackCols { current: Col; soc: Col; voltage: Col; cell_max: Col; cell_min: Col; delta: Col; temp_max: Col; temp_min: Col }
export interface SystemCols { current: Col; soc: Col; voltage: Col; ccl: Col; dcl: Col; cvl: Col; cell_max: Col; cell_min: Col }
export interface HistoryEvent { pack: number; key: string; severity: Severity | ""; start: number; end: number | null }
export interface Series {
  from: number; to: number; step: number; x: number[];
  packs: Record<string, PackCols>; system: SystemCols | null; cells: Col[]; events: HistoryEvent[];
}
/** Health report (`health.rs`): per pack and local day spread (mV, all rows and at rest), last cycle count, warmest cell. */
export interface PackDay {
  day: number; rows: number; spread_p50: number | null; spread_p95: number | null;
  /** the spread figures (all and rest) come from raw rows; false: from minute maxima, a different measure */
  spread_raw: boolean;
  /** rest_minutes: distinct clock minutes with rest values (of the kind taken) */
  rest_rows: number; rest_minutes: number; rest_p50: number | null; rest_p95: number | null; cycles: number | null; temp_max: number | null;
  /** per quarter hour since local midnight the warmest cell's maximum (92, 96 or 100 slots by the day's length), null
   *  without a value */
  temp_slots: (number | null)[];
  /** BMS balancing bits of the day: OR (bit n = cell n+1), rows with a bit set of rows with a status; null without a status */
  bal_mask?: number | null; bal_rows?: number | null; bal_n?: number | null;
}
export interface PackReport {
  pack: number; days: PackDay[]; cycles_per_30d: number | null; cycle_resets: number;
  /** counted increase and the time it spans (ms), the segments of the rate; null without counts */
  cycles_gained: number | null; cycles_span_ms: number | null;
}
/** Per pair and day: matched samples under load, minutes with samples, the plus pack's share (%); `equalizing`: of them,
 *  samples the states of charge may explain (information, they still count). */
export interface PairDay {
  day: number; samples: number; minutes: number; share: number | null; equalizing: number; marked: boolean;
}
export interface PairReport { plus: number; minus: number; days: PairDay[] }
/** `spread_from`: first row with spread data in the period; `raw_from`: first raw row (pair figures start there). */
export interface HealthReport { from: number; to: number; packs: PackReport[]; pairs: PairReport[]; spread_from: number | null; raw_from: number | null }
export interface ExportStats { rows: number; files: string[] }
export interface ImportStats { rows: number; skipped: number; from: number | null; to: number | null }
/** custom = not in the vendor allowlist (only with custom firmware allowed) */
export interface FirmwareImage { release: string; device: string; bytes: number; blocks: number; sha256: string; kind: "original" | "maintenance" | "custom"; custom: boolean }
export type FirmwareStage = "preflight" | "bootloader" | "erase" | "block" | "finalize" | "app_restart" | "post_check";
export interface FirmwareProgress { stage: FirmwareStage; block: number | null; total: number | null }
/** End of a flash. written = W F E was sent, so after an error the board may have no application; pending = the
 *  pack may hang in its bootloader and its record stays. */
export interface FirmwareDone { ok: boolean; error: string | null; stage: FirmwareStage | null; written: boolean; before: DeviceInfo | null; after: DeviceInfo | null; log: string; pending: boolean; log_error?: string | null }
/** The right-bank check of a flash: "none" without an installation. */
/** `marker` (master only): "checked" when a fresh read of the installation's control channel decided C', "unchecked"
 *  without one (the marker last seen was not on). */
export interface FlashStarted { bank: "match" | "stored" | "unchecked" | "none"; marker?: "checked" | "unchecked" }

export type ControlEndpoint = { kind: "serial"; path: string } | { kind: "tcp"; addr: string };
/** An installation: endpoints may belong to several installations; `master_serial`, `archived`, `legacy` (the endpoint
 *  text it was moved in from, „übernommen“) and `last_connect` are set by the backend. */
export interface Installation { id: string; name: string; endpoints: string[]; control: ControlEndpoint | null; master_serial?: string | null; archived?: boolean; legacy?: string | null; last_connect?: number | null }
/** Settings an installation owns (JSON each, null: none). */
export interface InstallationSettings { cell_types: Record<string, unknown>; groups: unknown; pairs: unknown; diagnosis: unknown; notifications: unknown; devices: unknown }
export type SettingKey = "cell_types" | "groups" | "pairs" | "diagnosis" | "notifications" | "devices";
/** One slice of the balancer timeline (ms per state; time without a row is in none). External balancer computed:
 *  active / inactive / unknown / unassigned. BMS as reported: bal (a minute row by its share) / idle / no_status;
 *  cells: most cells at once in a single answer; minutes: minute rows that balanced (only their share is known). */
export interface BalancerSlice { from: number; to: number; active: number; inactive: number; unknown: number; unassigned: number; bal: number; idle: number; no_status: number; cells: number; minutes: boolean }
/** `balancer_timeline` (balancer.rs): measured is false while no device is read. */
export interface BalancerTimeline { from: number; to: number; slices: BalancerSlice[]; total: BalancerSlice; devices: string[]; first_assigned: number | null; measured: boolean }
/** `cell_analysis` (cell_analysis.rs): what the recorded raw rows say about each cell of a pack. */
export type CellClass = "ok" | "rest_ok" | "soc_offset" | "rest_offset" | "capacity" | "ambiguous" | "resistance" | "measurement" | "none";
export type CellReason =
  | { key: "within" } | { key: "partial"; top: boolean; bottom: boolean; r: boolean; steps: number } | { key: "rest_only"; rest: number } | { key: "rest"; rest: number }
  | { key: "same_sign"; top: number; bottom: number } | { key: "capacity"; top: number; bottom: number }
  | { key: "ambiguous"; top: number; bottom: number; steps: number } | { key: "resistance"; pct: number; steps: number }
  | { key: "offset"; rest: number; load: number } | { key: "jumps"; count: number } | { key: "no_rest"; rows: number }
  | { key: "top_only"; top: number } | { key: "bottom_only"; bottom: number } | { key: "pattern"; top: number; bottom: number }
  | { key: "r_slight"; pct: number };
/** One cell: median deviation from the row median (mV) per situation, null with too few rows; top/bottom also corrected by
 *  the cell's resistance (null without it); resistance in mΩ and % of the pack median; mean place passing 3.45 V. */
export interface CellResult {
  cell: number; rest: number | null; rest_rows: number; load: number | null; top: number | null; bottom: number | null;
  top_corr: number | null; bottom_corr: number | null; r_mohm: number | null; r_pct: number | null; steps: number; order: number | null; jumps: number;
  class: CellClass; reason: CellReason;
}
export interface CellAnalysis {
  from: number; to: number; cell_count: number; rows: number; incomplete: number; minute_rows: number;
  balancer_active: number; balancer_unknown: number; balancer: boolean;
  rest_rows: number; top_rows: number; bottom_rows: number; load_rows: number; bms_left_out: number; rest_no_status: number;
  steps: number; r_median: number | null; charges: number; cells: CellResult[];
  /** mean deviation (mV) per cell and slice of [from, to] */
  heat: (number | null)[][];
}
/** The connection's installation and why notifications are off for it: CAN socket with the master not matched
 *  (`can_no_serial`: none stored for address 0 to compare), RS485-1/2 with no pack confirmed yet. */
export interface NotifyStatus { installation: string | null; can_unchecked: boolean; can_no_serial: boolean; none_confirmed: boolean }
export interface Board { id: number; bms_serial: string; battery_serial: string | null; first_seen: number; last_seen: number; aliases: string[] }
/** board: "current" (the newest board at the place), "previous" (another board, swapped since), "unclear". */
export interface StoredSnapshot { id: number; address: number; bms_serial: string | null; at: number; kind: "read" | "written" | "refused"; board: "current" | "previous" | "unclear" }
export interface LogEntry { id: number; address: number | null; at: number; kind: string; release: string | null; result: string | null; file: string }
export interface PlaceView { address: number; boards: Board[]; cell_type: string | null; snapshot: StoredSnapshot | null; flash: LogEntry | null }
export interface InstallationOverview {
  places: PlaceView[];
  counts: { boards: number; places: number; snapshots: number; logs: number; layout: boolean; diagnosis: boolean };
  recorded: number; recorded_from: number | null; recorded_to: number | null; log_dir: string | null;
}
/** The newest stored parameter block of a place, parsed (null: not readable). */
export interface SnapshotParams { address: number; at: number; kind: StoredSnapshot["kind"]; board: StoredSnapshot["board"]; bms_serial: string | null; parameters: Parameters | null;
  /** a read after a refused write whose switches differ from the stored ones then: maybe live in RAM only */
  switches_unconfirmed?: boolean }
/** The old settings of one site key, raw text as found in the browser storage. */
export interface LegacyLocal { cellTypes?: string; groups?: string; pairs?: string; diag?: string }
/** What a patched firmware offers (by its patch version; never compared against a version number in the UI). */
export interface PatchCaps { status_split: boolean; uart1_params: boolean; uart1_identity: boolean; write_gate: boolean; start_gate: boolean }
/** The master as read over the CAN socket bus (0x64/03, then for a supported patch 0x64/02). kind: "patched" (patch
 *  functions by its patch version; `listed`: the build's name, null when the build is not listed), "unknown-patch",
 *  "no-0x64" (firmware not proven), "unknown". `polling` after a release: "unverified" or "no-slaves". */
export interface MasterView {
  kind: "patched" | "unknown-patch" | "no-0x64" | "unknown";
  version: string | null; patch: string | null; base: string | null; build_id: string | null; listed: string | null; caps: PatchCaps | null;
  marker: "off" | "on" | "foreign" | null; marker_word: string | null; slaves: number | null;
  may_halt: boolean; may_release: boolean; polling?: "unverified" | "no-slaves";
}
/** The release bits of a Group0 value, each only where the capability has it. */
export interface Gates { write: boolean | null; start: boolean | null }
/** The firmware profile of the monitor connection, read on its endpoint right after connecting (`profile.rs`). */
/** `gates_stale`: a write to the master since then left its release bits open (shown as possibly old). */
export interface ProfileView { gen: number; bus: Bus; endpoint: string; master: MasterView; gates: Gates | null; gates_stale?: boolean; at: number }
/** What proves a pack's patch version: the master's identity on this link, or a stored proof (after an app flash, or
 *  the user's confirmation) for the board with this String 2. */
export interface PackPatch { source: "master" | "flash" | "user"; release: string; caps: PatchCaps; at: number | null }
export type MaintError =
  | { kind: "connection"; message: string } | { kind: "unsupported" } | { kind: "no-answer" } | { kind: "rejected"; rtn: number }
  | { kind: "start-refused"; reason: string }
  | { kind: "timeout"; marker: "off" | "on" | "foreign"; slaves: number } | { kind: "refused"; reason: string } | { kind: "busy" }
  | { kind: "store"; message: string } | { kind: "other-bank"; serial: string } | { kind: "bootloader-pending"; pack: number; endpoint: string };
/** The panel's live status: a short listen on the pack bus, and with a CAN socket the master read. */
export interface LiveStatus { bus: "quiet" | "polling" | "traffic" | "no-port" | "error"; polled: number[]; bus_error: string | null; master: MasterView | null; master_error: MaintError | null }
export interface BankView { state: "match" | "stored" | "mismatch" | "unchecked" | "none"; serial: string | null }
/** `cancelled`: stopped by „Abbrechen“, `packs` holds what answered until then */
export interface PackScan { packs: [number, DeviceInfo][]; bank: BankView; cleared: number[]; cancelled: boolean }
/** A pack that may hang in its bootloader after an interrupted flash, per pack bus endpoint. */
export interface Pending { id: number; endpoint: string; pack: number; release: string; since: number; bms_serial?: string | null }
export interface Alerts { markers: { installation: string; name: string; at: number }[]; pending: Pending[] }


// tests run without a window: the preview API then
/** The installation chosen in the UI (null: „Nur ansehen“), sent with every write to a pack: the backend refuses a
 *  write when the connection belongs to another one. Set by the installation store. */
let chosen: string | null = null;
export const setChosenInstallation = (id: string | null) => { chosen = id; };
// the backend's generation of the last connect (`connect` returns it)
let linkGen = 0;
/** The chosen installation and the last connection as one key: a series of writes or a late answer that began under
 *  another key belongs to another bank or connection. */
export const linkNow = () => `${chosen ?? ""}|${linkGen}`;

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** What `connect` did: the endpoint linked to the installation now, the generation, the first pack that answered, and
 *  the firmware profile read on this endpoint. */
export interface Connected { linked: boolean; answered: number; gen: number; profile: ProfileView }

/** A write's result from the backend: the value and whether the target had no serial to check the bank with. The
 *  wrapper hands on the value and reports `unchecked` to the note in the Parameters view (`bank.ts`). */
const written = <T,>(address: number, p: Promise<{ value: T; unchecked: boolean }>): Promise<T> =>
  p.then((r) => { if (r.unchecked) noteUnchecked(address); return r.value; });

/** The demo has no device: a CAN socket whose master does not know 0x64. */
const DEMO_PROFILE: ProfileView = {
  gen: 0, bus: "can", endpoint: "demo", gates: null, at: 0,
  master: { kind: "no-0x64", version: null, patch: null, base: null, build_id: null, listed: null, caps: null, marker: null, marker_word: null, slaves: null, may_halt: false, may_release: false },
};

const tauriApi = {
  listPorts: () => invoke<string[]>("list_ports"),
  /** `installation` null: „Nur ansehen“ (nothing stored, no writes). `linked`: the endpoint was linked to it just now. */
  /** opens the port and checks that a pack answers (`addresses` first, then the rest of 0..15); no answer: error
   *  JSON `{ code: "no-answer", noise, intra }`, the port is closed again */
  connect: (endpoint: Endpoint, installation: string | null, addresses?: number[]) =>
    invoke<Connected>("connect", { endpoint, installation, addresses: addresses ?? null })
      .then((r) => { linkGen = r.gen; setProfile(r.profile); return r; }),
  /** the connection's firmware profile (null: not connected) */
  firmwareProfile: () => invoke<ProfileView | null>("firmware_profile"),
  /** „Erneut prüfen“ on connection `gen` (CAN socket) */
  firmwareProfileCheck: (gen: number) => invoke<ProfileView>("firmware_profile_check", { gen }).then((p) => { setProfile(p); return p; }),
  /** „Dieses Pack hat die Patch-Funktionen“, bound to String 2 read now (`serial`: the one the column was read with) */
  patchProofConfirm: (installation: string, address: number, serial: string, gen: number) => invoke<PackPatch>("patch_proof_confirm", { installation, address, serial, gen }),
  /** „Beleg zurücknehmen“ */
  patchProofRevoke: (installation: string, address: number) => invoke<void>("patch_proof_revoke", { installation, address }),
  /** USB port only: asks at the other bus's baud rate (read-only) after a connect found nothing */
  probeOtherBus: (path: string, bus: Bus, addresses?: number[]) =>
    invoke<OtherBus>("probe_other_bus", { path, bus, addresses: addresses ?? null }),
  /** `lost`: the UI drops a dead link to reconnect (notifications keep watching for the lost connection) */
  connectCancel: () => invoke<void>("connect_cancel"),
  disconnect: (lost = false) => invoke<void>("disconnect", { lost }).finally(() => setProfile(null)),
  /** „Erneut prüfen“ found another profile */
  onFirmwareProfile: (f: (p: ProfileView) => void) => listen<ProfileView>("firmware-profile", (e) => f(e.payload)),
  /** without addresses: search 0..15; with addresses: only read their device info */
  /** `connecting`: the search of a connect, stopped by its „Abbrechen“ */
  scan: (addresses?: number[], connecting = false) => invoke<[number, DeviceInfo][]>("scan", { addresses: addresses ?? null, connecting }),
  startPolling: (addresses: number[], intervalMs: number) => invoke<void>("start_polling", { addresses, intervalMs }),
  /** CAN socket bus: read the master's system values once and send them as a "system" event */
  systemOnce: () => invoke<void>("system_once"),
  stopPolling: () => invoke<void>("stop_polling"),
  /** battery and BMS serial number, as far as the connected bus shows them; `binary` (CAN socket): ask this pack over
   *  the binary service even when the connection's master does not offer it */
  serialNumbers: (address: number, binary = false) => invoke<SerialRead>("serial_numbers", { address, binary }),
  onPack: (f: (u: PackUpdate) => void) => listen<PackUpdate>("pack", (e) => f(e.payload)),
  onSystem: (f: (u: SystemUpdate) => void) => listen<SystemUpdate>("system", (e) => f(e.payload)),
  /** The backend dropped a dead link (gateway closed the socket, I/O error) and stopped polling. */
  onLinkLost: (f: (err: string) => void) => listen<string>("link-lost", (e) => f(e.payload)),
  /** The pack bus panel or a flash stopped the monitor to use the pack port: not a lost link. */
  onMonitorStopped: (f: () => void) => listen("monitor-stopped", () => f()),
  /** a serial read or a write's check found a pack whose board this installation does not know yet */
  onBankUnconfirmed: (f: (v: IdentityView) => void) => listen<IdentityView>("bank-unconfirmed", (e) => f(e.payload)),
  /** „Übernehmen“: the listed unconfirmed packs of this connection belong to `installation`; returns those recorded */
  bankConfirm: (installation: string, addresses: number[]) => invoke<number[]>("bank_confirm", { installation, addresses }),
  /** the serial reads after connection `gen` have ended (also with no pack found): a bank nothing decided records */
  bankReadsDone: (gen: number) => invoke<void>("bank_reads_done", { gen }),
  /** RS485-1/2: the endpoint was linked to the installation once its bank was decided; the installation's name */
  onBankLinked: (f: (name: string) => void) => listen<string>("bank-linked", (e) => f(e.payload)),
  loadUserCells: () => invoke<unknown[]>("load_user_cells"),
  saveUserCells: (cells: unknown[]) => invoke<void>("save_user_cells", { cells }),
  historyStatus: () => invoke<HistoryStatus>("history_status"),
  setHistorySettings: (settings: HistorySettings) => invoke<HistoryStatus>("history_settings", { settings }),
  historySites: () => invoke<SiteSpan[]>("history_sites"),
  historySeries: (site: string, from: number, to: number, packs: number[], cellsPack: number | null, maxPoints: number) =>
    invoke<Series>("history_series", { site, from, to, packs, cellsPack, maxPoints }),
  healthReport: (site: string, from: number, to: number, pairs: { plus: number; minus: number }[]) =>
    invoke<HealthReport>("health_report", { site, from, to, pairs }),
  /** both balancing lanes of a pack in `slices` parts of the span; `devices`: the installation's device setting */
  balancerTimeline: (site: string, pack: number, from: number, to: number, devices: unknown, slices: number) =>
    invoke<BalancerTimeline>("balancer_timeline", { site, pack, from, to, devices, slices }),
  /** the cell analysis of a pack from its raw rows; `slices` heatmap columns (0 = none) */
  cellAnalysis: (site: string, pack: number, from: number, to: number, devices: unknown, slices: number) =>
    invoke<CellAnalysis>("cell_analysis", { site, pack, from, to, devices, slices }),
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
  /** RS485-1/2 only, like the parameters */
  bmsClock: (address: number) => invoke<BmsTime>("bms_clock", { address }),
  /** sets the clock to `time` and returns the clock read back */
  setBmsClock: (address: number, time: BmsTime) => written(address, invoke<{ value: BmsTime; unchecked: boolean }>("set_bms_clock", { address, time, installation: chosen })),
  /** RS485-1/2 only: telemetry and status of one pack, for the calibration's live values */
  packLive: (address: number) => invoke<PackLive>("pack_live", { address }),
  /** The learned full capacity (0x42), on both buses. */
  packFullCapacity: (address: number) => invoke<number>("pack_full_capacity", { address }),
  /** RS485-1/2 only; a refusal by the BMS fails with `{"kind":"refused"|"invalid"}` (see `refusal` in calibration.ts) */
  bmsCalibrate: (address: number, calibration: Calibration) => written(address, invoke<{ value: void; unchecked: boolean }>("bms_calibrate", { address, calibration, installation: chosen })),
  /** RS485-1/2 only, 1…6; once accepted, the device info read back or why that read failed */
  setCanProtocol: (address: number, protocol: number) => written(address, invoke<{ value: CanProtocolSet; unchecked: boolean }>("set_can_protocol", { address, protocol, installation: chosen })),
  /** RS485-1/2 only: reads 0x47 again, writes `edited` (changes against `base`) on top of it and reads back;
   *  `restore` sends `edited` as it is (the stored block after a refusal) */
  /** `serial`: the String 2 read with the edited column (CAN socket); the write checks the pack still has it. */
  writeParameters: (address: number, base: Wire, edited: Wire, restore: boolean, serial?: string | null) => written(address, invoke<{ value: ParamWrite; unchecked: boolean }>("write_parameters", { address, base, edited, restore, serial: serial ?? null, installation: chosen })),
  /** After the user confirmed it: the board row at `address` a CAN socket write found still listed is no longer there. */
  bankMarkGone: (address: number, board: number, at: number | null, gen: number) => invoke<boolean>("bank_mark_gone", { installation: chosen, address, board, at, gen }),
  paramHistory: (address: number) => invoke<ParamHistory>("param_history", { address }),
  /** RS485-1/2 only: 0xA3 (battery) / 0xA5 (BMS), read back; a refusal fails like `bmsCalibrate` */
  setSerial: (address: number, which: "battery" | "bms", text: string) => written(address, invoke<{ value: SerialSet; unchecked: boolean }>("set_serial", { address, which, text, installation: chosen })),
  /** RS485-1/2 only, one pack each; every one fails with `{"kind":"pack-busy",…}` while another action holds the pack
   *  (see `packBusy` in control.ts) and with a refusal like `bmsCalibrate`. Restart: 0x51 before, then up to 15 s
   *  watching for the pack to answer again */
  packRestart: (address: number) => written(address, invoke<{ value: RestartResult; unchecked: boolean }>("pack_restart", { address, installation: chosen })),
  packOnOff: (address: number, on: boolean) => written(address, invoke<{ value: void; unchecked: boolean }>("pack_on_off", { address, on, installation: chosen })),
  packHeater: (address: number, on: boolean) => written(address, invoke<{ value: void; unchecked: boolean }>("pack_heater", { address, on, installation: chosen })),
  /** history, energy counters and cycle count to 0; not reversible */
  packClearHistory: (address: number) => written(address, invoke<{ value: void; unchecked: boolean }>("pack_clear_history", { address, installation: chosen })),
  /** remaining capacity in 10 mAh; RAM only until the BMS saves its state */
  packSetRemaining: (address: number, raw: number) => written(address, invoke<{ value: void; unchecked: boolean }>("pack_set_remaining", { address, raw, installation: chosen })),
  /** the whole internal history; polling pauses meanwhile; progress as "bms-history-progress" */
  bmsHistory: (address: number) => invoke<BmsHistoryRead>("bms_history", { address }),
  bmsHistoryCancel: () => invoke<void>("bms_history_cancel"),
  onBmsHistoryProgress: (f: (p: { address: number; n: number }) => void) => listen<{ address: number; n: number }>("bms-history-progress", (e) => f(e.payload)),
  /** the end of every history read, also of one this UI lost track of */
  /** writes the CSV to a path from the save dialog */
  saveBmsHistory: (path: string, content: string) => invoke<void>("save_bms_history", { path, content }),
  firmwareCheckImage: (path: string, allowCustom: boolean, lang: string) => invoke<FirmwareImage>("firmware_check_image", { path, allowCustom, lang }),
  /** Disconnects the monitor, then reads the pack on the pack bus without resetting it. */
  firmwareInspect: (port: string, address: number, lang: string, installation: string | null) => invoke<DeviceInfo>("firmware_inspect", { port, address, lang, installation }),
  /** Returns at once; progress and result arrive as events. A flash belongs to an installation; `checks`: its master
   *  and bank checks apply (its own RS485-1/2 port). */
  firmwareStart: (port: string, address: number, path: string, allowCustom: boolean, expectedSha256: string, confirmation: string, lang: string, installation: string, checks: boolean) =>
    invoke<FlashStarted>("firmware_start", { port, address, path, allowCustom, expectedSha256, confirmation, lang, installation, checks }),
  maintInstallations: () => invoke<Installation[]>("maint_installations"),
  /** lookup only: every installation that lists the endpoint or has it as CAN socket */
  maintInstallationsOf: (endpoint: Endpoint, activeOnly = false) => invoke<Installation[]>("maint_installations_of", { endpoint, activeOnly }),
  maintSaveInstallation: (installation: Installation) => invoke<Installation>("maint_save_installation", { installation }),
  /** refused "name-required", "name-taken"; nothing is created then */
  maintCreateInstallation: (name: string, control: ControlEndpoint | null, endpoints: string[]) => invoke<Installation>("maint_create_installation", { name, control, endpoints }),
  /** the one-time move of the old data; returns the site keys done (their browser storage may go). Refused
   *  "history-preparing" while the recording's file is being prepared. */
  installationsMigrate: (local: Record<string, LegacyLocal>) => invoke<string[]>("installations_migrate", { local }),
  installationSettings: (installation: string) => invoke<InstallationSettings>("installation_settings", { installation }),
  setInstallationSetting: (installation: string, key: Exclude<SettingKey, "cell_types">, value: unknown) => invoke<void>("set_installation_setting", { installation, key, value }),
  /** the cell types of the changed places (null removes one); the others stay */
  setInstallationCellTypes: (installation: string, changes: Record<number, unknown>) => invoke<void>("set_installation_cell_types", { installation, changes }),
  installationOverview: (installation: string) => invoke<InstallationOverview>("installation_overview", { installation }),
  /** merges `source` into `target`: adds, never overwrites; the log of the move keeps what was not taken */
  installationMerge: (source: string, target: string) => invoke<unknown>("installation_merge", { source, target }),
  installationArchive: (installation: string, archived: boolean) => invoke<Installation>("installation_archive", { installation, archived }),
  installationRename: (installation: string, name: string) => invoke<Installation>("installation_rename", { installation, name }),
  /** refused "installation-busy" (open work) or "installation-connected"; deletes its recording too, log files stay */
  installationDelete: (installation: string) => invoke<void>("installation_delete", { installation }),
  installationSnapshots: (installation: string) => invoke<SnapshotParams[]>("installation_snapshots", { installation }),
  /** the installation's ZIP (boards, settings, parameter states, logs; the recording only when asked) */
  installationExport: (installation: string, path: string, recording: boolean) => invoke<Exported>("installation_export", { installation, path, recording }),
  /** unpacks and checks a ZIP; writes nothing to the databases */
  installationImportInspect: (path: string) => invoke<ImportPreview>("installation_import_inspect", { path }),
  installationImportApply: (exportId: string, into: ImportInto) => invoke<ImportApplied>("installation_import_apply", { exportId, into }),
  installationImportsOpen: () => invoke<OpenImport[]>("installation_imports_open"),
  installationImportDiscard: (exportId: string) => invoke<void>("installation_import_discard", { exportId }),
  installationImportAbandon: (exportId: string) => invoke<void>("installation_import_abandon", { exportId }),
  /** firmware logs of no installation (full paths), newest first */
  logsUnassigned: () => invoke<string[]>("logs_unassigned"),
  /** the master's new serial number becomes the stored one */
  maintAdoptMasterSerial: (installation: string, serial: string) => invoke<Installation>("maint_adopt_master_serial", { installation, serial }),
  maintAlerts: () => invoke<Alerts>("maint_alerts"),
  /** „Pack X ist abgeklemmt oder repariert“ */
  maintClearPending: (id: number) => invoke<void>("maint_clear_pending", { id }),
  /** refused busy while a flash or another operation runs */
  maintLive: (installation: string | null, port: string) => invoke<LiveStatus>("maint_live", { installation, port }),
  /** `port`: the RS485-1/2 endpoint key (`serial:…` / `tcp:…`), like `maintLive`, `firmwareInspect` and `firmwareStart` */
  maintScanPacks: (installation: string | null, port: string) => invoke<PackScan>("maint_scan_packs", { installation, port }),
  maintScanCancel: () => invoke<void>("maint_scan_cancel"),
  maintHalt: (installation: string) => invoke<MasterView>("maint_halt", { installation }),
  /** `expectedSlaves`: the DIP slave count the user confirmed when the halt kept none */
  maintRelease: (installation: string, expectedSlaves: number | null = null) => invoke<MasterView>("maint_release", { installation, expectedSlaves }),
  /** the DIP slave count kept by the halt for the installation's master (null: none) */
  maintExpectedSlaves: (installation: string) => invoke<number | null>("maint_expected_slaves", { installation }),
  /** before flashing an image with the patch functions: the target's Group0 from a fresh read on the pack bus (null:
   *  not readable now) */
  firmwareGroup0: (port: string, address: number) => invoke<number | null>("firmware_group0", { port, address }),
  onFirmwareProgress: (f: (p: FirmwareProgress) => void) => listen<FirmwareProgress>("firmware-progress", (e) => f(e.payload)),
  onFirmwareDone: (f: (d: FirmwareDone) => void) => listen<FirmwareDone>("firmware-done", (e) => f(e.payload)),
  /** The window was not closed because a flash is running. */
  onFirmwareBusy: (f: () => void) => listen("firmware-busy", () => f()),
  revealFirmwareLogs: (path: string | null) => invoke<void>("reveal_firmware_logs", { path }),
  /** frame capture: one at a time for the whole app; starting again continues it, a full one stays stopped until cleared */
  captureStart: () => invoke<CaptureStatus>("capture_start"),
  captureStop: () => invoke<CaptureStatus>("capture_stop"),
  captureClear: () => invoke<CaptureStatus>("capture_clear"),
  captureStatus: () => invoke<CaptureStatus>("capture_status"),
  /** writes the capture as text to a .txt path from the save dialog; returns the number of entries */
  captureExport: (path: string, lang: string) => invoke<number>("capture_export", { path, lang }),
  /** the wording of the notifications in the UI's language (`notifyTexts` in notifications.ts) */
  notifyTexts: (texts: Record<string, unknown>) => invoke<void>("notify_texts", { texts }),
  notifyTest: (installation: string | null) => invoke<void>("notify_test", { installation }),
  notifyStatus: () => invoke<NotifyStatus>("notify_status"),
};

export const api = inTauri ? tauriApi : demoApi();
export const isDemo = !inTauri;

/** Preview only: ?balance=<pack> shows cells 3 and 7 of that pack as balancing. */
const balanceDemo = typeof location === "undefined" ? null : new URLSearchParams(location.search).get("balance");

const idleCapture: CaptureStatus = { running: false, entries: 0, bytes: 0, timeouts: 0, span_ms: 0, full: null };

/** Browser preview without Tauri: replays a recorded capture of a 12-pack system. */
function demoApi(): typeof tauriApi {
  const packL = new Set<(u: PackUpdate) => void>();
  const sysL = new Set<(u: SystemUpdate) => void>();
  const histL = new Set<(p: { address: number; n: number }) => void>();
  let demoCancel = false;
  let timer: number | undefined;
  const load = () => import("./demo/capture.json").then((m) => m.default as unknown as {
    scan: { address: number; device: DeviceInfo }[];
    packs: { address: number; telemetry: Telemetry; status: Status }[];
    system: SystemValues;
  });
  // the capture's pack, or the copy pack control changed
  const demoControl = new Map<number, PackLive>();
  const demoPack = async (address: number): Promise<PackLive | undefined> => {
    if (demoControl.has(address)) return demoControl.get(address);
    const p = (await load()).packs.find((x) => x.address === address);
    return p && { telemetry: p.telemetry, status: p.status };
  };
  const sub = <T,>(set: Set<(u: T) => void>, f: (u: T) => void) => { set.add(f); return Promise.resolve(() => { set.delete(f); }); };
  return {
    listPorts: async () => ["/dev/cu.usbserial-demo"],
    connect: async () => ({ linked: false, answered: 0, gen: 0, profile: DEMO_PROFILE }),
    firmwareProfile: async () => DEMO_PROFILE,
    firmwareProfileCheck: async () => DEMO_PROFILE,
    patchProofConfirm: async () => { throw new Error("desktop app only"); },
    patchProofRevoke: async () => { throw new Error("desktop app only"); },
    probeOtherBus: async () => ({ answered: null, noise: false, intra: false, pack_at_9600: false }),
    connectCancel: async () => {},
    disconnect: async () => { clearInterval(timer); setProfile(null); },
    onFirmwareProfile: async () => () => {},
    scan: async (addresses?: number[]) => (await load()).scan.filter((s) => !addresses || addresses.includes(s.address))
      .map((s) => [s.address, demoProtocol.has(s.address) ? { ...s.device, can_protocol: demoProtocol.get(s.address)! } : s.device] as [number, DeviceInfo]),
    systemOnce: async () => { const c = await load(); sysL.forEach((f) => f({ values: c.system, error: null })); },
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
    // preview: synthetic serial numbers, one pack still with the factory placeholder
    serialNumbers: async (address) => {
      if (!(await load()).scan.some((s) => s.address === address)) throw new Error("timeout");
      const n = String(address + 1).padStart(4, "0");
      return {
        battery: address === 2 ? { kind: "placeholder", value: "EMU110110S-XXXX-20000101-00000" } : { kind: "set", value: `DEMO-BAT-${n}`, truncated: false },
        bms: { kind: "set", value: `SEPLOS-0000000${n}`, truncated: false },
        board: null,
      };
    },
    onPack: (f) => sub(packL, f),
    onSystem: (f) => sub(sysL, f),
    onLinkLost: async () => () => {},
    onMonitorStopped: async () => () => {},
    onBankUnconfirmed: async () => () => {},
    bankConfirm: async (_installation, addresses) => addresses,
    bankReadsDone: async () => {},
    onBankLinked: async () => () => {},
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
    healthReport: async (_site, from, to, pairs) => demoReport(from, to, (await load()).packs.map((p) => p.address), pairs),
    balancerTimeline: async (_site, _pack, from, to, _devices, slices) => demoBalancer(from, to, slices),
    cellAnalysis: async (_site, pack, from, to, _devices, slices) => demoCells(pack, from, to, slices),
    historyExport: async () => { throw new Error("preview"); },
    historyImport: async () => { throw new Error("preview"); },
    historyClear: async () => {},
    historyDirs: async () => ["", ""],
    revealHistory: async () => {},
    parameters: async (address) => {
      await new Promise((r) => setTimeout(r, 250));
      if (address === 0) throw new Error("timeout after 1000 ms");
      return demoParams(address);
    },
    // preview: blocks that break the firmware's rules are refused like RTN 06; nothing is kept beyond the session
    writeParameters: async (address, _base, edited) => {
      await new Promise((r) => setTimeout(r, 400));
      const stored = demoParams(address);
      const signed = edited.raws.map((r, i) => (isSigned(i) && r >= 0x8000 ? r - 0x10000 : r));
      if (violations(signed).length) return { kind: "refused", refusal: "refused", stored };
      demoWritten.set(address, { raws: signed, switches: edited.switches, at: Date.now(), previous: stored });
      return { kind: "written", read_back: demoParams(address), read_error: null };
    },
    bankMarkGone: async () => true,
    paramHistory: async (address) => {
      const w = demoWritten.get(address);
      return { last_written: w?.at ?? null, previous: w?.previous ?? null };
    },
    setSerial: async (_address, _which, text) => ({ value: { kind: "set", value: text.trim(), truncated: false }, read_error: null, master_updated: [], master_unchecked: false, board_unchecked: false }),
    saveParameters: async (name, content) => {
      const a = document.createElement("a");
      a.href = URL.createObjectURL(new Blob([content], { type: "application/xml" }));
      a.download = name; a.click(); URL.revokeObjectURL(a.href);
      return name;
    },
    revealParameters: async () => {},
    // preview: a clock 3 min 12 s behind, and a synthetic history of a few days
    bmsClock: async (address) => {
      await new Promise((r) => setTimeout(r, 150));
      if (address === 0) throw new Error("timeout after 1500 ms");
      const d = new Date(Date.now() - 192_000);
      return { year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate(), hour: d.getHours(), minute: d.getMinutes(), second: d.getSeconds() };
    },
    setBmsClock: async (_address, time) => time,
    // preview: live values from the capture; calibrations are taken unless the current is off by 3 A or more
    packLive: async (address) => {
      await new Promise((r) => setTimeout(r, 120));
      const p = await demoPack(address);
      if (!p) throw new Error("timeout after 1500 ms");
      return p;
    },
    packFullCapacity: async (address) => {
      await new Promise((r) => setTimeout(r, 120));
      const p = await demoPack(address);
      if (!p) throw new Error("timeout after 1500 ms");
      return p.telemetry.full_capacity_ah;
    },
    bmsCalibrate: async (address, calibration) => {
      await new Promise((r) => setTimeout(r, 300));
      const p = (await load()).packs.find((x) => x.address === address);
      if (calibration.kind === "current" && p && Math.abs(calibration.centi_amps / 100 - p.telemetry.current) >= 3.01) throw JSON.stringify({ kind: "refused" });
    },
    setCanProtocol: async (address, protocol) => {
      await new Promise((r) => setTimeout(r, 200));
      demoProtocol.set(address, CAN_PROTOCOLS.find((x) => x.n === protocol)?.text ?? "");
      const d = (await load()).scan.find((s) => s.address === address)?.device;
      if (!d) throw new Error("timeout after 1500 ms");
      return { device: { ...d, can_protocol: demoProtocol.get(address)! }, read_error: null };
    },
    // preview: control acts on a copy of the capture's status and telemetry, kept for the session
    packRestart: async (address) => {
      await new Promise((r) => setTimeout(r, 1500));
      const p = await demoPack(address);
      if (!p) return { kind: "not_reachable" };
      demoControl.set(address, { ...p, status: { ...p.status, system_status: p.status.system_status & ~0x20, switch_state: p.status.switch_state & ~0x08 } });
      return { kind: "back", after_ms: 3200 };
    },
    packOnOff: async (address, on) => {
      await new Promise((r) => setTimeout(r, 200));
      const p = await demoPack(address);
      if (!p) throw new Error("timeout after 1500 ms");
      const s = p.status;
      demoControl.set(address, { ...p, status: { ...s, system_status: on ? s.system_status & ~0x20 : s.system_status | 0x20, switch_state: on ? s.switch_state | 0x01 : s.switch_state & ~0x01 } });
    },
    packHeater: async (address, on) => {
      await new Promise((r) => setTimeout(r, 200));
      const p = await demoPack(address);
      if (!p) throw new Error("timeout after 1500 ms");
      demoControl.set(address, { ...p, status: { ...p.status, switch_state: on ? p.status.switch_state | 0x08 : p.status.switch_state & ~0x08 } });
    },
    packClearHistory: async (address) => {
      await new Promise((r) => setTimeout(r, 300));
      const p = await demoPack(address);
      if (!p) throw new Error("timeout after 1500 ms");
      demoControl.set(address, { ...p, telemetry: { ...p.telemetry, cycles: 0, energy_charged_kwh: 0, energy_discharged_kwh: 0 } });
    },
    packSetRemaining: async (address, raw) => {
      await new Promise((r) => setTimeout(r, 200));
      const p = await demoPack(address);
      if (!p) throw new Error("timeout after 1500 ms");
      if (raw > Math.round(p.telemetry.full_capacity_ah * 100)) throw JSON.stringify({ kind: "refused" });
      const ah = raw / 100;
      demoControl.set(address, { ...p, telemetry: { ...p.telemetry, remaining_capacity_ah: ah, soc: Math.round((ah / p.telemetry.full_capacity_ah) * 1000) / 10 } });
    },
    bmsHistory: async (address) => {
      demoCancel = false;
      const records: BmsHistoryRecord[] = [];
      const n = 48;
      for (let i = 0; i < n; i++) {
        if (demoCancel) return { address, records, end: "cancelled", error: null };
        await new Promise((r) => setTimeout(r, 40));
        records.push(demoRecord(Date.now() - 192_000 - (n - 1 - i) * 2 * HOUR_MS, i));
        histL.forEach((f) => f({ address, n: records.length }));
      }
      return { address, records, end: "last", error: null };
    },
    bmsHistoryCancel: async () => { demoCancel = true; },
    onBmsHistoryProgress: (f) => sub(histL, f),
    saveBmsHistory: async () => {},
    captureStart: async () => { throw new Error("desktop app only"); },
    captureStop: async () => idleCapture,
    captureClear: async () => idleCapture,
    captureStatus: async () => idleCapture,
    captureExport: async () => { throw new Error("desktop app only"); },
    notifyTexts: async () => {},
    notifyTest: async () => { throw new Error("desktop app only"); },
    notifyStatus: async () => ({ installation: null, can_unchecked: false, can_no_serial: false, none_confirmed: false }),
    firmwareCheckImage: async () => { throw new Error("desktop app only"); },
    firmwareInspect: async () => { throw new Error("desktop app only"); },
    firmwareStart: async () => { throw new Error("desktop app only"); },
    maintInstallations: async () => [],
    maintInstallationsOf: async () => [],
    maintSaveInstallation: async () => { throw new Error("desktop app only"); },
    maintCreateInstallation: async () => { throw new Error("desktop app only"); },
    // preview: no installations, nothing stored
    installationsMigrate: async () => [],
    installationSettings: async () => ({ cell_types: {}, groups: null, pairs: null, diagnosis: null, notifications: null, devices: null }),
    setInstallationSetting: async () => {},
    setInstallationCellTypes: async () => {},
    installationOverview: async () => { throw new Error("desktop app only"); },
    installationMerge: async () => { throw new Error("desktop app only"); },
    installationArchive: async () => { throw new Error("desktop app only"); },
    installationRename: async () => { throw new Error("desktop app only"); },
    installationDelete: async () => { throw new Error("desktop app only"); },
    installationSnapshots: async () => [],
    installationExport: async () => { throw new Error("desktop app only"); },
    installationImportInspect: async () => { throw new Error("desktop app only"); },
    installationImportApply: async () => { throw new Error("desktop app only"); },
    installationImportsOpen: async () => [],
    installationImportDiscard: async () => {},
    installationImportAbandon: async () => {},
    logsUnassigned: async () => [],
    maintAdoptMasterSerial: async () => { throw new Error("desktop app only"); },
    maintAlerts: async () => ({ markers: [], pending: [] }),
    maintClearPending: async () => { throw new Error("desktop app only"); },
    maintLive: async () => { throw new Error("desktop app only"); },
    maintScanPacks: async () => { throw new Error("desktop app only"); },
    maintScanCancel: async () => {},
    maintHalt: async () => { throw new Error("desktop app only"); },
    maintRelease: async () => { throw new Error("desktop app only"); },
    maintExpectedSlaves: async () => null,
    firmwareGroup0: async () => null,
    onFirmwareProgress: async () => () => {},
    onFirmwareDone: async () => () => {},
    onFirmwareBusy: async () => () => {},
    revealFirmwareLogs: async () => {},
  };
}
/** Preview: parameter blocks written in this session. */
const demoWritten = new Map<number, { raws: number[]; switches: number[]; at: number; previous: Parameters }>();
/** Preview: the same configuration in every pack, pack 05 with a different balancing start and switch byte. */
function demoParams(address: number): Parameters {
  const w = demoWritten.get(address);
  const raws = w?.raws ?? DEMO_PARAMS.map((v, i) => toRaw(i, address === 5 && i === 8 ? 3.45 : v));
  return {
    address, device_name: "1101-SP75",
    function_switches: w?.switches ?? (address === 5 ? [0xff, 0xff, 0xff, 0x3f, 0xbf, 0x9f, 0xbf, 0x1f] : [0xff, 0xdf, 0xff, 0x3f, 0xbf, 0x9f, 0xbf, 0x1f]),
    parameters: raws.map((r, index) => ({ index, key: KEYS[index], raw: r < 0 ? r + 0x10000 : r, value: fromRaw(index, r), unit: UNITS[index] })),
  };
}
/** Preview: CAN protocols set in this session. */
const demoProtocol = new Map<number, string>();
const HOUR_MS = 3_600_000;
/** Preview: one synthetic history record; charging by day, discharging at night, one warning in the evening. */
function demoRecord(ms: number, i: number): BmsHistoryRecord {
  const d = new Date(ms), h = d.getHours();
  const current = h > 9 && h < 16 ? 40 : h > 18 || h < 6 ? -15 : 0;
  const soc = Math.max(10, Math.min(98, 50 + (h - 12) * 4));
  const cell = (c: number) => Math.round((3.28 + soc / 2000 + Math.sin(c * 1.7 + i) * 0.004) * 1000) / 1000;
  const warn = h === 20 && i % 2 === 0;
  return {
    time: { year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate(), hour: h, minute: d.getMinutes(), second: d.getSeconds() },
    system_status: current > 0 ? 0x02 : current < 0 ? 0x01 : 0x10, events: [0, warn ? 0x04 : 0, 0, 0, 0, 0, 0, 0],
    alarms: warn ? [{ id: "alarm.ev2.b2", key: "cell_low_voltage_warning", severity: "warning" }] : [],
    cell_voltages: Array.from({ length: 16 }, (_, c) => cell(c)), cell_temperatures: [21.5, 22.1, 21.8, 22.4],
    ambient_temperature: 20.3, power_temperature: 23.9, current, pack_voltage: 52.8 + soc / 50,
    remaining_capacity_ah: (280 * soc) / 100, full_capacity_ah: 280, soc, rated_capacity_ah: 280, cycles: 120, soh: 100, port_voltage: 52.9 + soc / 50,
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

/** Preview: a synthetic health report; pack 05's rest spread rises, pack 08 counts cycles faster, pair 00/01 is uneven. */
function demoReport(from: number, to: number, packs: number[], pairs: { plus: number; minus: number }[]): HealthReport {
  const start = new Date(Math.max(from, to - 90 * 86_400_000)); start.setHours(0, 0, 0, 0);
  const days: number[] = [];
  for (const d = new Date(start); d.getTime() < to; d.setDate(d.getDate() + 1)) days.push(d.getTime());
  const wobble = (a: number, i: number) => Math.round(Math.sin(a * 1.7 + i * 0.9) * 2);
  return {
    from: days[0] ?? from, to, spread_from: days[0] ?? null, raw_from: days[Math.max(0, days.length - 30)] ?? null,
    packs: packs.map((a) => ({
      pack: a, cycle_resets: 0, cycles_per_30d: a === 8 ? 16.8 : 12 + (a % 3) * 0.4,
      cycles_gained: Math.round((a === 8 ? 16.8 : 12 + (a % 3) * 0.4) * (days.length - 1) / 30), cycles_span_ms: Math.max(0, days.length - 1) * 86_400_000,
      days: days.map((day, i) => {
        const rest = 7 + (a % 3) + wobble(a, i) + (a === 5 ? Math.round(i * 0.35) : 0);
        return { day, rows: 17_000, rest_minutes: 900, spread_p50: rest + 6, spread_p95: rest + 22, spread_raw: i >= days.length - 30, rest_rows: 6_000, rest_p50: rest, rest_p95: rest + 3,
          cycles: Math.round(140 + i * (a === 8 ? 0.56 : 0.4)), temp_max: 24 + (a % 4) + wobble(a, i) / 2,
          temp_slots: Array.from({ length: 96 }, (_, s) => 22 + (a % 4) + wobble(a, i) / 2 + (s >= 40 && s < 72 ? 2 : 0)) };
      }),
    })),
    pairs: pairs.map(({ plus, minus }) => ({
      plus, minus,
      days: days.slice(-30).map((day, i) => {
        const share = plus === 0 ? 57 + wobble(plus, i) / 2 : 50 + wobble(plus, i) / 2;
        return { day, samples: 900, minutes: 140, share, equalizing: 60, marked: Math.abs(share - 50) > 5 };
      }),
    })),
  };
}

/** Preview: an external balancer that works in the afternoon (top of charge), the BMS balancing three cells near full. */
function demoBalancer(from: number, to: number, slices: number): BalancerTimeline {
  const width = (to - from) / Math.max(1, slices);
  const hour = (t: number) => ((t / 3_600_000) % 24 + 24) % 24;
  const list: BalancerSlice[] = Array.from({ length: slices }, (_, i) => {
    const a = Math.round(from + i * width), b = Math.round(from + (i + 1) * width), ms = b - a, h = hour((a + b) / 2);
    const on = h > 13 && h < 17, bms = h > 15 && h < 16.5;
    return { from: a, to: b, active: on ? ms : 0, inactive: on ? 0 : ms, unknown: 0, unassigned: 0, bal: bms ? ms : 0, idle: bms ? 0 : ms, no_status: 0, cells: bms ? 3 : 0, minutes: false };
  });
  const total = list.reduce((s, x) => ({ ...s, active: s.active + x.active, inactive: s.inactive + x.inactive, bal: s.bal + x.bal, idle: s.idle + x.idle, cells: Math.max(s.cells, x.cells) }),
    { from, to, active: 0, inactive: 0, unknown: 0, unassigned: 0, bal: 0, idle: 0, no_status: 0, cells: 0, minutes: false });
  return { from, to, slices: list, total, devices: ["Demo 4 A"], first_assigned: from - 86_400_000, measured: false };
}

/** Preview: a 16-cell pack; in pack 03 cell 5 has less capacity, cell 9 more resistance, cell 12 reads 40 mV high;
 *  elsewhere cell 2 has an SOC offset. */
function demoCells(pack: number, from: number, to: number, slices: number): CellAnalysis {
  const n = 16, hour = (t: number) => ((t / 3_600_000) % 24 + 24) % 24;
  const base = (k: number): Omit<CellResult, "cell"> => ({ rest: (k % 3) - 1, rest_rows: 4_000, load: (k % 3) - 1, top: 2 - (k % 4), bottom: (k % 4) - 2,
    top_corr: 2 - (k % 4), bottom_corr: (k % 4) - 2, r_mohm: 0.42 + (k % 3) * 0.01, r_pct: 100 + ((k % 3) - 1) * 2, steps: 212, order: 4 + (k % 9), jumps: 0, class: "ok", reason: { key: "within" } });
  const odd: Record<number, Partial<CellResult>> = pack === 3 ? {
    5: { top: 38, bottom: -27, top_corr: 36, bottom_corr: -26, order: 1.2, class: "capacity", reason: { key: "capacity", top: 36, bottom: -26 } },
    9: { r_mohm: 0.71, r_pct: 168, top: 21, bottom: -18, top_corr: 1, bottom_corr: -2, class: "resistance", reason: { key: "resistance", pct: 168, steps: 212 } },
    12: { rest: 40, load: 41, top: 39, bottom: 40, top_corr: 39, bottom_corr: 40, class: "measurement", reason: { key: "offset", rest: 40, load: 41 } },
  } : { 2: { rest: 7, top: 31, bottom: 19, top_corr: 30, bottom_corr: 19, class: "soc_offset", reason: { key: "same_sign", top: 30, bottom: 19 } } };
  const cells = Array.from({ length: n }, (_, i) => ({ cell: i + 1, ...base(i + 1), ...odd[i + 1] }));
  const width = (to - from) / Math.max(1, slices);
  const heat = cells.map((c) => Array.from({ length: slices }, (_, j) => {
    const h = hour(from + (j + 0.5) * width), top = h > 13 && h < 16, bottom = h > 4 && h < 6;
    return top ? (c.top ?? 0) : bottom ? (c.bottom ?? 0) : (c.rest ?? 0);
  }));
  return { from, to, cell_count: n, rows: 240_000, incomplete: 12, minute_rows: 0, balancer_active: 9_000, balancer_unknown: 0, balancer: true,
    rest_rows: 64_000, top_rows: 6_100, bottom_rows: 2_300, load_rows: 90_000, bms_left_out: 1_400, rest_no_status: 0, steps: 212, r_median: 0.42, charges: 6, cells, heat };
}

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
