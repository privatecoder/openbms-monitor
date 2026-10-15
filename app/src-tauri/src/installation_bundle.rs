//! Export and import of one installation as a ZIP (plan: docs/superpowers/plans/2026-10-15-installation-zip.md).
//! Export: `installation.json`, the logs, optionally the recording (values and events), the manifest last; written to
//! `<file>.part` and renamed when complete. Import: unpack and check (`inspect`), the user picks where it goes, then
//! `apply` in three resumable steps (store, recording, log files) bound to the export id.

use crate::bundle::{self, Manifest, Writer, EVENTS, INSTALLATION, MANIFEST, RECORDING};
use crate::history::{self, History};
use crate::maint_store::{ImportInto, ImportRow, InstallationData, LogPlan, Store};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Shared<T> = Arc<Mutex<T>>;

fn store_do<T>(store: &Shared<Option<Store>>, f: impl FnOnce(&Store) -> Result<T, String>) -> Result<T, String> {
    let g = store.lock().map_err(|e| e.to_string())?;
    f(g.as_ref().ok_or("maintenance database not open")?)
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// An export id nobody else produces: time, process and a counter, hashed.
fn new_export_id(now: i64) -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let h = bundle::sha256_hex(format!("{now}-{nanos}-{}-{n}", std::process::id()).as_bytes());
    format!("exp-{now:x}-{}", &h[..16])
}

#[derive(Serialize, Debug)]
pub struct Exported {
    pub path: String,
    pub export_id: String,
    pub logs: usize,
    pub logs_missing: usize,
    pub recording: bool,
    pub bytes: u64,
}

