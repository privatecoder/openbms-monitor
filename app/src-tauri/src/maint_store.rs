//! Maintenance database (`maintenance.sqlite` in the app data folder): installations as an address book (name,
//! CAN socket endpoint, pack bus endpoints, the master's BMS serial number), the last master read per
//! installation, and packs that may hang in their bootloader after an interrupted flash, per pack bus endpoint.
//! A file of its own, so a pending bootloader record never depends on the recording.
//!
//! Prepared for installations that own their data (not used yet): boards per place (installation, pack address)
//! identified by String 2, a place's cell type, groups and pairs, diagnosis settings, parameter snapshots, a log
//! index, the state of installation imports, and archived installations.
//!
//! SCHEMA is the base layout with the one-time copies of earlier versions below (by table presence); everything
//! after it are migrations counted in `user_version`, each after a copy `maintenance.sqlite.bak-<n>` (`migrate.rs`).
//! Tables of earlier versions stay in old files and are no longer read.

use crate::history::SnapshotKind;
use openbms_proto::SerialValue;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS installations(id TEXT PRIMARY KEY, name TEXT NOT NULL, control TEXT, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS installation_endpoints(endpoint TEXT PRIMARY KEY, installation TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS installation_links(installation TEXT NOT NULL, endpoint TEXT NOT NULL, PRIMARY KEY(installation, endpoint));
CREATE TABLE IF NOT EXISTS master_serial(installation TEXT PRIMARY KEY, serial TEXT NOT NULL, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS master_read(installation TEXT PRIMARY KEY, data TEXT NOT NULL, marker_on INTEGER NOT NULL, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS bootloader_pending(id INTEGER PRIMARY KEY, endpoint TEXT NOT NULL, pack INTEGER NOT NULL,
  release TEXT NOT NULL, since INTEGER NOT NULL, cleared INTEGER, how TEXT);
";

/// Migrations after SCHEMA, in order. The SQL is frozen as it was written: later changes are new steps.
const MIGRATIONS: &[crate::migrate::Step] = &[installation_data];

/// 1: the tables of installations that own their data.
fn installation_data(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "ALTER TABLE installations ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
         -- one row per board (BMS) seen at a place; another real String 2 there adds a row
         CREATE TABLE boards(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, address INTEGER NOT NULL, bms_serial TEXT NOT NULL,
           battery_serial TEXT, first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL);
         CREATE INDEX boards_place ON boards(installation, address, id);
         -- earlier String 2 values of a board the app itself rewrote (0xA5)
         CREATE TABLE serial_alias(id INTEGER PRIMARY KEY, board INTEGER NOT NULL, serial TEXT NOT NULL, at INTEGER NOT NULL);
         CREATE INDEX serial_alias_board ON serial_alias(board);
         -- JSON settings: per place (cells stay when a board is swapped) and per installation
         CREATE TABLE place(installation TEXT NOT NULL, address INTEGER NOT NULL, cell_type TEXT NOT NULL, PRIMARY KEY(installation, address)) WITHOUT ROWID;
         CREATE TABLE layout(installation TEXT PRIMARY KEY, groups TEXT, pairs TEXT);
         CREATE TABLE diagnosis(installation TEXT PRIMARY KEY, data TEXT NOT NULL);
         -- bms_serial: the board's String 2 at the time, NULL if unclear
         CREATE TABLE param_snapshots(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, address INTEGER NOT NULL, bms_serial TEXT,
           at INTEGER NOT NULL, kind TEXT NOT NULL, block BLOB NOT NULL);
         CREATE INDEX param_snapshots_place ON param_snapshots(installation, address, id);
         -- files per installation (firmware logs); installation NULL: a file from before, never assigned by guessing
         CREATE TABLE logs(id INTEGER PRIMARY KEY, installation TEXT, address INTEGER, at INTEGER NOT NULL, kind TEXT NOT NULL,
           result TEXT, file TEXT NOT NULL UNIQUE);
         CREATE INDEX logs_installation ON logs(installation, at);
         -- an installation import by its export id: completed only when both databases and all files are done
         CREATE TABLE import_status(export_id TEXT PRIMARY KEY, installation TEXT NOT NULL, started INTEGER NOT NULL,
           completed INTEGER, state TEXT);",
    )
}

fn mark_version(db: &Connection, n: usize) -> rusqlite::Result<()> {
    db.pragma_update(None, "user_version", n as i64)
}

/// A display name: required (not blank) and not used by another installation (case-insensitive), so the picker
/// never shows two equal entries. Returns the trimmed name or a refusal key.
pub fn valid_name<'a>(name: &str, others: impl IntoIterator<Item = &'a str>) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("name-required".into());
    }
    if others.into_iter().any(|o| o.trim().to_lowercase() == n.to_lowercase()) {
        return Err("name-taken".into());
    }
    Ok(n.to_string())
}

static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn new_id(now: i64) -> String {
    let n = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("inst-{now:x}-{:x}-{n:x}", std::process::id())
}

/// Physical endpoint, independent of the bus setting: `tcp:host:port` (lower case) or `serial:<path>`.
pub fn endpoint_key(tcp: bool, s: &str) -> String {
    if tcp { format!("tcp:{}", s.trim().to_ascii_lowercase()) } else { format!("serial:{}", s.trim()) }
}

