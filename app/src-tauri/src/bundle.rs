//! The ZIP of one installation: writing it, and unpacking a received one with checks before anything is used.
//! Only known names are accepted (no paths, no links), sizes are counted while extracting, and every file must be
//! in the manifest with its size and SHA-256.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const FORMAT: u32 = 1;
pub const MANIFEST: &str = "manifest.json";
pub const INSTALLATION: &str = "installation.json";
pub const RECORDING: &str = "recording.jsonl";
pub const EVENTS: &str = "events.jsonl";

/// Size limits of an unpacked bundle, counted while extracting.
#[derive(Clone, Copy)]
pub struct Limits {
    pub per_file: u64,
    pub total: u64,
    /// `manifest.json` and `installation.json` are parsed in memory
    pub metadata: u64,
    pub entries: usize,
}

pub const LIMITS: Limits = Limits { per_file: 2 << 30, total: 4 << 30, metadata: 64 << 20, entries: 10_000 };

/// Refuse a path that is a symlink (a reused unpack folder, a `.part` target): a link there could lead the
/// allowed relative names out of the folder.
pub fn no_symlink(p: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_symlink() => Err(format!("bundle-symlink: {}", p.display())),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub format: u32,
    pub app: String,
    pub export_id: String,
    pub created: i64,
    pub installation: String,
    pub files: BTreeMap<String, FileEntry>,
}

/// A file name of the bundle's `logs/` folder or an export id: letters, digits, `.`, `_`, `-`, not starting with `.`.
pub fn safe_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 200 && !n.starts_with('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// An entry name the bundle may contain.
pub fn allowed_entry(name: &str) -> bool {
    match name {
        MANIFEST | INSTALLATION | RECORDING | EVENTS => true,
        _ => name.strip_prefix("logs/").is_some_and(safe_name),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Writes a bundle: files are added one by one (streamed), the manifest last.
pub struct Writer {
    zip: zip::ZipWriter<File>,
    files: BTreeMap<String, FileEntry>,
}

impl Writer {
    pub fn create(path: &Path) -> Result<Self, String> {
        let f = File::create(path).map_err(|e| e.to_string())?;
        Ok(Self { zip: zip::ZipWriter::new(f), files: BTreeMap::new() })
    }

    fn options() -> zip::write::SimpleFileOptions {
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true)
    }

    /// Add one entry from a reader.
    pub fn add(&mut self, name: &str, mut src: impl Read) -> Result<(), String> {
        if !allowed_entry(name) || name == MANIFEST {
            return Err(format!("not a bundle entry: {name}"));
        }
        self.zip.start_file(name, Self::options()).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = src.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            self.zip.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            size += n as u64;
        }
        self.files.insert(name.into(), FileEntry { size, sha256: hex(&hash.finalize()) });
        Ok(())
    }

    /// Write the manifest (with the checksums of everything added) and close the file.
    pub fn finish(mut self, app: &str, export_id: &str, created: i64, installation: &str) -> Result<Manifest, String> {
        let m = Manifest { format: FORMAT, app: app.into(), export_id: export_id.into(), created, installation: installation.into(), files: self.files.clone() };
        self.zip.start_file(MANIFEST, Self::options()).map_err(|e| e.to_string())?;
        self.zip.write_all(&serde_json::to_vec_pretty(&m).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let f = self.zip.finish().map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        Ok(m)
    }
}

/// Unpack `zip` into `dir` (created; must be empty or absent) and check it against its manifest. Nothing outside
/// `dir` is written. On any error the caller removes `dir`.
pub fn unpack_checked(zip_path: &Path, dir: &Path, limits: Limits) -> Result<Manifest, String> {
    let f = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("bundle-unreadable: {e}"))?;
    if zip.len() > limits.entries {
        return Err("bundle-too-large".into());
    }
    no_symlink(dir)?;
    std::fs::create_dir_all(dir.join("logs")).map_err(|e| e.to_string())?;
    no_symlink(&dir.join("logs"))?;
    let mut total = 0u64;
    let mut seen: BTreeMap<String, FileEntry> = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| format!("bundle-unreadable: {e}"))?;
        let name = entry.name().to_string();
        // directories, links and unknown names are refused, not skipped: a bundle of this app has none
        if entry.is_dir() || entry.is_symlink() || !allowed_entry(&name) {
            return Err(format!("bundle-entry: {name}"));
        }
        if seen.contains_key(&name) {
            return Err(format!("bundle-entry: {name} twice"));
        }
        let cap = if name == MANIFEST || name == INSTALLATION { limits.metadata.min(limits.per_file) } else { limits.per_file };
        let out_path = dir.join(&name);
        no_symlink(&out_path)?;
        let mut out = File::create(&out_path).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = entry.read(&mut buf).map_err(|e| format!("bundle-unreadable: {e}"))?;
            if n == 0 {
                break;
            }
            size += n as u64;
            total += n as u64;
            if size > cap || total > limits.total {
                return Err("bundle-too-large".into());
            }
            hash.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
        out.sync_all().map_err(|e| e.to_string())?;
        seen.insert(name, FileEntry { size, sha256: hex(&hash.finalize()) });
    }
    let raw = std::fs::read(dir.join(MANIFEST)).map_err(|_| "bundle-no-manifest".to_string())?;
    let m: Manifest = serde_json::from_slice(&raw).map_err(|e| format!("bundle-manifest: {e}"))?;
    if m.format != FORMAT {
        return Err(format!("bundle-format: {}", m.format));
    }
    if !safe_name(&m.export_id) {
        return Err("bundle-manifest: export id".into());
    }
    check(&m, &seen)?;
    Ok(m)
}

