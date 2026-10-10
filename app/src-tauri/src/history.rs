//! History database (SQLite, `history.sqlite` in the app data folder).
//!
//! Recording is off by default; once switched on it writes while the app is connected. Values are stored
//! as scaled integers (mA, ‰, mV, 0.1 °C), cells and temperatures of a pack as small blobs. Rows older than
//! `keep_full_days` are merged into one row per minute (`n` = answers merged); minute rows older than
//! `keep_full_days + keep_minutes_days` are deleted (0 = kept forever). Alarms are stored as events with
//! start and end. Old JSON Lines recordings can be imported; any span can be exported as JSON Lines or CSV.
//!
//! No migrations before the first release: a database with another schema version is moved aside
//! (`history.sqlite.v<old>-<time>`) and a new one is created.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i32 = 1;
const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;
/// Source id of the master's system values in the interval bookkeeping.
const SYSTEM: u8 = 255;

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- one row per connection (or import); first/last: time span of its data
CREATE TABLE sessions(id INTEGER PRIMARY KEY, site TEXT NOT NULL, bus TEXT NOT NULL, source TEXT NOT NULL,
  first INTEGER, last INTEGER);
CREATE TABLE site_packs(site TEXT NOT NULL, pack INTEGER NOT NULL, PRIMARY KEY(site, pack)) WITHOUT ROWID;
CREATE TABLE pack(pack INTEGER NOT NULL, t INTEGER NOT NULL, session INTEGER NOT NULL, n INTEGER NOT NULL,
  current INTEGER, soc INTEGER, voltage INTEGER, remaining INTEGER, cycles INTEGER,
  cell_max INTEGER, cell_min INTEGER, temp_max INTEGER, temp_min INTEGER, ambient INTEGER, mosfet INTEGER,
  cells BLOB, temps BLOB, PRIMARY KEY(pack, t)) WITHOUT ROWID;
CREATE INDEX pack_t ON pack(t);
CREATE TABLE system(t INTEGER PRIMARY KEY, session INTEGER NOT NULL, n INTEGER NOT NULL,
  current INTEGER, soc INTEGER, voltage INTEGER, ccl INTEGER, dcl INTEGER, cvl INTEGER,
  cell_max INTEGER, cell_min INTEGER) WITHOUT ROWID;
CREATE TABLE events(id INTEGER PRIMARY KEY, session INTEGER NOT NULL, pack INTEGER NOT NULL, key TEXT NOT NULL,
  severity TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER);
CREATE INDEX events_start ON events(start);
";

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Record while connected. Off by default.
    pub enabled: bool,
    /// Pack addresses to record; empty = every polled pack.
    pub packs: Vec<u8>,
    /// Also record the master's Modbus system values (CAN socket bus only).
    pub system: bool,
    /// Minimum seconds between two rows of the same pack (0 = every poll).
    pub interval_s: u64,
    /// Days every answer is kept; older ones are merged into minute values.
    pub keep_full_days: u32,
    /// Days minute values are kept after that; 0 = forever.
    pub keep_minutes_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, packs: vec![], system: true, interval_s: 0, keep_full_days: 30, keep_minutes_days: 0 }
    }
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub settings: Settings,
    /// recording right now (enabled and connected)
    pub active: bool,
    pub path: Option<String>,
    pub bytes: u64,
    /// rows written since the app started
    pub rows: u64,
    pub error: Option<String>,
}

/// The recording side: one write connection, used by the polling thread.
#[derive(Default)]
pub struct History {
    db: Option<Connection>,
    path: Option<PathBuf>,
    settings: Settings,
    /// site and bus of the current connection, if any
    link: Option<(String, String)>,
    session: Option<i64>,
    last: HashMap<u8, i64>,
    open_events: OpenEvents,
    last_maintain: i64,
    rows: u64,
    error: Option<String>,
}

pub type SharedHistory = Arc<Mutex<History>>;

/// Alarms currently active per pack: alarm id (help id, e.g. alarm.ev2.b1) → event row id.
type OpenEvents = HashMap<u8, HashMap<String, i64>>;

/// Open (or create) a database at `path` with the current schema.
fn open_db(path: &Path) -> Result<Connection, String> {
    let e = |e: rusqlite::Error| format!("{}: {e}", path.display());
    if path.exists() {
        let version: i32 = Connection::open(path).and_then(|c| c.pragma_query_value(None, "user_version", |r| r.get(0))).map_err(e)?;
        if version != SCHEMA_VERSION {
            // before the first release there are no migrations: keep the old file aside
            let aside = path.with_extension(format!("sqlite.v{version}-{}", now_ms()));
            std::fs::rename(path, &aside).map_err(|err| format!("{}: {err}", path.display()))?;
            for ext in ["sqlite-wal", "sqlite-shm"] {
                let _ = std::fs::remove_file(path.with_extension(ext));
            }
        }
    }
    let fresh = !path.exists();
    let c = Connection::open(path).map_err(e)?;
    if fresh {
        // auto_vacuum must be set before the first table
        c.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;").map_err(e)?;
        c.execute_batch(SCHEMA).map_err(e)?;
        c.pragma_update(None, "user_version", SCHEMA_VERSION).map_err(e)?;
    }
    c.pragma_update(None, "journal_mode", "WAL").map_err(e)?;
    c.pragma_update(None, "synchronous", "NORMAL").map_err(e)?;
    c.busy_timeout(std::time::Duration::from_secs(5)).map_err(e)?;
    Ok(c)
}

/// A read connection for queries and exports, so they do not block the recording.
pub fn reader(path: &Path) -> Result<Connection, String> {
    let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    c.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
    Ok(c)
}

impl History {
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let db = open_db(path)?;
        self.settings = db
            .query_row("SELECT value FROM meta WHERE key = 'settings'", [], |r| r.get::<_, String>(0))
            .optional()
            .map_err(|e| e.to_string())?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // events left open by a crash end where their session's data ends
        db.execute("UPDATE events SET end = (SELECT last FROM sessions WHERE id = events.session) WHERE end IS NULL", [])
            .map_err(|e| e.to_string())?;
        self.db = Some(db);
        self.path = Some(path.to_path_buf());
        self.maintain(now_ms());
        Ok(())
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    pub fn status(&self) -> Status {
        let bytes = self.path.as_ref().map_or(0, |p| {
            ["sqlite", "sqlite-wal"].iter().map(|ext| std::fs::metadata(p.with_extension(ext)).map_or(0, |m| m.len())).sum()
        });
        Status {
            settings: self.settings.clone(),
            active: self.settings.enabled && self.link.is_some() && self.db.is_some(),
            path: self.path.as_ref().map(|p| p.display().to_string()),
            bytes,
            rows: self.rows,
            error: self.error.clone(),
        }
    }

    pub fn set_settings(&mut self, s: Settings) -> Result<Status, String> {
        let db = self.db.as_ref().ok_or("history database not open")?;
        db.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('settings', ?1)", [serde_json::to_string(&s).map_err(|e| e.to_string())?])
            .map_err(|e| e.to_string())?;
        let retention = s.keep_full_days != self.settings.keep_full_days || s.keep_minutes_days != self.settings.keep_minutes_days;
        if !s.enabled {
            self.end_session(now_ms());
        }
        self.settings = s;
        if retention {
            self.maintain(now_ms());
        }
        Ok(self.status())
    }