/// Write the bundle of `inst` to `out`. `history_db`: the recording file (None or `with_recording` false: none).
pub fn export(store: &Shared<Option<Store>>, history_db: Option<&Path>, logs_dir: &Path, inst: &str, out: &Path, with_recording: bool, now: i64) -> Result<Exported, String> {
    let mut data = store_do(store, |s| s.export_data(inst))?;
    for l in &mut data.logs {
        l.missing = !bundle::safe_name(&l.file) || !logs_dir.join(&l.file).is_file();
    }
    let part = with_suffix(out, ".part");
    let (rec, ev) = (with_suffix(out, ".rec.part"), with_suffix(out, ".ev.part"));
    for p in [&part, &rec, &ev] {
        bundle::no_symlink(p)?;
    }
    let result = (|| {
        let mut w = Writer::create(&part)?;
        w.add(INSTALLATION, &serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?[..])?;
        let mut logs = 0;
        for l in data.logs.iter().filter(|l| !l.missing) {
            w.add(&format!("logs/{}", l.file), std::fs::File::open(logs_dir.join(&l.file)).map_err(|e| e.to_string())?)?;
            logs += 1;
        }
        let mut recording = false;
        if let (true, Some(db)) = (with_recording, history_db) {
            let reader = history::reader(db)?;
            let packs = history::site_packs(&reader, inst)?;
            history::export(&reader, inst, i64::MIN, i64::MAX, &packs, history::Format::Jsonl, &rec)?;
            history::export_events(&reader, inst, &ev)?;
            w.add(RECORDING, std::fs::File::open(&rec).map_err(|e| e.to_string())?)?;
            w.add(EVENTS, std::fs::File::open(&ev).map_err(|e| e.to_string())?)?;
            recording = true;
        }
        let export_id = new_export_id(now);
        w.finish(&format!("openbms-monitor {}", env!("CARGO_PKG_VERSION")), &export_id, now, inst)?;
        Ok::<_, String>((export_id, logs, recording))
    })();
    let _ = std::fs::remove_file(&rec);
    let _ = std::fs::remove_file(&ev);
    match result {
        Ok((export_id, logs, recording)) => {
            std::fs::rename(&part, out).map_err(|e| e.to_string())?;
            let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
            Ok(Exported { path: out.display().to_string(), export_id, logs, logs_missing: data.logs.iter().filter(|l| l.missing).count(), recording, bytes })
        }
        Err(err) => {
            let _ = std::fs::remove_file(&part);
            Err(err)
        }
    }
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Named {
    pub id: String,
    pub name: String,
}

/// What the import dialog shows before anything is written.
#[derive(Serialize, Debug)]
pub struct Preview {
    pub export_id: String,
    pub installation: String,
    pub name: String,
    pub created: i64,
    pub master_serial: Option<String>,
    pub places: usize,
    pub boards: usize,
    pub snapshots: usize,
    pub logs: usize,
    pub recording_bytes: Option<u64>,
    /// the export's id exists here: its name, and whether the stored master serial differs
    pub same: Option<Named>,
    pub master_differs: bool,
    /// other installations here with the export's master serial (merge offered)
    pub master_matches: Vec<Named>,
    /// this export id was imported before (completed) or is unfinished (target)
    pub completed: Option<i64>,
    pub open_into: Option<Named>,
}

fn read_data(dir: &Path) -> Result<InstallationData, String> {
    serde_json::from_slice(&std::fs::read(dir.join(INSTALLATION)).map_err(|e| e.to_string())?).map_err(|e| format!("bundle-manifest: {e}"))
}

fn manifest_sha(dir: &Path) -> Result<String, String> {
    bundle::file_sha256(&dir.join(MANIFEST))
}

/// Unpack `zip` below `imports` (a folder per export id) and describe it. An unfinished import of the same export
/// keeps its folder (it is checked again); another file under a known export id → `import-conflict`.
pub fn inspect(store: &Shared<Option<Store>>, imports: &Path, zip: &Path) -> Result<Preview, String> {
    std::fs::create_dir_all(imports).map_err(|e| e.to_string())?;
    bundle::no_symlink(imports)?;
    let tmp = imports.join(format!("tmp-{}", new_export_id(history::now_ms())));
    let unpacked = bundle::unpack_checked(zip, &tmp, bundle::LIMITS);
    let m: Manifest = match unpacked {
        Ok(m) => m,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(err);
        }
    };
    let dir = bundle::unpack_dir(imports, &m.export_id)?;
    let sha = manifest_sha(&tmp)?;
    let row = store_do(store, |s| s.import_row(&m.export_id))?;
    if let Some(r) = &row {
        if r.state.manifest_sha256 != sha && !r.state.manifest_sha256.is_empty() {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err("import-conflict".into());
        }
    }
    if dir.exists() {
        bundle::no_symlink(&dir)?;
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &dir).map_err(|e| e.to_string())?;
    let data = match read_data(&dir) {
        Ok(d) => d,
        Err(err) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(err);
        }
    };
    store_do(store, |s| {
        let same = s.installation(&data.id).ok();
        let master_differs = matches!((&same.as_ref().and_then(|i| i.master_serial.clone()), &data.master_serial), (Some(a), Some(b)) if a != b);
        let master_matches = match &data.master_serial {
            Some(m) => s.installations()?.into_iter().filter(|i| i.id != data.id && i.master_serial.as_deref() == Some(m)).map(|i| Named { id: i.id, name: i.name }).collect(),
            None => vec![],
        };
        let open_into = row
            .as_ref()
            .filter(|r| r.completed.is_none() && r.state.maint)
            .map(|r| Named { id: r.installation.clone(), name: s.installation(&r.installation).map(|i| i.name).unwrap_or_default() });
        let places: std::collections::BTreeSet<u8> = data.boards.iter().map(|b| b.address).chain(data.settings.cell_types.keys().copied()).chain(data.snapshots.iter().map(|x| x.address)).collect();
        Ok(Preview {
            export_id: m.export_id.clone(),
            installation: data.id.clone(),
            name: data.name.clone(),
            created: m.created,
            master_serial: data.master_serial.clone(),
            places: places.len(),
            boards: data.boards.len(),
            snapshots: data.snapshots.len(),
            logs: data.logs.iter().filter(|l| !l.missing).count(),
            recording_bytes: m.files.get(RECORDING).map(|f| f.size + m.files.get(EVENTS).map_or(0, |e| e.size)),
            same: same.map(|i| Named { id: i.id, name: i.name }),
            master_differs,
            master_matches,
            completed: row.as_ref().and_then(|r| r.completed),
            open_into,
        })
    })
}

#[derive(Serialize, Debug)]
pub struct Applied {
    pub target: String,
    pub report: serde_json::Value,
    pub recording_rows: u64,
    pub recording_events: u64,
    pub logs_copied: usize,
    /// imported before: nothing done now
    pub already: bool,
}