/// Accepts `tcp:<addr>` / `serial:<path>` (prefix in any case) and returns the canonical key.
pub fn normalize_key(s: &str) -> Result<String, String> {
    let s = s.trim();
    let (prefix, rest) = s.split_once(':').ok_or("bad endpoint")?;
    if rest.trim().is_empty() {
        return Err("bad endpoint".into());
    }
    match prefix.to_ascii_lowercase().as_str() {
        "tcp" => Ok(endpoint_key(true, rest)),
        "serial" => Ok(endpoint_key(false, rest)),
        _ => Err("bad endpoint".into()),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ControlEndpoint {
    Serial { path: String },
    Tcp { addr: String },
}

impl ControlEndpoint {
    pub fn key(&self) -> String {
        match self {
            ControlEndpoint::Serial { path } => endpoint_key(false, path),
            ControlEndpoint::Tcp { addr } => endpoint_key(true, addr),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    pub id: String,
    pub name: String,
    /// pack bus endpoints in stored order (the first serial one is the RS485-1/2 port); an endpoint may belong
    /// to several installations
    pub endpoints: Vec<String>,
    pub control: Option<ControlEndpoint>,
    /// the master's BMS serial number (0xA4 at address 0), stored at the first flash; set only by the store
    #[serde(default)]
    pub master_serial: Option<String>,
    /// kept with all its data, never preselected; set only by the store
    #[serde(default)]
    pub archived: bool,
}

/// A board (BMS) at a place, identified by its String 2 (BMS serial number).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub id: i64,
    pub bms_serial: String,
    pub battery_serial: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    /// earlier String 2 values the app itself wrote over, oldest first
    pub aliases: Vec<String>,
}

/// What a serial number read says about the board at a place.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BoardSeen {
    /// the first board identified at this place
    New,
    /// the newest board at this place (also under a String 2 the app wrote over)
    Same,
    /// another real String 2 than the newest board's: a new board row, the previous serial for the notice
    Other { previous: String },
    /// String 2 empty, the placeholder or not read: nothing is claimed
    Unclear,
}

/// Only a real String 2 identifies a board: not empty, not the placeholder, read completely.
fn real(v: &SerialValue) -> Option<&str> {
    match v {
        SerialValue::Set { value, truncated: false } => Some(value),
        _ => None,
    }
}

/// A pack that may sit in its bootloader after an interrupted flash: no other address on this pack bus endpoint
/// is flashed until it answers again or the user confirms it is disconnected or repaired.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub id: i64,
    pub endpoint: String,
    pub pack: u8,
    pub release: String,
    pub since: i64,
}

/// An installation whose last master read showed the marker on, for the banner.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MarkerOn {
    pub installation: String,
    pub name: String,
    pub at: i64,
}

pub struct Store {
    db: Connection,
}

/// (installation, endpoint) of the one-owner table of earlier versions in stored order, without the installation's
/// own CAN socket key.
fn migrated_pack_endpoints(db: &Connection) -> Result<Vec<(String, String)>, String> {
    let mut stmt = db
        .prepare("SELECT e.installation, e.endpoint, i.control FROM installation_endpoints e LEFT JOIN installations i ON i.id = e.installation ORDER BY e.rowid")
        .map_err(e)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))
        .map_err(e)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(e)?;
    Ok(rows
        .into_iter()
        .filter(|(_, ep, control)| {
            let key = control.as_deref().and_then(|c| serde_json::from_str::<ControlEndpoint>(c).ok()).map(|c| c.key());
            key.as_deref() != Some(ep.as_str())
        })
        .map(|(i, ep, _)| (i, ep))
        .collect())
}

