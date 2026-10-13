//! Maintenance database (`maintenance.sqlite` in the app data folder): installations as an address book (name,
//! CAN socket endpoint, pack bus endpoints, the master's BMS serial number), the last master read per
//! installation, and packs that may hang in their bootloader after an interrupted flash, per pack bus endpoint.
//! A file of its own: the history database is moved aside when its schema changes, and a pending bootloader
//! record must never vanish that way. Tables are only ever added; tables of earlier versions stay in old files
//! and are no longer read.

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
        Self::init(db)
    }

    fn init(db: Connection) -> Result<Self, String> {
        let had_links: bool = db
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'installation_links'", [], |r| r.get::<_, i64>(0))
            .map_err(e)?
            > 0;
        db.execute_batch(SCHEMA).map_err(e)?;
        if !had_links {
            // once, when the table is new: the endpoints of the one-owner table of earlier versions, in their order
            db.execute("INSERT OR IGNORE INTO installation_links(installation, endpoint) SELECT installation, endpoint FROM installation_endpoints ORDER BY rowid", [])
                .map_err(e)?;
        }
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
        let (name, control): (String, Option<String>) = self
            .db
            .query_row("SELECT name, control FROM installations WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(e)?
            .ok_or("unknown-installation")?;
        let mut stmt = self.db.prepare("SELECT endpoint FROM installation_links WHERE installation = ?1 ORDER BY rowid").map_err(e)?;
        let endpoints = stmt.query_map([id], |r| r.get(0)).map_err(e)?.collect::<rusqlite::Result<Vec<String>>>().map_err(e)?;
        let control = control.map(|c| serde_json::from_str(&c)).transpose().map_err(|err| err.to_string())?;
        let master_serial = self.db.query_row("SELECT serial FROM master_serial WHERE installation = ?1", [id], |r| r.get(0)).optional().map_err(e)?;
        Ok(Installation { id: id.into(), name, endpoints, control, master_serial })
    }

    pub fn installations(&self) -> Result<Vec<Installation>, String> {
        let mut stmt = self.db.prepare("SELECT id FROM installations ORDER BY created, rowid").map_err(e)?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>().map_err(e)?;
        ids.iter().map(|id| self.installation(id)).collect()
    }

    /// Every installation that lists the endpoint, or has it as its CAN socket.
    pub fn installations_of(&self, endpoint: &str) -> Result<Vec<Installation>, String> {
        Ok(self.installations()?.into_iter().filter(|i| i.endpoints.iter().any(|k| k == endpoint) || i.control.as_ref().is_some_and(|c| c.key() == endpoint)).collect())
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
            s.save_rows(&Installation { id: id.clone(), name, endpoints, control, master_serial: None })?;
            s.installation(&id)
        })
    }

    /// Delete an installation with its endpoints, stored serial number and master read. Pending bootloader records
    /// belong to the endpoint, not to the installation: they stay.
    pub fn delete_installation(&self, id: &str) -> Result<(), String> {
        self.atomic(|s| {
            s.installation(id)?;
            for sql in [
                "DELETE FROM installation_links WHERE installation = ?1",
                "DELETE FROM master_serial WHERE installation = ?1",
                "DELETE FROM master_read WHERE installation = ?1",
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
        let ids = |k: &str| s.installations_of(k).unwrap().into_iter().map(|i| i.id).collect::<Vec<_>>();
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
                 INSERT INTO installation_endpoints VALUES ('serial:/dev/p', 'old');
                 INSERT INTO installation_endpoints VALUES ('tcp:gw:4196', 'old');
                 INSERT INTO flash_blocks(installation, pack, reason, since) VALUES ('old', 3, 'update-unclear', 1);",
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        let old = s.installation("old").unwrap();
        assert_eq!((old.name.as_str(), old.endpoints.clone()), ("Old", vec!["serial:/dev/p".to_string(), "tcp:gw:4196".to_string()]));
        assert_eq!(old.control, Some(ControlEndpoint::Tcp { addr: "gw:4196".into() }));
        // the copy runs once: endpoints removed later are not copied back at the next start
        s.save_installation(&Installation { endpoints: vec![], ..old }).unwrap();
        drop(s);
        let s = Store::open(&path).unwrap();
        assert!(s.installation("old").unwrap().endpoints.is_empty());
        // the old tables stay, unread
        let n: i64 = s.db.query_row("SELECT COUNT(*) FROM flash_blocks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
}
