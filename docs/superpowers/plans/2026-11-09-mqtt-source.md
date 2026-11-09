# Home Assistant (MQTT) Source Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** OpenBMS Monitor gets a third, read-only connection type, "Home Assistant (MQTT)". It reads the add-on's OpenBMS feed and feeds overview, pair check, balancing line, recording, health report and events, without touching the RS485 bus.

**Architecture:**
- A pure Rust module `feed.rs` decodes feed messages into the existing `Telemetry`/`Status`/`SystemValues` types and tracks the source state (boot, dedup, clock).
- A module `mqtt_source.rs` runs one MQTT client thread in place of the poll thread. It emits the same `pack`/`system` events and records through the same `history` calls.
- One central guard refuses every BMS I/O command while the source is MQTT, including commands that open their own control or pack ports.
- Feed parameters are kept in memory for this connection and are not stored as parameter snapshots in v1.

**Tech Stack:** Rust (Tauri 2), crate `rumqttc` (sync client, rustls), React 19 + TypeScript, vitest, cargo test.

**Spec:** `docs/exchange/openbms-feed-v1.md`. Task 1 copies it from the add-on repo. The add-on plan `seplos-mqtt-addon/docs/superpowers/plans/2026-11-09-openbms-feed.md` is implemented first.

## Global Constraints