    /// A connection is up: rows from now on belong to a new session (created with its first row).
    pub fn connected(&mut self, site: &str, bus: &str) {
        self.end_session(now_ms());
        self.link = Some((site.to_string(), bus.to_string()));
    }

    pub fn disconnected(&mut self) {
        self.end_session(now_ms());
        self.link = None;
    }

    fn end_session(&mut self, t: i64) {
        if let (Some(db), Some(_)) = (self.db.as_ref(), self.session) {
            close_events(db, &mut self.open_events, t);
        }
        self.open_events.clear();
        self.session = None;
        self.last.clear();
    }

    /// The session rows go to, if recording; created with the first row.
    fn session(&mut self) -> Option<i64> {
        if !self.settings.enabled {
            return None;
        }
        if self.session.is_none() {
            let (site, bus) = self.link.clone()?;
            let db = self.db.as_ref()?;
            match db.execute("INSERT INTO sessions(site, bus, source) VALUES(?1, ?2, 'live')", params![site, bus]) {
                Ok(_) => self.session = Some(db.last_insert_rowid()),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        self.session
    }

    /// A pack answer (PackUpdate as JSON). Events are tracked on every answer, values per the interval.
    pub fn pack(&mut self, t: i64, address: u8, update: &Value) {
        if !self.settings.packs.is_empty() && !self.settings.packs.contains(&address) {
            return;
        }
        let Some(session) = self.session() else { return };
        let due = self.due(address, t);
        let site = self.link.as_ref().map(|l| l.0.clone()).unwrap_or_default();
        let Some(db) = self.db.as_ref() else { return };
        let r = (|| -> rusqlite::Result<bool> {
            track_events(db, &mut self.open_events, session, t, address, update)?;
            if !due {
                return Ok(false);
            }
            let wrote = insert_pack(db, session, t, address, update, false)?;
            if wrote {
                touch(db, session, &site, address, t)?;
            }
            Ok(wrote)
        })();
        self.after_write(r, t);
    }

    /// The master's system values (SystemUpdate as JSON).
    pub fn system(&mut self, t: i64, update: &Value) {
        if !self.settings.system {
            return;
        }
        let Some(session) = self.session() else { return };
        if !self.due(SYSTEM, t) {
            return;
        }
        let Some(db) = self.db.as_ref() else { return };
        let r = insert_system(db, session, t, update, false).and_then(|wrote| {
            if wrote {
                db.execute("UPDATE sessions SET first = COALESCE(first, ?2), last = ?2 WHERE id = ?1", params![session, t])?;
            }
            Ok(wrote)
        });
        self.after_write(r, t);
    }

    fn after_write(&mut self, r: rusqlite::Result<bool>, t: i64) {
        match r {
            Ok(true) => {
                self.rows += 1;
                self.error = None;
            }
            Ok(false) => {}
            Err(e) => self.error = Some(e.to_string()),
        }
        if t - self.last_maintain > 3_600_000 {
            self.maintain(t);
        }
    }

    fn due(&mut self, source: u8, t: i64) -> bool {
        let interval = self.settings.interval_s as i64 * 1000;
        match self.last.get(&source) {
            Some(&last) if interval > 0 && t - last < interval => false,
            _ => {
                self.last.insert(source, t);
                true
            }
        }
    }

    /// Merge old rows into minute values and delete what is past the retention.
    pub fn maintain(&mut self, now: i64) {
        self.last_maintain = now;
        let Some(db) = self.db.as_ref() else { return };
        if let Err(e) = maintain(db, &self.settings, now) {
            self.error = Some(format!("maintenance: {e}"));
        }
    }

    /// Delete all data of one installation, or everything (site None).
    pub fn clear(&mut self, site: Option<&str>) -> Result<(), String> {
        self.end_session(now_ms());
        let db = self.db.as_ref().ok_or("history database not open")?;
        let e = |e: rusqlite::Error| e.to_string();
        match site {
            Some(s) => {
                let ids = "(SELECT id FROM sessions WHERE site = ?1)";
                for table in ["pack", "system", "events"] {
                    db.execute(&format!("DELETE FROM {table} WHERE session IN {ids}"), [s]).map_err(e)?;
                }
                db.execute("DELETE FROM site_packs WHERE site = ?1", [s]).map_err(e)?;
                db.execute("DELETE FROM sessions WHERE site = ?1", [s]).map_err(e)?;
            }
            None => db.execute_batch("DELETE FROM pack; DELETE FROM system; DELETE FROM events; DELETE FROM site_packs; DELETE FROM sessions;").map_err(e)?,
        }
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;").map_err(e)
    }

    /// Import a JSON Lines recording (the format of earlier versions and of the export) as its own session.
    pub fn import(&mut self, file: &Path, site: &str) -> Result<ImportStats, String> {
        let db = self.db.as_mut().ok_or("history database not open")?;
        import(db, file, site)
    }
}

fn e2s(e: rusqlite::Error) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------------------------
// writing

fn scaled(v: &Value, factor: f64) -> Option<i64> {
    v.as_f64().map(|x| (x * factor).round() as i64)
}

fn blob_u16(vals: &[i64]) -> Vec<u8> {
    vals.iter().flat_map(|&v| (v.clamp(0, u16::MAX as i64) as u16).to_le_bytes()).collect()
}
fn blob_i16(vals: &[i64]) -> Vec<u8> {
    vals.iter().flat_map(|&v| (v.clamp(i16::MIN as i64, i16::MAX as i64) as i16).to_le_bytes()).collect()
}
fn unblob_u16(b: &[u8]) -> Vec<i64> {
    b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as i64).collect()
}
fn unblob_i16(b: &[u8]) -> Vec<i64> {
    b.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as i64).collect()
}

/// One pack row from a PackUpdate; false if the answer carries no telemetry.
/// `keep_existing`: an import never overwrites what is already there (same pack and time).
fn insert_pack(db: &Connection, session: i64, t: i64, address: u8, update: &Value, keep_existing: bool) -> rusqlite::Result<bool> {
    let tm = &update["telemetry"];
    if !tm.is_object() {
        return Ok(false);
    }
    let cells: Vec<i64> = tm["cell_voltages"].as_array().map_or(vec![], |a| a.iter().filter_map(|v| scaled(v, 1000.0)).collect());
    let temps: Vec<i64> = tm["cell_temperatures"].as_array().map_or(vec![], |a| a.iter().filter_map(|v| scaled(v, 10.0)).collect());
    // like the live view: the 1 mA idle current where the regular value reads 0
    let current = match scaled(&tm["current"], 1000.0) {
        Some(0) | None => tm["idle_current_ma"].as_i64().or(Some(0)),
        c => c,
    };
    let changed = db
        .prepare_cached(&format!(
            "INSERT OR {} INTO pack(pack, t, session, n, current, soc, voltage, remaining, cycles, cell_max, cell_min,
               temp_max, temp_min, ambient, mosfet, cells, temps) VALUES(?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            if keep_existing { "IGNORE" } else { "REPLACE" }
        ))?
        .execute(params![
        address,
        t,
        session,
        current,
        scaled(&tm["soc"], 10.0),
        scaled(&tm["pack_voltage"], 100.0),
        scaled(&tm["remaining_capacity_ah"], 100.0),
        tm["cycles"].as_i64(),
        cells.iter().max(),
        cells.iter().min(),
        temps.iter().max(),
        temps.iter().min(),
        scaled(&tm["ambient_temperature"], 10.0),
        scaled(&tm["power_temperature"], 10.0),
        blob_u16(&cells),
        blob_i16(&temps),
    ])?;
    Ok(changed > 0)
}

