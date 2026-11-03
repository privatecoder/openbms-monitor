//! Data & storage (About view): what the data folder holds, clearing the recording, deleting the migration backups
//! and emptying the folder, each optionally after a backup as installation ZIPs.
//!
//! **Backup and delete are one backend operation.** The affected installations stay reserved and the recording stays
//! paused from the moment the targets are chosen until the delete is done, so nothing recorded after the backup is
//! deleted unsaved. If a backup fails, nothing is deleted. The backup folder must lie outside the data folder (the
//! emptying would otherwise delete the backup it just made). New ZIPs never overwrite a file.
//!
//! **Emptying** also deletes files an open database or log may hold (Windows), so it leaves a job file
//! (`pending-empty.json`, written atomically) and restarts. The next start runs the file steps before any database is
//! open (`apply_pending_empty`) and the database steps right after the maintenance store opens
//! (`finish_pending_empty`). Every finished step is written down, so a later run repeats only what is left. If a step
//! fails, the job turns `failed` and the start goes on without deleting anything more: the About view says why and
//! offers to try again or to drop the job. An unreadable job file deletes nothing.
//!
//! Refused while an import is open (its progress marks would no longer match), without the maintenance database (no
//! proof which imports are open), and while a firmware update, pack or maintenance action, BMS history read,
//! parameter write or installation operation runs. The installations, their boards and settings, the BMS archive and
//! every pack's safety state stay (`Store::empty_data`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::{history, maint_store::Store, AppState};

const JOB: &str = "pending-empty.json";
/// The file steps of an emptying in order, then the database steps.
const FILE_STEPS: [&str; 6] = ["recording", "backups", "firmware-logs", "parameters", "exports", "recordings"];
const STORE_STEPS: [&str; 2] = ["store", "imports"];
/// The categories in display order.
const KEYS: [&str; 8] = ["recording", "installations", "firmware_logs", "parameters", "exports", "backups", "imports", "other"];

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    /// waits for the next start
    Requested,
    /// a step failed; waits for the user (try again / drop)
    Failed,
}

/// The emptying job on disk.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub state: JobState,
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

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
    /// recording (values or events), firmware logs or BMS records: a backup before a delete takes this installation
    pub has_data: bool,
    pub recording_rows: u64,
    pub recording_events: u64,
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
    /// the emptying job, if one waits or failed
    pub job: Option<Job>,
}