- **Messages and topics:**
  - Feed version `v == 1`. Any other `v` is rejected with a visible message: "feed version {v} not supported".
  - Subscriptions: `<base>/openbms/v1/#` and `<base>/availability` (the add-on's last will). `meta` is retained; `pack/<DIP>` and `system` are not.
  - The **current boot** is the `boot` of the latest `meta`. Pack and system messages of another boot are dropped. Within the current boot, duplicates by `seq` are dropped.
- **Times:**
  - Recording keeps the source read times. The row time is `r42.t` (`read_at` in the value), the status-event time is `r44.t` (the `t` argument of `history.pack`), and the system row is `r1000.t`.
  - Pairing uses the unchanged source read times; no per-message offset goes into `read_at`.
  - The live pair window runs in source time: `sourceNow = last sent_at + (local monotonic ms since that message)`.
  - The clock offset `local − sent_at` is only shown, when |offset| > 5 s.
- **Pair check:** the tolerance stays `MATCH_MS = 3000`; nothing is relaxed.
- **Read-only:** no write, maintenance, flash, BMS history, calibration, restart, scan or pack check while the source is MQTT. Local files, archives and stored data stay usable.
- **Connection state:**
  - Connected = fresh measurements arrive. A retained `meta.on == true` alone is "waiting for data".
  - `meta.on == false` or availability `offline` → "Feed is off" / "Add-on offline".
- **System block:** `r1000` only goes to `SystemValues::from_registers` (exactly 21 registers). `r0074` is ignored in v1; its failure never affects `r1000`.
- **Housekeeping:**
  - The repo is public: no real installation data in tests; synthetic frames only.
  - UI text DE/EN in `i18n.ts`; help via `tools/gen_help.py`, then `python3 tools/gen_help.py`.

## Review Focus

1. **Feed message for a DIP the installation doesn't know** (pack added in the add-on): shown and recorded like a bus answer from that address, no crash. → test in Task 2.
2. **Wrong broker password while connected over the bus:** connect fails with the broker's reason; the bus connection and recording go on untouched. → test in Task 4.
3. **The add-on restarts (new `boot`) while connected:** after the new `meta`, messages of the new boot are accepted from `seq = 1`; late old-boot messages are dropped; live pair samples and hysteresis are reset. → tests in Tasks 2 and 4.
4. **Hex of wrong length or invalid hex:** that part counts as failed with the parse error; the other parts of the message still apply. → test in Task 2.
5. **Maintenance or firmware command with MQTT active and a stored control endpoint:** refused, no port opened. → test in Task 4.

---

### Task 1: Spec copy and dependency

**Files:**
- Create: `docs/exchange/openbms-feed-v1.md` (copy from the add-on repo)
- Modify: `app/src-tauri/Cargo.toml`: add `rumqttc = { version = "<current>", default-features = false, features = ["use-rustls"] }`. The rustls versions must unify with the existing `rustls 0.23`/`ring`.

- [ ] **Step 1:** Copy the spec and add the dependency.
- [ ] **Step 2:** Run `cd app/src-tauri && cargo build`. Expected: it builds, and `cargo tree -d | grep rustls` shows one rustls 0.23.
- [ ] **Step 3:** Commit `"Feed spec, rumqttc dependency"`.

### Task 2: `feed.rs` — decoding and source state (pure)

**Files:**
- Create: `app/src-tauri/src/feed.rs` (`mod feed;` in `lib.rs`)
- Test: in-file `#[cfg(test)]`

**Interfaces:**
- Produces:

```rust
pub struct Part<T> { pub t: i64, pub value: Result<T, String> }   // err: "busy" | "failed" | "unsupported" | parse error
pub struct FeedPack { pub boot: String, pub seq: u64, pub sent_at: i64, pub address: u8,
    pub telemetry: Option<Part<Telemetry>>, pub status: Option<Part<Status>>, pub params: Option<Part<Vec<u8>>> } // 169-byte block
pub struct FeedSystem { pub boot: String, pub seq: u64, pub sent_at: i64, pub values: Option<Part<SystemValues>> }
pub struct MetaPack { pub address: u8, pub device: Option<Part<DeviceInfo>>, pub battery_serial: Option<Part<SerialValue>>,
    pub bms_serial: Option<Part<SerialValue>>, pub params: Option<Part<Vec<u8>>> }
pub struct FeedMeta { pub on: bool, pub boot: String, pub bus: String, pub auto_off: Option<i64>, pub packs: Vec<MetaPack> }
pub fn decode_pack(v: &serde_json::Value) -> Result<FeedPack, String>;     // Err only for v != 1 or missing boot/seq/addr
pub fn decode_system(v: &serde_json::Value) -> Result<FeedSystem, String>; // r1000 only
pub fn decode_meta(v: &serde_json::Value) -> Result<FeedMeta, String>;
pub enum Accept { Yes, Duplicate, OtherBoot, NoMeta }
pub struct SourceState { … }
impl SourceState {
    pub fn meta(&mut self, m: &FeedMeta) -> bool;            // true when the boot changed (caller resets)
    pub fn accept(&mut self, boot: &str, seq: u64) -> Accept;
    pub fn clock(&mut self, sent_at: i64, local_mono_ms: i64); // keeps the latest sent_at and its local monotonic time
    pub fn source_now(&self, local_mono_ms: i64) -> Option<i64>;
}
```

- **Packs:** `r42.hex` goes to `Telemetry::parse` and `r44.hex` to `Status::parse`. `r47.hex` is length-checked (169 bytes) and kept raw.
- **System:** `r1000.hex` (42 bytes, BE) becomes 21 × u16, then `SystemValues::from_registers`.
- **Meta:**
  - `r51` goes to `DeviceInfo::parse`.
  - `a2`/`a4` (exactly 60 hex characters, 30 bytes) go to `openbms_proto::serial::parse_ascii_serial`.
  - The master check uses only `a2` of address 0.

- [ ] **Step 1: Write failing tests** with synthetic INFO bytes:
  - `pack_with_both_parts`;
  - `pack_r42_failed_has_no_status`;
  - `bad_hex_fails_only_that_part`;
  - `wrong_version_is_rejected`;
  - `unknown_address_decodes` (addr 15);
  - `system_ignores_r0074_and_its_error`;
  - `meta_parses_device_serials_params`;
  - `meta_a2_wrong_length_is_a_failed_part`;
  - `no_meta_rejects_packs`;
  - `duplicate_and_older_seq_rejected`;
  - `new_boot_via_meta_accepts_seq_one_and_reports_change`;
  - `late_old_boot_message_rejected`;
  - `source_now_advances_with_local_monotonic_time`.
- [ ] **Step 2:** Run `cargo test feed::` in `app/src-tauri`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit `"feed: decode OpenBMS feed messages, source state"`.

### Task 3: Settings storage

**Files:**
- Create: `app/src-tauri/src/mqtt_source.rs` (the settings part)
- Modify: `app/src-tauri/src/lib.rs` (register the commands), `app/src/api.ts` (types, demo stubs)

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Deserialize, Clone)] pub struct MqttSettings { host: String, port: u16, user: String, password: String, tls: bool, base: String }`;
  - commands `load_mqtt_settings() -> Option<MqttSettings>` and `save_mqtt_settings(s: MqttSettings)`;
  - storage in the file `app_data_dir/mqtt.json`, mode 0600 on unix;
  - `api.loadMqttSettings()`, `api.saveMqttSettings(s)`.
- [ ] **Step 1:** Write the test `settings_round_trip_and_mode_0600` (temp dir).
- [ ] **Step 2:** Run it. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run it. Expected: PASS.
- [ ] **Step 5:** Commit `"MQTT source settings"`.

### Task 4: Source guard, MQTT thread, connect/disconnect

**Files:**
- Modify: `app/src-tauri/src/mqtt_source.rs`, `app/src-tauri/src/lib.rs` (`AppState.source`, `connect_mqtt`, `stop_monitor_as`, the guard calls), `app/src-tauri/src/{maint,firmware,control,bms_memory,power,bank,diagnosis,installations}.rs` (guard calls), `app/src/api.ts`
- Test: in-file tests. The thread core is a function over a channel of fake connection events (CONNACK, auth refusal, publish, disconnect), so tests need no broker.

**Interfaces:**
- Consumes: `feed.rs` (Task 2), `MqttSettings` (Task 3).
- Produces:
  - `AppState.source: Mutex<Option<SourceKind>>` with `enum SourceKind { Bus, Mqtt }`; `Connected.source: "bus" | "mqtt"`.
  - `pub(crate) fn bms_io_allowed(state: &AppState) -> Result<(), String>`: `Err("mqtt-source")` while the source is MQTT.
    - It is called first in every command that sends to a BMS or opens a port, before `stop_monitor` or any port open.
    - To find them, grep `state.conn`, `with_link`, `with_control`, `open_pack_for`, `firmware::open_port`, `port::open`; the command list in `lib.rs` `generate_handler!` is the checklist.
  - Command `connect_mqtt(settings: MqttSettings, installation: Option<String>) -> Result<Connected, String>`.
  - Events:
    - `"pack"` (`PackUpdate`, `read_at = r42.t`) and `"system"` (`SystemUpdate`);
    - `"feed"`: `{ state: "waiting"|"live"|"off"|"offline", boot: String, boot_changed: bool, source_now: Option<i64>, skew_ms: Option<i64>, version_error: Option<String>, packs: Vec<MetaPackView> }`, where `MetaPackView` holds the device info, serials and 0x47 block of one pack, as the UI shows them for a bus read.
  - `fn handle(state: &mut SourceState, topic: &str, payload: &[u8], local_ms: i64, mono_ms: i64) -> Vec<Effect>`, with `Effect` = `Emit(..)` | `RecordPack { t, addr, value }` | `RecordSystem { t, value }` | `NewSession` | `RoundEnd`.

`connect_mqtt` order:
1. Port slot, flashing check, `reserve_installations`.
2. **Broker first:** connect the client and wait up to 10 s for CONNACK. On refusal or timeout, return the broker's reason. The current source, link and recording are untouched.
3. Only then hand over:
   - stop polling, `lock(&state.conn)?.take()`, `new_link`, `history.disconnected()`, `wiring::bind`;
   - `bank::begin(&state, gen, false, None)`, `notifications::begin`, `h.connected(id, "mqtt")`, `source = Mqtt`;
   - subscribe.
4. **The thread** stops on a new `gen` or a stop flag; `stop_monitor_as` sets the flag, disconnects, and sets `source = None`.
   - A boot change is handled like a reconnect, before any pack effect of the new boot:
     - `new_link` (new `gen`), `bank::begin(&state, gen, false, None)`, `notifications::begin`;
     - the feed identity/parameter caches are cleared, and A2 of the new boot is checked again;
     - the session restarts through `NewSession` (see source-time session end below);
     - `feed.boot_changed = true`, so the UI resets the live pairs.
   - On the first `a2` of address 0 with a real serial: `can_master_check(&state, gen, &inst, &serial)`. Without it, nothing is checked, as on a CAN socket whose master doesn't answer.
   - After each pack message: `notifications::round_end`. A single pack on UART0 sends no `system` message.
   - **Source-time session end:** a feed session closes its open events at the latest source status time seen (`r44.t`), not at the Monitor's clock: new `History::disconnected_at(end_ms: i64)` (events: `end = MAX(start, end_ms)`, session `last = MAX(last, end_ms)`). It is used on boot change and on disconnect of an MQTT source; bus sessions keep `disconnected()`.
   - Per pack message: `notifications::answer` and `profile::observe_hard_stop`, as in the poll thread.
   - No parameter snapshot is stored from the feed in v1. Parameters reach the UI through `pack` (`params`) and `feed.packs`.

- [ ] **Step 1: Write failing tests:**
  - `pack_message_records_source_times` (row `read_at = r42.t`, event `t = r44.t`, UI `read_at` unshifted);
  - `duplicate_and_other_boot_ignored`;
  - `boot_change_starts_new_session`;
  - `availability_offline_sets_feed_offline`;
  - `meta_on_without_packs_is_waiting`;
  - `old_gen_produces_nothing`;
  - `boot_change_resets_bank_confirmation` (boot A confirmed, boot B without A2: no confirmed notifications, no hard-stop observation stored);
  - `uart0_feed_without_system_flushes_notifications`;
  - `session_end_uses_source_time` (HA clock 120 s behind and 120 s ahead: the event end equals the last `r44.t`, never before the start);
  - `auth_refusal_keeps_bus_source` (fake CONNACK refusal while `source = Bus` and a fake `conn`: `Err`, the source and `gen` are unchanged, and history keeps its session);
  - `guard_refuses_maint_halt_scan_firmware_group0_firmware_start` (source MQTT, `conn = None`, a stored control endpoint: each returns `Err("mqtt-source")`, and the fake opener counts 0 opens).
- [ ] **Step 2:** Run `cargo test mqtt_source::`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test`. Expected: all PASS.
- [ ] **Step 5:** Commit `"Home Assistant (MQTT) source"`.

