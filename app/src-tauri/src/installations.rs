//! Installations own the data about a bank (design: docs/superpowers/specs/2026-10-14-installations-own-the-data.md):
//! their settings (cell type per place, groups, pairs, diagnosis), the one-time move of the old data (site keys of the
//! recording and of the browser storage) into installations of their own, merge, archive, delete, and what the
//! installations view and the offline Parameters view show.

use crate::history::{self, SiteMerge};
use crate::maint::{self, with_store, MaintError};
use crate::maint_store::{looks_like_id, Installation, Overview, Settings, Store};
use crate::AppState;
use openbms_proto::Parameters;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use tauri::{AppHandle, State};

fn text(e: impl std::fmt::Display) -> MaintError {
    MaintError::Store { message: e.to_string() }
}

/// The old settings of one site key as the UI found them in its storage (`cellTypes:`, `groups:`, `pairs:`, `diag:`
/// + site), raw text each.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalSettings {
    pub cell_types: Option<String>,
    pub groups: Option<String>,
    pub pairs: Option<String>,
    pub diag: Option<String>,
}

/// The old texts as settings; a text that is no JSON is not taken but kept in the log of the move (second value).
pub fn local_settings(l: &LocalSettings) -> (Settings, Vec<String>) {
    let mut s = Settings::default();
    let mut unreadable = vec![];
    let json = |t: &Option<String>, unreadable: &mut Vec<String>| -> Option<Value> {
        let t = t.as_deref()?;
        serde_json::from_str::<Value>(t).map_err(|_| unreadable.push(t.to_string())).ok()
    };
    if let Some(Value::Object(m)) = json(&l.cell_types, &mut unreadable) {
        for (k, v) in m {
            match k.parse::<u8>() {
                Ok(a) if v.is_object() => {
                    s.cell_types.insert(a, v.to_string());
                }
                _ => unreadable.push(format!("{k}: {v}")),
            }
        }
    }
    // an empty list or object is no setting: the target keeps what it has
    let some = |v: Option<Value>| v.filter(|v| !(v.as_array().is_some_and(Vec::is_empty) || v.as_object().is_some_and(|o| o.is_empty()))).map(|v| v.to_string());
    s.groups = some(json(&l.groups, &mut unreadable));
    s.pairs = some(json(&l.pairs, &mut unreadable));
    s.diagnosis = some(json(&l.diag, &mut unreadable));
    (s, unreadable)
}

/// Move one old site key: its installation („übernommen“, created once), the recording under that id, the parameter
/// blocks of earlier versions into the store, the old settings added (never overwriting), and one log row. Every step
/// is safe to repeat; true when it is done (the UI then removes the old keys).
fn move_source(store: &Store, h: &std::sync::Mutex<history::History>, source: &str, in_history: bool, local: &LocalSettings) -> Result<bool, String> {
    let now = history::now_ms();
    let Some(inst) = store.legacy_installation(source, now)? else { return Ok(true) };
    let mut merged = SiteMerge::default();
    let (mut snapshots, mut skipped) = (0, 0);
    if in_history {
        let mut h = h.lock().map_err(|e| e.to_string())?;
        h.merge_site(source, &inst)?;
        // what moved, also when an earlier run moved it and stopped before its log row
        merged = h.merged(source, &inst)?;
        let (rows, bad) = h.old_snapshots(&inst)?;
        snapshots = store.import_snapshots(&inst, &rows)?;
        skipped = bad;
        h.drop_old_snapshots(&inst)?;
    }
    let (settings, unreadable) = local_settings(local);
    let history = serde_json::json!({ "merge": merged, "unreadable": unreadable });
    store.finish_legacy(source, &inst, &settings, history, serde_json::json!({ "moved": snapshots, "unreadable_skipped": skipped }), now)?;
    Ok(true)
}