/// Every manifest file present with its size and hash, and nothing else besides the manifest.
fn check(m: &Manifest, seen: &BTreeMap<String, FileEntry>) -> Result<(), String> {
    for (name, want) in &m.files {
        match seen.get(name) {
            Some(got) if got == want => {}
            Some(_) => return Err(format!("bundle-checksum: {name}")),
            None => return Err(format!("bundle-missing: {name}")),
        }
    }
    if let Some(extra) = seen.keys().find(|n| n.as_str() != MANIFEST && !m.files.contains_key(*n)) {
        return Err(format!("bundle-entry: {extra} not in the manifest"));
    }
    if !m.files.contains_key(INSTALLATION) {
        return Err(format!("bundle-missing: {INSTALLATION}"));
    }
    Ok(())
}

/// Re-check an already unpacked bundle (an interrupted import is resumed from it).
pub fn recheck(dir: &Path) -> Result<Manifest, String> {
    no_symlink(dir)?;
    no_symlink(&dir.join("logs"))?;
    let m: Manifest = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut seen = BTreeMap::new();
    for name in m.files.keys() {
        if !allowed_entry(name) {
            return Err(format!("bundle-entry: {name}"));
        }
        no_symlink(&dir.join(name))?;
        let mut f = File::open(dir.join(name)).map_err(|_| format!("bundle-missing: {name}"))?;
        let mut hash = Sha256::new();
        let size = std::io::copy(&mut f, &mut hash).map_err(|e| e.to_string())?;
        seen.insert(name.clone(), FileEntry { size, sha256: hex(&hash.finalize()) });
    }
    seen.insert(MANIFEST.into(), FileEntry { size: 0, sha256: String::new() });
    check(&m, &seen)?;
    Ok(m)
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// SHA-256 of a file on disk (a log already in `firmware-logs`).
pub fn file_sha256(path: &Path) -> Result<String, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    std::io::copy(&mut f, &mut hash).map_err(|e| e.to_string())?;
    Ok(hex(&hash.finalize()))
}