### Task 5: UI — connection type, read-only, feed state, source time

**Files:**
- Modify: `app/src/views/Sidebar.tsx`:
  - connection kind `"mqtt"`, with a form for host, port (1883), user, password, TLS and base topic (default `seplos`);
  - connect and reconnect through `api.connectMqtt`;
  - the fixed connect deadline (`Sidebar.tsx:398–404`) waits for the first `pack` or `feed.state == "live"` instead of a bus answer.
- Modify: `app/src/store.ts`:
  - new state `source: "bus" | "mqtt" | null` and `feed`;
  - on `feed.boot_changed`, call `resetLivePairs()`;
  - device info and serials from `feed.packs` into the pack entries.
- Modify: `app/src/livePairs.ts`, `app/src/pairs.ts`: the window's "now" comes from `windowNow()`. That is `Date.now()` for the bus and `feed.source_now` advanced by local elapsed time for MQTT.
- Modify: `app/src/balancingHint.ts` / `PackDetail.tsx`: with source MQTT the balancing line uses only this connection's feed 0x47 block; without one the thresholds show as unknown. Stored blocks stay viewable elsewhere.
- Modify: views that start BMS I/O; each hides or disables its action via `canTalkToBms(source)`, with the hint "Not available over Home Assistant (MQTT) — stop the add-on and connect directly". At least:
  - `Parameters.tsx`: read and write;
  - `PackDetail.tsx`: its actions;
  - `Firmware.tsx`, `BmsHistory.tsx`, `MaintSetup.tsx`, `PackBusPanel.tsx`;
  - `Diagnosis.tsx`: its probes;
  - the calibration UI and the device search.