/// The one-time move of the old data (idempotent, run at every start until nothing is left): every site key of the
/// recording and of the UI's old storage becomes its own installation, named after the endpoint. Returns the keys
/// done; the UI removes their old storage only then.
#[tauri::command]
pub async fn installations_migrate(state: State<'_, AppState>, local: BTreeMap<String, LocalSettings>) -> Result<Vec<String>, MaintError> {
    let (keys, open) = {
        let h = state.history.lock().map_err(text)?;
        if h.preparing() {
            return Err(maint::refused("history-preparing"));
        }
        let open = h.path().is_some();
        (if open { h.site_keys().map_err(text)? } else { vec![] }, open)
    };
    let ids: Vec<String> = with_store(&state, |s| Ok(s.installations()?.into_iter().map(|i| i.id).collect()))?;
    // an installation id (also of one deleted since) is never an old endpoint text
    let local: BTreeMap<String, LocalSettings> = local.into_iter().filter(|(k, _)| !looks_like_id(k)).collect();
    let mut done = vec![];
    let mut sources: Vec<&String> = keys.iter().filter(|k| !ids.contains(k) && !looks_like_id(k)).chain(local.keys()).collect();
    sources.sort();
    sources.dedup();
    for source in sources {
        let in_history = keys.contains(source);
        let empty = LocalSettings::default();
        let l = local.get(source).unwrap_or(&empty);
        match with_store(&state, |s| move_source(s, &state.history, source, in_history && open, l)) {
            Ok(true) => done.push(source.clone()),
            Ok(false) => {}
            Err(e) => eprintln!("moving {source}: {e:?}"),
        }
    }
    // blocks of an installation whose move stopped between the recording and the store
    for id in keys.iter().filter(|k| ids.contains(k)) {
        let r = with_store(&state, |s| {
            let h = state.history.lock().map_err(|e| e.to_string())?;
            let (rows, _) = h.old_snapshots(id)?;
            if !rows.is_empty() {
                s.import_snapshots(id, &rows)?;
                h.drop_old_snapshots(id)?;
            }
            Ok(())
        });
        if let Err(e) = r {
            eprintln!("parameter blocks of {id}: {e:?}");
        }
    }
    Ok(done)
}

/// An installation's settings, JSON each (null: none).
#[derive(Serialize, Default)]
pub struct SettingsView {
    cell_types: BTreeMap<u8, Value>,
    groups: Value,
    pairs: Value,
    diagnosis: Value,
}