#[derive(Serialize, Debug, Clone)]
pub struct Done {
    /// the recording file shrank (false: deleted, but the space stays inside the file for now)
    pub compacted: bool,
    /// the ZIPs written
    pub backups: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// measuring

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

/// A finished migration backup: `<name>.sqlite.bak-<number>` exactly (a `.tmp` being written is not one).
fn is_backup(name: &str) -> bool {
    name.rsplit_once(".sqlite.bak-").is_some_and(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
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

// ---------------------------------------------------------------------------------------------
// deleting

/// Delete the migration backups; the bytes freed.
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

fn remove_path(p: &Path) -> Result<(), String> {
    let r = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
    match r {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

/// The job file, or None without one. An unreadable file is a failed job that deletes nothing.
pub fn read_job(dir: &Path) -> Option<Job> {
    let text = match std::fs::read(dir.join(JOB)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => return Some(Job { state: JobState::Failed, done: vec![], error: Some(format!("{JOB}: {e}")) }),
    };
    Some(serde_json::from_slice(&text).unwrap_or_else(|e| Job { state: JobState::Failed, done: vec![], error: Some(format!("{JOB}: {e}")) }))
}

/// Write the job atomically (a torn write cannot shorten it).
pub fn write_job(dir: &Path, job: &Job) -> Result<(), String> {
    let tmp = dir.join(format!("{JOB}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(job).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(JOB)).map_err(|e| e.to_string())
}

fn run_steps(job: &mut Job, steps: &[&str], mut step: impl FnMut(&str) -> Result<(), String>) {
    for s in steps {
        if job.done.iter().any(|d| d == s) {
            continue;
        }
        match step(s) {
            Ok(()) => job.done.push(s.to_string()),
            Err(e) => {
                job.state = JobState::Failed;
                job.error = Some(format!("{s}: {e}"));
                return;
            }
        }
    }
}

/// First half of a requested emptying, before any database is open: the recording and the files. Returns the job as
/// it stands (None: nothing to do). Never fails the start.
pub fn apply_pending_empty(dir: &Path) -> Option<Job> {
    let mut job = read_job(dir)?;
    if job.state != JobState::Requested {
        return Some(job);
    }
    run_steps(&mut job, &FILE_STEPS, |s| match s {
        "recording" => history::empty_file(&dir.join("history.sqlite")),
        "backups" => delete_backups(dir).map(|_| ()),
        d => remove_path(&dir.join(d)),
    });
    if let Err(e) = write_job(dir, &job) {
        eprintln!("storage: {JOB}: {e}");
    }
    Some(job)
}

/// Second half, once the maintenance store is open: its rows and the import leftovers (the open imports are asked
/// from the store now). The job file goes once every step is done.
pub fn finish_pending_empty(store: &Store, dir: &Path, job: Option<Job>) {
    let Some(mut job) = job else { return };
    if job.state != JobState::Requested {
        return;
    }
    run_steps(&mut job, &STORE_STEPS, |s| match s {
        "store" => store.empty_data(),
        _ => {
            let open: Vec<String> = store.open_imports()?.into_iter().map(|r| r.export_id).collect();
            for i in std::fs::read_dir(dir.join("imports")).into_iter().flatten().flatten() {
                if !open.iter().any(|o| *o == i.file_name().to_string_lossy()) {
                    remove_path(&i.path())?;
                }
            }
            Ok(())
        }
    });
    let result = if job.state == JobState::Requested && FILE_STEPS.iter().chain(&STORE_STEPS).all(|s| job.done.iter().any(|d| d == s)) {
        std::fs::remove_file(dir.join(JOB)).map_err(|e| e.to_string())
    } else {
        write_job(dir, &job)
    };
    if let Err(e) = result {
        eprintln!("storage: {JOB}: {e}");
    }
}

// ---------------------------------------------------------------------------------------------
// backups

/// The file name of an installation ZIP, as the export view names it (`openbms-<name>-<date>.zip`).
fn bundle_name(name: &str, date: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' }).collect();
    let safe = safe.trim_matches('-');
    let safe = if safe.is_empty() { "anlage" } else { safe };
    format!("openbms-{safe}-{date}.zip")
}

/// A path in `folder` that does not exist yet: the name, else "name (2).zip", "name (3).zip" …
fn free_path(folder: &Path, file: &str) -> PathBuf {
    let first = folder.join(file);
    if !first.exists() {
        return first;
    }
    let stem = file.strip_suffix(".zip").unwrap_or(file);
    (2..).map(|n| folder.join(format!("{stem} ({n}).zip"))).find(|p| !p.exists()).expect("a free name")
}

/// The backup folder exists and lies outside the data folder.
fn backup_folder(data: &Path, folder: &str) -> Result<PathBuf, String> {
    let f = std::fs::canonicalize(folder).map_err(|e| format!("{folder}: {e}"))?;
    let d = std::fs::canonicalize(data).unwrap_or_else(|_| data.to_path_buf());
    if f.starts_with(&d) {
        return Err("backup-inside-data".into());
    }
    Ok(f)
}

/// Back up the installations with data (all, or one) as ZIPs into `folder`; the paths written.
fn back_up(state: &AppState, logs: &Path, folder: &Path, only: Option<&str>) -> Result<Vec<String>, String> {
    let targets: Vec<InstallationData> = installations(state)?.into_iter().filter(|i| i.has_data && only.is_none_or(|o| o == i.id)).collect();
    let db = state.history.lock().map_err(|e| e.to_string())?.path();
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut written = Vec::new();
    for t in targets {
        let out = free_path(folder, &bundle_name(&t.name, &date));
        crate::installation_bundle::export(&state.maint, db.as_deref(), logs, &t.id, &out, true, history::now_ms())?;
        written.push(out.display().to_string());
    }
    Ok(written)
}

// ---------------------------------------------------------------------------------------------
// commands

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// The installations with what they hold.
fn installations(state: &AppState) -> Result<Vec<InstallationData>, String> {
    let (counts, names) = {
        let m = state.maint.lock().map_err(|e| e.to_string())?;
        let s = m.as_ref().ok_or("maint-missing")?;
        (s.data_counts()?, s.installations()?.into_iter().map(|i| (i.id, i.name)).collect::<Vec<_>>())
    };
    let hpath = state.history.lock().map_err(|e| e.to_string())?.path();
    let rec = match hpath.as_deref().and_then(|p| history::reader(p).ok()) {
        Some(db) => history::recording_counts(&db)?,
        None => vec![],
    };
    Ok(names
        .into_iter()
        .map(|(id, name)| {
            let (rows, events) = rec.iter().find(|(s, _, _)| *s == id).map_or((0, 0), |(_, r, e)| (*r, *e));
            let (logs, bms) = counts.iter().find(|(i, _, _)| *i == id).map_or((0, 0), |(_, l, b)| (*l, *b));
            InstallationData { id, name, has_data: rows + events + logs + bms > 0, recording_rows: rows, recording_events: events }
        })
        .collect())
}

/// Refused while something else runs, without the maintenance database, or while an import is open (all, or for one
/// installation).
fn refusal(state: &AppState, only: Option<&str>) -> Result<(), String> {
    if let Some(b) = crate::updater::busy(state) {
        return Err(format!("busy:{b}"));
    }
    let m = state.maint.lock().map_err(|e| e.to_string())?;
    let s = m.as_ref().ok_or("maint-missing")?;
    if s.open_imports()?.iter().any(|r| only.is_none_or(|o| o == r.installation)) {
        return Err("open-import".into());
    }
    Ok(())
}

/// The recording stays paused while this lives.
struct Paused<'a>(&'a AppState);

impl<'a> Paused<'a> {
    fn new(state: &'a AppState) -> Result<Self, String> {
        state.history.lock().map_err(|e| e.to_string())?.pause(true);
        Ok(Paused(state))
    }
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        if let Ok(mut h) = self.0.history.lock() {
            h.pause(false);
        }
    }
}

/// What the data folder holds (About → Data & storage).
#[tauri::command]
pub async fn storage_overview(app: AppHandle, state: State<'_, AppState>) -> Result<Overview, String> {
    let dir = data_dir(&app)?;
    let open: Vec<String> = match state.maint.lock().map_err(|e| e.to_string())?.as_ref() {
        Some(s) => s.open_imports()?.into_iter().map(|r| r.export_id).collect(),
        None => vec![],
    };
    let list = installations(&state).unwrap_or_default();
    let bms = match state.maint.lock().map_err(|e| e.to_string())?.as_ref() {
        Some(s) => s.data_counts()?.iter().map(|(_, _, b)| b).sum(),
        None => 0,
    };
    let hpath = state.history.lock().map_err(|e| e.to_string())?.path();
    tauri::async_runtime::spawn_blocking(move || {
        let categories = measure(&dir, &open);
        let (total, _) = walk(&dir);
        let (mut rows, mut span, mut reclaimable) = (0u64, None::<(i64, i64)>, 0);
        if let Some(db) = hpath.as_deref().and_then(|p| history::reader(p).ok()) {
            for s in history::sites(&db)? {
                if let (Some(f), Some(t)) = (s.from, s.to) {
                    span = Some(span.map_or((f, t), |(a, b): (i64, i64)| (a.min(f), b.max(t))));
                }
            }
            rows = history::recording_counts(&db)?.iter().map(|(_, r, _)| r).sum();
            reclaimable = history::reclaimable(&db)?;
        }
        Ok(Overview {
            dir: dir.display().to_string(),
            total,
            free: crate::migrate::free_space(&dir),
            categories,
            recording_rows: rows,
            recording_span: span,
            reclaimable,
            bms_records: bms,
            open_imports: open.len() as u64,
            installations: list,
            job: read_job(&dir),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Clear the recording of one installation (or of all with None), after a backup into `backup` if given.
/// Installations, boards, parameter states and the BMS archive stay; the backup can be restored again afterwards.
#[tauri::command]
pub async fn storage_clear(app: AppHandle, state: State<'_, AppState>, installation: Option<String>, backup: Option<String>) -> Result<Done, String> {
    refusal(&state, installation.as_deref())?;
    let dir = data_dir(&app)?;
    let folder = backup.as_deref().map(|b| backup_folder(&dir, b)).transpose()?;
    let id = installation.clone().unwrap_or_else(|| crate::control::ALL.to_string());
    let held = crate::control::reserve_installations(&state.ops, &[id.as_str()], "clear").map_err(|_| "busy:installations".to_string())?;
    let logs = crate::firmware::logs_dir(&app)?;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _held = held;
        let state = app2.state::<AppState>();
        let _paused = Paused::new(&state)?;
        let backups = match &folder {
            Some(f) => back_up(&state, &logs, f, installation.as_deref())?,
            None => vec![],
        };
        // repeatable and harmless first: once the rows are gone, a restore of these ZIPs must not end as "done"
        if let Some(s) = state.maint.lock().map_err(|e| e.to_string())?.as_ref() {
            s.forget_completed_imports(installation.as_deref())?;
        }
        let done = state.history.lock().map_err(|e| e.to_string())?.clear(installation.as_deref())?;
        Ok(Done { compacted: done.compacted, backups })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Delete the migration backups; the bytes freed. Not while the databases are still being prepared at start.
#[tauri::command]
pub fn storage_delete_backups(app: AppHandle, state: State<AppState>) -> Result<u64, String> {
    if let Some(b) = crate::updater::busy(&state) {
        return Err(format!("busy:{b}"));
    }
    if state.history.lock().map_err(|e| e.to_string())?.preparing() {
        return Err("history-preparing".into());
    }
    delete_backups(&data_dir(&app)?)
}

/// Empty the data folder at the next start (see the module doc), after a backup into `backup` if given, and restart.
/// Holds the bus and every installation until the process ends.
#[tauri::command]
pub async fn storage_empty(app: AppHandle, backup: Option<String>) -> Result<(), String> {
    // the backup may take a while: off the main thread
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        refusal(&state, None)?;
        let dir = data_dir(&app)?;
        let folder = backup.as_deref().map(|b| backup_folder(&dir, b)).transpose()?;
        let slot = crate::updater::install_slot(&state).map_err(|why| format!("busy:{why}"))?;
        let paused = Paused::new(&state)?;
        if let Some(f) = &folder {
            back_up(&state, &crate::firmware::logs_dir(&app)?, f, None)?;
        }
        write_job(&dir, &Job { state: JobState::Requested, done: vec![], error: None })?;
        // slot and pause live until the process ends
        std::mem::forget(paused);
        std::mem::forget(slot);
        app.restart()
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A failed emptying: try the remaining steps again at a restart now.
#[tauri::command]
pub fn storage_empty_retry(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    refusal(&state, None)?;
    let dir = data_dir(&app)?;
    let mut job = read_job(&dir).ok_or("no-job")?;
    job.state = JobState::Requested;
    job.error = None;
    let slot = crate::updater::install_slot(&state).map_err(|why| format!("busy:{why}"))?;
    write_job(&dir, &job)?;
    std::mem::forget(slot);
    app.restart()
}

/// A failed emptying: drop it; what was deleted stays deleted, nothing more goes.
#[tauri::command]
pub fn storage_empty_drop(app: AppHandle) -> Result<(), String> {
    remove_path(&data_dir(&app)?.join(JOB))
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

    fn requested(dir: &Path) {
        write_job(dir, &Job { state: JobState::Requested, done: vec![], error: None }).unwrap();
    }

    #[test]
    fn measure_sorts_files_into_categories() {
        let d = temp("measure");
        for (rel, n) in [
            ("history.sqlite", 10),
            ("history.sqlite-wal", 5),
            ("maintenance.sqlite", 7),
            ("history.sqlite.bak-2", 3),
            ("history.sqlite.bak-3.tmp", 1),
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
            vec![("recording", 15), ("installations", 7), ("firmware_logs", 4), ("parameters", 2), ("exports", 6), ("backups", 3), ("imports", 8), ("other", 10)]
        );
    }

    #[test]
    fn deleting_backups_frees_their_bytes_and_leaves_a_copy_being_written() {
        let d = temp("bak");
        put(&d, "history.sqlite.bak-1", 3);
        put(&d, "maintenance.sqlite.bak-13", 4);
        put(&d, "history.sqlite.bak-4.tmp", 5);
        put(&d, "history.sqlite", 9);
        assert_eq!(delete_backups(&d).unwrap(), 7);
        assert!(!d.join("history.sqlite.bak-1").exists() && !d.join("maintenance.sqlite.bak-13").exists());
        assert!(d.join("history.sqlite").exists() && d.join("history.sqlite.bak-4.tmp").exists());
    }

    fn seeded_store(d: &Path) -> (Store, String) {
        let store = Store::open(&d.join("maintenance.sqlite")).unwrap();
        let inst = store.create_installation("Bank", None, vec![]).unwrap().id;
        let mut rec = vec![0x01, 0x07, 0xEA, 3, 9, 7, 1, 0];
        rec.resize(openbms_proto::memory::RECORD_LEN, 0);
        store.add_bms_records(&inst, 0, Some("SYN-1"), &[&rec], 5).unwrap();
        store.add_param_snapshot(&inst, 0, history::SnapshotKind::Read, &[0u8; 4], 5).unwrap();
        store.add_log(Some(&inst), Some(0), "flash", None, "f.log", 5).unwrap();
        (store, inst)
    }

    #[test]
    fn emptying_keeps_installations_open_imports_and_the_recording_settings() {
        let d = temp("empty");
        {
            let mut h = History::default();
            h.open(&d.join("history.sqlite")).unwrap();
            h.set_settings(Settings { enabled: true, keep_full_days: 7, ..Settings::default() }).unwrap();
            h.connected("inst", "can");
            h.pack(1000, 0, &serde_json::json!({ "address": 0, "telemetry": { "cell_voltages": [3.3], "current": 1.0 } }));
        }
        let (store, inst) = seeded_store(&d);
        {
            let raw = rusqlite::Connection::open(d.join("maintenance.sqlite")).unwrap();
            raw.execute("INSERT INTO import_status(export_id, installation, started, completed) VALUES('done1', ?1, 1, 2)", [&inst]).unwrap();
            raw.execute("INSERT INTO import_status(export_id, installation, started, completed) VALUES('open1', ?1, 1, NULL)", [&inst]).unwrap();
        }
        for rel in ["firmware-logs/f.log", "parameters/p.xml", "exports/e.zip", "recordings/r.csv", "imports/open1/m", "imports/done1/m"] {
            put(&d, rel, 3);
        }
        put(&d, "history.sqlite.bak-1", 3);
        requested(&d);

        let job = apply_pending_empty(&d);
        finish_pending_empty(&store, &d, job);

        assert!(read_job(&d).is_none());
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
    fn a_failed_empty_stops_and_a_retry_repeats_only_what_is_left() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("fail");
        put(&d, "firmware-logs/a.log", 3);
        put(&d, "parameters/locked/p.xml", 3);
        let locked = d.join("parameters/locked");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        let (store, _) = seeded_store(&d);
        requested(&d);
        let job = apply_pending_empty(&d);
        finish_pending_empty(&store, &d, job);
        let job = read_job(&d).unwrap();
        assert_eq!(job.state, JobState::Failed);
        assert!(job.error.as_deref().unwrap_or("").starts_with("parameters"));
        assert!(job.done.contains(&"firmware-logs".to_string()));
        // the store step did not run: the parameter states are still there
        let raw = rusqlite::Connection::open(d.join("maintenance.sqlite")).unwrap();
        let snaps = || -> i64 { raw.query_row("SELECT COUNT(*) FROM param_snapshots", [], |r| r.get(0)).unwrap() };
        assert_eq!(snaps(), 1);
        // a start in the failed state deletes nothing more, also not what came new since
        put(&d, "firmware-logs/new.log", 3);
        let job = apply_pending_empty(&d);
        finish_pending_empty(&store, &d, job);
        assert!(d.join("firmware-logs/new.log").exists());
        assert_eq!(snaps(), 1);
        // the user tries again: only the steps left run, the new log of a done step stays
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut job = read_job(&d).unwrap();
        job.state = JobState::Requested;
        write_job(&d, &job).unwrap();
        let job = apply_pending_empty(&d);
        finish_pending_empty(&store, &d, job);
        assert!(read_job(&d).is_none());
        assert!(d.join("firmware-logs/new.log").exists() && !d.join("parameters").exists());
        assert_eq!(snaps(), 0);
    }

    #[test]
    fn an_unreadable_job_deletes_nothing() {
        let d = temp("garbled");
        put(&d, "parameters/p.xml", 3);
        std::fs::write(d.join(JOB), [0xff, 0xfe, 0x00]).unwrap();
        let (store, _) = seeded_store(&d);
        let job = apply_pending_empty(&d);
        assert_eq!(job.as_ref().map(|j| j.state), Some(JobState::Failed));
        finish_pending_empty(&store, &d, job);
        assert!(d.join("parameters/p.xml").exists());
        let raw = rusqlite::Connection::open(d.join("maintenance.sqlite")).unwrap();
        let n: i64 = raw.query_row("SELECT COUNT(*) FROM param_snapshots", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn without_a_job_nothing_happens() {
        let d = temp("none");
        put(&d, "parameters/p.xml", 3);
        assert!(apply_pending_empty(&d).is_none());
        assert!(d.join("parameters/p.xml").exists());
    }

    #[test]
    fn a_backup_folder_inside_the_data_folder_is_refused() {
        let d = temp("inside");
        std::fs::create_dir_all(d.join("exports/sub")).unwrap();
        assert_eq!(backup_folder(&d, d.join("exports/sub").to_str().unwrap()).unwrap_err(), "backup-inside-data");
        assert_eq!(backup_folder(&d, d.join("exports/sub/..").to_str().unwrap()).unwrap_err(), "backup-inside-data");
        let outside = temp("outside");
        assert!(backup_folder(&d, outside.to_str().unwrap()).is_ok());
    }

    #[test]
    fn a_backup_never_overwrites_a_file() {
        let d = temp("names");
        assert_eq!(bundle_name("Bank Süd/2", "2026-11-03"), "openbms-Bank-S-d-2-2026-11-03.zip");
        assert_eq!(bundle_name("///", "2026-11-03"), "openbms-anlage-2026-11-03.zip");
        let f = "openbms-A-2026-11-03.zip";
        assert_eq!(free_path(&d, f), d.join(f));
        std::fs::write(d.join(f), b"old").unwrap();
        assert_eq!(free_path(&d, f), d.join("openbms-A-2026-11-03 (2).zip"));
        std::fs::write(d.join("openbms-A-2026-11-03 (2).zip"), b"old").unwrap();
        assert_eq!(free_path(&d, f), d.join("openbms-A-2026-11-03 (3).zip"));
    }
}