fn insert_system(db: &Connection, session: i64, t: i64, update: &Value, keep_existing: bool) -> rusqlite::Result<bool> {
    let v = &update["values"];
    if !v.is_object() {
        return Ok(false);
    }
    let changed = db
        .prepare_cached(&format!(
            "INSERT OR {} INTO system(t, session, n, current, soc, voltage, ccl, dcl, cvl, cell_max, cell_min)
             VALUES(?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            if keep_existing { "IGNORE" } else { "REPLACE" }
        ))?
        .execute(params![
        t,
        session,
        scaled(&v["current"], 1000.0),
        scaled(&v["soc"], 10.0),
        scaled(&v["voltage"], 100.0),
        scaled(&v["charge_current_limit"], 1000.0),
        scaled(&v["discharge_current_limit"], 1000.0),
        scaled(&v["charge_voltage_limit"], 100.0),
        scaled(&v["highest_cell_voltage"], 1000.0),
        scaled(&v["lowest_cell_voltage"], 1000.0),
    ])?;
    Ok(changed > 0)
}

/// Extend the session's time span and remember which packs the installation has.
fn touch(db: &Connection, session: i64, site: &str, address: u8, t: i64) -> rusqlite::Result<()> {
    db.prepare_cached("UPDATE sessions SET first = COALESCE(first, ?2), last = MAX(COALESCE(last, ?2), ?2) WHERE id = ?1")?.execute(params![session, t])?;
    db.prepare_cached("INSERT OR IGNORE INTO site_packs(site, pack) VALUES(?1, ?2)")?.execute(params![site, address])?;
    Ok(())
}

/// Open an event for each new alarm of the pack, close the ones that are gone.
fn track_events(db: &Connection, open: &mut OpenEvents, session: i64, t: i64, address: u8, update: &Value) -> rusqlite::Result<()> {
    let Some(alarms) = update["status"]["alarms"].as_array() else { return Ok(()) };
    let now: HashMap<String, String> = alarms
        .iter()
        .filter_map(|a| Some((a["id"].as_str().or(a["key"].as_str())?.to_string(), a["severity"].as_str().unwrap_or("").to_string())))
        .collect();
    let mine = open.entry(address).or_default();
    let gone: Vec<String> = mine.keys().filter(|k| !now.contains_key(*k)).cloned().collect();
    for k in gone {
        if let Some(id) = mine.remove(&k) {
            db.prepare_cached("UPDATE events SET end = ?2 WHERE id = ?1")?.execute(params![id, t])?;
        }
    }
    for (k, sev) in now {
        if let std::collections::hash_map::Entry::Vacant(e) = mine.entry(k) {
            db.prepare_cached("INSERT INTO events(session, pack, key, severity, start) VALUES(?1, ?2, ?3, ?4, ?5)")?
                .execute(params![session, address, e.key(), sev, t])?;
            e.insert(db.last_insert_rowid());
        }
    }
    Ok(())
}

fn close_events(db: &Connection, open: &mut OpenEvents, t: i64) {
    for (_, m) in open.drain() {
        for (_, id) in m {
            let _ = db.execute("UPDATE events SET end = ?2 WHERE id = ?1", params![id, t]);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// retention

/// Sum and count for a weighted mean, and the extremes.
#[derive(Default, Clone)]
struct Acc {
    sum: f64,
    n: f64,
    max: Option<i64>,
    min: Option<i64>,
}
impl Acc {
    fn add(&mut self, v: Option<i64>, w: i64) {
        if let Some(v) = v {
            self.sum += v as f64 * w as f64;
            self.n += w as f64;
            self.max = Some(self.max.map_or(v, |m| m.max(v)));
            self.min = Some(self.min.map_or(v, |m| m.min(v)));
        }
    }
    fn mean(&self) -> Option<i64> {
        (self.n > 0.0).then(|| (self.sum / self.n).round() as i64)
    }
}

fn maintain(db: &Connection, s: &Settings, now: i64) -> rusqlite::Result<()> {
    let cutoff = (now - s.keep_full_days as i64 * DAY_MS).div_euclid(MINUTE_MS) * MINUTE_MS;
    // one day at a time, minute-aligned, so each transaction stays small and minutes are never split;
    // the cursor moves on, since a minute with a single answer stays a row with n = 1
    let mut cursor = i64::MIN;
    loop {
        let first: Option<i64> = db.query_row(
            "SELECT MIN(t) FROM (SELECT MIN(t) AS t FROM pack WHERE n = 1 AND t >= ?2 AND t < ?1
               UNION ALL SELECT MIN(t) FROM system WHERE n = 1 AND t >= ?2 AND t < ?1)",
            [cutoff, cursor],
            |r| r.get(0),
        )?;
        let Some(first) = first else { break };
        let from = first.div_euclid(MINUTE_MS) * MINUTE_MS;
        let to = (from + DAY_MS).min(cutoff);
        cursor = to;
        let tx = db.unchecked_transaction()?;
        compact_packs(&tx, from, to)?;
        compact_system(&tx, from, to)?;
        tx.commit()?;
    }
    if s.keep_minutes_days > 0 {
        let limit = now - (s.keep_full_days as i64 + s.keep_minutes_days as i64) * DAY_MS;
        db.execute("DELETE FROM pack WHERE t < ?1", [limit])?;
        db.execute("DELETE FROM system WHERE t < ?1", [limit])?;
        db.execute("DELETE FROM events WHERE COALESCE(end, start) < ?1", [limit])?;
        db.execute("DELETE FROM sessions WHERE last < ?1", [limit])?;
        db.execute("UPDATE sessions SET first = ?1 WHERE first < ?1", [limit])?;
    }
    db.execute_batch("PRAGMA incremental_vacuum;")?;
    Ok(())
}

struct PackAcc {
    session: i64,
    n: i64,
    /// current, soc, voltage, remaining, cycles, ambient, mosfet
    cols: [Acc; 7],
    cell_max: Acc,
    cell_min: Acc,
    temp_max: Acc,
    temp_min: Acc,
    cells: Vec<Acc>,
    temps: Vec<Acc>,
}

fn compact_packs(tx: &Connection, from: i64, to: i64) -> rusqlite::Result<()> {
    let mut groups: BTreeMap<(u8, i64), PackAcc> = BTreeMap::new();
    {
        let mut st = tx.prepare(
            "SELECT pack, t, session, n, current, soc, voltage, remaining, cycles, ambient, mosfet, cell_max, cell_min,
               temp_max, temp_min, cells, temps FROM pack WHERE t >= ?1 AND t < ?2",
        )?;
        let mut rows = st.query([from, to])?;
        while let Some(r) = rows.next()? {
            let t: i64 = r.get(1)?;
            let key = (r.get::<_, u8>(0)?, t.div_euclid(MINUTE_MS) * MINUTE_MS);
            let w: i64 = r.get(3)?;
            let g = groups.entry(key).or_insert_with(|| PackAcc {
                session: r.get(2).unwrap_or(0),
                n: 0,
                cols: Default::default(),
                cell_max: Acc::default(),
                cell_min: Acc::default(),
                temp_max: Acc::default(),
                temp_min: Acc::default(),
                cells: vec![],
                temps: vec![],
            });
            g.n += w;
            for (i, col) in [4, 5, 6, 7, 8, 9, 10].into_iter().enumerate() {
                g.cols[i].add(r.get(col)?, w);
            }
            g.cell_max.add(r.get(11)?, w);
            g.cell_min.add(r.get(12)?, w);
            g.temp_max.add(r.get(13)?, w);
            g.temp_min.add(r.get(14)?, w);
            for (list, blob) in [(&mut g.cells, unblob_u16(&r.get::<_, Vec<u8>>(15).unwrap_or_default())), (&mut g.temps, unblob_i16(&r.get::<_, Vec<u8>>(16).unwrap_or_default()))] {
                if list.len() < blob.len() {
                    list.resize(blob.len(), Acc::default());
                }
                for (a, v) in list.iter_mut().zip(blob) {
                    a.add(Some(v), w);
                }
            }
        }
    }
    tx.execute("DELETE FROM pack WHERE t >= ?1 AND t < ?2", [from, to])?;
    let mut ins = tx.prepare(
        "INSERT INTO pack(pack, t, session, n, current, soc, voltage, remaining, cycles, ambient, mosfet, cell_max, cell_min,
           temp_max, temp_min, cells, temps) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )?;
    for ((pack, t), g) in groups {
        let m = |i: usize| g.cols[i].mean();
        let cells: Vec<i64> = g.cells.iter().filter_map(Acc::mean).collect();
        let temps: Vec<i64> = g.temps.iter().filter_map(Acc::mean).collect();
        ins.execute(params![
            pack, t, g.session, g.n, m(0), m(1), m(2), m(3), g.cols[4].max, m(5), m(6),
            g.cell_max.max, g.cell_min.min, g.temp_max.max, g.temp_min.min, blob_u16(&cells), blob_i16(&temps)
        ])?;
    }
    Ok(())
}

fn compact_system(tx: &Connection, from: i64, to: i64) -> rusqlite::Result<()> {
    let mut groups: BTreeMap<i64, (i64, i64, Vec<Acc>)> = BTreeMap::new();
    {
        let mut st = tx.prepare("SELECT t, session, n, current, soc, voltage, ccl, dcl, cvl, cell_max, cell_min FROM system WHERE t >= ?1 AND t < ?2")?;
        let mut rows = st.query([from, to])?;
        while let Some(r) = rows.next()? {
            let t: i64 = r.get(0)?;
            let w: i64 = r.get(2)?;
            let g = groups.entry(t.div_euclid(MINUTE_MS) * MINUTE_MS).or_insert_with(|| (r.get(1).unwrap_or(0), 0, vec![Acc::default(); 8]));
            g.1 += w;
            for i in 0..8 {
                g.2[i].add(r.get(3 + i)?, w);
            }
        }
    }
    tx.execute("DELETE FROM system WHERE t >= ?1 AND t < ?2", [from, to])?;
    let mut ins = tx.prepare("INSERT INTO system(t, session, n, current, soc, voltage, ccl, dcl, cvl, cell_max, cell_min) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)")?;
    for (t, (session, n, a)) in groups {
        // limits: the lowest in the minute (the stricter one), extremes as extremes
        ins.execute(params![t, session, n, a[0].mean(), a[1].mean(), a[2].mean(), a[3].min, a[4].min, a[5].min, a[6].max, a[7].min])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// reading

#[derive(Serialize)]
pub struct SiteSpan {
    pub site: String,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub packs: Vec<u8>,
    pub has_system: bool,
}

/// Installations in the database with their time span and packs, most recent first.
pub fn sites(db: &Connection) -> Result<Vec<SiteSpan>, String> {
    let mut st = db.prepare("SELECT site, MIN(first), MAX(last) FROM sessions WHERE first IS NOT NULL GROUP BY site ORDER BY MAX(last) DESC").map_err(e2s)?;
    let spans: Vec<(String, Option<i64>, Option<i64>)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(e2s)?.collect::<Result<_, _>>().map_err(e2s)?;
    spans
        .into_iter()
        .map(|(site, from, to)| {
            let packs = db
                .prepare("SELECT pack FROM site_packs WHERE site = ?1 ORDER BY pack")
                .and_then(|mut s| s.query_map([&site], |r| r.get(0))?.collect::<Result<Vec<u8>, _>>())
                .map_err(e2s)?;
            let has_system = db
                .query_row("SELECT EXISTS(SELECT 1 FROM system WHERE session IN (SELECT id FROM sessions WHERE site = ?1))", [&site], |r| r.get(0))
                .map_err(e2s)?;
            Ok(SiteSpan { site, from, to, packs, has_system })
        })
        .collect()
}

#[derive(Serialize, Default)]
pub struct PackCols {
    pub current: Vec<Option<f64>>,
    pub soc: Vec<Option<f64>>,
    pub voltage: Vec<Option<f64>>,
    pub cell_max: Vec<Option<f64>>,
    pub cell_min: Vec<Option<f64>>,
    pub delta: Vec<Option<f64>>,
    pub temp_max: Vec<Option<f64>>,
    pub temp_min: Vec<Option<f64>>,
}

#[derive(Serialize, Default)]
pub struct SystemCols {
    pub current: Vec<Option<f64>>,
    pub soc: Vec<Option<f64>>,
    pub voltage: Vec<Option<f64>>,
    pub ccl: Vec<Option<f64>>,
    pub dcl: Vec<Option<f64>>,
    pub cvl: Vec<Option<f64>>,
    pub cell_max: Vec<Option<f64>>,
    pub cell_min: Vec<Option<f64>>,
}

#[derive(Serialize)]
pub struct Event {
    pub pack: u8,
    pub key: String,
    pub severity: String,
    pub start: i64,
    pub end: Option<i64>,
}

#[derive(Serialize)]
pub struct Series {
    pub from: i64,
    pub to: i64,
    /// bucket width (ms) and bucket start times
    pub step: f64,
    pub x: Vec<i64>,
    pub packs: BTreeMap<u8, PackCols>,
    pub system: Option<SystemCols>,
    /// all packs' cells of `cells_pack`: cells[i][bucket]
    pub cells: Vec<Vec<Option<f64>>>,
    pub events: Vec<Event>,
}

/// A time grid over [from, to] with at most `max` buckets, none finer than 1.5 × the typical interval.
struct Grid {
    from: i64,
    step: f64,
    n: usize,
}
impl Grid {
    fn bucket(&self, t: i64) -> Option<usize> {
        if t < self.from {
            return None;
        }
        let b = ((t - self.from) as f64 / self.step) as usize;
        Some(b.min(self.n - 1))
    }
}

/// Bucketed values: weighted mean or extremes, then short holes bridged.
struct Col {
    acc: Vec<Acc>,
}
impl Col {
    fn new(n: usize) -> Self {
        Col { acc: vec![Acc::default(); n] }
    }
    fn finish(&self, how: Reduce, scale: f64, bridge: usize) -> Vec<Option<f64>> {
        let mut out: Vec<Option<f64>> = self
            .acc
            .iter()
            .map(|a| match how {
                Reduce::Mean => (a.n > 0.0).then(|| a.sum / a.n / scale),
                Reduce::Max => a.max.map(|v| v as f64 / scale),
                Reduce::Min => a.min.map(|v| v as f64 / scale),
            })
            .collect();
        // a run of up to `bridge` empty buckets between two values takes the previous value
        let mut last: Option<f64> = None;
        let mut run = 0;
        for i in 0..out.len() {
            match out[i] {
                Some(v) => {
                    if run > 0 && run <= bridge {
                        if let Some(l) = last {
                            for o in &mut out[i - run..i] {
                                *o = Some(l);
                            }
                        }
                    }
                    last = Some(v);
                    run = 0;
                }
                None if last.is_some() => run += 1,
                None => {}
            }
        }
        out
    }
}

#[derive(Clone, Copy)]
enum Reduce {
    Mean,
    Max,
    Min,
}

/// The data for the charts: every requested pack and the master on a common grid of at most `max_points`.
pub fn series(db: &Connection, site: &str, from: i64, to: i64, packs: &[u8], cells_pack: Option<u8>, max_points: usize) -> Result<Series, String> {
    let to = to.max(from + 1);
    let sessions = "session IN (SELECT id FROM sessions WHERE site = ?1)";
    let pack_list = if packs.is_empty() { "0".to_string() } else { packs.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",") };
    let in_packs = format!("pack IN ({pack_list})");

    // typical interval: span per row of the busiest source
    let busiest: i64 = db
        .query_row(
            &format!("SELECT COALESCE(MAX(c), 0) FROM (SELECT COUNT(*) AS c FROM pack WHERE {in_packs} AND t >= ?2 AND t <= ?3 AND {sessions} GROUP BY pack)"),
            params![site, from, to],
            |r| r.get(0),
        )
        .map_err(e2s)?;
    let sys_rows: i64 = db
        .query_row(&format!("SELECT COUNT(*) FROM system WHERE t >= ?2 AND t <= ?3 AND {sessions}"), params![site, from, to], |r| r.get(0))
        .map_err(e2s)?;
    let per_source = busiest.max(sys_rows);
    let span = (to - from) as f64;
    let interval = if per_source > 1 { span / per_source as f64 } else { 1000.0 }.max(200.0);
    let step = (span / max_points.max(1) as f64).max(interval * 1.5);
    let n = ((span / step).ceil() as usize).max(1);
    let grid = Grid { from, step, n };
    let bridge = ((3.0 * interval).max(step) / step).floor() as usize;

    let mut packs_out = BTreeMap::new();
    {
        // current, soc, voltage, cell max, cell min, cell difference, warmest, coldest
        let mut cols: BTreeMap<u8, [Col; 8]> = BTreeMap::new();
        let mut st = db
            .prepare(&format!("SELECT pack, t, n, current, soc, voltage, cell_max, cell_min, temp_max, temp_min FROM pack WHERE {in_packs} AND t >= ?2 AND t <= ?3 AND {sessions}"))
            .map_err(e2s)?;
        let mut q = st.query(params![site, from, to]).map_err(e2s)?;
        while let Some(r) = q.next().map_err(e2s)? {
            let p: u8 = r.get(0).map_err(e2s)?;
            let Some(b) = grid.bucket(r.get(1).map_err(e2s)?) else { continue };
            let w: i64 = r.get(2).map_err(e2s)?;
            let mut v = [None; 7];
            for (i, x) in v.iter_mut().enumerate() {
                *x = r.get::<_, Option<i64>>(3 + i).map_err(e2s)?;
            }
            let [cur, soc, volt, cmax, cmin, tmax, tmin] = v;
            let c = cols.entry(p).or_insert_with(|| std::array::from_fn(|_| Col::new(n)));
            for (i, x) in [cur, soc, volt, cmax, cmin, cmax.zip(cmin).map(|(a, b)| a - b), tmax, tmin].into_iter().enumerate() {
                c[i].acc[b].add(x, w);
            }
        }
        for (p, c) in cols {
            packs_out.insert(
                p,
                PackCols {
                    current: c[0].finish(Reduce::Mean, 1000.0, bridge),
                    soc: c[1].finish(Reduce::Mean, 10.0, bridge),
                    voltage: c[2].finish(Reduce::Mean, 100.0, bridge),
                    cell_max: c[3].finish(Reduce::Max, 1000.0, bridge),
                    cell_min: c[4].finish(Reduce::Min, 1000.0, bridge),
                    delta: c[5].finish(Reduce::Max, 1.0, bridge),
                    temp_max: c[6].finish(Reduce::Max, 10.0, bridge),
                    temp_min: c[7].finish(Reduce::Min, 10.0, bridge),
                },
            );
        }
    }

    let system = if sys_rows > 0 {
        let mut c: [Col; 8] = std::array::from_fn(|_| Col::new(n));
        let mut st = db
            .prepare(&format!("SELECT t, n, current, soc, voltage, ccl, dcl, cvl, cell_max, cell_min FROM system WHERE t >= ?2 AND t <= ?3 AND {sessions}"))
            .map_err(e2s)?;
        let mut q = st.query(params![site, from, to]).map_err(e2s)?;
        while let Some(r) = q.next().map_err(e2s)? {
            let Some(b) = grid.bucket(r.get(0).map_err(e2s)?) else { continue };
            let w: i64 = r.get(1).map_err(e2s)?;
            for (i, col) in c.iter_mut().enumerate() {
                col.acc[b].add(r.get(2 + i).map_err(e2s)?, w);
            }
        }
        Some(SystemCols {
            current: c[0].finish(Reduce::Mean, 1000.0, bridge),
            soc: c[1].finish(Reduce::Mean, 10.0, bridge),
            voltage: c[2].finish(Reduce::Mean, 100.0, bridge),
            ccl: c[3].finish(Reduce::Min, 1000.0, bridge),
            dcl: c[4].finish(Reduce::Min, 1000.0, bridge),
            cvl: c[5].finish(Reduce::Min, 100.0, bridge),
            cell_max: c[6].finish(Reduce::Max, 1000.0, bridge),
            cell_min: c[7].finish(Reduce::Min, 1000.0, bridge),
        })
    } else {
        None
    };

    let mut cells = vec![];
    if let Some(p) = cells_pack {
        let mut cols: Vec<Col> = vec![];
        let mut st = db.prepare(&format!("SELECT t, n, cells FROM pack WHERE pack = ?4 AND t >= ?2 AND t <= ?3 AND {sessions}")).map_err(e2s)?;
        let mut q = st.query(params![site, from, to, p]).map_err(e2s)?;
        while let Some(r) = q.next().map_err(e2s)? {
            let Some(b) = grid.bucket(r.get(0).map_err(e2s)?) else { continue };
            let w: i64 = r.get(1).map_err(e2s)?;
            let vals = unblob_u16(&r.get::<_, Option<Vec<u8>>>(2).map_err(e2s)?.unwrap_or_default());
            while cols.len() < vals.len() {
                cols.push(Col::new(n));
            }
            for (c, v) in cols.iter_mut().zip(vals) {
                c.acc[b].add(Some(v), w);
            }
        }
        cells = cols.iter().map(|c| c.finish(Reduce::Mean, 1000.0, bridge)).collect();
    }

    let mut events = vec![];
    {
        let mut st = db
            .prepare(&format!("SELECT pack, key, severity, start, end FROM events WHERE start <= ?3 AND COALESCE(end, ?3) >= ?2 AND {sessions} ORDER BY start LIMIT 5000"))
            .map_err(e2s)?;
        let rows = st
            .query_map(params![site, from, to], |r| Ok(Event { pack: r.get(0)?, key: r.get(1)?, severity: r.get(2)?, start: r.get(3)?, end: r.get(4)? }))
            .map_err(e2s)?;
        for e in rows {
            events.push(e.map_err(e2s)?);
        }
    }

    Ok(Series { from, to, step, x: (0..n).map(|i| from + (i as f64 * step) as i64).collect(), packs: packs_out, system, cells, events })
}

// ---------------------------------------------------------------------------------------------
// export and import

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Jsonl,
    Csv,
}

#[derive(Serialize, Default, Debug)]
pub struct ExportStats {
    pub rows: u64,
    pub files: Vec<String>,
}

/// UTC time as ISO 8601 with milliseconds (no time zone database needed).
pub fn iso(ms: i64) -> String {
    let s = ms.div_euclid(1000);
    let (days, rem) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms.rem_euclid(1000))
}

fn f(v: Option<i64>, scale: f64) -> Value {
    v.map_or(Value::Null, |v| json!(v as f64 / scale))
}

/// Write a span as JSON Lines (the recording format, readable by earlier versions and by import) or CSV
/// (one file for the packs, one for the master if it has values).
pub fn export(db: &Connection, site: &str, from: i64, to: i64, packs: &[u8], format: Format, out: &Path) -> Result<ExportStats, String> {
    let mut stats = ExportStats::default();
    let sessions = "session IN (SELECT id FROM sessions WHERE site = ?1)";
    let pack_list = if packs.is_empty() { "0".to_string() } else { packs.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",") };
    let io = |e: std::io::Error| format!("{}: {e}", out.display());
    let mut ps = db
        .prepare(&format!(
            "SELECT t, pack, current, soc, voltage, remaining, cycles, ambient, mosfet, cells, temps, n FROM pack
             WHERE pack IN ({pack_list}) AND t >= ?2 AND t <= ?3 AND {sessions} ORDER BY t, pack"
        ))
        .map_err(e2s)?;
    let mut ss = db
        .prepare(&format!("SELECT t, current, soc, voltage, ccl, dcl, cvl, cell_max, cell_min FROM system WHERE t >= ?2 AND t <= ?3 AND {sessions} ORDER BY t"))
        .map_err(e2s)?;

    let pack_json = |r: &rusqlite::Row| -> rusqlite::Result<(i64, Value)> {
        let t: i64 = r.get(0)?;
        let a: u8 = r.get(1)?;
        let cells: Vec<f64> = unblob_u16(&r.get::<_, Option<Vec<u8>>>(9)?.unwrap_or_default()).into_iter().map(|v| v as f64 / 1000.0).collect();
        let temps: Vec<f64> = unblob_i16(&r.get::<_, Option<Vec<u8>>>(10)?.unwrap_or_default()).into_iter().map(|v| v as f64 / 10.0).collect();
        let tm = json!({
            "address": a, "cell_voltages": cells, "cell_temperatures": temps,
            "current": f(r.get(2)?, 1000.0), "soc": f(r.get(3)?, 10.0), "pack_voltage": f(r.get(4)?, 100.0),
            "remaining_capacity_ah": f(r.get(5)?, 100.0), "cycles": r.get::<_, Option<i64>>(6)?,
            "ambient_temperature": f(r.get(7)?, 10.0), "power_temperature": f(r.get(8)?, 10.0),
            "samples": r.get::<_, i64>(11)?,
        });
        Ok((t, json!({ "t": t, "pack": { "address": a, "telemetry": tm, "status": null, "error": null } })))
    };
    let sys_json = |r: &rusqlite::Row| -> rusqlite::Result<(i64, Value)> {
        let t: i64 = r.get(0)?;
        let v = json!({
            "current": f(r.get(1)?, 1000.0), "soc": f(r.get(2)?, 10.0), "voltage": f(r.get(3)?, 100.0),
            "charge_current_limit": f(r.get(4)?, 1000.0), "discharge_current_limit": f(r.get(5)?, 1000.0),
            "charge_voltage_limit": f(r.get(6)?, 100.0), "highest_cell_voltage": f(r.get(7)?, 1000.0), "lowest_cell_voltage": f(r.get(8)?, 1000.0),
        });
        Ok((t, json!({ "t": t, "system": { "values": v, "error": null } })))
    };

    match format {
        Format::Jsonl => {
            let mut w = BufWriter::new(std::fs::File::create(out).map_err(io)?);
            writeln!(w, "{}", json!({ "t": from, "start": { "packs": packs, "system": true, "interval_s": 0, "bus": "", "site": site, "export": true } })).map_err(io)?;
            let mut pr = ps.query(params![site, from, to]).map_err(e2s)?;
            let mut sr = ss.query(params![site, from, to]).map_err(e2s)?;
            let mut p = pr.next().map_err(e2s)?.map(pack_json).transpose().map_err(e2s)?;
            let mut s = sr.next().map_err(e2s)?.map(sys_json).transpose().map_err(e2s)?;
            // merge both, in time order
            loop {
                let take_pack = match (&p, &s) {
                    (Some((tp, _)), Some((ts, _))) => tp <= ts,
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => break,
                };
                if take_pack {
                    writeln!(w, "{}", p.take().unwrap().1).map_err(io)?;
                    p = pr.next().map_err(e2s)?.map(pack_json).transpose().map_err(e2s)?;
                } else {
                    writeln!(w, "{}", s.take().unwrap().1).map_err(io)?;
                    s = sr.next().map_err(e2s)?.map(sys_json).transpose().map_err(e2s)?;
                }
                stats.rows += 1;
            }
            w.flush().map_err(io)?;
            stats.files.push(out.display().to_string());
        }
        Format::Csv => {
            let mut w = BufWriter::new(std::fs::File::create(out).map_err(io)?);
            let mut rows = vec![];
            let mut pr = ps.query(params![site, from, to]).map_err(e2s)?;
            let (mut ncells, mut ntemps) = (0, 0);
            while let Some(r) = pr.next().map_err(e2s)? {
                let (t, v) = pack_json(r).map_err(e2s)?;
                let tm = &v["pack"]["telemetry"];
                ncells = ncells.max(tm["cell_voltages"].as_array().map_or(0, Vec::len));
                ntemps = ntemps.max(tm["cell_temperatures"].as_array().map_or(0, Vec::len));
                rows.push((t, tm.clone()));
            }
            let mut head = "time_utc,pack,samples,current_a,soc_pct,voltage_v,remaining_ah,cycles,ambient_c,mosfet_c".to_string();
            (1..=ncells).for_each(|i| head += &format!(",cell_{i}_v"));
            (1..=ntemps).for_each(|i| head += &format!(",temp_{i}_c"));
            writeln!(w, "{head}").map_err(io)?;
            // plain numbers (50, not 50.0), empty for missing values
            let cell = |v: &Value| match (v.as_i64(), v.as_f64()) {
                (Some(i), _) => i.to_string(),
                (None, Some(x)) => x.to_string(),
                _ => String::new(),
            };
            for (t, tm) in &rows {
                let mut line = format!("{},{},{}", iso(*t), tm["address"], tm["samples"]);
                for k in ["current", "soc", "pack_voltage", "remaining_capacity_ah", "cycles", "ambient_temperature", "power_temperature"] {
                    line += &format!(",{}", cell(&tm[k]));
                }
                for (key, count) in [("cell_voltages", ncells), ("cell_temperatures", ntemps)] {
                    let a = tm[key].as_array().cloned().unwrap_or_default();
                    for i in 0..count {
                        line += &format!(",{}", a.get(i).map_or(String::new(), cell));
                    }
                }
                writeln!(w, "{line}").map_err(io)?;
                stats.rows += 1;
            }
            w.flush().map_err(io)?;
            stats.files.push(out.display().to_string());

            let mut sr = ss.query(params![site, from, to]).map_err(e2s)?;
            let mut sys = vec![];
            while let Some(r) = sr.next().map_err(e2s)? {
                sys.push(sys_json(r).map_err(e2s)?);
            }
            if !sys.is_empty() {
                let stem = out.file_stem().map_or("export".into(), |s| s.to_string_lossy().into_owned());
                let path = out.with_file_name(format!("{stem}_master.csv"));
                let mut w = BufWriter::new(std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?);
                let keys = ["current", "soc", "voltage", "charge_current_limit", "discharge_current_limit", "charge_voltage_limit", "highest_cell_voltage", "lowest_cell_voltage"];
                writeln!(w, "time_utc,current_a,soc_pct,voltage_v,charge_limit_a,discharge_limit_a,charge_voltage_v,highest_cell_v,lowest_cell_v").map_err(io)?;
                for (t, v) in sys {
                    let vals = &v["system"]["values"];
                    let line: Vec<String> = keys.iter().map(|k| cell(&vals[*k])).collect();
                    writeln!(w, "{},{}", iso(t), line.join(",")).map_err(io)?;
                    stats.rows += 1;
                }
                w.flush().map_err(io)?;
                stats.files.push(path.display().to_string());
            }
        }
    }
    Ok(stats)
}

