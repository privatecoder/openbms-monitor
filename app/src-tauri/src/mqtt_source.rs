//! The "Home Assistant (MQTT)" source: the Seplos BMS add-on's OpenBMS feed read from an MQTT broker instead of the
//! RS485 bus (`docs/exchange/openbms-feed-v1.md`, decoding in `feed.rs`).

use crate::feed::{self, Accept, FeedMeta, SourceState};
use crate::{bank, control, history, link_now, maint, new_link, notifications, profile, AppState, Connected, Link, PackUpdate, SourceKind, SystemUpdate};
use openbms_transport::Bus;
use rumqttc::{Client, Connection, Event, MqttOptions, Packet, QoS, Transport};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, State};
use openbms_proto::params::Parameters;
use openbms_proto::{DeviceInfo, SerialValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// The broker and the add-on's topic prefix (`mqtt_topic`, default `seplos`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttSettings {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub tls: bool,
    pub base: String,
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("mqtt.json"))
}

/// The broker an installation keeps (`maint_store` table `mqtt`): everything but the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub tls: bool,
    pub base: String,
}

impl BrokerConfig {
    pub fn of(s: &MqttSettings) -> Self {
        Self { host: s.host.clone(), port: s.port, user: s.user.clone(), tls: s.tls, base: s.base.clone() }
    }
    fn with_password(self, password: String) -> MqttSettings {
        MqttSettings { host: self.host, port: self.port, user: self.user, password, tls: self.tls, base: self.base }
    }
}

/// An installation's password, bound to the broker it was given for: after a change of host, port, TLS or user it is
/// never sent (a password of broker A must not go to broker B). The topic prefix is no part of the binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Bound {
    host: String,
    port: u16,
    tls: bool,
    user: String,
    password: String,
}

impl Bound {
    fn fits(&self, c: &BrokerConfig) -> bool {
        (self.host.as_str(), self.port, self.tls, self.user.as_str()) == (c.host.as_str(), c.port, c.tls, c.user.as_str())
    }
}

/// `mqtt.json` in the app data: the "view only" connection (no installation) and each installation's password. Never in
/// an installation export. Earlier versions kept one flat `MqttSettings`: it becomes the "view only" one.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Passwords {
    version: u32,
    view_only: Option<MqttSettings>,
    #[serde(default)]
    installations: std::collections::BTreeMap<String, Bound>,
}

/// One read-modify-write of `mqtt.json` at a time.
static FILE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_passwords(path: &Path) -> Result<Passwords, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Passwords { version: 2, ..Default::default() }),
        Err(e) => return Err(e.to_string()),
    };
    if let Ok(p) = serde_json::from_slice::<Passwords>(&bytes) {
        if p.version >= 2 {
            return Ok(p);
        }
    }
    match serde_json::from_slice::<MqttSettings>(&bytes) {
        Ok(old) => Ok(Passwords { version: 2, view_only: Some(old), installations: Default::default() }),
        // a damaged file is never overwritten silently
        Err(e) => Err(format!("mqtt.json is not readable: {e}")),
    }
}

/// The settings to show: an installation's broker (from its store row) with its password if bound to that broker,
/// else the "view only" one.
pub(crate) fn load_from(path: &Path, installation: Option<(&str, Option<BrokerConfig>)>) -> Option<MqttSettings> {
    let file = read_passwords(path).ok();
    match installation {
        None => file?.view_only,
        Some((id, config)) => {
            let config = config?;
            let password = file.and_then(|f| f.installations.get(id).filter(|b| b.fits(&config)).map(|b| b.password.clone())).unwrap_or_default();
            Some(config.with_password(password))
        }
    }
}

/// Keep `s` as the "view only" connection, or as `installation`'s password bound to its broker.
pub(crate) fn save_to(path: &Path, installation: Option<&str>, s: &MqttSettings) -> Result<(), String> {
    let _lock = FILE.lock().map_err(|e| e.to_string())?;
    let mut file = read_passwords(path)?;
    file.version = 2;
    match installation {
        None => file.view_only = Some(s.clone()),
        Some(id) => {
            file.installations.insert(id.to_owned(), Bound { host: s.host.clone(), port: s.port, tls: s.tls, user: s.user.clone(), password: s.password.clone() });
        }
    }
    write_file(path, &file)
}

/// An installation is gone: its password too.
pub(crate) fn forget_in(path: &Path, installation: &str) -> Result<(), String> {
    let _lock = FILE.lock().map_err(|e| e.to_string())?;
    let mut file = read_passwords(path)?;
    if file.installations.remove(installation).is_some() {
        write_file(path, &file)?;
    }
    Ok(())
}

/// Written readable by the user only: the file holds broker passwords.
fn write_file(path: &Path, file: &Passwords) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(file).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&text).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// The broker of `installation` (its stored row, its password if bound to that broker), or the "view only" one.
#[tauri::command]
pub fn load_mqtt_settings(app: AppHandle, state: State<'_, AppState>, installation: Option<String>) -> Option<MqttSettings> {
    let path = settings_path(&app).ok()?;
    match installation {
        None => load_from(&path, None),
        Some(id) => {
            let config = maint::with_store(&state, |s| s.mqtt(&id)).ok().flatten().and_then(|t| serde_json::from_str::<BrokerConfig>(&t).ok());
            load_from(&path, Some((&id, config)))
        }
    }
}

