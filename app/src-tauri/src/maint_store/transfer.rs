//! An installation's rows for the ZIP bundle, and their import: additive, deduplicated, in one transaction, with an
//! import status per export id that binds content and target (see `docs/superpowers/plans/2026-10-15-installation-zip.md`).

use super::{e, new_id, valid_name, Settings, Store};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AliasData {
    pub serial: String,
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BoardData {
    pub address: u8,
    pub bms_serial: String,
    pub battery_serial: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    #[serde(default)]
    pub aliases: Vec<AliasData>,
    /// the current board at its place on the exporting computer
    #[serde(default)]
    pub current: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SnapshotData {
    pub address: u8,
    pub bms_serial: Option<String>,
    pub at: i64,
    pub kind: String,
    /// the 169-byte block as hex
    pub block: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LogData {
    /// a plain file name (`bundle::safe_name`); the file is `logs/<file>` in the bundle unless `missing`
    pub file: String,
    pub address: Option<u8>,
    pub at: i64,
    pub kind: String,
    pub result: Option<String>,
    pub release: Option<String>,
    #[serde(default)]
    pub missing: bool,
}

/// `installation.json` of a bundle.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstallationData {
    pub id: String,
    pub name: String,
    pub created: i64,
    #[serde(default)]
    pub archived: bool,
    pub last_connect: Option<i64>,
    pub legacy: Option<String>,
    /// information only: never linked on import (the adapters and gateways of another computer)
    #[serde(default)]
    pub endpoints: Vec<String>,
    pub master_serial: Option<String>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub boards: Vec<BoardData>,
    #[serde(default)]
    pub snapshots: Vec<SnapshotData>,
    #[serde(default)]
    pub logs: Vec<LogData>,
}

/// Where an import goes, as decided in the dialog.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Into {
    /// the export's own id exists here
    Same,
    /// the export's id exists here but its master serial differs, and the user merges anyway
    MergeAnyway,
    /// a new installation; `copy`: with a new id (the export's id exists here already)
    New { copy: bool },
    /// another installation with the same master serial
    Merge { id: String },
}

/// What an import has done so far, stored per export id.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ImportState {
    pub manifest_sha256: String,
    pub target: String,
    pub maint: bool,
    pub recording: bool,
    pub logs: bool,
    /// bundle log name → name in `firmware-logs` (only those to copy)
    pub log_names: BTreeMap<String, String>,
    /// what was added and what was not taken (for the result line)
    pub report: serde_json::Value,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ImportRow {
    pub export_id: String,
    pub installation: String,
    pub started: i64,
    pub completed: Option<i64>,
    pub state: ImportState,
}

/// A log of the bundle and what is on disk under a name: `disk(name)` gives the hash of `firmware-logs/<name>` if
/// that file exists.
pub struct LogPlan<'a> {
    pub bundle_hash: &'a BTreeMap<String, String>,
    pub disk: &'a dyn Fn(&str) -> Option<String>,
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `<stem>-imp-<tag>[-n].<ext>`
fn renamed(name: &str, tag: &str, n: u32) -> String {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, x)) if !s.is_empty() => (s, format!(".{x}")),
        _ => (name, String::new()),
    };
    if n == 0 { format!("{stem}-imp-{tag}{ext}") } else { format!("{stem}-imp-{tag}-{n}{ext}") }
}