fn parse(t: Option<String>) -> Value {
    t.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

#[tauri::command]
pub async fn installation_settings(state: State<'_, AppState>, installation: String) -> Result<SettingsView, MaintError> {
    let s = with_store(&state, |s| {
        s.installation(&installation)?;
        s.settings(&installation)
    })
    .map_err(maint::store_refusal)?;
    Ok(SettingsView {
        cell_types: s.cell_types.into_iter().filter_map(|(a, c)| serde_json::from_str(&c).ok().map(|v| (a, v))).collect(),
        groups: parse(s.groups),
        pairs: parse(s.pairs),
        diagnosis: parse(s.diagnosis),
    })
}

/// The cell types of the places the UI changed (address → type, null removes it); the others stay.
#[tauri::command]
pub async fn set_installation_cell_types(state: State<'_, AppState>, installation: String, changes: BTreeMap<u8, Value>) -> Result<(), MaintError> {
    let changes: BTreeMap<u8, Option<String>> = changes.into_iter().map(|(a, v)| (a, (!v.is_null()).then(|| v.to_string()))).collect();
    with_store(&state, |s| {
        s.installation(&installation)?;
        s.change_cell_types(&installation, &changes)
    })
    .map_err(maint::store_refusal)
}

/// Store one setting the UI edits as a whole: "groups", "pairs" or "diagnosis"; null removes it.
#[tauri::command]
pub async fn set_installation_setting(state: State<'_, AppState>, installation: String, key: String, value: Value) -> Result<(), MaintError> {
    let text = (!value.is_null()).then(|| value.to_string());
    with_store(&state, |s| {
        s.installation(&installation)?;
        match key.as_str() {
            "groups" => s.set_groups(&installation, text.as_deref()),
            "pairs" => s.set_pairs(&installation, text.as_deref()),
            "diagnosis" => match &text {
                Some(t) => s.set_diagnosis(&installation, t),
                None => s.clear_diagnosis(&installation),
            },
            other => Err(format!("unknown setting {other}")),
        }
    })
    .map_err(maint::store_refusal)
}

/// The installations view of one installation: places, what it owns here and in the recording, the folder of its logs.
#[derive(Serialize)]
pub struct InstallationView {
    #[serde(flatten)]
    overview: Overview,
    /// rows of the recording (packs and master)
    recorded: i64,
    recorded_from: Option<i64>,
    recorded_to: Option<i64>,
    log_dir: Option<String>,
}

#[tauri::command]
pub async fn installation_overview(app: AppHandle, state: State<'_, AppState>, installation: String) -> Result<InstallationView, MaintError> {
    let overview = with_store(&state, |s| s.overview(&installation)).map_err(maint::store_refusal)?;
    let path = state.history.lock().map_err(text)?.path();
    let (recorded, recorded_from, recorded_to) = match path {
        Some(p) => history::site_summary(&history::reader(&p).map_err(text)?, &installation).map_err(text)?,
        None => (0, None, None),
    };
    let log_dir = crate::firmware::logs_dir(&app).ok().map(|d| d.display().to_string());
    Ok(InstallationView { overview, recorded, recorded_from, recorded_to, log_dir })
}

/// Merge `source` into `target` (recording first, then the store; both safe to repeat after an interruption). Not
/// while connected with `source`.
#[tauri::command]
pub async fn installation_merge(state: State<'_, AppState>, source: String, target: String) -> Result<Value, MaintError> {
    if source == target {
        return Err(maint::refused("merge-self"));
    }
    // answers of the running connection would still arrive for the source
    if crate::link_now(&state).installation.as_deref() == Some(source.as_str()) {
        return Err(maint::refused("installation-connected"));
    }
    with_store(&state, |s| s.installation(&source).and(s.installation(&target))).map_err(maint::store_refusal)?;
    // an unfinished import resumes into its target: checked before the recording moves, so it is never split
    if with_store(&state, |s| Ok(s.import_open_for(&source)? || s.import_open_for(&target)?))? {
        return Err(maint::refused("installation-busy"));
    }
    let merged = {
        let mut h = state.history.lock().map_err(text)?;
        h.merge_site(&source, &target).map_err(text)?;
        // also what an interrupted earlier attempt moved
        h.merged(&source, &target).map_err(text)?
    };
    with_store(&state, |s| s.merge(&source, &target, serde_json::to_value(&merged).unwrap_or_default(), history::now_ms())).map_err(maint::store_refusal)
}

#[tauri::command]
pub async fn installation_archive(state: State<'_, AppState>, installation: String, archived: bool) -> Result<Installation, MaintError> {
    with_store(&state, |s| {
        s.set_archived(&installation, archived)?;
        s.installation(&installation)
    })
    .map_err(maint::store_refusal)
}

#[tauri::command]
pub async fn installation_rename(state: State<'_, AppState>, installation: String, name: String) -> Result<Installation, MaintError> {
    with_store(&state, |s| s.rename(&installation, &name)).map_err(maint::store_refusal)
}

/// Delete an installation and its recording; refused while it has open work or is connected. Log files stay („ohne
/// Anlage“).
#[tauri::command]
pub async fn installation_delete(state: State<'_, AppState>, installation: String) -> Result<(), MaintError> {
    if crate::link_now(&state).installation.as_deref() == Some(installation.as_str()) {
        return Err(maint::refused("installation-connected"));
    }
    with_store(&state, |s| s.may_delete(&installation)).map_err(maint::store_refusal)?;
    // the recording first: an installation without its store rows but with its recording would come back at the next
    // start as moved-in old data
    {
        let mut h = state.history.lock().map_err(text)?;
        if !h.ready() {
            return Err(maint::refused("history-preparing"));
        }
        h.clear(Some(&installation)).map_err(text)?;
    }
    with_store(&state, |s| s.delete_installation(&installation)).map_err(maint::store_refusal)
}

/// The newest stored parameter block of every place (offline Parameters view).
#[derive(Serialize)]
pub struct SnapshotView {
    address: u8,
    at: i64,
    kind: String,
    board: &'static str,
    bms_serial: Option<String>,
    parameters: Option<Parameters>,
}

#[tauri::command]
pub async fn installation_snapshots(state: State<'_, AppState>, installation: String) -> Result<Vec<SnapshotView>, MaintError> {
    let rows = with_store(&state, |s| s.latest_snapshots(&installation))?;
    Ok(rows
        .into_iter()
        .map(|x| SnapshotView { address: x.address, at: x.at, kind: x.kind, board: x.board, bms_serial: x.bms_serial, parameters: Parameters::parse(&x.block).ok() })
        .collect())
}

/// A failure of the bundle code as a refusal key for the UI (`bundle-checksum: logs/x` → `bundle-checksum`).
fn bundle_error(err: String) -> MaintError {
    const KEYS: [&str; 12] = [
        "bundle-unreadable", "bundle-entry", "bundle-too-large", "bundle-no-manifest", "bundle-manifest", "bundle-format", "bundle-checksum",
        "bundle-missing", "bundle-symlink", "import-conflict", "import-master-differs", "import-id-exists",
    ];
    if err == "disk-full" {
        return maint::refused("disk-full");
    }
    let key = err.split(':').next().unwrap_or_default();
    if KEYS.contains(&key) || ["unknown-installation", "installation-busy", "history-preparing"].contains(&key) {
        maint::refused(key)
    } else {
        text(err)
    }
}

fn imports_dir(app: &AppHandle) -> Result<std::path::PathBuf, MaintError> {
    use tauri::Manager;
    Ok(app.path().app_data_dir().map_err(text)?.join("imports"))
}

/// Write an installation's bundle (ZIP) to the file the user picked; the recording only when asked.
#[tauri::command]
pub async fn installation_export(app: AppHandle, state: State<'_, AppState>, installation: String, path: String, recording: bool) -> Result<crate::installation_bundle::Exported, MaintError> {
    let store = state.maint.clone();
    let db = {
        let h = state.history.lock().map_err(text)?;
        // asked for the recording but it cannot be read now: said, not silently left out
        if recording && !h.ready() {
            return Err(maint::refused("history-preparing"));
        }
        h.path()
    };
    if recording && db.is_none() {
        return Err(maint::refused("history-preparing"));
    }
    let logs = crate::firmware::logs_dir(&app).map_err(text)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::installation_bundle::export(&store, db.as_deref(), &logs, &installation, std::path::Path::new(&path), recording, history::now_ms())
    })
    .await
    .map_err(text)?
    .map_err(bundle_error)
}