fn e(err: rusqlite::Error) -> String {
    err.to_string()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        }
        let db = Connection::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(e)?;
        Self::init(db, path)
    }

    fn init(db: Connection, path: &Path) -> Result<Self, String> {
        let has = |name: &str| -> Result<bool, String> {
            Ok(db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |r| r.get::<_, i64>(0)).map_err(e)? > 0)
        };
        let fresh = !has("installations")?;
        let (had_links, had_pending, had_read) = (has("installation_links")?, has("bootloader_pending")?, has("master_read")?);
        let (old_blocks, old_state) = (has("flash_blocks")?, has("master_state")?);
        // the new tables and the copies of earlier versions' open state: all of it or nothing
        let tx = db.unchecked_transaction().map_err(e)?;
        db.execute_batch(SCHEMA).map_err(e)?;
        let mut pack_eps: Vec<(String, String)> = Vec::new();
        if !had_links || !had_pending {
            pack_eps = migrated_pack_endpoints(&db)?;
        }
        if !had_links {
            // the endpoints of the one-owner table, in their order; the installation's own CAN socket key is not a
            // pack bus endpoint
            for (inst, ep) in &pack_eps {
                db.execute("INSERT OR IGNORE INTO installation_links(installation, endpoint) VALUES (?1, ?2)", params![inst, ep]).map_err(e)?;
            }
        }
        if !had_pending && old_blocks {
            // an unclear or interrupted update stays visible: one record per serial pack bus endpoint of its installation
            let mut stmt = db
                .prepare("SELECT installation, pack, since FROM flash_blocks WHERE cleared IS NULL AND reason IN ('update-interrupted', 'bootloader-unclear', 'update-unclear') ORDER BY id")
                .map_err(e)?;
            let blocks = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u8>(1)?, r.get::<_, i64>(2)?))).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>().map_err(e)?;
            for (inst, pack, since) in blocks {
                for (_, ep) in pack_eps.iter().filter(|(i, ep)| *i == inst && ep.starts_with("serial:")) {
                    db.execute("INSERT INTO bootloader_pending(endpoint, pack, release, since) VALUES (?1, ?2, '', ?3)", params![ep, pack, since]).map_err(e)?;
                }
            }
        }
        if !had_read && old_state {
            // a set marker stays in the banner until a read shows it off
            db.execute(
                "INSERT OR IGNORE INTO master_read(installation, data, marker_on, at) SELECT installation, '{}', 1, at FROM master_state
                 WHERE json_extract(state, '$.state') IN ('patched-on', 'marker-unexplained')",
                [],
            )
            .map_err(e)?;
        }
        tx.commit().map_err(e)?;
        let done: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(e)?;
        // a file created just now has nothing to keep
        crate::migrate::run(&db, (!fresh).then_some(path), done.max(0) as usize, MIGRATIONS, mark_version)?;
        Ok(Self { db })
    }

    /// Run `f` in one transaction: every row it writes is stored, or (on an error) none.
    pub fn atomic<T>(&self, f: impl FnOnce(&Self) -> Result<T, String>) -> Result<T, String> {
        let tx = self.db.unchecked_transaction().map_err(e)?;
        let r = f(self)?;
        tx.commit().map_err(e)?;
        Ok(r)
    }

    pub fn installation(&self, id: &str) -> Result<Installation, String> {
        let (name, control, archived): (String, Option<String>, bool) = self
            .db
            .query_row("SELECT name, control, archived FROM installations WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()
            .map_err(e)?
            .ok_or("unknown-installation")?;
        let mut stmt = self.db.prepare("SELECT endpoint FROM installation_links WHERE installation = ?1 ORDER BY rowid").map_err(e)?;
        let endpoints = stmt.query_map([id], |r| r.get(0)).map_err(e)?.collect::<rusqlite::Result<Vec<String>>>().map_err(e)?;
        let control = control.map(|c| serde_json::from_str(&c)).transpose().map_err(|err| err.to_string())?;
        let master_serial = self.db.query_row("SELECT serial FROM master_serial WHERE installation = ?1", [id], |r| r.get(0)).optional().map_err(e)?;
        Ok(Installation { id: id.into(), name, endpoints, control, master_serial, archived })
    }

    pub fn installations(&self) -> Result<Vec<Installation>, String> {
        let mut stmt = self.db.prepare("SELECT id FROM installations ORDER BY created, rowid").map_err(e)?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>().map_err(e)?;
        ids.iter().map(|id| self.installation(id)).collect()
    }

    /// Every installation that lists the endpoint, or has it as its CAN socket. `active_only`: without archived ones
    /// (they are never preselected).
    pub fn installations_of(&self, endpoint: &str, active_only: bool) -> Result<Vec<Installation>, String> {
        Ok(self
            .installations()?
            .into_iter()
            .filter(|i| !(active_only && i.archived))
            .filter(|i| i.endpoints.iter().any(|k| k == endpoint) || i.control.as_ref().is_some_and(|c| c.key() == endpoint))
            .collect())
    }

    /// Archive (or bring back) an installation; everything it owns stays.
    #[allow(dead_code)] // for the installations view
    pub fn set_archived(&self, id: &str, archived: bool) -> Result<(), String> {
        self.installation(id)?;
        self.db.execute("UPDATE installations SET archived = ?2 WHERE id = ?1", params![id, archived]).map_err(e)?;
        Ok(())
    }

    /// Name, CAN socket and endpoints. Endpoints are shared freely between installations.
    pub fn save_installation(&self, i: &Installation) -> Result<(), String> {
        self.atomic(|s| s.save_rows(i))
    }

    fn save_rows(&self, i: &Installation) -> Result<(), String> {
        let old = self.installation(&i.id)?;
        let mut endpoints: Vec<String> = Vec::new();
        for ep in &i.endpoints {
            let k = normalize_key(ep)?;
            if !endpoints.contains(&k) {
                endpoints.push(k);
            }
        }
        let others: Vec<String> = self.installations()?.into_iter().filter(|o| o.id != i.id).map(|o| o.name).collect();
        let name = if i.name.trim() == old.name.trim() { old.name } else { valid_name(&i.name, others.iter().map(String::as_str))? };
        let control = i.control.as_ref().map(serde_json::to_string).transpose().map_err(|err| err.to_string())?;
        self.db.execute("UPDATE installations SET name = ?2, control = ?3 WHERE id = ?1", params![i.id, name, control]).map_err(e)?;
        self.db.execute("DELETE FROM installation_links WHERE installation = ?1", [&i.id]).map_err(e)?;
        for ep in &endpoints {
            self.db.execute("INSERT INTO installation_links(installation, endpoint) VALUES (?1, ?2)", params![i.id, ep]).map_err(e)?;
        }
        Ok(())
    }

    /// A new installation with its name, CAN socket and endpoints, in one transaction.
    pub fn create_installation(&self, name: &str, control: Option<ControlEndpoint>, endpoints: Vec<String>) -> Result<Installation, String> {
        self.atomic(|s| {
            let names: Vec<String> = s.installations()?.into_iter().map(|i| i.name).collect();
            let name = valid_name(name, names.iter().map(String::as_str))?;
            let now = crate::history::now_ms();
            let id = new_id(now);
            s.db.execute("INSERT INTO installations(id, name, control, created) VALUES (?1, ?2, NULL, ?3)", params![id, name, now]).map_err(e)?;
            s.save_rows(&Installation { id: id.clone(), name, endpoints, control, master_serial: None, archived: false })?;
            s.installation(&id)
        })
    }

    /// Delete an installation with its endpoints, stored serial number, master read, boards, settings and snapshots;
    /// log files and their index rows stay. Refused ("installation-busy") while it has open work: the last master
    /// read showed the marker on, or a pack on one of its endpoints may hang in its bootloader.
    pub fn delete_installation(&self, id: &str) -> Result<(), String> {
        self.atomic(|s| {
            let inst = s.installation(id)?;
            let mut pending = false;
            for ep in &inst.endpoints {
                pending |= !s.pending_on(ep)?.is_empty();
            }
            if pending || s.marker_last_on(id)? {
                return Err("installation-busy".into());
            }
            for sql in [
                "DELETE FROM installation_links WHERE installation = ?1",
                "DELETE FROM master_serial WHERE installation = ?1",
                "DELETE FROM master_read WHERE installation = ?1",
                "DELETE FROM serial_alias WHERE board IN (SELECT id FROM boards WHERE installation = ?1)",
                "DELETE FROM boards WHERE installation = ?1",
                "DELETE FROM place WHERE installation = ?1",
                "DELETE FROM layout WHERE installation = ?1",
                "DELETE FROM diagnosis WHERE installation = ?1",
                "DELETE FROM param_snapshots WHERE installation = ?1",
                "DELETE FROM installations WHERE id = ?1",
            ] {
                s.db.execute(sql, [id]).map_err(e)?;
            }
            Ok(())
        })
    }

    pub fn set_master_serial(&self, inst: &str, serial: &str, now: i64) -> Result<(), String> {
        self.installation(inst)?;
        self.db.execute("INSERT OR REPLACE INTO master_serial(installation, serial, at) VALUES (?1, ?2, ?3)", params![inst, serial, now]).map_err(e)?;
        Ok(())
    }

    /// The last master read worth keeping (JSON for the UI) and whether it showed the marker on.
    pub fn set_master_read(&self, inst: &str, data: &str, marker_on: bool, now: i64) -> Result<(), String> {
        self.db.execute("INSERT OR REPLACE INTO master_read(installation, data, marker_on, at) VALUES (?1, ?2, ?3, ?4)", params![inst, data, marker_on, now]).map_err(e)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn master_read(&self, inst: &str) -> Result<Option<(String, i64)>, String> {
        self.db.query_row("SELECT data, at FROM master_read WHERE installation = ?1", [inst], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(e)
    }

    /// The last master read worth keeping showed the marker on.
    pub fn marker_last_on(&self, inst: &str) -> Result<bool, String> {
        Ok(self.db.query_row("SELECT marker_on FROM master_read WHERE installation = ?1", [inst], |r| r.get::<_, bool>(0)).optional().map_err(e)?.unwrap_or(false))
    }

    /// Installations whose last read showed the marker on (banner), oldest first.
    pub fn markers_on(&self) -> Result<Vec<MarkerOn>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT m.installation, i.name, m.at FROM master_read m JOIN installations i ON i.id = m.installation WHERE m.marker_on = 1 ORDER BY m.at")
            .map_err(e)?;
        let rows = stmt.query_map([], |r| Ok(MarkerOn { installation: r.get(0)?, name: r.get(1)?, at: r.get(2)? })).map_err(e)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
    }

    /// Written before the restart into the bootloader, so a crash or power loss leaves it behind.
    pub fn add_pending(&self, endpoint: &str, pack: u8, release: &str, now: i64) -> Result<i64, String> {
        self.db
            .execute("INSERT INTO bootloader_pending(endpoint, pack, release, since) VALUES (?1, ?2, ?3, ?4)", params![endpoint, pack, release, now])
            .map_err(e)?;
        Ok(self.db.last_insert_rowid())
    }

    /// `how`: "flashed", "preflight" (failed before the restart), "answered" (the pack answered 0x51 again) or
    /// "confirmed" (the user: disconnected or repaired).
    pub fn clear_pending(&self, id: i64, how: &str, now: i64) -> Result<(), String> {
        self.db.execute("UPDATE bootloader_pending SET cleared = ?2, how = ?3 WHERE id = ?1 AND cleared IS NULL", params![id, now, how]).map_err(e)?;
        Ok(())
    }

    /// Clear every record of these packs on the endpoint: they answered 0x51. Returns the packs cleared.
    pub fn clear_answered(&self, endpoint: &str, packs: &[u8], now: i64) -> Result<Vec<u8>, String> {
        let mut cleared = Vec::new();
        for p in self.pending_on(endpoint)?.into_iter().filter(|p| packs.contains(&p.pack)) {
            self.clear_pending(p.id, "answered", now)?;
            cleared.push(p.pack);
        }
        Ok(cleared)
    }

    fn pending_where(&self, filter: &str, arg: Option<&str>) -> Result<Vec<Pending>, String> {
        let sql = format!("SELECT id, endpoint, pack, release, since FROM bootloader_pending WHERE cleared IS NULL {filter} ORDER BY id");
        let mut stmt = self.db.prepare(&sql).map_err(e)?;
        let map = |r: &rusqlite::Row| Ok(Pending { id: r.get(0)?, endpoint: r.get(1)?, pack: r.get(2)?, release: r.get(3)?, since: r.get(4)? });
        let params: Vec<&str> = arg.into_iter().collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(params), map).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>();
        rows.map_err(e)
    }

    pub fn pending_on(&self, endpoint: &str) -> Result<Vec<Pending>, String> {
        self.pending_where("AND endpoint = ?1", Some(endpoint))
    }

    pub fn all_pending(&self) -> Result<Vec<Pending>, String> {
        self.pending_where("", None)
    }
}