#[derive(Serialize, Default, Debug)]
pub struct ImportStats {
    pub rows: u64,
    pub skipped: u64,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

fn import(db: &mut Connection, file: &Path, site: &str) -> Result<ImportStats, String> {
    let io = |e: std::io::Error| format!("{}: {e}", file.display());
    let reader = std::io::BufReader::new(std::fs::File::open(file).map_err(io)?);
    let name = file.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let tx = db.transaction().map_err(e2s)?;
    tx.execute("INSERT INTO sessions(site, bus, source) VALUES(?1, '', ?2)", params![site, name]).map_err(e2s)?;
    let session = tx.last_insert_rowid();
    let mut stats = ImportStats::default();
    let mut open = OpenEvents::new();
    let mut seen_packs = HashSet::new();
    let mut last_t = 0;
    for line in reader.lines() {
        let line = line.map_err(io)?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            stats.skipped += 1;
            continue;
        };
        let Some(t) = v["t"].as_i64() else {
            stats.skipped += 1;
            continue;
        };
        let wrote = if let Some(bus) = v["start"]["bus"].as_str() {
            tx.execute("UPDATE sessions SET bus = ?2 WHERE id = ?1", params![session, bus]).map_err(e2s)?;
            false
        } else if v["pack"].is_object() {
            let a = v["pack"]["address"].as_u64().unwrap_or(0) as u8;
            track_events(&tx, &mut open, session, t, a, &v["pack"]).map_err(e2s)?;
            let w = insert_pack(&tx, session, t, a, &v["pack"], true).map_err(e2s)?;
            if w {
                seen_packs.insert(a);
            }
            w
        } else if v["system"].is_object() {
            insert_system(&tx, session, t, &v["system"], true).map_err(e2s)?
        } else {
            stats.skipped += 1;
            false
        };
        if wrote {
            stats.rows += 1;
            stats.from = Some(stats.from.map_or(t, |f| f.min(t)));
            stats.to = Some(stats.to.map_or(t, |x| x.max(t)));
        }
        last_t = last_t.max(t);
    }
    close_events(&tx, &mut open, last_t);
    tx.execute("UPDATE sessions SET first = ?2, last = ?3 WHERE id = ?1", params![session, stats.from, stats.to]).map_err(e2s)?;
    for a in seen_packs {
        tx.execute("INSERT OR IGNORE INTO site_packs(site, pack) VALUES(?1, ?2)", params![site, a]).map_err(e2s)?;
    }
    if stats.rows == 0 {
        tx.execute("DELETE FROM sessions WHERE id = ?1", [session]).map_err(e2s)?;
    }
    tx.commit().map_err(e2s)?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_update(address: u8, current: f64, cells: &[f64], alarms: &[&str]) -> Value {
        json!({
            "address": address,
            "telemetry": { "address": address, "cell_voltages": cells, "cell_temperatures": [20.0, 22.5], "ambient_temperature": 25.0,
              "power_temperature": 26.0, "current": current, "pack_voltage": 53.2, "remaining_capacity_ah": 140.0, "soc": 50.0,
              "cycles": 78, "idle_current_ma": 120 },
            "status": { "alarms": alarms.iter().map(|k| json!({ "id": k, "key": k, "severity": "warning" })).collect::<Vec<_>>() },
            "error": null
        })
    }
    fn system_update(current: f64) -> Value {
        json!({ "values": { "current": current, "soc": 50.0, "voltage": 53.2, "charge_current_limit": 150.0, "discharge_current_limit": 200.0,
          "charge_voltage_limit": 56.0, "highest_cell_voltage": 3.34, "lowest_cell_voltage": 3.30 }, "error": null })
    }