/// Unpack and check a bundle; what it holds and where it may go. Nothing is written to the databases.
#[tauri::command]
pub async fn installation_import_inspect(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<crate::installation_bundle::Preview, MaintError> {
    let store = state.maint.clone();
    let imports = imports_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || crate::installation_bundle::inspect(&store, &imports, std::path::Path::new(&path)))
        .await
        .map_err(text)?
        .map_err(bundle_error)
}

/// Import an inspected bundle (or resume an unfinished import of it) into the place the user chose.
#[tauri::command]
pub async fn installation_import_apply(app: AppHandle, state: State<'_, AppState>, export_id: String, into: crate::maint_store::ImportInto) -> Result<crate::installation_bundle::Applied, MaintError> {
    let (store, hist) = (state.maint.clone(), state.history.clone());
    let imports = imports_dir(&app)?;
    let logs = crate::firmware::logs_dir(&app).map_err(text)?;
    tauri::async_runtime::spawn_blocking(move || crate::installation_bundle::apply(&store, &hist, &imports, &logs, &export_id, &into))
        .await
        .map_err(text)?
        .map_err(bundle_error)
}

/// The import dialog was closed: its unpacked copy goes.
#[tauri::command]
pub async fn installation_import_discard(app: AppHandle, state: State<'_, AppState>, export_id: String) -> Result<(), MaintError> {
    crate::installation_bundle::discard(&state.maint, &imports_dir(&app)?, &export_id).map_err(bundle_error)
}

/// Give up an unfinished import that cannot complete; its target is free again.
#[tauri::command]
pub async fn installation_import_abandon(app: AppHandle, state: State<'_, AppState>, export_id: String) -> Result<(), MaintError> {
    crate::installation_bundle::abandon(&state.maint, &imports_dir(&app)?, &export_id).map_err(bundle_error)
}

#[derive(Serialize)]
pub struct OpenImport {
    export_id: String,
    installation: String,
    name: String,
    started: i64,
}

/// Unfinished imports (resumed by importing the same file again).
#[tauri::command]
pub async fn installation_imports_open(state: State<'_, AppState>) -> Result<Vec<OpenImport>, MaintError> {
    let rows = crate::installation_bundle::open_imports(&state.maint).map_err(bundle_error)?;
    Ok(rows.into_iter().map(|(r, name)| OpenImport { export_id: r.export_id, installation: r.installation, name, started: r.started }).collect())
}