impl Store {
    /// Everything an installation owns here, for its bundle. Logs are listed as indexed; the caller marks the ones
    /// whose file is gone.
    pub fn export_data(&self, inst: &str) -> Result<InstallationData, String> {
        let i = self.installation(inst)?;
        let created: i64 = self.db.query_row("SELECT created FROM installations WHERE id = ?1", [inst], |r| r.get(0)).map_err(e)?;
        let mut boards = vec![];
        let mut stmt = self
            .db
            .prepare("SELECT id, address, bms_serial, battery_serial, first_seen, last_seen FROM boards WHERE installation = ?1 ORDER BY address, id")
            .map_err(e)?;
        let rows = stmt
            .query_map([inst], |r| {
                Ok((r.get::<_, i64>(0)?, BoardData { address: r.get(1)?, bms_serial: r.get(2)?, battery_serial: r.get(3)?, first_seen: r.get(4)?, last_seen: r.get(5)?, aliases: vec![], current: false }))
            })
            .map_err(e)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(e)?;
        let mut alias = self.db.prepare("SELECT serial, at FROM serial_alias WHERE board = ?1 ORDER BY id").map_err(e)?;
        for (id, b) in rows {
            let aliases = alias.query_map([id], |r| Ok(AliasData { serial: r.get(0)?, at: r.get(1)? })).map_err(e)?.collect::<rusqlite::Result<_>>().map_err(e)?;
            let current = self.current_board(inst, b.address)?.is_some_and(|c| c.id == id);
            boards.push(BoardData { aliases, current, ..b });
        }
        let mut stmt = self.db.prepare("SELECT address, bms_serial, at, kind, block FROM param_snapshots WHERE installation = ?1 ORDER BY id").map_err(e)?;
        let snapshots = stmt
            .query_map([inst], |r| Ok(SnapshotData { address: r.get(0)?, bms_serial: r.get(1)?, at: r.get(2)?, kind: r.get(3)?, block: hex(&r.get::<_, Vec<u8>>(4)?) }))
            .map_err(e)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(e)?;
        let mut stmt = self.db.prepare("SELECT file, address, at, kind, result, release FROM logs WHERE installation = ?1 ORDER BY at, id").map_err(e)?;
        let logs = stmt
            .query_map([inst], |r| Ok(LogData { file: r.get(0)?, address: r.get(1)?, at: r.get(2)?, kind: r.get(3)?, result: r.get(4)?, release: r.get(5)?, missing: false }))
            .map_err(e)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(e)?;
        Ok(InstallationData {
            id: i.id,
            name: i.name,
            created,
            archived: i.archived,
            last_connect: i.last_connect,
            legacy: i.legacy,
            endpoints: i.endpoints,
            master_serial: i.master_serial,
            settings: self.settings(inst)?,
            boards,
            snapshots,
            logs,
        })
    }