/// Write an inspected import (folder `imports/<export_id>`): store, recording, log files, each step once; then the
/// import counts as completed and its folder goes. A resume uses the stored target, `into` is ignored then.
pub fn apply(store: &Shared<Option<Store>>, history: &Shared<History>, imports: &Path, logs_dir: &Path, export_id: &str, into: &ImportInto) -> Result<Applied, String> {
    if let Some(r) = store_do(store, |s| s.import_row(export_id))? {
        if r.completed.is_some() {
            if let Ok(dir) = bundle::unpack_dir(imports, export_id) {
                let _ = std::fs::remove_dir_all(dir);
            }
            return Ok(Applied { target: r.installation, report: r.state.report, recording_rows: 0, recording_events: 0, logs_copied: 0, already: true });
        }
    }
    if !history.lock().map_err(|e| e.to_string())?.ready() {
        return Err("history-preparing".into());
    }
    let dir = bundle::unpack_dir(imports, export_id)?;
    let m = bundle::recheck(&dir)?;
    if m.export_id != export_id {
        return Err("import-conflict".into());
    }
    let sha = manifest_sha(&dir)?;
    let data = read_data(&dir)?;
    let bundle_hash: BTreeMap<String, String> = m.files.iter().filter_map(|(n, f)| Some((n.strip_prefix("logs/")?.to_string(), f.sha256.clone()))).collect();
    let disk = |n: &str| -> Option<String> {
        let p = logs_dir.join(n);
        if bundle::safe_name(n) && p.is_file() { bundle::file_sha256(&p).ok() } else { None }
    };
    let now = history::now_ms();
    let mut state = store_do(store, |s| s.import_maint(&data, export_id, &sha, into, &LogPlan { bundle_hash: &bundle_hash, disk: &disk }, now))?;
    let (mut rows, mut events) = (0, 0);
    if !state.recording {
        let values = m.files.contains_key(RECORDING).then(|| dir.join(RECORDING));
        let ev = m.files.contains_key(EVENTS).then(|| dir.join(EVENTS));
        if values.is_some() || ev.is_some() {
            let st = history.lock().map_err(|e| e.to_string())?.import_bundle(values.as_deref(), ev.as_deref(), &state.target, export_id)?;
            (rows, events) = (st.rows, st.events);
        }
        state.recording = true;
        store_do(store, |s| s.set_import_state(export_id, &state))?;
    }
    let mut copied = 0;
    if !state.logs {
        std::fs::create_dir_all(logs_dir).map_err(|e| e.to_string())?;
        for (src, dst) in &state.log_names {
            if !bundle::safe_name(src) || !bundle::safe_name(dst) {
                return Err(format!("bundle-entry: logs/{src}"));
            }
            let target = logs_dir.join(dst);
            // copied before an interruption: the same content is there already
            if disk(dst).as_deref() == bundle_hash.get(src).map(String::as_str) {
                continue;
            }
            let part = with_suffix(&target, ".part");
            bundle::no_symlink(&part)?;
            bundle::no_symlink(&target)?;
            std::fs::copy(dir.join("logs").join(src), &part).map_err(|e| e.to_string())?;
            std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
            copied += 1;
        }
        state.logs = true;
        store_do(store, |s| s.set_import_state(export_id, &state))?;
    }
    store_do(store, |s| s.complete_import(export_id, history::now_ms()))?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(Applied { target: state.target, report: state.report, recording_rows: rows, recording_events: events, logs_copied: copied, already: false })
}

