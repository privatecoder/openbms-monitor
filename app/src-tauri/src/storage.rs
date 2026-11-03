//! Data & storage (About view): what the data folder holds, clearing the recording, deleting the migration backups
//! and emptying the folder.
//!
//! Clearing the recording runs while the app runs, under the installation reservation; the history mutex serialises
//! it with the recorder. Emptying the folder also deletes files an open database or log may hold (Windows), so it
//! only leaves a marker (`pending-empty`, the open imports one per line) and restarts: the next start applies it
//! before the databases open (`apply_pending_empty`) and right after the maintenance store opens
//! (`finish_pending_empty`). A step that fails keeps the marker for the next start and says why in
//! `pending-empty.err`. The installations, their boards and settings, the BMS archive and every pack's safety state
//! stay (`Store::empty_data`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::{history, maint_store::Store, AppState};

const MARKER: &str = "pending-empty";
const MARKER_ERR: &str = "pending-empty.err";
/// Folders emptying deletes as a whole.
const EMPTIED_DIRS: [&str; 4] = ["firmware-logs", "parameters", "exports", "recordings"];
/// The categories in display order.
const KEYS: [&str; 8] = ["recording", "installations", "firmware_logs", "parameters", "exports", "backups", "imports", "other"];

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub key: &'static str,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Serialize, Debug, Clone)]
pub struct InstallationData {
    pub id: String,
    pub name: String,
    /// recording rows, firmware logs or BMS records: a backup before a delete takes this installation
    pub has_data: bool,
    pub recording_rows: u64,
}

#[derive(Serialize, Debug, Clone)]
pub struct Overview {
    pub dir: String,
    pub total: u64,
    /// free space on the drive; None where it cannot be asked
    pub free: Option<u64>,
    pub categories: Vec<Category>,
    pub recording_rows: u64,
    pub recording_span: Option<(i64, i64)>,
    /// space inside the recording file that deleted rows left: "deleted, but the file is not yet smaller"
    pub reclaimable: u64,
    pub bms_records: u64,
    pub open_imports: u64,
    pub installations: Vec<InstallationData>,
    /// emptying is waiting for the next start
    pub empty_pending: bool,
    /// why the last emptying did not finish (it is tried again at the next start)
    pub empty_error: Option<String>,
}

/// Bytes and files below a path (a file: itself).
fn walk(p: &Path) -> (u64, u64) {
    let Ok(meta) = std::fs::symlink_metadata(p) else { return (0, 0) };
    if !meta.is_dir() {
        return (meta.len(), 1);
    }
    std::fs::read_dir(p)
        .map(|rd| {
            rd.flatten().fold((0, 0), |(b, f), e| {
                let (b2, f2) = walk(&e.path());
                (b + b2, f + f2)
            })
        })
        .unwrap_or((0, 0))
}

fn is_backup(name: &str) -> bool {
    name.contains(".sqlite.bak-")
}

fn key_of(name: &str) -> &'static str {
    match name {
        "history.sqlite" | "history.sqlite-wal" | "history.sqlite-shm" => "recording",
        "maintenance.sqlite" | "maintenance.sqlite-wal" | "maintenance.sqlite-shm" => "installations",
        "firmware-logs" => "firmware_logs",
        "parameters" => "parameters",
        "exports" | "recordings" => "exports",
        "imports" => "imports",
        n if is_backup(n) => "backups",
        _ => "other",
    }
}

/// Sort the folder's contents into the categories. Open imports count nowhere: they are not leftovers.
pub fn measure(dir: &Path, open_imports: &[String]) -> Vec<Category> {
    let mut out: Vec<Category> = KEYS.iter().map(|&key| Category { key, bytes: 0, files: 0 }).collect();
    let mut add = |key: &str, (bytes, files): (u64, u64)| {
        if let Some(c) = out.iter_mut().find(|c| c.key == key) {
            c.bytes += bytes;
            c.files += files;
        }
    };
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let key = key_of(&name);
        if key == "imports" {
            for i in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
                if !open_imports.iter().any(|o| *o == i.file_name().to_string_lossy()) {
                    add("imports", walk(&i.path()));
                }
            }
        } else {
            add(key, walk(&e.path()));
        }
    }
    out
}