/// The unpack folder of an export id below `base`.
pub fn unpack_dir(base: &Path, export_id: &str) -> Result<PathBuf, String> {
    if !safe_name(export_id) {
        return Err("bundle-manifest: export id".into());
    }
    Ok(base.join(export_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("openbms-bundle-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn good(dir: &Path) -> PathBuf {
        let p = dir.join("b.zip");
        let mut w = Writer::create(&p).unwrap();
        w.add(INSTALLATION, &b"{}"[..]).unwrap();
        w.add("logs/a_pack0.log", &b"log"[..]).unwrap();
        w.finish("test", "exp-1", 1, "inst-1").unwrap();
        p
    }

    /// A zip with raw entries, for bundles this app would never write.
    fn raw(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let p = dir.join("raw.zip");
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        for (n, b) in entries {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
        p
    }

    fn manifest_for(files: &[(&str, &[u8])]) -> Vec<u8> {
        let files = files
            .iter()
            .map(|(n, b)| (n.to_string(), FileEntry { size: b.len() as u64, sha256: hex(&Sha256::digest(b)) }))
            .collect();
        serde_json::to_vec(&Manifest { format: FORMAT, app: "t".into(), export_id: "e1".into(), created: 0, installation: "i".into(), files }).unwrap()
    }

    #[test]
    fn round_trip_and_recheck() {
        let d = tmp("rt");
        let m = unpack_checked(&good(&d), &d.join("out"), LIMITS).unwrap();
        assert_eq!(m.export_id, "exp-1");
        assert_eq!(m.files.len(), 2);
        assert_eq!(std::fs::read(d.join("out/logs/a_pack0.log")).unwrap(), b"log");
        assert_eq!(recheck(&d.join("out")).unwrap(), m);
        std::fs::write(d.join("out/logs/a_pack0.log"), b"changed").unwrap();
        assert!(recheck(&d.join("out")).unwrap_err().starts_with("bundle-checksum"));
    }

    #[test]
    fn names_outside_the_bundle_layout_are_refused() {
        for bad in ["../x", "/etc/x", "logs/../x", "logs/.hidden", "logs/a/b", "other.json", "logs/", "logs\\x"] {
            assert!(!allowed_entry(bad), "{bad}");
        }
        let d = tmp("names");
        let p = raw(&d, &[("../evil", b"x")]);
        assert!(unpack_checked(&p, &d.join("out"), LIMITS).unwrap_err().starts_with("bundle-entry"));
        assert!(!d.join("evil").exists());
    }

    #[test]
    fn missing_extra_and_changed_files_are_refused() {
        let d = tmp("check");
        let inst: &[u8] = b"{}";
        // a file in the manifest but not in the zip
        let m = manifest_for(&[(INSTALLATION, inst), ("logs/x.log", b"1")]);
        let p = raw(&d, &[(INSTALLATION, inst), (MANIFEST, &m)]);
        assert!(unpack_checked(&p, &d.join("o1"), LIMITS).unwrap_err().starts_with("bundle-missing"));
        // a file in the zip but not in the manifest
        let m = manifest_for(&[(INSTALLATION, inst)]);
        let p = raw(&d, &[(INSTALLATION, inst), ("logs/x.log", b"1"), (MANIFEST, &m)]);
        assert!(unpack_checked(&p, &d.join("o2"), LIMITS).unwrap_err().starts_with("bundle-entry"));
        // content changed
        let m = manifest_for(&[(INSTALLATION, inst)]);
        let p = raw(&d, &[(INSTALLATION, b"{ }"), (MANIFEST, &m)]);
        assert!(unpack_checked(&p, &d.join("o3"), LIMITS).unwrap_err().starts_with("bundle-checksum"));
    }

    #[test]
    fn sizes_are_counted_while_extracting() {
        let d = tmp("size");
        let big = vec![0u8; 5000];
        let m = manifest_for(&[(INSTALLATION, &big)]);
        let p = raw(&d, &[(INSTALLATION, &big), (MANIFEST, &m)]);
        let small = Limits { per_file: 4096, ..LIMITS };
        assert_eq!(unpack_checked(&p, &d.join("o"), small).unwrap_err(), "bundle-too-large");
        let tight = Limits { total: 4096, ..LIMITS };
        assert_eq!(unpack_checked(&p, &d.join("o2"), tight).unwrap_err(), "bundle-too-large");
        // metadata has its own, smaller cap
        let meta = Limits { metadata: 4096, ..LIMITS };
        assert_eq!(unpack_checked(&p, &d.join("o3"), meta).unwrap_err(), "bundle-too-large");
        let few = Limits { entries: 1, ..LIMITS };
        assert_eq!(unpack_checked(&p, &d.join("o4"), few).unwrap_err(), "bundle-too-large");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_unpack_folder_is_refused() {
        let d = tmp("link");
        std::fs::create_dir_all(d.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(d.join("elsewhere"), d.join("out")).unwrap();
        assert!(unpack_checked(&good(&d), &d.join("out"), LIMITS).unwrap_err().starts_with("bundle-symlink"));
    }
}