/// Keep the broker: for `installation` in its store row (no password) and its password locally; else as "view only".
#[tauri::command]
pub fn save_mqtt_settings(app: AppHandle, state: State<'_, AppState>, installation: Option<String>, settings: MqttSettings) -> Result<(), String> {
    if settings.host.trim().is_empty() || settings.port == 0 || settings.base.is_empty() || settings.base.contains(['+', '#']) {
        return Err("invalid broker settings".into());
    }
    let path = settings_path(&app)?;
    if let Some(id) = &installation {
        let config = serde_json::to_string(&BrokerConfig::of(&settings)).map_err(|e| e.to_string())?;
        maint::with_store(&state, |s| {
            s.installation(id)?;
            s.set_mqtt(id, Some(&config))
        })
        .map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    }
    save_to(&path, installation.as_deref(), &settings)
}

/// An installation was deleted: drop its password (a failure only leaves an unused entry).
pub(crate) fn forget(app: &AppHandle, installation: &str) {
    if let Ok(path) = settings_path(app) {
        let _ = forget_in(&path, installation);
    }
}

// ---- the source's state and what a message makes of it (pure) ------------------------------------------------------

/// What the UI shows of the feed (event "feed").
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FeedView {
    /// "waiting" (no measurement of this boot yet), "live", "off" (switched off in the add-on), "offline" (the add-on
    /// reports itself gone), "broker" (the connection to the broker failed; the client reconnects), "denied" (the broker
    /// refused the feed subscription)
    pub state: &'static str,
    /// the broker connection's error while state is "broker"
    pub link_error: Option<String>,
    pub boot: Option<String>,
    /// the add-on restarted: live samples and hysteresis start over
    pub boot_changed: bool,
    /// the add-on's clock minus the Monitor's at the latest message (ms)
    pub skew_ms: Option<i64>,
    /// "feed version 2 not supported" and the like
    pub version_error: Option<String>,
    pub packs: Vec<MetaPackView>,
}

/// Identity and parameters of one pack as the add-on last read them.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct MetaPackView {
    pub address: u8,
    pub device: Option<DeviceInfo>,
    pub battery_serial: Option<SerialValue>,
    pub bms_serial: Option<SerialValue>,
    pub parameters: Option<Parameters>,
    /// when the parameters were read (add-on clock, ms)
    pub parameters_at: Option<i64>,
}

#[derive(Debug)]
pub enum Effect {
    /// a pack answer: the UI update, and the row for the recorder (`read_at` = 0x42 time, `t` = status time)
    Pack { ui: PackUpdate, record_t: i64, record: Value },
    System { ui: SystemUpdate, t: i64 },
    Feed(FeedView),
    /// the add-on restarted: a new link, bank check, notifications and recording session
    NewBoot,
    /// the battery serial of address 0, for the installation's master check
    MasterSerial(SerialValue),
    RoundEnd,
}

/// How often the feed state goes to the UI while nothing about it changes (for the clock offset).
const FEED_EVERY_MS: i64 = 10_000;

#[derive(Default)]
pub struct Feeder {
    base: String,
    source: SourceState,
    on: bool,
    available: bool,
    measured: bool,
    master_sent: bool,
    version_error: Option<String>,
    link_error: Option<String>,
    /// the broker refused the feed subscription (ACL of the account)
    denied: bool,
    packs: BTreeMap<u8, MetaPackView>,
    skew: Option<i64>,
    sent_view: Option<FeedView>,
    sent_at_mono: i64,
}

impl Feeder {
    pub fn new(base: &str) -> Self {
        Self { base: base.trim_end_matches('/').to_owned(), available: true, ..Default::default() }
    }

    pub fn topics(&self) -> [String; 2] {
        [format!("{}/openbms/v1/#", self.base), format!("{}/availability", self.base)]
    }

    /// Both subscriptions in one request, so the broker's answer lists them in this order (`subscribed`).
    pub fn filters(&self) -> Vec<rumqttc::SubscribeFilter> {
        self.topics().into_iter().map(|t| rumqttc::SubscribeFilter::new(t, QoS::AtMostOnce)).collect()
    }

    fn view(&self, boot_changed: bool) -> FeedView {
        let state = if self.denied {
            "denied"
        } else if self.link_error.is_some() {
            "broker"
        } else if !self.available {
            "offline"
        } else if self.source.boot().is_some() && !self.on {
            "off"
        } else if self.measured {
            "live"
        } else {
            "waiting"
        };
        FeedView {
            state,
            boot: self.source.boot().map(str::to_owned),
            boot_changed,
            skew_ms: self.skew,
            version_error: self.version_error.clone(),
            link_error: self.link_error.clone(),
            packs: self.packs.values().cloned().collect(),
        }
    }

    /// The feed view when it changed (or every `FEED_EVERY_MS` for the clock offset, or on a boot change).
    fn feed_effect(&mut self, out: &mut Vec<Effect>, boot_changed: bool, mono_ms: i64, force: bool) {
        let v = self.view(boot_changed);
        let same = self.sent_view.as_ref().is_some_and(|s| FeedView { skew_ms: v.skew_ms, ..s.clone() } == v);
        if !force && !boot_changed && same && mono_ms - self.sent_at_mono < FEED_EVERY_MS {
            return;
        }
        self.sent_view = Some(FeedView { boot_changed: false, ..v.clone() });
        self.sent_at_mono = mono_ms;
        out.push(Effect::Feed(v));
    }