/// What an installation owns per place. Not used yet: the connection does not carry its installation so far.
#[allow(dead_code)]
impl Store {
    /// The boards seen at a place, newest first.
    pub fn boards(&self, inst: &str, address: u8) -> Result<Vec<Board>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT id, bms_serial, battery_serial, first_seen, last_seen FROM boards WHERE installation = ?1 AND address = ?2 ORDER BY id DESC")
            .map_err(e)?;
        let rows = stmt
            .query_map(params![inst, address], |r| {
                Ok(Board { id: r.get(0)?, bms_serial: r.get(1)?, battery_serial: r.get(2)?, first_seen: r.get(3)?, last_seen: r.get(4)?, aliases: vec![] })
            })
            .map_err(e)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(e)?;
        let mut alias = self.db.prepare("SELECT serial FROM serial_alias WHERE board = ?1 ORDER BY id").map_err(e)?;
        rows.into_iter()
            .map(|b| Ok(Board { aliases: alias.query_map([b.id], |r| r.get(0)).map_err(e)?.collect::<rusqlite::Result<_>>().map_err(e)?, ..b }))
            .collect()
    }

    /// The newest board at a place: the one the place's data belongs to until another String 2 is read there.
    pub fn current_board(&self, inst: &str, address: u8) -> Result<Option<Board>, String> {
        Ok(self.boards(inst, address)?.into_iter().next())
    }

    /// A successful serial number read at a place (String 2 and String 1 as read).
    pub fn board_seen(&self, inst: &str, address: u8, bms: &SerialValue, battery: &SerialValue, now: i64) -> Result<BoardSeen, String> {
        self.installation(inst)?;
        self.atomic(|s| s.seen_rows(inst, address, bms, battery, now))
    }

    fn seen_rows(&self, inst: &str, address: u8, bms: &SerialValue, battery: &SerialValue, now: i64) -> Result<BoardSeen, String> {
        let Some(serial) = real(bms) else { return Ok(BoardSeen::Unclear) };
        // String 1 is changeable and only informational: kept as last read; not read leaves it as it was
        let battery = match battery {
            SerialValue::Set { value, .. } => Some(Some(value.as_str())),
            SerialValue::Empty | SerialValue::Placeholder { .. } => Some(None),
            SerialValue::Unavailable | SerialValue::NoAnswer => None,
        };
        match self.current_board(inst, address)? {
            Some(b) if b.bms_serial == serial || b.aliases.iter().any(|a| a == serial) => {
                // a String 2 the app wrote over came back (e.g. rewritten by another tool): still this board
                if b.bms_serial != serial {
                    self.rename_board(&b, serial, now)?;
                }
                self.db.execute("UPDATE boards SET last_seen = ?2 WHERE id = ?1", params![b.id, now]).map_err(e)?;
                if let Some(bat) = battery {
                    self.db.execute("UPDATE boards SET battery_serial = ?2 WHERE id = ?1", params![b.id, bat]).map_err(e)?;
                }
                Ok(BoardSeen::Same)
            }
            previous => {
                self.db
                    .execute(
                        "INSERT INTO boards(installation, address, bms_serial, battery_serial, first_seen, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                        params![inst, address, serial, battery.flatten(), now],
                    )
                    .map_err(e)?;
                Ok(previous.map_or(BoardSeen::New, |b| BoardSeen::Other { previous: b.bms_serial }))
            }
        }
    }

    fn rename_board(&self, b: &Board, serial: &str, now: i64) -> Result<(), String> {
        self.db.execute("INSERT INTO serial_alias(board, serial, at) VALUES (?1, ?2, ?3)", params![b.id, b.bms_serial, now]).map_err(e)?;
        self.db.execute("UPDATE boards SET bms_serial = ?2, last_seen = ?3 WHERE id = ?1", params![b.id, serial, now]).map_err(e)?;
        Ok(())
    }

    /// The app's own String 2 write (0xA5) the BMS accepted. `before`: String 2 read right before the write;
    /// `after`: read back after it. Only when `before` was a real serial is it the same board for sure: it is recorded
    /// (a board swap shows there) and keeps its row under the new serial, the old one as an alias. Otherwise the
    /// read-back value counts like any read. A read-back that is not a real serial (empty, the placeholder, not
    /// read) changes nothing: that board can no longer be told apart.
    pub fn board_written(&self, inst: &str, address: u8, before: &SerialValue, after: &SerialValue, now: i64) -> Result<(), String> {
        self.installation(inst)?;
        let Some(after_serial) = real(after) else { return Ok(()) };
        self.atomic(|s| {
            if real(before).is_none() {
                return s.seen_rows(inst, address, after, &SerialValue::NoAnswer, now).map(|_| ());
            }
            s.seen_rows(inst, address, before, &SerialValue::NoAnswer, now)?;
            // the newest board now carries `before`
            match s.current_board(inst, address)? {
                Some(b) if b.bms_serial != after_serial => s.rename_board(&b, after_serial, now),
                _ => Ok(()),
            }
        })
    }

    /// Keep a parameter block of a place with the String 2 of its newest board (None: no board identified yet).
    pub fn add_param_snapshot(&self, inst: &str, address: u8, kind: SnapshotKind, block: &[u8], now: i64) -> Result<i64, String> {
        let serial = self.current_board(inst, address)?.map(|b| b.bms_serial);
        self.db
            .execute(
                "INSERT INTO param_snapshots(installation, address, bms_serial, at, kind, block) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![inst, address, serial, now, kind.as_str(), block],
            )
            .map_err(e)?;
        Ok(self.db.last_insert_rowid())
    }

    /// The cell type of a place (JSON), None removes it.
    pub fn set_cell_type(&self, inst: &str, address: u8, cell_type: Option<&str>) -> Result<(), String> {
        match cell_type {
            Some(c) => self.db.execute("INSERT OR REPLACE INTO place(installation, address, cell_type) VALUES (?1, ?2, ?3)", params![inst, address, c]),
            None => self.db.execute("DELETE FROM place WHERE installation = ?1 AND address = ?2", params![inst, address]),
        }
        .map_err(e)?;
        Ok(())
    }

    /// Address → cell type (JSON) of every place that has one.
    pub fn cell_types(&self, inst: &str) -> Result<Vec<(u8, String)>, String> {
        let mut stmt = self.db.prepare("SELECT address, cell_type FROM place WHERE installation = ?1 ORDER BY address").map_err(e)?;
        let rows = stmt.query_map([inst], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>();
        rows.map_err(e)
    }

    /// Groups and pairs (JSON each).
    pub fn set_layout(&self, inst: &str, groups: Option<&str>, pairs: Option<&str>) -> Result<(), String> {
        self.db.execute("INSERT OR REPLACE INTO layout(installation, groups, pairs) VALUES (?1, ?2, ?3)", params![inst, groups, pairs]).map_err(e)?;
        Ok(())
    }

    pub fn layout(&self, inst: &str) -> Result<(Option<String>, Option<String>), String> {
        let r = self.db.query_row("SELECT groups, pairs FROM layout WHERE installation = ?1", [inst], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(e)?;
        Ok(r.unwrap_or((None, None)))
    }

    /// Diagnosis settings (JSON).
    pub fn set_diagnosis(&self, inst: &str, data: &str) -> Result<(), String> {
        self.db.execute("INSERT OR REPLACE INTO diagnosis(installation, data) VALUES (?1, ?2)", params![inst, data]).map_err(e)?;
        Ok(())
    }

    pub fn diagnosis(&self, inst: &str) -> Result<Option<String>, String> {
        self.db.query_row("SELECT data FROM diagnosis WHERE installation = ?1", [inst], |r| r.get(0)).optional().map_err(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("openbms-maint-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("maintenance.sqlite")
    }

    #[test]
    fn endpoint_keys_name_the_physical_endpoint() {
        assert_eq!(endpoint_key(true, " Gateway.local:4196 "), "tcp:gateway.local:4196");
        assert_eq!(endpoint_key(false, "/dev/cu.usbserial-X"), "serial:/dev/cu.usbserial-X");
        assert_eq!(normalize_key("TCP: GW:4196 ").unwrap(), "tcp:gw:4196");
        assert_eq!(normalize_key("Serial:/dev/X").unwrap(), "serial:/dev/X");
        assert_eq!(normalize_key("nonsense"), Err("bad endpoint".to_string()));
        assert_eq!(normalize_key("tcp:"), Err("bad endpoint".to_string()));
    }

    #[test]
    fn endpoints_are_shared_between_installations() {
        let s = Store::open(&temp("shared")).unwrap();
        let a = s.create_installation("A", Some(ControlEndpoint::Tcp { addr: "GW:4196".into() }), vec!["serial:/dev/p".into()]).unwrap();
        let b = s.create_installation("B", Some(ControlEndpoint::Tcp { addr: "gw:4196".into() }), vec!["serial:/dev/p".into(), "SERIAL:/dev/p".into()]).unwrap();
        assert_eq!(b.endpoints, vec!["serial:/dev/p".to_string()]);
        let ids = |k: &str| s.installations_of(k, false).unwrap().into_iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ids("serial:/dev/p"), vec![a.id.clone(), b.id.clone()]);
        assert_eq!(ids("tcp:gw:4196"), vec![a.id.clone(), b.id.clone()]);
        // removing it from one leaves the other
        let mut a2 = a.clone();
        a2.endpoints = vec!["serial:/dev/q".into(), "serial:/dev/p".into()];
        s.save_installation(&a2).unwrap();
        assert_eq!(s.installation(&a.id).unwrap().endpoints, vec!["serial:/dev/q".to_string(), "serial:/dev/p".to_string()]);
        a2.endpoints.clear();
        s.save_installation(&a2).unwrap();
        assert_eq!(ids("serial:/dev/p"), vec![b.id.clone()]);
    }

    #[test]
    fn names_are_required_and_unique() {
        assert_eq!(valid_name("  Lab  ", ["Bank"]), Ok("Lab".to_string()));
        assert_eq!(valid_name("   ", []), Err("name-required".to_string()));
        assert_eq!(valid_name("bank ", ["Bank"]), Err("name-taken".to_string()));
        let s = Store::open(&temp("names")).unwrap();
        let a = s.create_installation("Bank A", None, vec![]).unwrap();
        assert_eq!(s.create_installation(" bank a", None, vec![]).err(), Some("name-taken".to_string()));
        assert_eq!(s.installations().unwrap().len(), 1, "a refused create leaves no row");
        let b = s.create_installation("B", None, vec![]).unwrap();
        let mut b2 = b.clone();
        b2.name = "BANK A".into();
        assert_eq!(s.save_installation(&b2), Err("name-taken".to_string()));
        assert!(s.save_installation(&Installation { id: "nope".into(), ..a }).is_err());
    }

    #[test]
    fn master_serial_and_read_belong_to_the_installation_and_go_with_it() {
        let s = Store::open(&temp("serial")).unwrap();
        let a = s.create_installation("A", None, vec!["serial:/dev/p".into()]).unwrap();
        s.set_master_serial(&a.id, "SN-1", 1).unwrap();
        assert_eq!(s.installation(&a.id).unwrap().master_serial.as_deref(), Some("SN-1"));
        // saving the address book never touches the stored serial
        s.save_installation(&Installation { master_serial: None, ..s.installation(&a.id).unwrap() }).unwrap();
        assert_eq!(s.installation(&a.id).unwrap().master_serial.as_deref(), Some("SN-1"));
        s.set_master_read(&a.id, "{}", true, 2).unwrap();
        assert_eq!(s.markers_on().unwrap(), vec![MarkerOn { installation: a.id.clone(), name: "A".into(), at: 2 }]);
        s.set_master_read(&a.id, "{}", false, 3).unwrap();
        assert!(s.markers_on().unwrap().is_empty());
        let p = s.add_pending("serial:/dev/p", 3, "16.06.13", 4).unwrap();
        // open work: a pack on its endpoint may hang in its bootloader, or the marker was on
        assert_eq!(s.delete_installation(&a.id), Err("installation-busy".to_string()));
        s.clear_pending(p, "confirmed", 5).unwrap();
        s.set_master_read(&a.id, "{}", true, 6).unwrap();
        assert_eq!(s.delete_installation(&a.id), Err("installation-busy".to_string()));
        s.set_master_read(&a.id, "{}", false, 7).unwrap();
        let p = s.add_pending("serial:/dev/other", 3, "16.06.13", 8).unwrap();
        s.delete_installation(&a.id).unwrap();
        assert!(s.installations().unwrap().is_empty());
        assert_eq!(s.master_read(&a.id).unwrap(), None);
        assert!(s.set_master_serial(&a.id, "SN-2", 5).is_err());
        // the pending record belongs to the endpoint
        assert_eq!(s.all_pending().unwrap().iter().map(|x| x.id).collect::<Vec<_>>(), vec![p]);
    }

    #[test]
    fn pending_survives_reopen_and_clears_by_answer_or_confirmation() {
        let path = temp("pending");
        let (p3, p5) = {
            let s = Store::open(&path).unwrap();
            (s.add_pending("serial:/dev/p", 3, "16.06.13", 1).unwrap(), s.add_pending("serial:/dev/p", 5, "16.06.13", 2).unwrap())
        };
        let s = Store::open(&path).unwrap();
        assert_eq!(s.pending_on("serial:/dev/p").unwrap().iter().map(|p| p.pack).collect::<Vec<_>>(), vec![3, 5]);
        assert!(s.pending_on("serial:/dev/q").unwrap().is_empty());
        assert_eq!(s.clear_answered("serial:/dev/p", &[0, 1, 3], 3).unwrap(), vec![3]);
        assert_eq!(s.pending_on("serial:/dev/p").unwrap().iter().map(|p| p.id).collect::<Vec<_>>(), vec![p5]);
        s.clear_pending(p5, "confirmed", 4).unwrap();
        assert!(s.all_pending().unwrap().is_empty());
        let how: String = s.db.query_row("SELECT how FROM bootloader_pending WHERE id = ?1", [p3], |r| r.get(0)).unwrap();
        assert_eq!(how, "answered");
    }

    fn version(s: &Store) -> i64 {
        s.db.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_file_of_the_base_layout_is_migrated_once_with_a_copy() {
        let path = temp("migrate");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(SCHEMA).unwrap();
            db.execute_batch(
                "INSERT INTO installations VALUES ('a', 'A', NULL, 1);
                 INSERT INTO installation_links VALUES ('a', 'serial:/dev/p');
                 INSERT INTO master_serial VALUES ('a', 'SN-1', 2);
                 INSERT INTO bootloader_pending(endpoint, pack, release, since) VALUES ('serial:/dev/p', 3, '16.06.13', 3);",
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(version(&s), MIGRATIONS.len() as i64);
        let a = s.installation("a").unwrap();
        assert_eq!((a.endpoints.clone(), a.master_serial.as_deref(), a.archived), (vec!["serial:/dev/p".to_string()], Some("SN-1"), false));
        assert_eq!(s.all_pending().unwrap().iter().map(|p| p.pack).collect::<Vec<_>>(), vec![3]);
        // the copy holds the file as it was
        assert_eq!(crate::migrate::backups(&path), vec!["maintenance.sqlite.bak-1".to_string()]);
        let bak = Connection::open(path.with_file_name("maintenance.sqlite.bak-1")).unwrap();
        assert!(bak.prepare("SELECT archived FROM installations").is_err());
        assert_eq!(bak.query_row("SELECT COUNT(*) FROM bootloader_pending", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        drop(s);
        let s = Store::open(&path).unwrap();
        assert_eq!(crate::migrate::backups(&path).len(), 1, "nothing left to migrate");
        assert_eq!(s.all_pending().unwrap().len(), 1);
    }

    #[test]
    fn a_new_file_has_every_table_and_no_copy() {
        let path = temp("fresh");
        let s = Store::open(&path).unwrap();
        assert_eq!(version(&s), MIGRATIONS.len() as i64);
        assert!(crate::migrate::backups(&path).is_empty());
        let a = s.create_installation("A", None, vec![]).unwrap();
        s.set_cell_type(&a.id, 1, Some("{\"id\":\"x\"}")).unwrap();
        s.set_cell_type(&a.id, 2, Some("{}")).unwrap();
        s.set_cell_type(&a.id, 2, None).unwrap();
        assert_eq!(s.cell_types(&a.id).unwrap(), vec![(1, "{\"id\":\"x\"}".to_string())]);
        assert_eq!(s.layout(&a.id).unwrap(), (None, None));
        s.set_layout(&a.id, Some("[]"), None).unwrap();
        assert_eq!(s.layout(&a.id).unwrap(), (Some("[]".into()), None));
        s.set_diagnosis(&a.id, "{}").unwrap();
        assert_eq!(s.diagnosis(&a.id).unwrap().as_deref(), Some("{}"));
        drop(s);
        Store::open(&path).unwrap();
        assert!(crate::migrate::backups(&path).is_empty());
    }

    #[test]
    fn archived_installations_can_be_left_out_of_the_endpoint_lookup() {
        let s = Store::open(&temp("archived")).unwrap();
        let a = s.create_installation("A", Some(ControlEndpoint::Tcp { addr: "gw:4196".into() }), vec![]).unwrap();
        let b = s.create_installation("B", Some(ControlEndpoint::Tcp { addr: "gw:4196".into() }), vec![]).unwrap();
        s.set_archived(&a.id, true).unwrap();
        let ids = |active: bool| s.installations_of("tcp:gw:4196", active).unwrap().into_iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ids(true), vec![b.id.clone()]);
        assert_eq!(ids(false), vec![a.id.clone(), b.id.clone()]);
        // saving the address book keeps the flag
        s.save_installation(&Installation { archived: false, ..s.installation(&a.id).unwrap() }).unwrap();
        assert!(s.installation(&a.id).unwrap().archived);
        s.set_archived(&a.id, false).unwrap();
        assert_eq!(ids(true).len(), 2);
        assert!(s.set_archived("nope", true).is_err());
    }

    fn set(v: &str) -> SerialValue {
        SerialValue::Set { value: v.into(), truncated: false }
    }

    fn serials(s: &Store, inst: &str, address: u8) -> Vec<(String, Vec<String>)> {
        s.boards(inst, address).unwrap().into_iter().map(|b| (b.bms_serial, b.aliases)).collect()
    }

    #[test]
    fn only_a_real_string_2_identifies_a_board() {
        let s = Store::open(&temp("boards")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap();
        let none = SerialValue::NoAnswer;
        for unclear in [SerialValue::Empty, SerialValue::Placeholder { value: "EMU110110S-XXXX-20000101-00001".into() }, SerialValue::NoAnswer, SerialValue::Unavailable,
            SerialValue::Set { value: "SYN-CUT".into(), truncated: true }]
        {
            assert_eq!(s.board_seen(&a.id, 7, &unclear, &set("BAT-1"), 1).unwrap(), BoardSeen::Unclear);
        }
        assert!(s.boards(&a.id, 7).unwrap().is_empty());
        assert_eq!(s.board_seen(&a.id, 7, &set("SYN-A"), &set("BAT-1"), 10).unwrap(), BoardSeen::New);
        assert_eq!(s.board_seen(&a.id, 7, &set("SYN-A"), &none, 20).unwrap(), BoardSeen::Same);
        let b = s.current_board(&a.id, 7).unwrap().unwrap();
        assert_eq!((b.first_seen, b.last_seen, b.battery_serial.as_deref()), (10, 20, Some("BAT-1")), "String 1 not read: kept");
        s.board_seen(&a.id, 7, &set("SYN-A"), &SerialValue::Empty, 25).unwrap();
        assert_eq!(s.current_board(&a.id, 7).unwrap().unwrap().battery_serial, None, "read as empty");
        // an unreadable String 2 claims nothing, not even last_seen
        s.board_seen(&a.id, 7, &SerialValue::Empty, &set("BAT-2"), 30).unwrap();
        assert_eq!(s.current_board(&a.id, 7).unwrap().unwrap().last_seen, 25);
        // another real serial at the place: a new board row
        assert_eq!(s.board_seen(&a.id, 7, &set("SYN-B"), &none, 40).unwrap(), BoardSeen::Other { previous: "SYN-A".into() });
        assert_eq!(serials(&s, &a.id, 7), vec![("SYN-B".into(), vec![]), ("SYN-A".into(), vec![])]);
        // places are apart: the same serial at another address is a board there
        assert_eq!(s.board_seen(&a.id, 8, &set("SYN-B"), &none, 41).unwrap(), BoardSeen::New);
        assert!(s.board_seen("nope", 7, &set("SYN-B"), &none, 42).is_err());
    }

    #[test]
    fn the_apps_own_string_2_write_renames_the_board() {
        let s = Store::open(&temp("rename")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap();
        let none = SerialValue::NoAnswer;
        s.board_seen(&a.id, 2, &set("SYN-A"), &none, 10).unwrap();
        s.board_written(&a.id, 2, &set("SYN-A"), &set("SYN-A2"), 20).unwrap();
        assert_eq!(serials(&s, &a.id, 2), vec![("SYN-A2".into(), vec!["SYN-A".into()])], "no board swap");
        let b = s.current_board(&a.id, 2).unwrap().unwrap();
        assert_eq!((b.first_seen, b.last_seen), (10, 20));
        assert_eq!(s.board_seen(&a.id, 2, &set("SYN-A2"), &none, 30).unwrap(), BoardSeen::Same);
        // the old serial written back by another tool: still this board
        assert_eq!(s.board_seen(&a.id, 2, &set("SYN-A"), &none, 40).unwrap(), BoardSeen::Same);
        assert_eq!(serials(&s, &a.id, 2), vec![("SYN-A".into(), vec!["SYN-A".into(), "SYN-A2".into()])]);
        // read back as empty or the placeholder: the board can no longer be told apart, nothing changes
        s.board_written(&a.id, 2, &set("SYN-A"), &SerialValue::Empty, 50).unwrap();
        s.board_written(&a.id, 2, &set("SYN-A"), &SerialValue::Placeholder { value: "EMU110110S-XXXX".into() }, 50).unwrap();
        assert_eq!(s.current_board(&a.id, 2).unwrap().unwrap().last_seen, 40);
        // String 2 before the write not read: not sure it is the same board, the new serial counts like a read
        s.board_written(&a.id, 2, &none, &set("SYN-Z"), 55).unwrap();
        assert_eq!(serials(&s, &a.id, 2).len(), 2);
        assert_eq!(serials(&s, &a.id, 2)[0], ("SYN-Z".into(), vec![]));
        // read before the write shows another board: that one is recorded and renamed, the others stay
        s.board_written(&a.id, 2, &set("SYN-B"), &set("SYN-B2"), 60).unwrap();
        assert_eq!(serials(&s, &a.id, 2)[0], ("SYN-B2".into(), vec!["SYN-B".into()]));
        assert_eq!(s.boards(&a.id, 2).unwrap().len(), 3);
        // a place without a board (placeholder before): the read-back serial identifies it from now on
        s.board_written(&a.id, 5, &SerialValue::Placeholder { value: "EMU110110S-XXXX".into() }, &set("SYN-C"), 70).unwrap();
        assert_eq!(serials(&s, &a.id, 5), vec![("SYN-C".into(), vec![])]);
    }

    #[test]
    fn snapshots_carry_the_board_and_go_with_the_installation() {
        let s = Store::open(&temp("snap")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap();
        s.add_param_snapshot(&a.id, 1, SnapshotKind::Read, &[1, 2], 5).unwrap();
        s.board_seen(&a.id, 1, &set("SYN-A"), &SerialValue::NoAnswer, 6).unwrap();
        s.add_param_snapshot(&a.id, 1, SnapshotKind::Written, &[3], 7).unwrap();
        let rows: Vec<(Option<String>, String, Vec<u8>)> = s
            .db
            .prepare("SELECT bms_serial, kind, block FROM param_snapshots ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![(None, "read".into(), vec![1, 2]), (Some("SYN-A".into()), "written".into(), vec![3])]);
        s.board_written(&a.id, 1, &set("SYN-A"), &set("SYN-A2"), 8).unwrap();
        s.set_cell_type(&a.id, 1, Some("{}")).unwrap();
        let b = s.create_installation("B", None, vec![]).unwrap();
        s.board_seen(&b.id, 1, &set("SYN-X"), &SerialValue::NoAnswer, 9).unwrap();
        s.delete_installation(&a.id).unwrap();
        for table in ["boards", "serial_alias", "param_snapshots", "place"] {
            let n: i64 = s.db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
            assert_eq!(n, i64::from(table == "boards"), "{table}");
        }
    }

    #[test]
    fn an_existing_database_keeps_its_installations_and_copies_the_endpoints_once() {
        let path = temp("legacy");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE installations(id TEXT PRIMARY KEY, name TEXT NOT NULL, control TEXT, created INTEGER NOT NULL);
                 CREATE TABLE installation_endpoints(endpoint TEXT PRIMARY KEY, installation TEXT NOT NULL);
                 CREATE TABLE master_state(installation TEXT PRIMARY KEY, state TEXT NOT NULL, at INTEGER NOT NULL);
                 CREATE TABLE flash_blocks(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, pack INTEGER NOT NULL, reason TEXT NOT NULL,
                   expect TEXT, since INTEGER NOT NULL, cleared INTEGER, how TEXT);
                 INSERT INTO installations VALUES ('old', 'Old', '{\"kind\":\"tcp\",\"addr\":\"gw:4196\"}', 1);
                 INSERT INTO installations VALUES ('one', 'One', '{\"kind\":\"serial\",\"path\":\"/dev/c\"}', 2);
                 INSERT INTO installation_endpoints VALUES ('serial:/dev/p', 'old');
                 INSERT INTO installation_endpoints VALUES ('tcp:gw:4196', 'old');
                 INSERT INTO installation_endpoints VALUES ('serial:/dev/c', 'one');
                 INSERT INTO flash_blocks(installation, pack, reason, since) VALUES ('old', 3, 'update-unclear', 1);
                 INSERT INTO flash_blocks(installation, pack, reason, since) VALUES ('old', 4, 'master-unclear', 2);
                 INSERT INTO flash_blocks(installation, pack, reason, since, cleared) VALUES ('old', 5, 'bootloader-unclear', 3, 4);
                 INSERT INTO master_state VALUES ('old', '{\"state\":\"patched-on\"}', 7);
                 INSERT INTO master_state VALUES ('one', '{\"state\":\"no-0x64\"}', 8);",
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        let old = s.installation("old").unwrap();
        assert_eq!((old.name.as_str(), old.endpoints.clone()), ("Old", vec!["serial:/dev/p".to_string()]));
        assert_eq!(old.control, Some(ControlEndpoint::Tcp { addr: "gw:4196".into() }));
        // the CAN socket key of an installation is no pack bus link
        assert!(s.installation("one").unwrap().endpoints.is_empty());
        // the open unclear update stays visible on the serial pack bus endpoint; master-unclear and cleared ones do not
        assert_eq!(s.all_pending().unwrap().iter().map(|p| (p.endpoint.as_str(), p.pack, p.since)).collect::<Vec<_>>(), vec![("serial:/dev/p", 3, 1)]);
        // a set marker stays in the banner
        assert_eq!(s.markers_on().unwrap().iter().map(|m| (m.installation.as_str(), m.at)).collect::<Vec<_>>(), vec![("old", 7)]);
        assert!(s.marker_last_on("old").unwrap() && !s.marker_last_on("one").unwrap());
        // the copy runs once: endpoints removed later are not copied back at the next start
        s.save_installation(&Installation { endpoints: vec![], ..old }).unwrap();
        drop(s);
        let s = Store::open(&path).unwrap();
        assert!(s.installation("old").unwrap().endpoints.is_empty());
        assert_eq!(s.all_pending().unwrap().len(), 1, "the copies run once");
        // the old tables stay, unread
        let n: i64 = s.db.query_row("SELECT COUNT(*) FROM flash_blocks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 3);
    }
}