    fn temp_db(name: &str) -> (PathBuf, History) {
        let dir = std::env::temp_dir().join(format!("openbms-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("history.sqlite");
        let mut h = History::default();
        h.open(&path).unwrap();
        (path, h)
    }

    #[test]
    fn records_only_when_enabled_and_connected() {
        let (path, mut h) = temp_db("enable");
        h.connected("gw:4196", "can");
        h.pack(1_000, 0, &pack_update(0, 10.0, &[3.3, 3.31], &[]));
        assert_eq!(h.status().rows, 0, "off by default");
        h.set_settings(Settings { enabled: true, ..Settings::default() }).unwrap();
        h.pack(2_000, 0, &pack_update(0, 10.0, &[3.3, 3.31], &[]));
        h.system(2_100, &system_update(40.0));
        h.disconnected();
        h.pack(3_000, 0, &pack_update(0, 10.0, &[3.3, 3.31], &[]));
        assert_eq!(h.status().rows, 2);
        let s = sites(&reader(&path).unwrap()).unwrap();
        assert_eq!((s[0].site.as_str(), s[0].from, s[0].to, s[0].packs.clone(), s[0].has_system), ("gw:4196", Some(2_000), Some(2_100), vec![0], true));
        // settings survive a restart
        let mut h2 = History::default();
        h2.open(&path).unwrap();
        assert!(h2.status().settings.enabled);
    }

    #[test]
    fn buckets_bridges_and_events() {
        let (path, mut h) = temp_db("series");
        h.set_settings(Settings { enabled: true, ..Settings::default() }).unwrap();
        h.connected("gw", "can");
        // pack 0 every 10 s for 10 min, one alarm between 100 s and 200 s; pack 1 silent between 200 and 400 s
        for i in 0..60 {
            let t = i * 10_000;
            let alarms: &[&str] = if (10..20).contains(&i) { &["cell_high"] } else { &[] };
            h.pack(t, 0, &pack_update(0, 20.0 + i as f64, &[3.3, 3.32], alarms));
            if !(20..40).contains(&i) {
                h.pack(t + 500, 1, &pack_update(1, 0.0, &[3.31, 3.31], &[]));
            }
        }
        let s = series(&reader(&path).unwrap(), "gw", 0, 600_000, &[0, 1], Some(0), 60).unwrap();
        assert_eq!(s.x.len(), 40); // step 15 s (1.5 × 10 s)
        let p0 = &s.packs[&0];
        assert!(p0.current.iter().all(Option::is_some));
        assert_eq!(p0.delta[0], Some(20.0));
        assert_eq!(p0.cell_max[0], Some(3.32));
        // pack 1: idle current 120 mA where the regular value is 0; a 200 s hole stays a hole
        let p1 = &s.packs[&1];
        assert_eq!(p1.current[0], Some(0.12));
        assert!(p1.current[20].is_none());
        assert_eq!(s.cells.len(), 2);
        assert_eq!(s.events.len(), 1);
        assert_eq!((s.events[0].start, s.events[0].end), (100_000, Some(200_000)));
    }

    #[test]
    fn merges_old_rows_into_minutes_and_deletes_past_retention() {
        let (path, mut h) = temp_db("retention");
        h.set_settings(Settings { enabled: true, keep_full_days: 1, keep_minutes_days: 0, ..Settings::default() }).unwrap();
        h.connected("gw", "can");
        let base = 10 * DAY_MS;
        for i in 0..12 {
            // 2 minutes of answers every 10 s, current rising
            h.pack(base + i * 10_000, 3, &pack_update(3, i as f64, &[3.30, 3.30 + i as f64 / 1000.0], &[]));
            h.system(base + i * 10_000, &system_update(i as f64));
        }
        h.maintain(base + 2 * DAY_MS);
        let db = reader(&path).unwrap();
        let rows: Vec<(i64, i64, i64, i64)> = db
            .prepare("SELECT t, n, current, cell_max FROM pack ORDER BY t")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        // minute 1: answers 0..5 (0 A reads as the 120 mA idle current: mean 2.52 A), minute 2: 6..11 (8.5 A); maxima kept
        assert_eq!(rows, vec![(base, 6, 2520, 3305), (base + MINUTE_MS, 6, 8500, 3311)]);
        let n_sys: i64 = db.query_row("SELECT COUNT(*) FROM system", [], |r| r.get(0)).unwrap();
        assert_eq!(n_sys, 2);
        // running again changes nothing
        h.maintain(base + 2 * DAY_MS);
        let n: i64 = reader(&path).unwrap().query_row("SELECT SUM(n) FROM pack", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 12);
        // minute values kept 1 day more, then gone
        h.set_settings(Settings { enabled: true, keep_full_days: 1, keep_minutes_days: 1, ..Settings::default() }).unwrap();
        h.maintain(base + 3 * DAY_MS);
        let n: i64 = reader(&path).unwrap().query_row("SELECT COUNT(*) FROM pack", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn export_and_import_round_trip() {
        let (path, mut h) = temp_db("export");
        h.set_settings(Settings { enabled: true, ..Settings::default() }).unwrap();
        h.connected("gw", "can");
        for i in 0..5 {
            h.pack(i * 1000, 2, &pack_update(2, -12.5, &[3.25, 3.26, 3.27], if i == 2 { &["cell_low"] } else { &[] }));
            h.system(i * 1000 + 300, &system_update(-12.5));
        }
        let db = reader(&path).unwrap();
        let dir = path.parent().unwrap();
        let out = dir.join("export.jsonl");
        let st = export(&db, "gw", 0, 10_000, &[2], Format::Jsonl, &out).unwrap();
        assert_eq!(st.rows, 10);
        let csv = dir.join("export.csv");
        let st = export(&db, "gw", 0, 10_000, &[2], Format::Csv, &csv).unwrap();
        assert_eq!(st.files.len(), 2);
        let text = std::fs::read_to_string(&csv).unwrap();
        assert!(text.starts_with("time_utc,pack,samples,current_a"));
        assert!(text.lines().nth(1).unwrap().starts_with("1970-01-01T00:00:00.000Z,2,1,-12.5,50,53.2,140,78,25,26,3.25,3.26,3.27,20,22.5"));

        let a = series(&db, "gw", 0, 5000, &[2], Some(2), 100).unwrap();
        // importing what is already there adds nothing
        assert_eq!(h.import(&out, "gw").unwrap().rows, 0);
        h.clear(None).unwrap();
        let im = h.import(&out, "gw").unwrap();
        assert_eq!((im.rows, im.from, im.to), (10, Some(0), Some(4300)));
        let b = series(&db, "gw", 0, 5000, &[2], Some(2), 100).unwrap();
        assert_eq!(serde_json::to_value(&a.packs).unwrap(), serde_json::to_value(&b.packs).unwrap());
        assert_eq!(serde_json::to_value(&a.cells).unwrap(), serde_json::to_value(&b.cells).unwrap());
        assert_eq!(b.system.unwrap().ccl[0], Some(150.0));
    }

    #[test]
    fn iso_times() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_791_641_683_466), "2026-10-10T14:14:43.466Z");
    }
}

/// Manual check with a real recording: `OPENBMS_IMPORT=/path/file.jsonl cargo test --release -- --ignored real_import --nocapture`
#[cfg(test)]
mod real {
    use super::*;