/// Firmware logs that belong to no installation (from before installations owned their logs), newest first.
#[tauri::command]
pub async fn logs_unassigned(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<String>, MaintError> {
    let dir = crate::firmware::logs_dir(&app).map_err(text)?;
    let assigned = with_store(&state, |s| s.assigned_logs())?;
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .map(|r| r.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    files.retain(|f| f.ends_with(".log") && f != "maint-events.log" && !assigned.contains(f));
    files.sort_by(|a, b| b.cmp(a));
    Ok(files.into_iter().map(|f| dir.join(f).display().to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_are_taken_as_json_or_kept_in_the_log() {
        let l = LocalSettings {
            cell_types: Some(r#"{"3":{"cell":"syn-280","parallel":1},"x":{"cell":"y"}}"#.into()),
            groups: Some("[]".into()),
            pairs: Some(r#"[{"plus":0,"minus":1}]"#.into()),
            diag: Some("not json".into()),
        };
        let (s, unreadable) = local_settings(&l);
        assert_eq!(s.cell_types.get(&3).map(String::as_str), Some(r#"{"cell":"syn-280","parallel":1}"#));
        assert_eq!(s.cell_types.len(), 1);
        assert_eq!(s.groups, None, "an empty list is no setting");
        assert_eq!(s.pairs.as_deref(), Some(r#"[{"minus":1,"plus":0}]"#));
        assert_eq!(s.diagnosis, None);
        assert_eq!(unreadable, vec![r#"x: {"cell":"y"}"#.to_string(), "not json".to_string()]);
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("openbms-inst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_old_site_moves_once_into_its_own_installation() {
        let dir = temp("legacy");
        let h = std::sync::Mutex::new(history::History::default());
        h.lock().unwrap().open(&dir.join("history.sqlite")).unwrap();
        {
            let mut g = h.lock().unwrap();
            g.set_settings(history::Settings { enabled: true, ..Default::default() }).unwrap();
            g.connected("gw:4196", "can");
            g.pack(1000, 2, &serde_json::json!({ "telemetry": { "cell_voltages": [3.3], "current": 1.0 }, "status": null }));
            g.disconnected();
        }
        rusqlite::Connection::open(dir.join("history.sqlite"))
            .unwrap()
            .execute("INSERT INTO param_snapshots(site, pack, t, kind, block) VALUES('gw:4196', 2, 500, 'read', '0102')", [])
            .unwrap();
        let store = Store::open(&dir.join("maintenance.sqlite")).unwrap();
        // a user's installation of that endpoint: the old data does not go there by itself
        let mine = store.create_installation("gw:4196", None, vec!["tcp:gw:4196".into()]).unwrap();
        let local = LocalSettings { groups: Some(r#"[{"name":"T1","packs":[2]}]"#.into()), ..Default::default() };
        assert!(move_source(&store, &h, "gw:4196", true, &local).unwrap());
        let all = store.installations().unwrap();
        let legacy = all.iter().find(|i| i.legacy.is_some()).unwrap();
        assert_eq!((legacy.name.as_str(), legacy.legacy.as_deref(), legacy.endpoints.len()), ("gw:4196 (2)", Some("gw:4196"), 0));
        assert_eq!(store.settings(&legacy.id).unwrap().groups.as_deref(), Some(r#"[{"name":"T1","packs":[2]}]"#));
        assert_eq!(store.latest_snapshots(&legacy.id).unwrap().iter().map(|x| (x.address, x.board)).collect::<Vec<_>>(), vec![(2, "unclear")]);
        assert_eq!(h.lock().unwrap().site_keys().unwrap(), vec![legacy.id.clone()]);
        // repeated: nothing doubles, the same installation
        assert!(move_source(&store, &h, "gw:4196", true, &local).unwrap());
        assert_eq!(store.installations().unwrap().len(), 2);
        assert_eq!(store.overview(&legacy.id).unwrap().counts.snapshots, 1);
        // merged into the user's installation: everything there, the old one gone, a repeat does nothing
        let merged = h.lock().unwrap().merge_site(&legacy.id, &mine.id).unwrap();
        assert!(looks_like_id(&legacy.id) && !looks_like_id("gw:4196") && !looks_like_id("inst-x"));
        assert_eq!(merged.pack, 1);
        store.merge(&legacy.id, &mine.id, serde_json::to_value(&merged).unwrap(), 2000).unwrap();
        assert_eq!(store.installations().unwrap().len(), 1);
        assert_eq!(store.overview(&mine.id).unwrap().counts.snapshots, 1);
        assert!(move_source(&store, &h, "gw:4196", false, &local).unwrap());
        assert_eq!(store.installations().unwrap().len(), 1);
    }
}