    pub fn import_row(&self, export_id: &str) -> Result<Option<ImportRow>, String> {
        self.db
            .query_row("SELECT installation, started, completed, state FROM import_status WHERE export_id = ?1", [export_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, Option<String>>(3)?))
            })
            .optional()
            .map_err(e)?
            .map(|(installation, started, completed, state)| {
                let state = state.as_deref().map(serde_json::from_str).transpose().map_err(|err| err.to_string())?.unwrap_or_default();
                Ok(ImportRow { export_id: export_id.into(), installation, started, completed, state })
            })
            .transpose()
    }

    /// Imports not completed (resumable; their target is open work).
    pub fn open_imports(&self) -> Result<Vec<ImportRow>, String> {
        let mut stmt = self.db.prepare("SELECT export_id FROM import_status WHERE completed IS NULL ORDER BY started").map_err(e)?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(e)?.collect::<rusqlite::Result<Vec<_>>>().map_err(e)?;
        ids.iter().filter_map(|id| self.import_row(id).transpose()).collect()
    }

    /// The target of an unfinished import (it must not be deleted or merged away meanwhile).
    pub fn import_open_for(&self, inst: &str) -> Result<bool, String> {
        let n: i64 = self.db.query_row("SELECT COUNT(*) FROM import_status WHERE installation = ?1 AND completed IS NULL", [inst], |r| r.get(0)).map_err(e)?;
        Ok(n > 0)
    }

    pub fn set_import_state(&self, export_id: &str, state: &ImportState) -> Result<(), String> {
        let s = serde_json::to_string(state).map_err(|err| err.to_string())?;
        self.db.execute("UPDATE import_status SET state = ?2 WHERE export_id = ?1", params![export_id, s]).map_err(e)?;
        Ok(())
    }

    /// Give up an unfinished import: what its store step added stays (it is complete and additive), the import no
    /// longer blocks its target. Importing the same file again then starts over (deduplicated).
    pub fn abandon_import(&self, export_id: &str) -> Result<(), String> {
        self.db.execute("DELETE FROM import_status WHERE export_id = ?1 AND completed IS NULL", [export_id]).map_err(e)?;
        Ok(())
    }

    pub fn complete_import(&self, export_id: &str, now: i64) -> Result<(), String> {
        self.db.execute("UPDATE import_status SET completed = ?2 WHERE export_id = ?1", params![export_id, now]).map_err(e)?;
        Ok(())
    }

    /// The installation an `Into` names, checked against what is here now. The id-merge of an export whose master
    /// serial differs from the one stored here needs `MergeAnyway`; a merge by master serial needs that serial to
    /// match.
    pub fn import_target(&self, data: &InstallationData, into: &Into) -> Result<Option<String>, String> {
        let exists = self.installation(&data.id).ok();
        match into {
            Into::Same | Into::MergeAnyway => {
                let i = exists.ok_or("unknown-installation")?;
                let conflict = matches!((&i.master_serial, &data.master_serial), (Some(a), Some(b)) if a != b);
                if conflict && *into == Into::Same {
                    return Err("import-master-differs".into());
                }
                Ok(Some(i.id))
            }
            Into::New { copy } => {
                if exists.is_some() && !copy {
                    return Err("import-id-exists".into());
                }
                Ok(None)
            }
            Into::Merge { id } => {
                let i = self.installation(id)?;
                match (&i.master_serial, &data.master_serial) {
                    (Some(a), Some(b)) if a == b => Ok(Some(i.id)),
                    _ => Err("import-master-differs".into()),
                }
            }
        }
    }

    /// The maint step of an import, all of it or nothing: target (new or existing), settings without overwriting,
    /// master serial if none, boards, aliases and snapshots deduplicated, log index rows under names fixed here, the
    /// import status row (maint done) and the log of the move. Returns the stored state. A row for this export id
    /// with another manifest → `import-conflict`; with maint done → the stored state, `into` ignored.
    pub fn import_maint(&self, data: &InstallationData, export_id: &str, manifest_sha256: &str, into: &Into, logs: &LogPlan, now: i64) -> Result<ImportState, String> {
        if let Some(row) = self.import_row(export_id)? {
            if row.state.manifest_sha256 != manifest_sha256 {
                return Err("import-conflict".into());
            }
            if row.state.maint {
                self.installation(&row.installation)?;
                return Ok(row.state);
            }
        }
        // an id of this app's form only: another text could equal an old endpoint key of the recording
        if !super::looks_like_id(&data.id) {
            return Err("bundle-manifest: installation id".into());
        }
        for l in &data.logs {
            if !crate::bundle::safe_name(&l.file) || (!l.missing && !logs.bundle_hash.contains_key(&l.file)) {
                return Err(format!("bundle-entry: logs/{}", l.file));
            }
        }
        let snapshots = data.snapshots.iter().map(|s| unhex(&s.block).map(|b| (s, b)).ok_or("bundle-manifest: snapshot block")).collect::<Result<Vec<_>, _>>()?;
        self.atomic(|s| {
            let existing = s.import_target(data, into)?;
            let target = match existing {
                Some(id) => id,
                None => {
                    let id = if matches!(into, Into::New { copy: true }) { new_id(now) } else { data.id.clone() };
                    let names: Vec<String> = s.installations()?.into_iter().map(|i| i.name).collect();
                    let mut name = data.name.clone();
                    let mut n = 1;
                    while valid_name(&name, names.iter().map(String::as_str)).is_err() {
                        name = if n == 1 { format!("{} (Import)", data.name) } else { format!("{} (Import {n})", data.name) };
                        n += 1;
                    }
                    s.db.execute(
                        "INSERT INTO installations(id, name, control, created, archived, last_connect, legacy) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6)",
                        params![id, valid_name(&name, std::iter::empty())?, data.created, data.archived, data.last_connect, data.legacy],
                    )
                    .map_err(e)?;
                    id
                }
            };
            let kept = s.add_settings(&target, &data.settings)?;
            let have = s.installation(&target)?;
            let master_not_taken = match (&have.master_serial, &data.master_serial) {
                (None, Some(m)) => {
                    s.set_master_serial(&target, m, now)?;
                    None
                }
                (Some(a), Some(b)) if a != b => Some(b.clone()),
                _ => None,
            };
            if let Some(lc) = data.last_connect {
                s.db.execute("UPDATE installations SET last_connect = MAX(COALESCE(last_connect, 0), ?2) WHERE id = ?1", params![target, lc]).map_err(e)?;
            }
            let (mut boards_added, mut aliases_added, mut snapshots_added) = (0, 0, 0);
            // places that have a board here keep it as current: another computer's clock decides nothing. A place
            // without one takes the export's current board, made current now in this computer's time.
            let mut places_with_board = std::collections::BTreeSet::new();
            for b in &data.boards {
                if s.current_board(&target, b.address)?.is_some() {
                    places_with_board.insert(b.address);
                }
            }
            for b in &data.boards {
                let id: Option<i64> = s
                    .db
                    .query_row(
                        "SELECT id FROM boards WHERE installation = ?1 AND address = ?2 AND bms_serial = ?3 AND first_seen = ?4",
                        params![target, b.address, b.bms_serial, b.first_seen],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(e)?;
                let id = match id {
                    Some(id) => {
                        // the newer sighting wins, with its String 1
                        s.db.execute(
                            "UPDATE boards SET battery_serial = CASE WHEN ?2 > last_seen THEN ?3 ELSE battery_serial END, last_seen = MAX(last_seen, ?2) WHERE id = ?1",
                            params![id, b.last_seen, b.battery_serial],
                        )
                        .map_err(e)?;
                        id
                    }
                    None => {
                        s.db.execute(
                            "INSERT INTO boards(installation, address, bms_serial, battery_serial, first_seen, last_seen, current_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                            params![target, b.address, b.bms_serial, b.battery_serial, b.first_seen, b.last_seen],
                        )
                        .map_err(e)?;
                        boards_added += 1;
                        s.db.last_insert_rowid()
                    }
                };
                if b.current && !places_with_board.contains(&b.address) {
                    s.db.execute("UPDATE boards SET current_at = ?2 WHERE id = ?1", params![id, now]).map_err(e)?;
                }
                for a in &b.aliases {
                    aliases_added += s
                        .db
                        .execute(
                            "INSERT INTO serial_alias(board, serial, at) SELECT ?1, ?2, ?3 WHERE NOT EXISTS(SELECT 1 FROM serial_alias WHERE board = ?1 AND serial = ?2 AND at = ?3)",
                            params![id, a.serial, a.at],
                        )
                        .map_err(e)?;
                }
            }
            for (x, block) in &snapshots {
                snapshots_added += s
                    .db
                    .execute(
                        "INSERT INTO param_snapshots(installation, address, bms_serial, at, kind, block) SELECT ?1, ?2, ?3, ?4, ?5, ?6
                         WHERE NOT EXISTS(SELECT 1 FROM param_snapshots WHERE installation = ?1 AND address = ?2 AND bms_serial IS ?3 AND at = ?4 AND kind = ?5 AND block = ?6)",
                        params![target, x.address, x.bms_serial, x.at, x.kind, block],
                    )
                    .map_err(e)?;
            }
            // log names fixed now, so the index and the copy agree also after a crash in between
            let tag = &crate::bundle::sha256_hex(export_id.as_bytes())[..8];
            let mut log_names = BTreeMap::new();
            let mut logs_skipped = 0;
            for l in data.logs.iter().filter(|l| !l.missing) {
                let hash = &logs.bundle_hash[&l.file];
                let row: Option<Option<String>> = s.db.query_row("SELECT installation FROM logs WHERE file = ?1", [&l.file], |r| r.get(0)).optional().map_err(e)?;
                let on_disk = (logs.disk)(&l.file);
                if row.as_ref() == Some(&Some(target.clone())) && on_disk.as_deref() == Some(hash.as_str()) {
                    logs_skipped += 1;
                    continue;
                }
                let free = |n: &str| -> Result<bool, String> {
                    let indexed: bool = s.db.query_row("SELECT COUNT(*) FROM logs WHERE file = ?1", [n], |r| r.get::<_, i64>(0)).map_err(e)? > 0;
                    Ok(!indexed && (logs.disk)(n).is_none() && !log_names.values().any(|v| v == n))
                };
                let mut name = l.file.clone();
                let mut n = 0;
                while !free(&name)? {
                    name = renamed(&l.file, tag, n);
                    n += 1;
                }
                s.db.execute(
                    "INSERT INTO logs(installation, address, at, kind, result, release, file) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![target, l.address, l.at, l.kind, l.result, l.release, name],
                )
                .map_err(e)?;
                log_names.insert(l.file.clone(), name);
            }
            let report = serde_json::json!({
                "source_name": data.name, "export_id": export_id, "endpoints": data.endpoints,
                "boards_added": boards_added, "aliases_added": aliases_added, "snapshots_added": snapshots_added,
                "logs_added": log_names.len(), "logs_skipped": logs_skipped, "logs_missing": data.logs.iter().filter(|l| l.missing).count(),
                "not_taken": kept, "master_serial_not_taken": master_not_taken,
            });
            let state = ImportState { manifest_sha256: manifest_sha256.into(), target: target.clone(), maint: true, recording: false, logs: false, log_names, report: report.clone() };
            s.db.execute(
                "INSERT OR REPLACE INTO import_status(export_id, installation, started, completed, state) VALUES (?1, ?2, ?3, NULL, ?4)",
                params![export_id, target, now, serde_json::to_string(&state).map_err(|err| err.to_string())?],
            )
            .map_err(e)?;
            s.log_move("import", export_id, &target, &report, now)?;
            Ok(state)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbms_proto::SerialValue;

    fn store() -> Store {
        Store::init(rusqlite::Connection::open_in_memory().unwrap(), std::path::Path::new(":memory:")).unwrap()
    }

    fn set(v: &str) -> SerialValue {
        SerialValue::Set { value: v.into(), truncated: false }
    }

    fn block(x: u8) -> Vec<u8> {
        vec![x; 169]
    }

    /// An installation with a board, a snapshot, settings, a master serial and a log.
    fn source() -> (Store, String) {
        let s = store();
        let i = s.create_installation("Bank A", None, vec![]).unwrap();
        s.board_seen(&i.id, 1, &set("SYN-B1"), &set("BAT-1"), 100).unwrap();
        s.add_param_snapshot(&i.id, 1, crate::maint_store::SnapshotKind::Read, &block(7), 110).unwrap();
        s.set_cell_type(&i.id, 1, Some("{\"cell\":\"x\"}")).unwrap();
        s.set_groups(&i.id, Some("[1]")).unwrap();
        s.set_master_serial(&i.id, "SYN-M", 90).unwrap();
        s.add_log(Some(&i.id), Some(1), "flash", Some("16.06.13"), "a_pack1.log", 120).unwrap();
        (s, i.id)
    }

    fn plan<'a>(h: &'a BTreeMap<String, String>, disk: &'a dyn Fn(&str) -> Option<String>) -> LogPlan<'a> {
        LogPlan { bundle_hash: h, disk }
    }

    fn hashes() -> BTreeMap<String, String> {
        BTreeMap::from([("a_pack1.log".to_string(), "h1".to_string())])
    }

    #[test]
    fn round_trip_into_an_empty_store_keeps_the_id_and_everything() {
        let (src, id) = source();
        let data = src.export_data(&id).unwrap();
        let dst = store();
        let h = hashes();
        let st = dst.import_maint(&data, "exp-1", "m1", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap();
        assert_eq!(st.target, id);
        assert_eq!(st.log_names["a_pack1.log"], "a_pack1.log");
        let again = dst.export_data(&id).unwrap();
        assert_eq!(again.boards, data.boards);
        assert_eq!(again.snapshots, data.snapshots);
        assert_eq!(again.settings, data.settings);
        assert_eq!(again.master_serial.as_deref(), Some("SYN-M"));
        assert_eq!(again.endpoints, Vec::<String>::new());
        assert!(dst.import_open_for(&id).unwrap());
        dst.complete_import("exp-1", 300).unwrap();
        assert!(!dst.import_open_for(&id).unwrap());
    }

    #[test]
    fn importing_again_adds_nothing() {
        let (src, id) = source();
        let data = src.export_data(&id).unwrap();
        let dst = store();
        let h = hashes();
        dst.import_maint(&data, "exp-1", "m1", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap();
        // the same export again: the stored state, nothing new (also with another choice in the dialog)
        let st = dst.import_maint(&data, "exp-1", "m1", &Into::New { copy: true }, &plan(&h, &|_| None), 210).unwrap();
        assert_eq!(st.target, id);
        // a later export of the same installation: merged by id, deduplicated; the log is there with the same hash
        let st = dst.import_maint(&data, "exp-2", "m2", &Into::Same, &plan(&h, &|n| (n == "a_pack1.log").then(|| "h1".into())), 220).unwrap();
        assert_eq!(st.report["boards_added"], 0);
        assert_eq!(st.report["snapshots_added"], 0);
        assert_eq!(st.report["logs_skipped"], 1);
        assert!(st.log_names.is_empty());
        assert_eq!(dst.export_data(&id).unwrap().boards.len(), 1);
    }

    #[test]
    fn another_manifest_under_a_known_export_id_is_refused() {
        let (src, id) = source();
        let data = src.export_data(&id).unwrap();
        let dst = store();
        let h = hashes();
        dst.import_maint(&data, "exp-1", "m1", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap();
        assert_eq!(dst.import_maint(&data, "exp-1", "other", &Into::Same, &plan(&h, &|_| None), 210).unwrap_err(), "import-conflict");
    }

    #[test]
    fn same_id_with_another_master_serial_needs_a_choice() {
        let (src, id) = source();
        let mut data = src.export_data(&id).unwrap();
        data.master_serial = Some("SYN-OTHER".into());
        let h = hashes();
        assert_eq!(src.import_maint(&data, "e1", "m", &Into::Same, &plan(&h, &|_| None), 200).unwrap_err(), "import-master-differs");
        assert_eq!(src.import_maint(&data, "e2", "m", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap_err(), "import-id-exists");
        // as a copy: a new installation with another id and a free name
        let st = src.import_maint(&data, "e3", "m", &Into::New { copy: true }, &plan(&h, &|_| None), 200).unwrap();
        assert_ne!(st.target, id);
        assert_eq!(src.installation(&st.target).unwrap().name, "Bank A (Import)");
        // the log name of the source exists here (index and disk): renamed per export
        assert!(st.log_names["a_pack1.log"].starts_with("a_pack1-imp-"));
        // merged anyway: the stored master serial stays, the other is reported
        let st = src.import_maint(&data, "e4", "m", &Into::MergeAnyway, &plan(&h, &|_| None), 200).unwrap();
        assert_eq!(src.installation(&id).unwrap().master_serial.as_deref(), Some("SYN-M"));
        assert_eq!(st.report["master_serial_not_taken"], "SYN-OTHER");
    }

    #[test]
    fn merge_by_master_serial_only_when_it_matches() {
        let (src, id) = source();
        let data = src.export_data(&id).unwrap();
        let dst = store();
        let other = dst.create_installation("Here", None, vec![]).unwrap();
        let h = hashes();
        assert_eq!(dst.import_maint(&data, "e1", "m", &Into::Merge { id: other.id.clone() }, &plan(&h, &|_| None), 200).unwrap_err(), "import-master-differs");
        dst.set_master_serial(&other.id, "SYN-M", 50).unwrap();
        dst.set_groups(&other.id, Some("[2]")).unwrap();
        let st = dst.import_maint(&data, "e2", "m", &Into::Merge { id: other.id.clone() }, &plan(&h, &|_| None), 200).unwrap();
        assert_eq!(st.target, other.id);
        // settings add, never overwrite
        assert_eq!(dst.settings(&other.id).unwrap().groups.as_deref(), Some("[2]"));
        assert_eq!(st.report["not_taken"]["groups"], "[1]");
    }

    #[test]
    fn an_old_board_from_a_backup_does_not_become_current() {
        let (src, id) = source();
        let data = src.export_data(&id).unwrap();
        // here the place has a newer board since
        let dst = store();
        let h = hashes();
        dst.import_maint(&data, "e1", "m", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap();
        dst.complete_import("e1", 201).unwrap();
        dst.board_seen(&id, 1, &set("SYN-B2"), &SerialValue::NoAnswer, 500).unwrap();
        let mut old = data.clone();
        old.boards[0].first_seen = 10; // an older sighting of the first board: another row
        old.boards[0].last_seen = 20;
        dst.import_maint(&old, "e2", "m2", &Into::Same, &plan(&h, &|_| None), 600).unwrap();
        assert_eq!(dst.current_board(&id, 1).unwrap().unwrap().bms_serial, "SYN-B2");
    }

    #[test]
    fn another_computers_clock_never_makes_a_board_current() {
        let (src, id) = source();
        let mut data = src.export_data(&id).unwrap();
        let dst = store();
        let here = dst.create_installation("Here", None, vec![]).unwrap();
        dst.set_master_serial(&here.id, "SYN-M", 50).unwrap();
        // here a newer board was read live; the archive's old board carries a clock a day ahead
        dst.board_seen(&here.id, 1, &set("SYN-B2"), &SerialValue::NoAnswer, 1_000).unwrap();
        data.boards[0].last_seen = 1_000 + 86_400_000;
        let h = hashes();
        dst.import_maint(&data, "e1", "m", &Into::Merge { id: here.id.clone() }, &plan(&h, &|_| None), 2_000).unwrap();
        assert_eq!(dst.current_board(&here.id, 1).unwrap().unwrap().bms_serial, "SYN-B2");
        // the old board read live again: current again, without a new row
        dst.board_seen(&here.id, 1, &set("SYN-B1"), &SerialValue::NoAnswer, 3_000).unwrap();
        assert_eq!(dst.current_board(&here.id, 1).unwrap().unwrap().bms_serial, "SYN-B1");
        assert_eq!(dst.boards(&here.id, 1).unwrap().len(), 2);
        // and the newer one again
        dst.board_seen(&here.id, 1, &set("SYN-B2"), &SerialValue::NoAnswer, 4_000).unwrap();
        assert_eq!(dst.current_board(&here.id, 1).unwrap().unwrap().bms_serial, "SYN-B2");
    }

    #[test]
    fn an_installation_of_an_earlier_version_imports() {
        let (src, id) = source();
        let mut data = src.export_data(&id).unwrap();
        data.id = "inst-1a0f00c0ffe-1b2c".into();
        let dst = store();
        let h = hashes();
        let st = dst.import_maint(&data, "e1", "m", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap();
        assert_eq!(st.target, "inst-1a0f00c0ffe-1b2c");
        data.id = "192.168.1.10:4196".into();
        assert!(dst.import_maint(&data, "e2", "m", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap_err().starts_with("bundle-manifest"));
    }

    #[test]
    fn log_names_must_be_plain_and_in_the_bundle() {
        let (src, id) = source();
        let mut data = src.export_data(&id).unwrap();
        let dst = store();
        let h = hashes();
        data.logs[0].file = "../x.log".into();
        assert!(dst.import_maint(&data, "e1", "m", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap_err().starts_with("bundle-entry"));
        data.logs[0].file = "not_in_bundle.log".into();
        assert!(dst.import_maint(&data, "e2", "m", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap_err().starts_with("bundle-entry"));
        data.logs[0].missing = true;
        let st = dst.import_maint(&data, "e3", "m", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap();
        assert_eq!(st.report["logs_missing"], 1);
    }

    #[test]
    fn an_open_import_blocks_delete_and_merge_of_its_target() {
        let (src, id) = source();
        let data = src.export_data(&id).unwrap();
        let dst = store();
        let other = dst.create_installation("Other", None, vec![]).unwrap();
        let h = hashes();
        dst.import_maint(&data, "e1", "m", &Into::New { copy: false }, &plan(&h, &|_| None), 200).unwrap();
        assert_eq!(dst.delete_installation(&id).unwrap_err(), "installation-busy");
        assert_eq!(dst.merge(&id, &other.id, serde_json::Value::Null, 300).unwrap_err(), "installation-busy");
        assert_eq!(dst.merge(&other.id, &id, serde_json::Value::Null, 300).unwrap_err(), "installation-busy");
        dst.complete_import("e1", 400).unwrap();
        dst.merge(&other.id, &id, serde_json::Value::Null, 500).unwrap();
    }
}