    fn params(&mut self, address: u8, part: &feed::Part<Vec<u8>>) -> bool {
        let Ok(block) = &part.value else { return false };
        let entry = self.packs.entry(address).or_insert_with(|| MetaPackView { address, ..Default::default() });
        if entry.parameters_at.is_some_and(|t| t >= part.t) {
            return false;
        }
        entry.parameters = Parameters::parse(block).ok();
        entry.parameters_at = Some(part.t);
        true
    }

    fn meta(&mut self, m: &FeedMeta, out: &mut Vec<Effect>) -> bool {
        let changed = self.source.meta(m);
        if changed {
            out.push(Effect::NewBoot);
            self.packs.clear();
            self.measured = false;
            self.master_sent = false;
        }
        self.on = m.on;
        for p in &m.packs {
            let entry = self.packs.entry(p.address).or_insert_with(|| MetaPackView { address: p.address, ..Default::default() });
            if let Some(d) = p.device.as_ref().and_then(|d| d.value.clone().ok()) {
                entry.device = Some(d);
            }
            if let Some(v) = p.battery_serial.as_ref().and_then(|d| d.value.clone().ok()) {
                entry.battery_serial = Some(v);
            }
            if let Some(v) = p.bms_serial.as_ref().and_then(|d| d.value.clone().ok()) {
                entry.bms_serial = Some(v);
            }
            if let Some(part) = &p.params {
                self.params(p.address, part);
            }
        }
        if !self.master_sent {
            if let Some(serial) = self.packs.get(&0).and_then(|p| p.battery_serial.clone()) {
                self.master_sent = true;
                out.push(Effect::MasterSerial(serial));
            }
        }
        changed
    }

    /// The broker connection failed (the client reconnects with its next poll): named until a message arrives again.
    pub fn link_lost(&mut self, error: &str, mono_ms: i64) -> Vec<Effect> {
        let mut out = Vec::new();
        self.link_error = Some(error.to_owned());
        // after the gap only new measurements count as live again
        self.measured = false;
        self.feed_effect(&mut out, false, mono_ms, false);
        out
    }

    /// The broker accepted the connection again (CONNACK): the broker error ends; what the add-on does shows with its
    /// next message (broker reachable is not add-on reachable).
    pub fn reconnected(&mut self, mono_ms: i64) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.link_error.take().is_some() {
            self.feed_effect(&mut out, false, mono_ms, true);
        }
        out
    }

    /// The broker's answer to the subscriptions, in `topics()` order: a refused feed subscription is named (nothing would
    /// ever arrive); a refused availability alone changes nothing.
    pub fn subscribed(&mut self, granted: &[bool], mono_ms: i64) -> Vec<Effect> {
        let mut out = Vec::new();
        let denied = granted.first() == Some(&false);
        // the add-on's availability cannot be read: an old "offline" says nothing any more
        if granted.get(1) == Some(&false) {
            self.available = true;
        }
        if denied != self.denied {
            self.denied = denied;
            self.feed_effect(&mut out, false, mono_ms, true);
        }
        out
    }

    /// One MQTT message: what it makes happen. `local_ms`: the Monitor's wall clock, `mono_ms`: its monotonic clock.
    pub fn handle(&mut self, topic: &str, payload: &[u8], local_ms: i64, mono_ms: i64) -> Vec<Effect> {
        let mut out = Vec::new();
        // a message arrived: the broker connection works again (the changed view goes out below)
        self.link_error = None;
        let rest = topic.strip_prefix(&self.base).unwrap_or(topic);
        if rest == "/availability" {
            self.available = payload != b"offline";
            self.feed_effect(&mut out, false, mono_ms, false);
            return out;
        }
        let Some(kind) = rest.strip_prefix("/openbms/v1/") else { return out };
        let Ok(v) = serde_json::from_slice::<Value>(payload) else { return out };
        if kind == "meta" {
            match feed::decode_meta(&v) {
                Ok(m) => {
                    self.version_error = None;
                    let changed = self.meta(&m, &mut out);
                    self.feed_effect(&mut out, changed, mono_ms, true);
                }
                Err(e) => {
                    self.version_error = Some(e);
                    self.feed_effect(&mut out, false, mono_ms, false);
                }
            }
            return out;
        }
        if let Some(address) = kind.strip_prefix("pack/") {
            let Ok(p) = feed::decode_pack(&v) else { return out };
            if address.parse::<u8>().ok() != Some(p.address) || self.source.accept(&p.boot, p.seq) != Accept::Yes {
                return out;
            }
            self.skew = Some(p.sent_at - local_ms);
            // live with the first measurement that decoded, not with a failure report
            self.measured |= p.telemetry.as_ref().is_some_and(|x| x.value.is_ok());
            let telemetry = p.telemetry.as_ref().and_then(|x| x.value.clone().ok());
            let status = p.status.as_ref().and_then(|x| x.value.clone().ok());
            let error = [p.telemetry.as_ref(), None].into_iter().flatten().find_map(|x| x.value.as_ref().err().cloned())
                .or_else(|| p.status.as_ref().and_then(|x| x.value.as_ref().err().cloned()))
                .or_else(|| p.status.is_none().then(|| "no status read".to_string()));
            let read_at = p.telemetry.as_ref().filter(|x| x.value.is_ok()).map(|x| x.t);
            let ui = PackUpdate { address: p.address, telemetry, status, error, read_at, sent_at: Some(p.sent_at) };
            let record_t = p.status.as_ref().map(|x| x.t).or(read_at).unwrap_or(p.sent_at);
            let record = serde_json::to_value(&ui).unwrap_or(Value::Null);
            let new_params = p.params.as_ref().is_some_and(|part| self.params(p.address, part));
            out.push(Effect::Pack { ui, record_t, record });
            out.push(Effect::RoundEnd);
            self.feed_effect(&mut out, false, mono_ms, new_params);
            return out;
        }
        if kind == "system" {
            let Ok(sys) = feed::decode_system(&v) else { return out };
            if self.source.accept(&sys.boot, sys.seq) != Accept::Yes {
                return out;
            }
            let Some(part) = sys.values else { return out };
            let ui = SystemUpdate { values: part.value.as_ref().ok().cloned(), error: part.value.err() };
            let first = ui.values.is_some() && !self.measured;
            out.push(Effect::System { ui, t: part.t });
            if first {
                self.measured = true;
                self.feed_effect(&mut out, false, mono_ms, true);
            }
        }
        out
    }
}