- Modify: `app/src/i18n.ts` and `tools/gen_help.py`. The help gets a topic "Home Assistant (MQTT)": what works, what doesn't, the add-on switch, the clock note. Then regenerate.
- Test: `app/src/*.test.ts`.

**Interfaces:**
- Consumes: `api.connectMqtt(settings, installation)`, `Connected.source`, the `"feed"` event (Task 4).
- Produces:
  - `canTalkToBms(source: Source): boolean` in `app/src/access.ts`;
  - `windowNow(): number` in `app/src/livePairs.ts`;
  - `feedText(feed): string | null` in `app/src/store.ts`.

- [ ] **Step 1: Write failing tests:**
  - `canTalkToBms`: `"bus"` → true, `"mqtt"` → false, `null` → false.
  - `feedText`: one case each for off, offline, waiting, skew and version, plus `null` when live.
  - Live pairs with the source clock 120 s behind the local clock: samples stay in the window.
  - Two reads 3.4 s apart arriving with different network delays (700 ms / 200 ms): they stay unpaired.
  - `boot_changed` clears the live pair samples.
  - Balancing with source MQTT: no feed block → thresholds unknown, even when a newer stored block exists.
- [ ] **Step 2:** Run `cd app && npx vitest run`. Expected: FAIL.
- [ ] **Step 3:** Implement. The status line under the connection:
  - off: „Datenstrom ist aus — im Seplos-BMS-Add-on einschalten“ / "Feed is off — turn it on in the Seplos BMS app";
  - offline: „Add-on offline“ / "Add-on offline";
  - waiting: „Warte auf Messwerte vom Add-on“ / "Waiting for measurements from the add-on";
  - skew: „Uhr des Home-Assistant-Hosts weicht um {{n}} s ab“ / "Clock of the Home Assistant host differs by {{n}} s".
- [ ] **Step 4:** Run `npx vitest run && npx tsc --noEmit && python3 tools/gen_help.py`. Expected: PASS, help regenerated.
- [ ] **Step 5:** Commit `"UI: Home Assistant (MQTT) connection, read-only, source time"`.

### Task 6: Changelog

- [ ] **Step 1:** Add a `CHANGELOG.md` entry under the next version.
- [ ] **Step 2:** Commit. The release follows only on the user's go.
