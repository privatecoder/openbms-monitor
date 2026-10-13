// used from Task 7 on
#![allow(dead_code)]

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
";

/// Physical endpoint, independent of the bus setting: `tcp:host:port` (lower case) or `serial:<path>`.
pub fn endpoint_key(tcp: bool, s: &str) -> String {
    if tcp { format!("tcp:{}", s.trim().to_ascii_lowercase()) } else { format!("serial:{}", s.trim()) }
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
        let id: Option<String> = self
            .db
            .query_row("SELECT installation FROM installation_endpoints WHERE endpoint = ?1", [endpoint], |r| r.get(0))
            .optional()
            .map_err(e)?;
        if let Some(id) = id {
            return self.installation(&id);
        }
        let now = crate::history::now_ms();
        let count: i64 = self.db.query_row("SELECT COUNT(*) FROM installations", [], |r| r.get(0)).map_err(e)?;
        let id = format!("inst-{now:x}-{:x}", std::process::id());
        self.db.execute("INSERT INTO installations(id, name, control, created) VALUES (?1, ?2, NULL, ?3)", params![id, format!("Installation {}", count + 1), now]).map_err(e)?;
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
                      + (SELECT COUNT(*) FROM master_state WHERE state LIKE '%\"patched-on\"%' OR state LIKE '%\"marker-unexplained\"%')",
                [],
                |r| r.get(0),
            )
            .map_err(e)?;
        Ok(n > 0)
    }

    pub fn installations(&self) -> Result<Vec<Installation>, String> {
        let mut stmt = self.db.prepare("SELECT id FROM installations ORDER BY created").map_err(e)?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>().map_err(e)?;
        ids.iter().map(|id| self.installation(id)).collect()
    }

    /// Name, control channel and endpoints; an endpoint listed here moves away from any other installation.
    /// A move is refused while the source or the target has open work: a block or a session must never be
    /// separated from the bus it belongs to.
    pub fn save_installation(&self, i: &Installation) -> Result<(), String> {
        // with open work the installation keeps every endpoint and its control adapter: a removed port would
        // be ownerless and could then be assigned elsewhere, away from its block
        if self.open_work(&i.id)? {
            let old = self.installation(&i.id)?;
            let kept = old.endpoints.iter().all(|ep| i.endpoints.contains(ep));
            if !kept || old.control != i.control {
                return Err("endpoint-in-use".into());
            }
        }
        let mut keys: Vec<String> = i.endpoints.clone();
        keys.extend(i.control.as_ref().map(ControlEndpoint::key));
        for key in &keys {
            if let Some(owner) = self.installation_of(key)? {
                if owner.id != i.id && (self.open_work(&owner.id)? || self.open_work(&i.id)?) {
                    return Err("endpoint-in-use".into());
                }
            }
        }
        let control = i.control.as_ref().map(serde_json::to_string).transpose().map_err(|err| err.to_string())?;
        let tx = self.db.unchecked_transaction().map_err(e)?;
        tx.execute("UPDATE installations SET name = ?2, control = ?3 WHERE id = ?1", params![i.id, i.name, control]).map_err(e)?;
        tx.execute("DELETE FROM installation_endpoints WHERE installation = ?1", [&i.id]).map_err(e)?;
        for ep in &i.endpoints {
            tx.execute("INSERT OR REPLACE INTO installation_endpoints(endpoint, installation) VALUES (?1, ?2)", params![ep, i.id]).map_err(e)?;
        }
        tx.commit().map_err(e)
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
    /// "flashed" (the flash and its post-check passed) or "preflight" (failed before the restart).
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
}
