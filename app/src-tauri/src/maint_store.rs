//! Maintenance state per installation (`maintenance.sqlite` in the app data folder): installations with
//! their endpoints and control channel, the last confirmed master state, the maintenance session and
//! flash blocks after an unclear update. A file of its own: the history database is moved aside when its
//! schema changes, and an open session must never vanish that way. Tables are only ever added.

use openbms_flash::maint::policy::MasterState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS installations(id TEXT PRIMARY KEY, name TEXT NOT NULL, control TEXT, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS installation_endpoints(endpoint TEXT PRIMARY KEY, installation TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS master_state(installation TEXT PRIMARY KEY, state TEXT NOT NULL, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS maint_sessions(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, kind TEXT NOT NULL,
  started INTEGER NOT NULL, expected_slaves INTEGER NOT NULL, done TEXT NOT NULL DEFAULT '[]', failing TEXT, ended INTEGER);
CREATE TABLE IF NOT EXISTS flash_blocks(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, pack INTEGER NOT NULL,
  reason TEXT NOT NULL, expect TEXT, since INTEGER NOT NULL, cleared INTEGER, how TEXT);
CREATE TABLE IF NOT EXISTS sector_notes(installation TEXT PRIMARY KEY, note TEXT NOT NULL, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sector_notes_resolved(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, note TEXT NOT NULL,
  at INTEGER NOT NULL, resolved INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS maint_archive(id INTEGER PRIMARY KEY, installation TEXT NOT NULL, name TEXT NOT NULL,
  data TEXT NOT NULL, deleted INTEGER NOT NULL);
";

/// Severity of a flash block for the banner, most severe first: a pack that may sit in its bootloader, an
/// update interrupted mid-way, an unclear update, an unclear master update (its application runs).
pub fn block_rank(reason: &str) -> u8 {
    match reason {
        "bootloader-unclear" => 0,
        "update-interrupted" => 1,
        "update-unclear" => 2,
        "master-unclear" => 3,
        _ => 4,
    }
}

/// The most severe uncleared block (`block_rank`), the oldest among equals.
pub fn most_severe(blocks: &[FlashBlock]) -> Option<&FlashBlock> {
    blocks.iter().min_by_key(|b| (block_rank(&b.reason), b.since, b.id))
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

/// The name of an installation created without asking (a single pack flashed without one): „Anlage n“ with the first
/// free n from the count + 1, unique like `valid_name` (the UI's `defaultName`).
pub fn default_name(names: &[String]) -> String {
    (names.len() + 1..)
        .map(|n| format!("Anlage {n}"))
        .find(|c| valid_name(c, names.iter().map(String::as_str)).is_ok())
        .unwrap_or_default()
}

/// Open session as the banner names it.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct OpenSession {
    pub kind: SessionKind,
    pub started: i64,
    pub expected_slaves: u8,
    pub failing: Option<String>,
}

/// One installation with open work, for the banner (`Store::open_work_list`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct OpenWork {
    pub id: String,
    pub name: String,
    pub session: Option<OpenSession>,
    /// the most severe uncleared block (`most_severe`)
    pub block: Option<FlashBlock>,
    pub state: Option<StoredMasterState>,
    pub state_at: Option<i64>,
    /// time of an unresolved sector note, if any
    pub sector_note_at: Option<i64>,
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
    pub endpoints: Vec<String>,
    pub control: Option<ControlEndpoint>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "state")]
pub enum StoredMasterState {
    #[serde(rename = "patched-off")]
    PatchedOff { slaves: u8 },
    #[serde(rename = "patched-on")]
    PatchedOn,
    #[serde(rename = "no-0x64")]
    No0x64,
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "marker-unexplained")]
    MarkerUnexplained,
}

impl From<MasterState> for StoredMasterState {
    fn from(m: MasterState) -> Self {
        match m {
            MasterState::PatchedOff { slaves } => Self::PatchedOff { slaves },
            MasterState::PatchedOn => Self::PatchedOn,
            MasterState::No0x64 => Self::No0x64,
            MasterState::Unknown => Self::Unknown,
            MasterState::MarkerUnexplained => Self::MarkerUnexplained,
        }
    }
}

impl From<&StoredMasterState> for MasterState {
    fn from(m: &StoredMasterState) -> Self {
        match m {
            StoredMasterState::PatchedOff { slaves } => Self::PatchedOff { slaves: *slaves },
            StoredMasterState::PatchedOn => Self::PatchedOn,
            StoredMasterState::No0x64 => Self::No0x64,
            StoredMasterState::Unknown => Self::Unknown,
            StoredMasterState::MarkerUnexplained => Self::MarkerUnexplained,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Marker,
    Dip,
}

impl SessionKind {
    fn as_str(self) -> &'static str {
        match self { SessionKind::Marker => "marker", SessionKind::Dip => "dip" }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MaintSession {
    pub id: i64,
    pub installation: String,
    pub kind: SessionKind,
    pub started: i64,
    pub expected_slaves: u8,
    /// what was done (e.g. "flash 3 16.06.13"), shown in the banner and the end dialog
    pub done: Vec<String>,
    /// the end check that failed last, if any
    pub failing: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FlashBlock {
    pub id: i64,
    pub pack: u8,
    /// "update-interrupted" (written before the restart, still there after a crash), "update-unclear",
    /// "bootloader-unclear", or "master-unclear" (the master's application runs, 0x64 shows not what the
    /// master-flash table expected)
    pub reason: String,
    /// for "master-unclear": the expected state after the flash ("marker-on", "zero", "unsupported" …)
    pub expect: Option<String>,
    pub since: i64,
}

pub struct Store {
    db: Connection,
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
        db.execute_batch(SCHEMA).map_err(e)?;
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(e)?;
        Ok(Self { db })
    }

    /// Run `f` in one transaction: every row it writes is stored, or (on an error) none.
    pub fn atomic<T>(&self, f: impl FnOnce(&Self) -> Result<T, String>) -> Result<T, String> {
        let tx = self.db.unchecked_transaction().map_err(e)?;
        let r = f(self)?;
        tx.commit().map_err(e)?;
        Ok(r)
    }

    fn installation(&self, id: &str) -> Result<Installation, String> {
        let (name, control): (String, Option<String>) = self
            .db
            .query_row("SELECT name, control FROM installations WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(e)?;
        let mut stmt = self.db.prepare("SELECT endpoint FROM installation_endpoints WHERE installation = ?1 ORDER BY rowid").map_err(e)?;
        let endpoints = stmt.query_map([id], |r| r.get(0)).map_err(e)?.collect::<rusqlite::Result<Vec<String>>>().map_err(e)?;
        let control = control.map(|c| serde_json::from_str(&c)).transpose().map_err(|err| err.to_string())?;
        Ok(Installation { id: id.into(), name, endpoints, control })
    }

    /// The installation an endpoint belongs to; a new one ("Installation n") if it is not assigned yet.
    pub fn installation_for(&self, endpoint: &str) -> Result<Installation, String> {
        if let Some(found) = self.installation_of(endpoint)? {
            return Ok(found);
        }
        let now = crate::history::now_ms();
        let names: Vec<String> = self.installations()?.into_iter().map(|i| i.name).collect();
        let id = new_id(now);
        self.db.execute("INSERT INTO installations(id, name, control, created) VALUES (?1, ?2, NULL, ?3)", params![id, default_name(&names), now]).map_err(e)?;
        self.db.execute("INSERT INTO installation_endpoints(endpoint, installation) VALUES (?1, ?2)", params![endpoint, id]).map_err(e)?;
        self.installation(&id)
    }

    /// The installation an endpoint is assigned to (as a main endpoint or as its control channel), without
    /// creating one.
    pub fn installation_of(&self, endpoint: &str) -> Result<Option<Installation>, String> {
        let id: Option<String> = self
            .db
            .query_row("SELECT installation FROM installation_endpoints WHERE endpoint = ?1", [endpoint], |r| r.get(0))
            .optional()
            .map_err(e)?;
        if let Some(id) = id {
            return self.installation(&id).map(Some);
        }
        Ok(self.installations()?.into_iter().find(|i| i.control.as_ref().is_some_and(|c| c.key() == endpoint)))
    }

    /// Some installation has an open session or an uncleared flash block.
    pub fn any_open(&self) -> Result<bool, String> {
        let n: i64 = self
            .db
            .query_row(
                "SELECT (SELECT COUNT(*) FROM maint_sessions WHERE ended IS NULL)
                      + (SELECT COUNT(*) FROM flash_blocks WHERE cleared IS NULL)
                      + (SELECT COUNT(*) FROM master_state WHERE json_extract(state, '$.state') IN ('patched-on', 'marker-unexplained'))",
                [],
                |r| r.get(0),
            )
            .map_err(e)?;
        Ok(n > 0)
    }

    pub fn installations(&self) -> Result<Vec<Installation>, String> {
        let mut stmt = self.db.prepare("SELECT id FROM installations ORDER BY created, rowid").map_err(e)?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>().map_err(e)?;
        ids.iter().map(|id| self.installation(id)).collect()
    }

    /// Name, control channel and endpoints; an endpoint listed here moves away from any other installation.
    /// A move is refused while the source or the target has open work: a block or a session must never be
    /// separated from the bus it belongs to.
    pub fn save_installation(&self, i: &Installation) -> Result<(), String> {
        self.atomic(|s| s.save_rows(i))
    }

    /// `save_installation` without its own transaction (the caller holds one).
    fn save_rows(&self, i: &Installation) -> Result<(), String> {
        let old = self.installation(&i.id)?; // unknown id: Err, no rows for a nonexistent installation
        let mut endpoints: Vec<String> = Vec::new();
        for ep in &i.endpoints {
            let k = normalize_key(ep)?;
            if !endpoints.contains(&k) {
                endpoints.push(k);
            }
        }
        let control_key = i.control.as_ref().map(ControlEndpoint::key);
        // a sector note counts as open work here: the bus whose 0x23000 is unexplained keeps its ports
        let own_open = self.holds_ports(&i.id)?;
        // with open work the installation keeps every endpoint and its control adapter: a removed port would
        // be ownerless and could then be assigned elsewhere, away from its block
        if own_open {
            let kept = old.endpoints.iter().all(|ep| endpoints.contains(ep));
            if !kept || old.control.as_ref().map(ControlEndpoint::key) != control_key {
                return Err("endpoint-in-use".into());
            }
        }
        // one physical key belongs to one installation: a foreign control is never taken over, a foreign
        // endpoint only if neither side has open work
        let others: Vec<Installation> = self.installations()?.into_iter().filter(|o| o.id != i.id).collect();
        let mut take_over: Vec<String> = Vec::new();
        let keys = endpoints.iter().chain(control_key.iter());
        for key in keys {
            for o in &others {
                if o.control.as_ref().is_some_and(|c| &c.key() == key) {
                    return Err("endpoint-in-use".into());
                }
                if o.endpoints.contains(key) {
                    if own_open || self.holds_ports(&o.id)? {
                        return Err("endpoint-in-use".into());
                    }
                    if control_key.as_ref() == Some(key) {
                        take_over.push(key.clone());
                    }
                }
            }
        }
        let name = if i.name.trim() == old.name.trim() { old.name.clone() } else { valid_name(&i.name, others.iter().map(|o| o.name.as_str()))? };
        let control = i.control.as_ref().map(serde_json::to_string).transpose().map_err(|err| err.to_string())?;
        let db = &self.db;
        db.execute("UPDATE installations SET name = ?2, control = ?3 WHERE id = ?1", params![i.id, name, control]).map_err(e)?;
        db.execute("DELETE FROM installation_endpoints WHERE installation = ?1", [&i.id]).map_err(e)?;
        for key in &take_over {
            db.execute("DELETE FROM installation_endpoints WHERE endpoint = ?1", [key]).map_err(e)?;
        }
        for ep in &endpoints {
            db.execute("INSERT OR REPLACE INTO installation_endpoints(endpoint, installation) VALUES (?1, ?2)", params![ep, i.id]).map_err(e)?;
        }
        Ok(())
    }

    /// A new installation with its name, control channel and endpoints, in one transaction: the ownership rules of
    /// `save_installation` apply (a foreign control is never taken over, a foreign endpoint only without open work
    /// on either side); on a refusal nothing is created.
    pub fn create_installation(&self, name: &str, control: Option<ControlEndpoint>, endpoints: Vec<String>) -> Result<Installation, String> {
        self.atomic(|s| {
            let names: Vec<String> = s.installations()?.into_iter().map(|i| i.name).collect();
            let name = valid_name(name, names.iter().map(String::as_str))?;
            let now = crate::history::now_ms();
            let id = new_id(now);
            s.db.execute("INSERT INTO installations(id, name, control, created) VALUES (?1, ?2, NULL, ?3)", params![id, name, now]).map_err(e)?;
            s.save_rows(&Installation { id: id.clone(), name, endpoints, control })?;
            s.installation(&id)
        })
    }

    /// Open work, or a sector note: what keeps an installation's ports and forbids deleting it.
    pub fn holds_ports(&self, inst: &str) -> Result<bool, String> {
        Ok(self.open_work(inst)? || self.sector_note(inst)?.is_some())
    }

    /// Delete an installation without open work and without a sector note (else "open-work"). Ended sessions and
    /// cleared blocks go to `maint_archive` as JSON; endpoints, master state and the installation row are deleted,
    /// so its ports are free again. One transaction.
    pub fn delete_installation(&self, id: &str, now: i64) -> Result<(), String> {
        self.atomic(|s| {
            let inst = s.installation(id)?;
            if s.holds_ports(id)? {
                return Err("open-work".into());
            }
            let mut stmt = s.db.prepare("SELECT kind, started, expected_slaves, done, ended FROM maint_sessions WHERE installation = ?1 ORDER BY id").map_err(e)?;
            let sessions = stmt
                .query_map([id], |r| {
                    let done: String = r.get(3)?;
                    Ok(serde_json::json!({ "kind": r.get::<_, String>(0)?, "started": r.get::<_, i64>(1)?, "expected_slaves": r.get::<_, i64>(2)?,
                        "done": serde_json::from_str::<serde_json::Value>(&done).unwrap_or_default(), "ended": r.get::<_, Option<i64>>(4)? }))
                })
                .map_err(e)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(e)?;
            let mut stmt = s.db.prepare("SELECT pack, reason, expect, since, cleared, how FROM flash_blocks WHERE installation = ?1 ORDER BY id").map_err(e)?;
            let blocks = stmt
                .query_map([id], |r| {
                    Ok(serde_json::json!({ "pack": r.get::<_, i64>(0)?, "reason": r.get::<_, String>(1)?, "expect": r.get::<_, Option<String>>(2)?,
                        "since": r.get::<_, i64>(3)?, "cleared": r.get::<_, Option<i64>>(4)?, "how": r.get::<_, Option<String>>(5)? }))
                })
                .map_err(e)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(e)?;
            let data = serde_json::json!({ "control": inst.control, "endpoints": inst.endpoints, "sessions": sessions, "blocks": blocks }).to_string();
            s.db.execute("INSERT INTO maint_archive(installation, name, data, deleted) VALUES (?1, ?2, ?3, ?4)", params![id, inst.name, data, now]).map_err(e)?;
            for sql in [
                "DELETE FROM maint_sessions WHERE installation = ?1",
                "DELETE FROM flash_blocks WHERE installation = ?1",
                "DELETE FROM installation_endpoints WHERE installation = ?1",
                "DELETE FROM master_state WHERE installation = ?1",
                "DELETE FROM installations WHERE id = ?1",
            ] {
                s.db.execute(sql, [id]).map_err(e)?;
            }
            Ok(())
        })
    }

    /// Every installation with open work or a sector note, for the banner (unsorted; the UI orders the lines).
    pub fn open_work_list(&self) -> Result<Vec<OpenWork>, String> {
        let mut out = Vec::new();
        for i in self.installations()? {
            let session = self.open_session(&i.id)?.map(|x| OpenSession { kind: x.kind, started: x.started, expected_slaves: x.expected_slaves, failing: x.failing });
            let block = most_severe(&self.blocks(&i.id)?).cloned();
            let ms = self.master_state(&i.id)?;
            let note: Option<i64> = self.db.query_row("SELECT at FROM sector_notes WHERE installation = ?1", [&i.id], |r| r.get(0)).optional().map_err(e)?;
            let marker = matches!(ms, Some((StoredMasterState::PatchedOn | StoredMasterState::MarkerUnexplained, _)));
            if session.is_none() && block.is_none() && !marker && note.is_none() {
                continue;
            }
            out.push(OpenWork { id: i.id, name: i.name, session, block, state_at: ms.as_ref().map(|(_, at)| *at), state: ms.map(|(m, _)| m), sector_note_at: note });
        }
        Ok(out)
    }

    /// The user marks the sector note as explained: it moves to `sector_notes_resolved` (never dropped).
    pub fn resolve_sector_note(&self, inst: &str, now: i64) -> Result<(), String> {
        self.atomic(|s| {
            let row: Option<(String, i64)> = s.db.query_row("SELECT note, at FROM sector_notes WHERE installation = ?1", [inst], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(e)?;
            let (note, at) = row.ok_or("no-sector-note")?;
            s.db.execute("INSERT INTO sector_notes_resolved(installation, note, at, resolved) VALUES (?1, ?2, ?3, ?4)", params![inst, note, at, now]).map_err(e)?;
            s.db.execute("DELETE FROM sector_notes WHERE installation = ?1", [inst]).map_err(e)?;
            Ok(())
        })
    }

    /// The installation has an open session, an uncleared block, or the last state is a set marker.
    pub fn open_work(&self, inst: &str) -> Result<bool, String> {
        let state = self.master_state(inst)?.map(|(m, _)| m);
        Ok(self.open_session(inst)?.is_some()
            || !self.blocks(inst)?.is_empty()
            || matches!(state, Some(StoredMasterState::PatchedOn | StoredMasterState::MarkerUnexplained)))
    }

    pub fn master_state(&self, inst: &str) -> Result<Option<(StoredMasterState, i64)>, String> {
        let row: Option<(String, i64)> = self.db.query_row("SELECT state, at FROM master_state WHERE installation = ?1", [inst], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(e)?;
        row.map(|(s, at)| serde_json::from_str(&s).map(|m| (m, at)).map_err(|err| err.to_string())).transpose()
    }

    pub fn set_master_state(&self, inst: &str, s: &StoredMasterState, at: i64) -> Result<(), String> {
        let json = serde_json::to_string(s).map_err(|err| err.to_string())?;
        self.db.execute("INSERT OR REPLACE INTO master_state(installation, state, at) VALUES (?1, ?2, ?3)", params![inst, json, at]).map_err(e)?;
        Ok(())
    }

    pub fn open_session(&self, inst: &str) -> Result<Option<MaintSession>, String> {
        self.db
            .query_row(
                "SELECT id, kind, started, expected_slaves, done, failing FROM maint_sessions WHERE installation = ?1 AND ended IS NULL",
                [inst],
                |r| {
                    let kind: String = r.get(1)?;
                    let done: String = r.get(4)?;
                    Ok(MaintSession {
                        id: r.get(0)?,
                        installation: inst.into(),
                        kind: if kind == "dip" { SessionKind::Dip } else { SessionKind::Marker },
                        started: r.get(2)?,
                        expected_slaves: r.get(3)?,
                        done: serde_json::from_str(&done).unwrap_or_default(),
                        failing: r.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(e)
    }

    pub fn start_session(&self, inst: &str, kind: SessionKind, expected_slaves: u8, now: i64) -> Result<MaintSession, String> {
        if self.open_session(inst)?.is_some() {
            return Err("a maintenance session is already open".into());
        }
        self.db.execute("INSERT INTO maint_sessions(installation, kind, started, expected_slaves) VALUES (?1, ?2, ?3, ?4)", params![inst, kind.as_str(), now, expected_slaves]).map_err(e)?;
        self.open_session(inst)?.ok_or_else(|| "session not stored".into())
    }

    /// After the master was flashed back to the original image, a marker session ends as a DIP session.
    pub fn set_session_kind(&self, id: i64, kind: SessionKind) -> Result<(), String> {
        self.db.execute("UPDATE maint_sessions SET kind = ?2 WHERE id = ?1", params![id, kind.as_str()]).map_err(e)?;
        Ok(())
    }

    /// The user corrected the slave count per DIP of an open session.
    pub fn set_expected_slaves(&self, id: i64, n: u8) -> Result<(), String> {
        self.db.execute("UPDATE maint_sessions SET expected_slaves = ?2 WHERE id = ?1 AND ended IS NULL", params![id, n]).map_err(e)?;
        Ok(())
    }

    pub fn note_done(&self, id: i64, what: &str) -> Result<(), String> {
        let done: String = self.db.query_row("SELECT done FROM maint_sessions WHERE id = ?1", [id], |r| r.get(0)).map_err(e)?;
        let mut list: Vec<String> = serde_json::from_str(&done).unwrap_or_default();
        list.push(what.into());
        self.db.execute("UPDATE maint_sessions SET done = ?2 WHERE id = ?1", params![id, serde_json::to_string(&list).map_err(|err| err.to_string())?]).map_err(e)?;
        Ok(())
    }

    pub fn set_failing(&self, id: i64, failing: Option<&str>) -> Result<(), String> {
        self.db.execute("UPDATE maint_sessions SET failing = ?2 WHERE id = ?1", params![id, failing]).map_err(e)?;
        Ok(())
    }

    pub fn end_session(&self, id: i64, now: i64) -> Result<(), String> {
        self.db.execute("UPDATE maint_sessions SET ended = ?2, failing = NULL WHERE id = ?1", params![id, now]).map_err(e)?;
        Ok(())
    }

    pub fn add_block(&self, inst: &str, pack: u8, reason: &str, now: i64) -> Result<i64, String> {
        self.db.execute("INSERT INTO flash_blocks(installation, pack, reason, since) VALUES (?1, ?2, ?3, ?4)", params![inst, pack, reason, now]).map_err(e)?;
        Ok(self.db.last_insert_rowid())
    }

    /// Unclear master update: the application runs, 0x64 did not show `expect` after the flash.
    pub fn add_master_block(&self, inst: &str, expect: &str, now: i64) -> Result<i64, String> {
        self.db.execute("INSERT INTO flash_blocks(installation, pack, reason, expect, since) VALUES (?1, 0, 'master-unclear', ?2, ?3)", params![inst, expect, now]).map_err(e)?;
        Ok(self.db.last_insert_rowid())
    }

    /// An `update-interrupted` block becomes the reason found when the flash returned.
    pub fn set_block_reason(&self, id: i64, reason: &str) -> Result<(), String> {
        self.db.execute("UPDATE flash_blocks SET reason = ?2 WHERE id = ?1", params![id, reason]).map_err(e)?;
        Ok(())
    }

    pub fn blocks(&self, inst: &str) -> Result<Vec<FlashBlock>, String> {
        let mut stmt = self.db.prepare("SELECT id, pack, reason, expect, since FROM flash_blocks WHERE installation = ?1 AND cleared IS NULL ORDER BY id").map_err(e)?;
        let rows = stmt.query_map([inst], |r| Ok(FlashBlock { id: r.get(0)?, pack: r.get(1)?, reason: r.get(2)?, expect: r.get(3)?, since: r.get(4)? })).map_err(e)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
    }

    /// `how`: "answered" (pack answered 0x51/0x47), "isolated" (the user confirmed isolation or recovery),
    /// "flashed" (the flash and its post-check passed), "preflight" (failed before the restart) or "checked"
    /// (master-unclear: a later 0x64 read showed the expected state).
    pub fn clear_block(&self, id: i64, how: &str, now: i64) -> Result<(), String> {
        self.db.execute("UPDATE flash_blocks SET cleared = ?2, how = ?3 WHERE id = ?1", params![id, now, how]).map_err(e)?;
        Ok(())
    }

    pub fn note_sector(&self, inst: &str, note: &str, now: i64) -> Result<(), String> {
        self.db.execute("INSERT OR REPLACE INTO sector_notes(installation, note, at) VALUES (?1, ?2, ?3)", params![inst, note, now]).map_err(e)?;
        Ok(())
    }

    pub fn sector_note(&self, inst: &str) -> Result<Option<String>, String> {
        self.db.query_row("SELECT note FROM sector_notes WHERE installation = ?1", [inst], |r| r.get(0)).optional().map_err(e)
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
    fn atomic_writes_all_or_nothing() {
        let s = Store::open(&temp("atomic")).unwrap();
        let i = s.installation_for("tcp:gw:4196").unwrap().id;
        let r: Result<(), String> = s.atomic(|s| {
            s.set_master_state(&i, &StoredMasterState::Unknown, 1)?;
            s.add_master_block(&i, "marker-on", 1)?;
            Err("disk full".into())
        });
        assert!(r.is_err());
        assert_eq!(s.master_state(&i).unwrap(), None);
        assert!(s.blocks(&i).unwrap().is_empty());
        s.atomic(|s| {
            s.set_master_state(&i, &StoredMasterState::Unknown, 2)?;
            s.add_master_block(&i, "marker-on", 2).map(|_| ())
        })
        .unwrap();
        assert_eq!(s.master_state(&i).unwrap(), Some((StoredMasterState::Unknown, 2)));
        assert_eq!(s.blocks(&i).unwrap().len(), 1);
    }

    #[test]
    fn endpoint_keys_name_the_physical_endpoint() {
        assert_eq!(endpoint_key(true, " Gateway.local:4196 "), "tcp:gateway.local:4196");
        assert_eq!(endpoint_key(false, "/dev/cu.usbserial-X"), "serial:/dev/cu.usbserial-X");
    }

    #[test]
    fn one_installation_per_endpoint_and_explicit_reassignment() {
        let s = Store::open(&temp("inst")).unwrap();
        let a = s.installation_for("tcp:gw:4196").unwrap();
        assert_eq!(s.installation_for("tcp:gw:4196").unwrap().id, a.id);
        let mut a2 = a.clone();
        a2.name = "Bank A".into();
        a2.endpoints.push("serial:/dev/x".into());
        a2.control = Some(ControlEndpoint::Serial { path: "/dev/y".into() });
        s.save_installation(&a2).unwrap();
        assert_eq!(s.installation_for("serial:/dev/x").unwrap(), a2);
        assert_eq!(s.installations().unwrap().len(), 1);
    }

    #[test]
    fn installation_for_finds_the_installation_holding_the_endpoint_as_control() {
        let s = Store::open(&temp("ctl")).unwrap();
        let mut a = s.installation_for("tcp:gw:4196").unwrap();
        a.control = Some(ControlEndpoint::Serial { path: "/dev/c".into() });
        s.save_installation(&a).unwrap();
        assert_eq!(s.installation_for("serial:/dev/c").unwrap().id, a.id);
        assert_eq!(s.installations().unwrap().len(), 1);
    }

    #[test]
    fn session_and_block_survive_reopen() {
        let path = temp("reopen");
        let id = {
            let s = Store::open(&path).unwrap();
            let i = s.installation_for("tcp:gw:4196").unwrap();
            s.set_master_state(&i.id, &StoredMasterState::PatchedOn, 1).unwrap();
            let ses = s.start_session(&i.id, SessionKind::Marker, 11, 2).unwrap();
            assert!(s.start_session(&i.id, SessionKind::Dip, 11, 3).is_err(), "only one open session");
            s.note_done(ses.id, "flash 3").unwrap();
            s.add_block(&i.id, 3, "update-unclear", 4).unwrap();
            i.id
        };
        let s = Store::open(&path).unwrap();
        let ses = s.open_session(&id).unwrap().unwrap();
        assert_eq!((ses.kind, ses.expected_slaves, ses.done.clone()), (SessionKind::Marker, 11, vec!["flash 3".to_string()]));
        assert_eq!(s.master_state(&id).unwrap().map(|(m, _)| m), Some(StoredMasterState::PatchedOn));
        let b = s.blocks(&id).unwrap();
        assert_eq!((b.len(), b[0].pack, b[0].reason.as_str()), (1, 3, "update-unclear"));
        s.clear_block(b[0].id, "answered", 5).unwrap();
        assert!(s.blocks(&id).unwrap().is_empty());
        s.end_session(ses.id, 6).unwrap();
        assert!(s.open_session(&id).unwrap().is_none());
    }

    #[test]
    fn expected_slaves_of_an_open_session_can_be_corrected() {
        let s = Store::open(&temp("slaves")).unwrap();
        let i = s.installation_for("tcp:gw:4196").unwrap();
        let ses = s.start_session(&i.id, SessionKind::Dip, 2, 1).unwrap();
        s.set_expected_slaves(ses.id, 5).unwrap();
        let back = s.open_session(&i.id).unwrap().unwrap();
        assert_eq!((back.id, back.kind, back.expected_slaves), (ses.id, SessionKind::Dip, 5));
    }

    #[test]
    fn lookup_without_creating_and_open_work_anywhere() {
        let s = Store::open(&temp("lookup")).unwrap();
        assert_eq!(s.installation_of("serial:/dev/p").unwrap(), None);
        assert!(s.installations().unwrap().is_empty(), "a lookup must not create an installation");
        assert!(!s.any_open().unwrap());
        let i = s.installation_for("tcp:gw:4196").unwrap();
        assert!(!s.any_open().unwrap());
        let b = s.add_block(&i.id, 2, "update-interrupted", 1).unwrap();
        assert!(s.any_open().unwrap());
        s.clear_block(b, "flashed", 2).unwrap();
        assert!(!s.any_open().unwrap());
        s.set_master_state(&i.id, &StoredMasterState::PatchedOn, 3).unwrap();
        assert!(s.any_open().unwrap(), "a set marker is open work too");
        s.set_master_state(&i.id, &StoredMasterState::PatchedOff { slaves: 3 }, 4).unwrap();
        assert!(!s.any_open().unwrap());
        // the control adapter of an installation counts as its endpoint (set before the session opens:
        // an installation with open work may not change its control adapter)
        let mut i2 = i.clone();
        i2.control = Some(ControlEndpoint::Serial { path: "/dev/c".into() });
        s.save_installation(&i2).unwrap();
        s.start_session(&i.id, SessionKind::Dip, 3, 5).unwrap();
        assert!(s.any_open().unwrap());
        assert_eq!(s.installation_of("serial:/dev/c").unwrap().map(|x| x.id), Some(i.id.clone()));
    }

    #[test]
    fn an_endpoint_with_open_work_cannot_move() {
        let s = Store::open(&temp("move")).unwrap();
        let a = s.installation_for("serial:/dev/p").unwrap();
        let b = s.installation_for("tcp:gw:4196").unwrap();
        s.add_block(&a.id, 3, "bootloader-unclear", 1).unwrap();
        // moving the blocked bus's port to another installation would hide the block
        let mut b2 = b.clone();
        b2.endpoints.push("serial:/dev/p".into());
        assert_eq!(s.save_installation(&b2), Err("endpoint-in-use".to_string()));
        assert_eq!(s.installation_of("serial:/dev/p").unwrap().map(|i| i.id), Some(a.id.clone()));
        // nor may a target with open work take over a foreign endpoint
        let c = s.installation_for("serial:/dev/q").unwrap();
        let mut a2 = a.clone();
        a2.endpoints.push("serial:/dev/q".into());
        assert_eq!(s.save_installation(&a2), Err("endpoint-in-use".to_string()));
        assert_eq!(s.installation_of("serial:/dev/q").unwrap().map(|i| i.id), Some(c.id.clone()));
        // nor may it drop its blocked port (it would become ownerless, then assignable elsewhere)
        let mut a3 = a.clone();
        a3.endpoints.clear();
        assert_eq!(s.save_installation(&a3), Err("endpoint-in-use".to_string()));
        let mut a4 = a.clone();
        a4.control = Some(ControlEndpoint::Tcp { addr: "gw2:4196".into() });
        assert_eq!(s.save_installation(&a4), Err("endpoint-in-use".to_string()));
        assert_eq!(s.installation_of("serial:/dev/p").unwrap().map(|i| i.id), Some(a.id.clone()));
        // renaming stays possible
        let mut a5 = a.clone();
        a5.name = "Bank X".into();
        s.save_installation(&a5).unwrap();
        // without open work the move is allowed
        let mut b3 = b.clone();
        b3.endpoints.push("serial:/dev/q".into());
        s.save_installation(&b3).unwrap();
        assert_eq!(s.installation_of("serial:/dev/q").unwrap().map(|i| i.id), Some(b.id.clone()));
        assert!(s.open_work(&a.id).unwrap());
        assert!(!s.open_work(&b.id).unwrap());
    }

    #[test]
    fn master_block_keeps_its_expectation() {
        let s = Store::open(&temp("masterblock")).unwrap();
        let i = s.installation_for("serial:/dev/p").unwrap();
        s.add_master_block(&i.id, "marker-on", 1).unwrap();
        let b = &s.blocks(&i.id).unwrap()[0];
        assert_eq!((b.pack, b.reason.as_str(), b.expect.as_deref()), (0, "master-unclear", Some("marker-on")));
    }

    #[test]
    fn interrupted_block_is_replaced_or_cleared() {
        let s = Store::open(&temp("interrupted")).unwrap();
        let i = s.installation_for("serial:/dev/p").unwrap();
        let b = s.add_block(&i.id, 4, "update-interrupted", 1).unwrap();
        s.set_block_reason(b, "bootloader-unclear").unwrap();
        assert_eq!(s.blocks(&i.id).unwrap()[0].reason, "bootloader-unclear");
        s.clear_block(b, "answered", 2).unwrap();
        assert!(s.blocks(&i.id).unwrap().is_empty());
    }

    #[test]
    fn master_state_serializes_with_the_spec_names() {
        let v = serde_json::to_value(StoredMasterState::No0x64).unwrap();
        assert_eq!(v, serde_json::json!({ "state": "no-0x64" }));
        let v = serde_json::to_value(StoredMasterState::PatchedOff { slaves: 3 }).unwrap();
        assert_eq!(v, serde_json::json!({ "state": "patched-off", "slaves": 3 }));
    }

    #[test]
    fn one_key_has_one_owner() {
        let s = Store::open(&temp("owner")).unwrap();
        let x = s.installation_for("serial:/dev/k").unwrap();
        let mut a = s.installation_for("tcp:gw:4196").unwrap();
        a.control = Some(ControlEndpoint::Serial { path: "/dev/k".into() });
        s.save_installation(&a).unwrap(); // x has no open work: the key moves to a as its control
        assert!(s.installation(&x.id).unwrap().endpoints.is_empty());
        s.add_block(&a.id, 1, "update-unclear", 1).unwrap();
        assert_eq!(s.installation_of("serial:/dev/k").unwrap().map(|i| i.id), Some(a.id.clone()));
        // a third installation cannot claim a control key as endpoint
        let mut c = s.installation_for("serial:/dev/z").unwrap();
        c.endpoints.push("serial:/dev/k".into());
        assert_eq!(s.save_installation(&c), Err("endpoint-in-use".to_string()));
        // nor take it as its own control
        c.endpoints.pop();
        c.control = Some(ControlEndpoint::Serial { path: "/dev/k".into() });
        assert_eq!(s.save_installation(&c), Err("endpoint-in-use".to_string()));
    }

    #[test]
    fn unknown_id_and_bad_or_differently_cased_keys() {
        let s = Store::open(&temp("keys")).unwrap();
        let ghost = Installation { id: "nope".into(), name: "x".into(), endpoints: vec!["tcp:a:1".into()], control: None };
        assert!(s.save_installation(&ghost).is_err());
        assert_eq!(s.installation_of("tcp:a:1").unwrap(), None);
        assert_eq!(normalize_key("TCP: GW:4196 ").unwrap(), "tcp:gw:4196");
        assert_eq!(normalize_key("Serial:/dev/X").unwrap(), "serial:/dev/X");
        assert_eq!(normalize_key("nonsense"), Err("bad endpoint".to_string()));
        assert_eq!(normalize_key("tcp:"), Err("bad endpoint".to_string()));
        let a = s.installation_for("tcp:gw:4196").unwrap();
        s.add_block(&a.id, 2, "update-unclear", 1).unwrap();
        let mut b = s.installation_for("serial:/dev/p").unwrap();
        b.endpoints.push("TCP:GW:4196".into());
        assert_eq!(s.save_installation(&b), Err("endpoint-in-use".to_string()));
        let mut a2 = a.clone();
        a2.endpoints = vec!["TCP:GW:4196".into()];
        s.save_installation(&a2).unwrap();
        assert_eq!(s.installation(&a.id).unwrap().endpoints, vec!["tcp:gw:4196".to_string()]);
    }

    #[test]
    fn open_work_by_master_state() {
        let s = Store::open(&temp("states")).unwrap();
        let i = s.installation_for("tcp:gw:4196").unwrap();
        for (st, open) in [
            (StoredMasterState::MarkerUnexplained, true),
            (StoredMasterState::PatchedOn, true),
            (StoredMasterState::No0x64, false),
            (StoredMasterState::Unknown, false),
            (StoredMasterState::PatchedOff { slaves: 3 }, false),
        ] {
            s.set_master_state(&i.id, &st, 1).unwrap();
            assert_eq!((s.any_open().unwrap(), s.open_work(&i.id).unwrap()), (open, open), "{st:?}");
        }
    }

    #[test]
    fn sources_with_a_session_or_a_set_marker_cannot_lose_an_endpoint() {
        let s = Store::open(&temp("sources")).unwrap();
        let a = s.installation_for("serial:/dev/p").unwrap();
        let b = s.installation_for("tcp:gw:4196").unwrap();
        let c = s.installation_for("serial:/dev/q").unwrap();
        s.start_session(&a.id, SessionKind::Dip, 3, 1).unwrap();
        s.set_master_state(&c.id, &StoredMasterState::PatchedOn, 1).unwrap();
        let mut b2 = b.clone();
        b2.endpoints.push("serial:/dev/p".into());
        assert_eq!(s.save_installation(&b2), Err("endpoint-in-use".to_string()));
        b2.endpoints = vec!["tcp:gw:4196".into(), "serial:/dev/q".into()];
        assert_eq!(s.save_installation(&b2), Err("endpoint-in-use".to_string()));
    }
    #[test]
    fn names_are_required_and_unique() {
        assert_eq!(valid_name("  Lab  ", ["Bank"]), Ok("Lab".to_string()));
        assert_eq!(valid_name("   ", []), Err("name-required".to_string()));
        assert_eq!(valid_name("bank ", ["Bank"]), Err("name-taken".to_string()));
    }

    #[test]
    fn create_follows_the_ownership_rules_and_creates_nothing_on_a_refusal() {
        let s = Store::open(&temp("create")).unwrap();
        let a = s.create_installation("Bank A", Some(ControlEndpoint::Tcp { addr: "GW:4196".into() }), vec!["serial:/dev/p".into()]).unwrap();
        assert_eq!((a.name.as_str(), a.endpoints.clone()), ("Bank A", vec!["serial:/dev/p".to_string()]));
        assert_eq!(s.installation_of("tcp:gw:4196").unwrap().map(|i| i.id), Some(a.id.clone()));
        // a foreign control is never taken over, neither as control nor as endpoint
        assert_eq!(s.create_installation("B", Some(ControlEndpoint::Tcp { addr: "gw:4196".into() }), vec![]).err(), Some("endpoint-in-use".to_string()));
        assert_eq!(s.create_installation("B", None, vec!["tcp:gw:4196".into()]).err(), Some("endpoint-in-use".to_string()));
        // a foreign endpoint with open work stays
        s.add_block(&a.id, 2, "update-unclear", 1).unwrap();
        assert_eq!(s.create_installation("B", None, vec!["serial:/dev/p".into()]).err(), Some("endpoint-in-use".to_string()));
        assert_eq!(s.create_installation(" bank a", None, vec![]).err(), Some("name-taken".to_string()));
        assert_eq!(s.create_installation("", None, vec![]).err(), Some("name-required".to_string()));
        assert_eq!(s.installations().unwrap().len(), 1, "a refused create leaves no row");
        let b = s.create_installation("B", None, vec![]).unwrap();
        assert_ne!(a.id, b.id);
        // a rename to a taken name is refused, keeping the own name is fine
        let mut b2 = b.clone();
        b2.name = "BANK A".into();
        assert_eq!(s.save_installation(&b2), Err("name-taken".to_string()));
        b2.name = "B".into();
        s.save_installation(&b2).unwrap();
    }

    #[test]
    fn delete_only_without_open_work_or_sector_note_and_archives_the_history() {
        let s = Store::open(&temp("delete")).unwrap();
        let a = s.create_installation("A", Some(ControlEndpoint::Serial { path: "/dev/c".into() }), vec!["serial:/dev/p".into()]).unwrap();
        let ses = s.start_session(&a.id, SessionKind::Dip, 2, 1).unwrap();
        assert_eq!(s.delete_installation(&a.id, 2), Err("open-work".to_string()));
        s.end_session(ses.id, 3).unwrap();
        let b = s.add_block(&a.id, 3, "update-unclear", 4).unwrap();
        assert_eq!(s.delete_installation(&a.id, 5), Err("open-work".to_string()));
        s.clear_block(b, "answered", 6).unwrap();
        for st in [StoredMasterState::PatchedOn, StoredMasterState::MarkerUnexplained] {
            s.set_master_state(&a.id, &st, 7).unwrap();
            assert_eq!(s.delete_installation(&a.id, 8), Err("open-work".to_string()), "{st:?}");
        }
        s.set_master_state(&a.id, &StoredMasterState::No0x64, 9).unwrap();
        s.note_sector(&a.id, "unexplained", 10).unwrap();
        assert_eq!(s.delete_installation(&a.id, 11), Err("open-work".to_string()), "a sector note is never dropped silently");
        s.resolve_sector_note(&a.id, 12).unwrap();
        assert_eq!(s.resolve_sector_note(&a.id, 12), Err("no-sector-note".to_string()));
        s.delete_installation(&a.id, 13).unwrap();
        assert!(s.installations().unwrap().is_empty());
        assert_eq!(s.installation_of("serial:/dev/p").unwrap(), None);
        assert_eq!(s.installation_of("serial:/dev/c").unwrap(), None);
        assert_eq!(s.master_state(&a.id).unwrap(), None);
        let (name, data): (String, String) = s.db.query_row("SELECT name, data FROM maint_archive WHERE installation = ?1", [&a.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        let v: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!((name.as_str(), v["sessions"].as_array().unwrap().len(), v["blocks"].as_array().unwrap().len()), ("A", 1, 1));
        let resolved: i64 = s.db.query_row("SELECT COUNT(*) FROM sector_notes_resolved WHERE installation = ?1", [&a.id], |r| r.get(0)).unwrap();
        assert_eq!(resolved, 1);
        // the ports are free for another installation
        s.create_installation("B", Some(ControlEndpoint::Serial { path: "/dev/c".into() }), vec!["serial:/dev/p".into()]).unwrap();
        assert!(s.delete_installation("nope", 14).is_err());
    }

    #[test]
    fn a_sector_note_keeps_the_ports() {
        let s = Store::open(&temp("note")).unwrap();
        let a = s.create_installation("A", None, vec!["serial:/dev/p".into()]).unwrap();
        let b = s.create_installation("B", None, vec![]).unwrap();
        s.note_sector(&a.id, "unexplained", 1).unwrap();
        let mut b2 = b.clone();
        b2.endpoints.push("serial:/dev/p".into());
        assert_eq!(s.save_installation(&b2), Err("endpoint-in-use".to_string()));
        let mut a2 = a.clone();
        a2.endpoints.clear();
        assert_eq!(s.save_installation(&a2), Err("endpoint-in-use".to_string()));
        s.resolve_sector_note(&a.id, 2).unwrap();
        s.save_installation(&b2).unwrap();
    }

    #[test]
    fn open_work_lists_every_installation_with_its_most_severe_block() {
        let s = Store::open(&temp("openlist")).unwrap();
        let a = s.create_installation("A", None, vec![]).unwrap();
        let b = s.create_installation("B", None, vec![]).unwrap();
        let c = s.create_installation("C", None, vec![]).unwrap();
        let d = s.create_installation("D", None, vec![]).unwrap();
        s.set_master_state(&d.id, &StoredMasterState::PatchedOff { slaves: 2 }, 1).unwrap();
        s.add_master_block(&a.id, "marker-on", 1).unwrap();
        s.add_block(&a.id, 4, "update-unclear", 2).unwrap();
        s.add_block(&a.id, 5, "bootloader-unclear", 3).unwrap();
        s.add_block(&a.id, 6, "update-interrupted", 0).unwrap();
        s.start_session(&b.id, SessionKind::Marker, 3, 5).unwrap();
        s.set_master_state(&b.id, &StoredMasterState::PatchedOn, 6).unwrap();
        s.note_sector(&c.id, "x", 7).unwrap();
        let list = s.open_work_list().unwrap();
        assert_eq!(list.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), vec!["A", "B", "C"]);
        assert_eq!(list[0].block.as_ref().map(|b| (b.pack, b.reason.as_str())), Some((5, "bootloader-unclear")));
        assert_eq!(list[1].session.as_ref().map(|x| (x.kind, x.started, x.expected_slaves)), Some((SessionKind::Marker, 5, 3)));
        assert_eq!((list[1].state.clone(), list[1].state_at), (Some(StoredMasterState::PatchedOn), Some(6)));
        assert_eq!(list[2].sector_note_at, Some(7));
    }

    #[test]
    fn implicit_installations_get_the_first_free_anlage_n() {
        assert_eq!(default_name(&[]), "Anlage 1");
        assert_eq!(default_name(&["A".into(), "anlage 3".into()]), "Anlage 4");
        let s = Store::open(&temp("defname")).unwrap();
        s.create_installation("Anlage 2", None, vec![]).unwrap();
        assert_eq!(s.installation_for("serial:/dev/p").unwrap().name, "Anlage 3");
        assert_eq!(s.installation_for("serial:/dev/q").unwrap().name, "Anlage 4");
    }

    #[test]
    fn block_severity_order() {
        let mut r: Vec<&str> = vec!["master-unclear", "update-unclear", "bootloader-unclear", "update-interrupted"];
        r.sort_by_key(|x| block_rank(x));
        assert_eq!(r, vec!["bootloader-unclear", "update-interrupted", "update-unclear", "master-unclear"]);
    }

    #[test]
    fn an_existing_database_keeps_its_data_and_gains_the_new_tables() {
        let path = temp("legacy");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(SCHEMA.split("CREATE TABLE IF NOT EXISTS sector_notes_resolved").next().unwrap()).unwrap();
            db.execute("INSERT INTO installations(id, name, control, created) VALUES ('old', 'Old', NULL, 1)", []).unwrap();
            db.execute("INSERT INTO maint_sessions(installation, kind, started, expected_slaves, ended) VALUES ('old', 'dip', 1, 2, 3)", []).unwrap();
            db.execute("INSERT INTO sector_notes(installation, note, at) VALUES ('old', 'n', 4)", []).unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.installations().unwrap()[0].name, "Old");
        assert_eq!(s.open_work_list().unwrap().len(), 1);
        s.resolve_sector_note("old", 5).unwrap();
        s.delete_installation("old", 6).unwrap();
    }
}