// ---- the running source ---------------------------------------------------------------------------------------------

/// The MQTT client of the current source and the flag that ends its thread.
pub struct Running {
    stop: Arc<AtomicBool>,
    client: Client,
}

/// End the running MQTT source, if any; true when there was one (its recording session ends at the add-on's clock).
pub(crate) fn stop(state: &AppState) -> bool {
    let running = state.mqtt.lock().ok().and_then(|mut m| m.take());
    let Some(r) = running else { return false };
    // waits for an effect being applied; the worker checks the flag under the same gate before each one
    let _gate = state.feed_gate.lock();
    r.stop.store(true, Ordering::SeqCst);
    let _ = r.client.disconnect();
    true
}

/// How long the broker gets to accept the connection.
const CONNACK_WAIT: Duration = Duration::from_secs(10);

/// The client options: the retained meta carries every pack's identity and parameter block, far above rumqttc's
/// default 10 KB limit (a larger message would end the connection again and again).
fn broker_options(s: &MqttSettings) -> MqttOptions {
    let id = format!("openbms-monitor-{:08x}", rand_id());
    let mut o = MqttOptions::new(id, s.host.clone(), s.port);
    o.set_keep_alive(Duration::from_secs(30));
    o.set_max_packet_size(MAX_PACKET, MAX_PACKET);
    if !s.user.is_empty() {
        o.set_credentials(s.user.clone(), s.password.clone());
    }
    o
}

/// The largest message taken from the broker (and sent: only subscriptions).
const MAX_PACKET: usize = 1024 * 1024;

/// Connect to the broker and wait for its acceptance; the broker's refusal (wrong password, unknown host) as text.
fn open_broker(s: &MqttSettings) -> Result<(Client, Connection), String> {
    let mut o = broker_options(s);
    o.set_clean_session(true);
    if s.tls {
        use rustls_platform_verifier::ConfigVerifierExt;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cfg = rustls::ClientConfig::with_platform_verifier().map_err(|e| e.to_string())?;
        o.set_transport(Transport::tls_with_config(rumqttc::TlsConfiguration::Rustls(Arc::new(cfg))));
    }
    let (client, mut conn) = Client::new(o, 64);
    let start = Instant::now();
    while start.elapsed() < CONNACK_WAIT {
        match conn.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(Event::Incoming(Packet::ConnAck(_)))) => return Ok((client, conn)),
            Ok(Ok(_)) | Err(_) => {}
            Ok(Err(e)) => return Err(e.to_string()),
        }
    }
    Err("the broker did not answer within 10 s".into())
}

fn rand_id() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    t ^ std::process::id()
}

/// The handover to the MQTT source, after the broker accepted: the old source ends, a new link (gen), bank check and
/// notifications as on the CAN socket, a recording session "mqtt". Returns the new gen.
pub(crate) fn take_over(state: &AppState, installation: Option<&str>) -> u64 {
    if let Ok(p) = state.polling.lock() {
        p.store(false, Ordering::SeqCst);
    }
    stop(state);
    if let Ok(mut c) = state.conn.lock() {
        c.take();
    }
    if let Ok(mut m) = state.main.lock() {
        *m = None;
    }
    if let Ok(mut h) = state.history.lock() {
        h.disconnected();
    }
    new_link(state, installation.map(str::to_owned));
    let gen = link_now(state).gen;
    bank::begin(state, gen, false, None);
    notifications::begin(state, gen, installation);
    if let (Some(id), Ok(mut h)) = (installation, state.history.lock()) {
        h.connected(id, "mqtt");
    }
    if let Ok(mut s) = state.source.lock() {
        *s = Some(SourceKind::Mqtt);
    }
    gen
}