/// Delete the migration backups (`*.sqlite.bak-N`); the bytes freed.
pub fn delete_backups(dir: &Path) -> Result<u64, String> {
    let mut freed = 0;
    for e in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        if is_backup(&e.file_name().to_string_lossy()) {
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            std::fs::remove_file(e.path()).map_err(|err| format!("{}: {err}", e.path().display()))?;
            freed += len;
        }
    }
    Ok(freed)
}

fn record_error(dir: &Path, msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(MARKER_ERR)) {
        let _ = writeln!(f, "{msg}");
    }
}

fn remove_path(p: &Path) -> Result<(), String> {
    let r = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
    match r {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

/// First half of a pending emptying, before any database is open: the files and the recording. True if a marker was
/// there. Never fails the start: errors go to `pending-empty.err`.
pub fn apply_pending_empty(dir: &Path) -> bool {
    let Ok(marker) = std::fs::read_to_string(dir.join(MARKER)) else { return false };
    let _ = std::fs::remove_file(dir.join(MARKER_ERR));
    let open: Vec<&str> = marker.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let mut errors = Vec::new();
    if let Err(e) = history::empty_file(&dir.join("history.sqlite")) {
        errors.push(format!("recording: {e}"));
    }
    if let Err(e) = delete_backups(dir) {
        errors.push(e);
    }
    for d in EMPTIED_DIRS {
        if let Err(e) = remove_path(&dir.join(d)) {
            errors.push(e);
        }
    }
    for i in std::fs::read_dir(dir.join("imports")).into_iter().flatten().flatten() {
        if !open.iter().any(|o| *o == i.file_name().to_string_lossy()) {
            if let Err(e) = remove_path(&i.path()) {
                errors.push(e);
            }
        }
    }
    for e in &errors {
        record_error(dir, e);
    }
    true
}

/// Second half, once the maintenance store is open: its rows. The marker goes only if both halves succeeded.
pub fn finish_pending_empty(store: &Store, dir: &Path) {
    if !dir.join(MARKER).exists() {
        return;
    }
    if let Err(e) = store.empty_data() {
        record_error(dir, &format!("installation data: {e}"));
    }
    if !dir.join(MARKER_ERR).exists() {
        let _ = std::fs::remove_file(dir.join(MARKER));
    }
}

/// The export ids of the imports that can still be resumed: never cleaned away.
fn open_ids(store: &Store) -> Result<Vec<String>, String> {
    Ok(store.open_imports()?.into_iter().map(|r| r.export_id).collect())
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

fn refusal(state: &AppState) -> Option<String> {
    crate::updater::busy(state).map(|b| format!("busy:{b}"))
}

/// What the data folder holds (About → Data & storage).
#[tauri::command]
pub async fn storage_overview(app: AppHandle, state: State<'_, AppState>) -> Result<Overview, String> {
    let dir = data_dir(&app)?;
    let (open, counts, names) = {
        let m = state.maint.lock().map_err(|e| e.to_string())?;
        match m.as_ref() {
            Some(s) => (open_ids(s)?, s.data_counts()?, s.installations()?.into_iter().map(|i| (i.id, i.name)).collect::<Vec<_>>()),
            None => (vec![], vec![], vec![]),
        }
    };
    let hpath = state.history.lock().map_err(|e| e.to_string())?.path();
    tauri::async_runtime::spawn_blocking(move || {
        let categories = measure(&dir, &open);
        let (total, _) = walk(&dir);
        let (mut rows_of, mut span, mut reclaimable) = (Vec::<(String, u64)>::new(), None::<(i64, i64)>, 0);
        if let Some(db) = hpath.as_deref().and_then(|p| history::reader(p).ok()) {
            for s in history::sites(&db)? {
                let (rows, _, _) = history::site_summary(&db, &s.site)?;
                rows_of.push((s.site.clone(), rows.max(0) as u64));
                if let (Some(f), Some(t)) = (s.from, s.to) {
                    span = Some(span.map_or((f, t), |(a, b): (i64, i64)| (a.min(f), b.max(t))));
                }
            }
            reclaimable = history::reclaimable(&db)?;
        }
        let rows = |id: &str| rows_of.iter().find(|(s, _)| s == id).map_or(0, |(_, n)| *n);
        let installations = names
            .iter()
            .map(|(id, name)| {
                let (logs, bms) = counts.iter().find(|(i, _, _)| i == id).map_or((0, 0), |(_, l, b)| (*l, *b));
                InstallationData { id: id.clone(), name: name.clone(), has_data: rows(id) > 0 || logs > 0 || bms > 0, recording_rows: rows(id) }
            })
            .collect();
        Ok(Overview {
            dir: dir.display().to_string(),
            total,
            free: crate::migrate::free_space(&dir),
            categories,
            recording_rows: rows_of.iter().map(|(_, n)| n).sum(),
            recording_span: span,
            reclaimable,
            bms_records: counts.iter().map(|(_, _, b)| b).sum(),
            open_imports: open.len() as u64,
            installations,
            empty_pending: dir.join(MARKER).exists(),
            empty_error: std::fs::read_to_string(dir.join(MARKER_ERR)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Clear the recording of one installation (or of all with None). Installations, boards, parameter states and the
/// BMS archive stay; a backup of this installation can be restored again afterwards.
#[tauri::command]
pub async fn storage_clear_recording(state: State<'_, AppState>, installation: Option<String>) -> Result<history::Cleared, String> {
    if let Some(r) = refusal(&state) {
        return Err(r);
    }
    let id = installation.clone().unwrap_or_else(|| crate::control::ALL.to_string());
    let held = crate::control::reserve_installations(&state.ops, &[id.as_str()], "clear").map_err(|_| "busy:installations".to_string())?;
    let (history, maint) = (state.history.clone(), state.maint.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _held = held;
        let done = history.lock().map_err(|e| e.to_string())?.clear(installation.as_deref())?;
        if let Some(s) = maint.lock().map_err(|e| e.to_string())?.as_ref() {
            s.forget_completed_imports(installation.as_deref())?;
        }
        Ok(done)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Delete the migration backups; the bytes freed. They are written only at start, so nothing else touches them now.
#[tauri::command]
pub fn storage_delete_backups(app: AppHandle) -> Result<u64, String> {
    delete_backups(&data_dir(&app)?)
}

/// Empty the data folder at the next start (see the module doc) and restart now. Holds the bus and every
/// installation until the process ends, so no flash, pack action or installation operation starts meanwhile.
#[tauri::command]
pub fn storage_empty(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    if let Some(r) = refusal(&state) {
        return Err(r);
    }
    let slot = crate::updater::install_slot(&state).map_err(|why| format!("busy:{why}"))?;
    let dir = data_dir(&app)?;
    let open = match state.maint.lock().map_err(|e| e.to_string())?.as_ref() {
        Some(s) => open_ids(s)?,
        None => vec![],
    };
    std::fs::write(dir.join(MARKER), open.join("\n") + "\n").map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(dir.join(MARKER_ERR));
    // the slot lives until the process ends
    std::mem::forget(slot);
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{History, Settings};

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("openbms-storage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(dir: &Path, rel: &str, n: usize) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'x'; n]).unwrap();
    }

    #[test]
    fn measure_sorts_files_into_categories() {
        let d = temp("measure");
        for (rel, n) in [
            ("history.sqlite", 10),
            ("history.sqlite-wal", 5),
            ("maintenance.sqlite", 7),
            ("history.sqlite.bak-2", 3),
            ("firmware-logs/a.log", 4),
            ("parameters/p.xml", 2),
            ("recordings/x.csv", 6),
            ("imports/open1/f", 1),
            ("imports/old/f", 8),
            ("stray.txt", 9),
        ] {
            put(&d, rel, n);
        }
        let got: Vec<(&str, u64)> = measure(&d, &["open1".into()]).into_iter().map(|c| (c.key, c.bytes)).collect();
        assert_eq!(
            got,
            vec![("recording", 15), ("installations", 7), ("firmware_logs", 4), ("parameters", 2), ("exports", 6), ("backups", 3), ("imports", 8), ("other", 9)]
        );
    }

    #[test]
    fn deleting_backups_frees_their_bytes() {
        let d = temp("bak");
        put(&d, "history.sqlite.bak-1", 3);
        put(&d, "maintenance.sqlite.bak-13", 4);
        put(&d, "history.sqlite", 9);
        assert_eq!(delete_backups(&d).unwrap(), 7);
        assert!(!d.join("history.sqlite.bak-1").exists() && !d.join("maintenance.sqlite.bak-13").exists());
        assert!(d.join("history.sqlite").exists());
    }

    #[test]
    fn emptying_keeps_installations_open_imports_and_the_recording_settings() {
        let d = temp("empty");
        // a recording with values and its settings
        {
            let mut h = History::default();
            h.open(&d.join("history.sqlite")).unwrap();
            h.set_settings(Settings { enabled: true, keep_full_days: 7, ..Settings::default() }).unwrap();
            h.connected("inst", "can");
            h.pack(1000, 0, &serde_json::json!({ "address": 0, "telemetry": { "cell_voltages": [3.3], "current": 1.0 } }));
        }
        let store = Store::open(&d.join("maintenance.sqlite")).unwrap();
        let inst = store.create_installation("Bank", None, vec![]).unwrap().id;
        let rec = {
            let mut r = vec![0x01, 0x07, 0xEA, 3, 9, 7, 1, 0];
            r.resize(openbms_proto::memory::RECORD_LEN, 0);
            r
        };
        store.add_bms_records(&inst, 0, Some("SYN-1"), &[&rec], 5).unwrap();
        store.add_param_snapshot(&inst, 0, history::SnapshotKind::Read, &[0u8; 4], 5).unwrap();
        store.add_log(Some(&inst), Some(0), "flash", None, "f.log", 5).unwrap();
        {
            let raw = rusqlite::Connection::open(d.join("maintenance.sqlite")).unwrap();
            raw.execute("INSERT INTO import_status(export_id, installation, started, completed) VALUES('done1', ?1, 1, 2)", [&inst]).unwrap();
            raw.execute("INSERT INTO import_status(export_id, installation, started, completed) VALUES('open1', ?1, 1, NULL)", [&inst]).unwrap();
        }
        for rel in ["firmware-logs/f.log", "parameters/p.xml", "exports/e.zip", "recordings/r.csv", "imports/open1/m", "imports/done1/m"] {
            put(&d, rel, 3);
        }
        put(&d, "history.sqlite.bak-1", 3);
        std::fs::write(d.join(MARKER), "open1\n").unwrap();

        assert!(apply_pending_empty(&d));
        finish_pending_empty(&store, &d);

        assert!(!d.join(MARKER).exists() && !d.join(MARKER_ERR).exists());
        assert!(store.installation(&inst).is_ok());
        let raw = rusqlite::Connection::open(d.join("maintenance.sqlite")).unwrap();
        let count = |sql: &str| -> i64 { raw.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT COUNT(*) FROM bms_records"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM param_snapshots"), 0);
        assert_eq!(count("SELECT COUNT(*) FROM logs"), 0);
        assert_eq!(count("SELECT COUNT(*) FROM import_status WHERE export_id = 'open1'"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM import_status WHERE export_id = 'done1'"), 0);
        assert!(d.join("imports/open1").exists() && !d.join("imports/done1").exists());
        for gone in ["firmware-logs", "parameters", "exports", "recordings", "history.sqlite.bak-1"] {
            assert!(!d.join(gone).exists(), "{gone}");
        }
        let hdb = rusqlite::Connection::open(d.join("history.sqlite")).unwrap();
        let rows: i64 = hdb.query_row("SELECT COUNT(*) FROM pack", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0);
        let mut h = History::default();
        h.open(&d.join("history.sqlite")).unwrap();
        assert_eq!(h.status().settings.keep_full_days, 7);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_empty_keeps_the_marker_and_says_why() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("fail");
        put(&d, "parameters/locked/p.xml", 3);
        let locked = d.join("parameters/locked");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        std::fs::write(d.join(MARKER), "\n").unwrap();
        let store = Store::open(&d.join("maintenance.sqlite")).unwrap();
        assert!(apply_pending_empty(&d));
        finish_pending_empty(&store, &d);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(d.join(MARKER).exists());
        assert!(!std::fs::read_to_string(d.join(MARKER_ERR)).unwrap().trim().is_empty());
        // the next start finishes it and clears the error
        assert!(apply_pending_empty(&d));
        finish_pending_empty(&store, &d);
        assert!(!d.join(MARKER).exists() && !d.join(MARKER_ERR).exists());
    }

    #[test]
    fn without_a_marker_nothing_happens() {
        let d = temp("none");
        put(&d, "parameters/p.xml", 3);
        assert!(!apply_pending_empty(&d));
        assert!(d.join("parameters/p.xml").exists());
    }
}