/// Unfinished imports with their target's name (the installations view offers „Fortsetzen“).
pub fn open_imports(store: &Shared<Option<Store>>) -> Result<Vec<(ImportRow, String)>, String> {
    store_do(store, |s| {
        s.open_imports()?.into_iter().map(|r| Ok((r.clone(), s.installation(&r.installation).map(|i| i.name).unwrap_or_default()))).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbms_proto::SerialValue;

    struct Env {
        dir: PathBuf,
        store: Shared<Option<Store>>,
        history: Shared<History>,
        logs: PathBuf,
        imports: PathBuf,
    }

    fn env(tag: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("openbms-ib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        let store = Arc::new(Mutex::new(Some(Store::open(&dir.join("maintenance.sqlite")).unwrap())));
        let mut h = History::default();
        h.open(&dir.join("history.sqlite")).unwrap();
        Env { logs: dir.join("logs"), imports: dir.join("imports"), store, history: Arc::new(Mutex::new(h)), dir }
    }

    fn seeded(e: &Env) -> String {
        let id = store_do(&e.store, |s| {
            let i = s.create_installation("Bank", None, vec!["serial:/dev/x".into()])?;
            s.board_seen(&i.id, 0, &SerialValue::Set { value: "SYN-1".into(), truncated: false }, &SerialValue::NoAnswer, 10)?;
            s.set_master_serial(&i.id, "SYN-1", 10)?;
            s.add_log(Some(&i.id), Some(0), "flash", None, "f_pack0.log", 20)?;
            s.add_log(Some(&i.id), Some(0), "flash", None, "gone_pack0.log", 21)?;
            Ok(i.id)
        })
        .unwrap();
        std::fs::write(e.logs.join("f_pack0.log"), b"flash log").unwrap();
        id
    }

    #[test]
    fn export_inspect_apply_and_again() {
        let a = env("a");
        let id = seeded(&a);
        let zip = a.dir.join("out.zip");
        let ex = export(&a.store, None, &a.logs, &id, &zip, false, 1000).unwrap();
        assert_eq!((ex.logs, ex.logs_missing, ex.recording), (1, 1, false));
        assert!(!with_suffix(&zip, ".part").exists());

        let b = env("b");
        let p = inspect(&b.store, &b.imports, &zip).unwrap();
        assert_eq!((p.name.as_str(), p.boards, p.logs, p.same.is_none()), ("Bank", 1, 1, true));
        let r = apply(&b.store, &b.history, &b.imports, &b.logs, &p.export_id, &ImportInto::New { copy: false }).unwrap();
        assert_eq!((r.target.as_str(), r.logs_copied, r.already), (id.as_str(), 1, false));
        assert_eq!(std::fs::read(b.logs.join("f_pack0.log")).unwrap(), b"flash log");
        assert!(!b.imports.join(&p.export_id).exists());
        // imported endpoints are information only
        assert!(store_do(&b.store, |s| s.installation(&id)).unwrap().endpoints.is_empty());
        // the same file again: said, nothing done
        let p2 = inspect(&b.store, &b.imports, &zip).unwrap();
        assert!(p2.completed.is_some());
        assert!(apply(&b.store, &b.history, &b.imports, &b.logs, &p2.export_id, &ImportInto::Same).unwrap().already);
    }

    #[test]
    fn a_resumed_import_keeps_its_target_and_copies_once() {
        let a = env("ra");
        let id = seeded(&a);
        let zip = a.dir.join("out.zip");
        export(&a.store, None, &a.logs, &id, &zip, false, 1000).unwrap();
        let b = env("rb");
        let p = inspect(&b.store, &b.imports, &zip).unwrap();
        // the store step done, then the app stopped
        let dir = b.imports.join(&p.export_id);
        let data = read_data(&dir).unwrap();
        let m = bundle::recheck(&dir).unwrap();
        let bh: BTreeMap<String, String> = m.files.iter().filter_map(|(n, f)| Some((n.strip_prefix("logs/")?.to_string(), f.sha256.clone()))).collect();
        store_do(&b.store, |s| s.import_maint(&data, &p.export_id, &manifest_sha(&dir).unwrap(), &ImportInto::New { copy: false }, &LogPlan { bundle_hash: &bh, disk: &|_| None }, 5))
            .unwrap();
        assert_eq!(open_imports(&b.store).unwrap().len(), 1);
        // resumed with another choice: the stored target wins
        let r = apply(&b.store, &b.history, &b.imports, &b.logs, &p.export_id, &ImportInto::New { copy: true }).unwrap();
        assert_eq!(r.target, id);
        assert!(open_imports(&b.store).unwrap().is_empty());
        assert_eq!(store_do(&b.store, |s| s.installations()).unwrap().len(), 1);
    }

    #[test]
    fn a_log_name_taken_here_gets_another_name() {
        let a = env("la");
        let id = seeded(&a);
        let zip = a.dir.join("out.zip");
        export(&a.store, None, &a.logs, &id, &zip, false, 1000).unwrap();
        let b = env("lb");
        std::fs::write(b.logs.join("f_pack0.log"), b"another bank's log").unwrap();
        let p = inspect(&b.store, &b.imports, &zip).unwrap();
        apply(&b.store, &b.history, &b.imports, &b.logs, &p.export_id, &ImportInto::New { copy: false }).unwrap();
        assert_eq!(std::fs::read(b.logs.join("f_pack0.log")).unwrap(), b"another bank's log");
        let renamed: Vec<String> = std::fs::read_dir(&b.logs).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| n.contains("-imp-")).collect();
        assert_eq!(renamed.len(), 1);
        assert_eq!(std::fs::read(b.logs.join(&renamed[0])).unwrap(), b"flash log");
    }

    #[test]
    fn the_recording_goes_along_when_asked() {
        let a = env("hra");
        let id = seeded(&a);
        let hp = a.dir.join("history.sqlite");
        {
            let db = rusqlite::Connection::open(&hp).unwrap();
            db.execute("INSERT INTO sessions(site, bus, source) VALUES(?1, 'can', 'live')", [&id]).unwrap();
            let s = db.last_insert_rowid();
            db.execute("INSERT INTO events(session, pack, key, severity, start, end) VALUES(?1, 0, 'alarm.x', 'alarm', 5, NULL)", [s]).unwrap();
        }
        let zip = a.dir.join("out.zip");
        assert!(export(&a.store, Some(&hp), &a.logs, &id, &zip, true, 1000).unwrap().recording);
        let b = env("hrb");
        let p = inspect(&b.store, &b.imports, &zip).unwrap();
        assert!(p.recording_bytes.is_some());
        let r = apply(&b.store, &b.history, &b.imports, &b.logs, &p.export_id, &ImportInto::New { copy: false }).unwrap();
        assert_eq!(r.recording_events, 1);
    }
}