/// The broker first, then the handover: a refused broker leaves the current source, link and recording as they are.
pub(crate) fn connect_with<B>(state: &AppState, installation: Option<&str>, open: impl FnOnce() -> Result<B, String>) -> Result<(B, u64), String> {
    let broker = open()?;
    Ok((broker, take_over(state, installation)))
}

/// Connect to the add-on's feed (read-only; nothing is sent to any BMS).
#[tauri::command]
pub async fn connect_mqtt(app: AppHandle, state: State<'_, AppState>, settings: MqttSettings, installation: Option<String>) -> Result<Connected, String> {
    let _port_slot = control::reserve_bus(&state).ok_or_else(|| crate::port_busy_error(crate::current_action(&state)))?;
    if state.flashing.load(Ordering::SeqCst) {
        return Err(crate::FLASHING.into());
    }
    let ids: Vec<&str> = installation.as_deref().into_iter().collect();
    let _inst = control::reserve_installations(&state.ops, &ids, "connect").map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    if let Some(id) = &installation {
        maint::with_store(&state, |s| s.installation(id)).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    }
    let s = settings.clone();
    let opened = tauri::async_runtime::spawn_blocking(move || open_broker(&s)).await.map_err(|e| e.to_string())?;
    let ((client, conn), gen) = connect_with(&state, installation.as_deref(), || opened)?;
    let feeder = Feeder::new(&settings.base);
    client.subscribe_many(feeder.filters()).map_err(|e| e.to_string())?;
    let stop_flag = Arc::new(AtomicBool::new(false));
    *state.mqtt.lock().map_err(|e| e.to_string())? = Some(Running { stop: stop_flag.clone(), client: client.clone() });
    let key = format!("mqtt:{}:{}", settings.host, settings.port);
    let profile = profile::of(&key, Bus::CanSocketBus, None);
    let thread_app = app.clone();
    std::thread::spawn(move || run(thread_app, client, conn, feeder, stop_flag, gen, installation));
    Ok(Connected { linked: false, answered: 0, gen, profile: profile::view(gen, &profile), source: SourceKind::Mqtt })
}

