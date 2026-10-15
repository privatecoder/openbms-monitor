//! Versioned migrations of the SQLite files that hold the user's data. Each step runs once, in its own transaction,
//! after a copy of the file as it was (`<file>.bak-<step>`), so a failed or unwanted step can be undone by hand. Where
//! the count of steps done is kept is up to the caller (`user_version` or a meta row).

use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub type Step = fn(&Connection) -> rusqlite::Result<()>;

/// Run `steps[done..]` in order. `mark(db, n)` records, inside the step's transaction, that n steps are done.
/// `backup`: the file to copy before each step (None: a database created just now, nothing to keep). Returns the
/// number of steps run.
pub fn run(db: &Connection, backup: Option<&Path>, done: usize, steps: &[Step], mark: fn(&Connection, usize) -> rusqlite::Result<()>) -> Result<usize, String> {
    if done >= steps.len() {
        return Ok(0);
    }
    for (i, step) in steps.iter().enumerate().skip(done) {
        let n = i + 1;
        let err = |e: rusqlite::Error| format!("migration {n}: {e}");
        if let Some(path) = backup {
            copy(db, path, n)?;
        }
        let tx = db.unchecked_transaction().map_err(err)?;
        step(&tx).and_then(|_| mark(&tx, n)).map_err(err)?;
        tx.commit().map_err(err)?;
    }
    // the rewritten tables out of the log, so the file alone is the whole database again; the data is safe without it
    if let Err(e) = db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
        eprintln!("checkpoint after migration: {e}");
    }
    Ok(steps.len() - done)
}

pub fn backup_path(path: &Path, n: usize) -> PathBuf {
    let name = path.file_name().map_or("db".into(), |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!("{name}.bak-{n}"))
}

/// A consistent copy (including what is still in the write-ahead log), written under a temporary name so that only
/// a complete copy carries the backup name. A sound copy of an earlier attempt is kept: that step never committed, so
/// the file is still as it was then. A damaged one is moved aside (`.bad-<time>`) and copied anew.
fn copy(db: &Connection, path: &Path, n: usize) -> Result<(), String> {
    let to = backup_path(path, n);
    if to.exists() {
        if sound_copy(db, &to) {
            return Ok(());
        }
        let bad = PathBuf::from(format!("{}.bad-{}", to.display(), crate::history::now_ms()));
        std::fs::rename(&to, &bad).map_err(|e| format!("backup {}: {e}", to.display()))?;
    }
    enough_space(path)?;
    let tmp = PathBuf::from(format!("{}.tmp", to.display()));
    let _ = std::fs::remove_file(&tmp);
    let r = db
        .execute("VACUUM INTO ?1", [tmp.to_string_lossy()])
        .map_err(|e| format!("backup {}: {e}", to.display()))
        .and_then(|_| std::fs::rename(&tmp, &to).map_err(|e| format!("backup {}: {e}", to.display())));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// The copy passes SQLite's quick check and has the tables the file has (a step that never committed changed none).
fn sound_copy(db: &Connection, copy: &Path) -> bool {
    let tables = |c: &Connection| -> rusqlite::Result<Vec<String>> {
        c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?.query_map([], |r| r.get(0))?.collect()
    };
    let Ok(c) = Connection::open_with_flags(copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { return false };
    let ok = c.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)).is_ok_and(|r| r == "ok");
    ok && matches!((tables(&c), tables(db)), (Ok(a), Ok(b)) if a == b)
}

/// The copy and the rewritten tables need about twice the file (with its log) on the disk.
fn enough_space(path: &Path) -> Result<(), String> {
    let size: u64 = ["", "-wal"].iter().map(|ext| std::fs::metadata(format!("{}{ext}", path.display())).map_or(0, |m| m.len())).sum();
    match free_space(path.parent().unwrap_or(Path::new("."))) {
        Some(free) if free < 2 * size => Err(format!(
            "{}: updating the database needs about {} MB of free disk space, {} MB are free",
            path.display(),
            (2 * size).div_ceil(1_000_000),
            free / 1_000_000
        )),
        _ => Ok(()),
    }
}

/// Bytes available to this user on the disk of `dir`; None where it cannot be asked (the copy then fails with
/// the disk's own error and leaves no partial file).
#[cfg(unix)]
pub(crate) fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: a valid C string and a zeroed struct the call fills
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)] // the field types differ between platforms
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(not(unix))]
pub(crate) fn free_space(_dir: &Path) -> Option<u64> {
    None
}

/// The backup copies next to `path`, for tests.
#[cfg(test)]
pub fn backups(path: &Path) -> Vec<String> {
    let name = format!("{}.bak-", path.file_name().unwrap().to_string_lossy());
    let mut v: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|f| f.starts_with(&name))
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openbms-migrate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("data.sqlite")
    }

    fn mark(db: &Connection, n: usize) -> rusqlite::Result<()> {
        db.pragma_update(None, "user_version", n as i64)
    }

    fn count(db: &Connection) -> i64 {
        db.query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_failed_step_keeps_its_copy_and_the_next_attempt_reuses_it() {
        let path = temp("fail");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);").unwrap();
        let bad: &[Step] = &[|db| db.execute_batch("INSERT INTO t VALUES (2); SELECT nonsense FROM nowhere;")];
        assert!(run(&db, Some(&path), 0, bad, mark).is_err());
        assert_eq!(count(&db), 1, "rolled back");
        assert_eq!(backups(&path), vec!["data.sqlite.bak-1".to_string()], "a complete copy, no temporary file");
        // a second attempt neither overwrites nor multiplies the copy
        let good: &[Step] = &[|db| db.execute_batch("INSERT INTO t VALUES (4);")];
        assert_eq!(run(&db, Some(&path), 0, good, mark).unwrap(), 1);
        assert_eq!((count(&db), backups(&path).len()), (2, 1));
        assert_eq!(count(&Connection::open(backup_path(&path, 1)).unwrap()), 1);
        assert_eq!(run(&db, Some(&path), 1, good, mark).unwrap(), 0);
        assert!(free_space(path.parent().unwrap()).is_none_or(|f| f > 0));
    }

    #[test]
    fn a_damaged_copy_is_moved_aside_and_made_anew() {
        let path = temp("damaged");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);").unwrap();
        std::fs::write(backup_path(&path, 1), b"not a database").unwrap();
        let good: &[Step] = &[|db| db.execute_batch("INSERT INTO t VALUES (2);")];
        run(&db, Some(&path), 0, good, mark).unwrap();
        let names = backups(&path);
        assert_eq!(names.len(), 2);
        assert!(names[1].starts_with("data.sqlite.bak-1.bad-"), "{names:?}");
        assert_eq!(count(&Connection::open(backup_path(&path, 1)).unwrap()), 1);
        // a sound copy of another layout (tables differ) is not taken either
        let other = temp("other");
        let db = Connection::open(&other).unwrap();
        db.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);").unwrap();
        Connection::open(backup_path(&other, 1)).unwrap().execute_batch("CREATE TABLE u(y);").unwrap();
        run(&db, Some(&other), 0, good, mark).unwrap();
        assert_eq!(count(&Connection::open(backup_path(&other, 1)).unwrap()), 1);
    }
}
