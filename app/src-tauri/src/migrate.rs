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
    for (i, step) in steps.iter().enumerate().skip(done) {
        let n = i + 1;
        let err = |e: rusqlite::Error| format!("migration {n}: {e}");
        if let Some(path) = backup {
            let to = backup_path(path, n);
            // a consistent copy, including what is still in the write-ahead log
            db.execute("VACUUM INTO ?1", [to.to_string_lossy()]).map_err(|e| format!("backup {}: {e}", to.display()))?;
        }
        let tx = db.unchecked_transaction().map_err(err)?;
        step(&tx).and_then(|_| mark(&tx, n)).map_err(err)?;
        tx.commit().map_err(err)?;
    }
    Ok(steps.len().saturating_sub(done))
}

/// `<file>.bak-<n>`; a copy left by an earlier, failed attempt of the same step is kept and the new one gets the time.
pub fn backup_path(path: &Path, n: usize) -> PathBuf {
    let name = path.file_name().map_or("db".into(), |n| n.to_string_lossy().into_owned());
    let p = path.with_file_name(format!("{name}.bak-{n}"));
    if p.exists() { path.with_file_name(format!("{name}.bak-{n}-{}", crate::history::now_ms())) } else { p }
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