/// The receive loop: each message through `Feeder::handle`, its effects applied here. Ends on stop, on a new link
/// (another connection, disconnect) or when the app closes.
fn run(app: AppHandle, client: Client, mut conn: Connection, mut feeder: Feeder, stop_flag: Arc<AtomicBool>, mut gen: u64, installation: Option<String>) {
    let state = app.state::<AppState>();
    let start = Instant::now();
    while !stop_flag.load(Ordering::SeqCst) && link_now(&state).gen == gen {
        match conn.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(Event::Incoming(Packet::Publish(p)))) => {
                let mono = start.elapsed().as_millis() as i64;
                for e in feeder.handle(&p.topic, &p.payload, history::now_ms(), mono) {
                    if !apply_gated(&app, &state, &stop_flag, e, &mut gen, installation.as_deref()) {
                        return;
                    }
                }
            }
            // a reconnect starts a clean session: subscribe again
            Ok(Ok(Event::Incoming(Packet::ConnAck(_)))) => {
                let _ = client.subscribe_many(feeder.filters());
                let mono = start.elapsed().as_millis() as i64;
                for e in feeder.reconnected(mono) {
                    if !apply_gated(&app, &state, &stop_flag, e, &mut gen, installation.as_deref()) {
                        return;
                    }
                }
            }
            Ok(Ok(Event::Incoming(Packet::SubAck(ack)))) => {
                let granted: Vec<bool> = ack.return_codes.iter().map(|c| !matches!(c, rumqttc::SubscribeReasonCode::Failure)).collect();
                let mono = start.elapsed().as_millis() as i64;
                for e in feeder.subscribed(&granted, mono) {
                    if !apply_gated(&app, &state, &stop_flag, e, &mut gen, installation.as_deref()) {
                        return;
                    }
                }
            }
            Ok(Ok(_)) | Err(_) => {}
            // the broker is gone for now: rumqttc reconnects with the next poll
            Ok(Err(err)) => {
                let mono = start.elapsed().as_millis() as i64;
                for e in feeder.link_lost(&err.to_string(), mono) {
                    if !apply_gated(&app, &state, &stop_flag, e, &mut gen, installation.as_deref()) {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

/// Apply one effect while this source is still the current one, checked and done under `feed_gate`; false: it is not.
fn apply_gated(app: &AppHandle, state: &AppState, stop_flag: &AtomicBool, e: Effect, gen: &mut u64, installation: Option<&str>) -> bool {
    let _gate = state.feed_gate.lock();
    if !current(state, stop_flag, *gen) {
        return false;
    }
    apply(app, state, e, gen, installation);
    true
}

/// This source is still the one: not stopped, and no other connection took the link since.
fn current(state: &AppState, stop_flag: &AtomicBool, gen: u64) -> bool {
    !stop_flag.load(Ordering::SeqCst) && link_now(state).gen == gen
}

fn apply(app: &AppHandle, state: &AppState, e: Effect, gen: &mut u64, installation: Option<&str>) {
    match e {
        Effect::Pack { ui, record_t, record } => {
            if ui.telemetry.is_some() {
                let _ = app.emit("answer", ());
            }
            notifications::answer(state, *gen, ui.address, ui.telemetry.as_ref(), ui.status.as_ref());
            if let Some(st) = ui.status.as_ref() {
                profile::observe_hard_stop(state, &Link { gen: *gen, installation: installation.map(str::to_owned) }, ui.address, st);
            }
            if let Ok(mut rec) = state.history.lock() {
                rec.pack(record_t, ui.address, &record);
            }
            let _ = app.emit("pack", ui);
        }
        Effect::System { ui, t } => {
            if let (Ok(mut rec), Ok(v)) = (state.history.lock(), serde_json::to_value(&ui)) {
                rec.system(t, &v);
            }
            let _ = app.emit("system", ui);
        }
        Effect::Feed(v) => {
            let _ = app.emit("feed", v);
        }
        Effect::NewBoot => {
            // the add-on restarted: like a reconnect, before any answer of the new boot
            if let Ok(mut h) = state.history.lock() {
                h.disconnected();
            }
            new_link(state, installation.map(str::to_owned));
            *gen = link_now(state).gen;
            bank::begin(state, *gen, false, None);
            notifications::begin(state, *gen, installation);
            if let (Some(id), Ok(mut h)) = (installation, state.history.lock()) {
                h.connected(id, "mqtt");
            }
        }
        Effect::MasterSerial(serial) => {
            if let Some(inst) = installation {
                crate::can_master_check(state, *gen, inst, &serial);
            }
        }
        Effect::RoundEnd => notifications::round_end(app, state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MqttSettings {
        MqttSettings { host: "broker.local".into(), port: 1883, user: "u".into(), password: "p".into(), tls: false, base: "seplos".into() }
    }

    #[test]
    fn passwords_per_installation_bound_to_their_broker_and_mode_0600() {
        let dir = std::env::temp_dir().join(format!("openbms-mqtt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mqtt.json");
        assert_eq!(load_from(&path, None), None);
        // an earlier flat file becomes the "view only" connection, never an installation's
        std::fs::write(&path, serde_json::to_vec(&sample()).unwrap()).unwrap();
        assert_eq!(load_from(&path, None), Some(sample()));
        assert_eq!(load_from(&path, Some(("a", Some(BrokerConfig::of(&sample()))))).unwrap().password, "");
        save_to(&path, Some("a"), &sample()).unwrap();
        assert_eq!(load_from(&path, Some(("a", Some(BrokerConfig::of(&sample()))))), Some(sample()));
        assert_eq!(load_from(&path, None), Some(sample()), "the view-only one stays");
        // another broker (or user) under the same installation never gets the password
        let moved = BrokerConfig { host: "other.example".into(), ..BrokerConfig::of(&sample()) };
        assert_eq!(load_from(&path, Some(("a", Some(moved)))).unwrap().password, "");
        // a new topic prefix keeps it
        let base = BrokerConfig { base: "bank2".into(), ..BrokerConfig::of(&sample()) };
        assert_eq!(load_from(&path, Some(("a", Some(base)))).unwrap().password, "p");
        assert_eq!(load_from(&path, Some(("a", None))), None, "no broker stored for it");
        forget_in(&path, "a").unwrap();
        assert_eq!(load_from(&path, Some(("a", Some(BrokerConfig::of(&sample()))))).unwrap().password, "");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // a damaged file is refused, not overwritten
        std::fs::write(&path, b"{not json").unwrap();
        assert!(save_to(&path, None, &sample()).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ---- Feeder ----

    use serde_json::json;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn telemetry_info() -> Vec<u8> {
        let mut i = vec![0x00, 0x00, 16];
        i.extend(std::iter::repeat_n([0x0D, 0x05], 16).flatten());
        i.push(6);
        i.extend(std::iter::repeat_n([0x0B, 0x27], 6).flatten());
        i.extend([0x00, 0x00, 0x14, 0xD6, 0x56, 0x9A, 0x0A, 0x6D, 0x60, 0x03, 0x17, 0x6D, 0x60, 0x00, 0x07, 0x03, 0xE8, 0x14, 0xD9]);
        i.extend(std::iter::repeat_n(0u8, 8));
        i
    }

    fn status_info() -> Vec<u8> {
        let mut i = vec![0x00, 0x00, 16];
        i.extend(std::iter::repeat_n(0u8, 16));
        i.push(6);
        i.extend(std::iter::repeat_n(0u8, 8));
        i.push(20);
        i.extend(std::iter::repeat_n(0u8, 20));
        i
    }

    fn meta(boot: &str, on: bool, packs: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"v": 1, "on": on, "auto_off": null, "boot": boot, "sent_at": 1, "bus": "uart1", "addon": "x", "packs": packs, "sets": []})).unwrap()
    }

    fn pack_msg(boot: &str, seq: u64, addr: u8, t42: i64, t44: i64) -> Vec<u8> {
        serde_json::to_vec(&json!({"v": 1, "boot": boot, "seq": seq, "sent_at": t44 + 10, "addr": addr,
            "r42": {"t": t42, "hex": hex(&telemetry_info())}, "r44": {"t": t44, "hex": hex(&status_info())}})).unwrap()
    }

    fn feeds(e: &[Effect]) -> Vec<&FeedView> {
        e.iter().filter_map(|x| if let Effect::Feed(v) = x { Some(v) } else { None }).collect()
    }

    #[test]
    fn pack_message_records_source_times() {
        let mut f = Feeder::new("seplos");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        let e = f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 1, 0, 4_000, 9_000), 500_000, 10);
        let Effect::Pack { ui, record_t, record } = &e[0] else { panic!("{e:?}") };
        assert_eq!((ui.read_at, *record_t, record["read_at"].as_i64()), (Some(4_000), 9_000, Some(4_000)));
        assert!(ui.telemetry.is_some() && ui.status.is_some() && ui.error.is_none());
        assert!(e.iter().any(|x| matches!(x, Effect::RoundEnd)));
        assert_eq!(feeds(&e)[0].state, "live");
        assert_eq!(feeds(&e)[0].skew_ms, Some(9_010 - 500_000));
    }

    #[test]
    fn duplicate_and_other_boot_ignored() {
        let mut f = Feeder::new("seplos");
        assert!(f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 1, 0, 1, 2), 0, 0).is_empty(), "no meta yet");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        assert!(!f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 1, 0, 1, 2), 0, 0).is_empty());
        assert!(f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 1, 0, 1, 2), 0, 0).is_empty());
        assert!(f.handle("seplos/openbms/v1/pack/1", &pack_msg("b0", 9, 1, 1, 2), 0, 0).is_empty());
        assert!(f.handle("seplos/openbms/v1/pack/1", &pack_msg("b1", 2, 0, 1, 2), 0, 0).is_empty(), "topic and addr differ");
    }

    #[test]
    fn boot_change_starts_new_session() {
        let mut f = Feeder::new("seplos");
        let first = f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        assert!(!first.iter().any(|x| matches!(x, Effect::NewBoot)));
        f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 7, 0, 1, 2), 0, 0);
        let e = f.handle("seplos/openbms/v1/meta", &meta("b2", true, json!([])), 0, 0);
        assert!(matches!(e[0], Effect::NewBoot));
        let v = feeds(&e)[0];
        assert!(v.boot_changed && v.state == "waiting");
        assert!(!f.handle("seplos/openbms/v1/pack/0", &pack_msg("b2", 1, 0, 1, 2), 0, 0).is_empty());
    }

    #[test]
    fn availability_offline_sets_feed_offline() {
        let mut f = Feeder::new("seplos");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        let e = f.handle("seplos/availability", b"offline", 0, 1);
        assert_eq!(feeds(&e)[0].state, "offline");
        let e = f.handle("seplos/availability", b"online", 0, 2);
        assert_eq!(feeds(&e)[0].state, "waiting");
    }

    #[test]
    fn meta_on_without_packs_is_waiting_and_off_is_off() {
        let mut f = Feeder::new("seplos");
        assert_eq!(feeds(&f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0))[0].state, "waiting");
        assert_eq!(feeds(&f.handle("seplos/openbms/v1/meta", &meta("b1", false, json!([])), 0, 0))[0].state, "off");
    }

    #[test]
    fn master_serial_from_meta_once_per_boot() {
        let mut serial = b"BAT-1".to_vec();
        serial.resize(30, 0);
        let packs = json!([{"addr": 0, "a2": {"t": 1, "hex": hex(&serial)}}]);
        let mut f = Feeder::new("seplos");
        let e = f.handle("seplos/openbms/v1/meta", &meta("b1", true, packs.clone()), 0, 0);
        assert!(e.iter().any(|x| matches!(x, Effect::MasterSerial(SerialValue::Set { .. }))));
        let e = f.handle("seplos/openbms/v1/meta", &meta("b1", true, packs.clone()), 0, 0);
        assert!(!e.iter().any(|x| matches!(x, Effect::MasterSerial(_))));
        let e = f.handle("seplos/openbms/v1/meta", &meta("b2", true, packs), 0, 0);
        assert!(e.iter().any(|x| matches!(x, Effect::MasterSerial(_))), "checked again for a new boot");
    }

    #[test]
    fn wrong_version_shows_an_error() {
        let mut f = Feeder::new("seplos");
        let mut m: Value = serde_json::from_slice(&meta("b1", true, json!([]))).unwrap();
        m["v"] = json!(2);
        let e = f.handle("seplos/openbms/v1/meta", &serde_json::to_vec(&m).unwrap(), 0, 0);
        assert_eq!(feeds(&e)[0].version_error.as_deref(), Some("feed version 2 not supported"));
    }

    #[test]
    fn system_message_uses_r1000_time() {
        let mut f = Feeder::new("seplos");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        let regs: Vec<u8> = (0..21u16).flat_map(|r| r.to_be_bytes()).collect();
        let msg = serde_json::to_vec(&json!({"v": 1, "boot": "b1", "seq": 2, "sent_at": 9, "r1000": {"t": 7, "hex": hex(&regs)}})).unwrap();
        let e = f.handle("seplos/openbms/v1/system", &msg, 0, 0);
        let Effect::System { ui, t } = &e[0] else { panic!() };
        assert_eq!(*t, 7);
        assert!(ui.values.is_some());
    }

    // ---- source switch and guard ----

    #[test]
    fn auth_refusal_keeps_the_current_source() {
        let state = AppState::default();
        *state.source.lock().unwrap() = Some(SourceKind::Bus);
        new_link(&state, Some("inst-a".into()));
        let gen = link_now(&state).gen;
        let r = connect_with(&state, Some("inst-a"), || Err::<(), _>("Connection refused, return code: `BadUserNamePassword`".to_string()));
        assert!(r.unwrap_err().contains("BadUserNamePassword"));
        assert_eq!(*state.source.lock().unwrap(), Some(SourceKind::Bus));
        assert_eq!(link_now(&state).gen, gen);
    }

    #[test]
    fn a_broker_that_accepts_takes_over() {
        let state = AppState::default();
        *state.source.lock().unwrap() = Some(SourceKind::Bus);
        let before = link_now(&state).gen;
        let (_, gen) = connect_with(&state, None, || Ok(())).unwrap();
        assert!(gen > before);
        assert_eq!(*state.source.lock().unwrap(), Some(SourceKind::Mqtt));
    }

    #[test]
    fn the_guard_refuses_bms_io_while_the_source_is_mqtt() {
        let state = AppState::default();
        assert!(crate::bms_io_allowed(&state).is_ok());
        *state.source.lock().unwrap() = Some(SourceKind::Mqtt);
        assert_eq!(crate::bms_io_allowed(&state).unwrap_err(), crate::MQTT_SOURCE);
        *state.source.lock().unwrap() = Some(SourceKind::Bus);
        assert!(crate::bms_io_allowed(&state).is_ok());
    }

    #[test]
    fn only_a_decoded_measurement_makes_the_feed_live() {
        let mut f = Feeder::new("seplos");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        let failed = serde_json::to_vec(&json!({"v": 1, "boot": "b1", "seq": 1, "sent_at": 5, "addr": 0, "r42": {"t": 4, "err": "failed"}})).unwrap();
        let e = f.handle("seplos/openbms/v1/pack/0", &failed, 0, 20_000);
        assert!(feeds(&e).iter().all(|v| v.state == "waiting"), "{e:?}");
        assert!(matches!(e[0], Effect::Pack { .. }), "the failure still reaches the UI");
        let e = f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 2, 0, 1, 2), 0, 40_000);
        assert_eq!(feeds(&e)[0].state, "live");
    }

    #[test]
    fn after_stop_no_effect_of_the_old_source_applies() {
        let state = AppState::default();
        let (_, gen) = connect_with(&state, None, || Ok(())).unwrap();
        let (client, _conn) = Client::new(MqttOptions::new("t", "127.0.0.1", 1), 1);
        let flag = Arc::new(AtomicBool::new(false));
        *state.mqtt.lock().unwrap() = Some(Running { stop: flag.clone(), client });
        assert!(current(&state, &flag, gen));
        // a new connection: stop first (under the gate), then the new link
        assert!(stop(&state));
        new_link(&state, Some("inst-b".into()));
        assert!(!current(&state, &flag, gen));
        assert!(!current(&state, &flag, link_now(&state).gen), "stopped stays stopped");
    }

    #[test]
    fn a_broker_error_is_named_and_cleared_by_the_next_message() {
        let mut f = Feeder::new("seplos");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        let e = f.link_lost("payload size limit exceeded", 1);
        let v = feeds(&e)[0];
        assert_eq!((v.state, v.link_error.as_deref()), ("broker", Some("payload size limit exceeded")));
        let e = f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 1, 0, 1, 2), 0, 2);
        let v = feeds(&e)[0];
        assert_eq!((v.state, v.link_error.as_deref()), ("live", None));
    }

    #[test]
    fn the_broker_takes_a_meta_far_above_10_kb() {
        let o = broker_options(&MqttSettings { host: "h".into(), port: 1883, user: String::new(), password: String::new(), tls: false, base: "seplos".into() });
        assert!(o.max_packet_size() >= 1024 * 1024);
    }

    #[test]
    fn a_reconnect_without_messages_clears_the_broker_error() {
        let mut f = Feeder::new("seplos");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        f.link_lost("connection reset", 1);
        let e = f.reconnected(2);
        assert_eq!((feeds(&e)[0].state, feeds(&e)[0].link_error.as_deref()), ("waiting", None));
    }

    #[test]
    fn a_reconnect_after_measurements_waits_for_new_ones() {
        let mut f = Feeder::new("seplos");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 1, 0, 1, 2), 0, 1);
        f.link_lost("connection reset", 2);
        assert_eq!(feeds(&f.reconnected(3))[0].state, "waiting");
    }

    #[test]
    fn a_refused_feed_subscription_is_named() {
        let mut f = Feeder::new("seplos");
        let e = f.subscribed(&[false, true], 1);
        assert_eq!(feeds(&e)[0].state, "denied");
        // only the availability refused: measurements still count
        let mut f = Feeder::new("seplos");
        assert!(f.subscribed(&[true, false], 1).is_empty());
    }

    #[test]
    fn a_refused_availability_drops_an_old_offline() {
        let mut f = Feeder::new("seplos");
        f.handle("seplos/openbms/v1/meta", &meta("b1", true, json!([])), 0, 0);
        f.handle("seplos/availability", b"offline", 0, 1);
        f.subscribed(&[true, false], 2);
        let e = f.handle("seplos/openbms/v1/pack/0", &pack_msg("b1", 1, 0, 1, 2), 0, 3);
        assert_eq!(feeds(&e)[0].state, "live");
    }
}