    #[test]
    #[ignore]
    fn real_import() {
        let Ok(file) = std::env::var("OPENBMS_IMPORT") else { return };
        let dir = std::env::temp_dir().join(format!("openbms-real-{}", std::process::id()));
        let path = dir.join("history.sqlite");
        let mut h = History::default();
        h.open(&path).unwrap();
        let t0 = std::time::Instant::now();
        let st = h.import(Path::new(&file), "real").unwrap();
        println!("import: {st:?} in {:?}, {} bytes", t0.elapsed(), std::fs::metadata(&path).unwrap().len() + std::fs::metadata(path.with_extension("sqlite-wal")).map_or(0, |m| m.len()));
        let db = reader(&path).unwrap();
        let s = &sites(&db).unwrap()[0];
        let t0 = std::time::Instant::now();
        let r = series(&db, "real", s.from.unwrap(), s.to.unwrap(), &s.packs, Some(1), 1500).unwrap();
        println!("series: {} buckets of {:.1} s, {} packs, {} events in {:?}", r.x.len(), r.step / 1000.0, r.packs.len(), r.events.len(), t0.elapsed());
        // everything older than now becomes minute values
        h.set_settings(Settings { keep_full_days: 0, ..Settings::default() }).unwrap();
        let n: i64 = db.query_row("SELECT COUNT(*) FROM pack", [], |r| r.get(0)).unwrap();
        println!("after merging into minutes: {n} pack rows");
    }
}
