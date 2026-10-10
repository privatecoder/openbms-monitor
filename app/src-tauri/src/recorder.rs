//! Recording of the polled data to a JSON Lines file, switched on and off from the UI.
//!
//! One line per event: `{"t":<unix ms>,"pack":{…}}` for a pack answer (or its error) and
//! `{"t":<unix ms>,"system":{…}}` for the master's Modbus system values. The first line describes
//! the recording: `{"t":…,"start":{"packs":[…],"system":true,"interval_s":0,"bus":"can"}}`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize, Serialize, Clone)]
pub struct RecordOptions {
    /// Pack addresses to record; empty = every polled pack.
    pub packs: Vec<u8>,
    /// Also record the master's Modbus system values (CAN socket bus only).
    pub system: bool,
    /// Minimum seconds between two lines of the same pack (0 = every poll).
    pub interval_s: u64,
}

#[derive(Serialize, Clone, Default)]
pub struct RecordingStatus {
    pub active: bool,
    pub path: Option<String>,
    pub started_ms: u64,
    pub lines: u64,
    pub bytes: u64,
    pub options: Option<RecordOptions>,
    /// Last write error; the recording stops on it.
    pub error: Option<String>,
}

struct Active {
    file: File,
    options: RecordOptions,
    /// Time of the last line per source (pack address, 255 = system values), for the interval.
    last: HashMap<u8, u64>,
}

#[derive(Default)]
pub struct Recorder {
    active: Option<Active>,
    status: RecordingStatus,
}

pub type SharedRecorder = Arc<Mutex<Recorder>>;

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `2026-10-10_16-20-05` in UTC, for file names (no time zone database needed).
fn stamp(ms: u64) -> String {
    let s = ms / 1000;
    let (days, rem) = (s / 86_400, s % 86_400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

impl Recorder {
    pub fn status(&self) -> RecordingStatus {
        self.status.clone()
    }

    pub fn start(&mut self, dir: &Path, options: RecordOptions, bus: &str) -> Result<RecordingStatus, String> {
        self.stop();
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let started = now_ms();
        let path: PathBuf = dir.join(format!("openbms_{}Z.jsonl", stamp(started)));
        let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.active = Some(Active { file, options: options.clone(), last: HashMap::new() });
        self.status = RecordingStatus {
            active: true,
            path: Some(path.display().to_string()),
            started_ms: started,
            options: Some(options.clone()),
            ..Default::default()
        };
        let head = serde_json::json!({ "packs": options.packs, "system": options.system, "interval_s": options.interval_s, "bus": bus });
        self.write_line(started, "start", &head);
        Ok(self.status())
    }

    pub fn stop(&mut self) -> RecordingStatus {
        if let Some(mut a) = self.active.take() {
            let _ = a.file.flush();
        }
        self.status.active = false;
        self.status()
    }

    /// Record a pack answer if this pack is selected and its interval has passed.
    pub fn pack(&mut self, address: u8, update: &impl Serialize) {
        let Some(a) = self.active.as_mut() else { return };
        if !a.options.packs.is_empty() && !a.options.packs.contains(&address) {
            return;
        }
        if self.due(address) {
            self.write_line(now_ms(), "pack", update);
        }
    }

    pub fn system(&mut self, update: &impl Serialize) {
        let Some(a) = self.active.as_ref() else { return };
        if a.options.system && self.due(255) {
            self.write_line(now_ms(), "system", update);
        }
    }

    fn due(&mut self, source: u8) -> bool {
        let Some(a) = self.active.as_mut() else { return false };
        let now = now_ms();
        let interval = a.options.interval_s * 1000;
        match a.last.get(&source) {
            Some(&t) if interval > 0 && now.saturating_sub(t) < interval => false,
            _ => {
                a.last.insert(source, now);
                true
            }
        }
    }

    fn write_line(&mut self, t: u64, key: &str, value: &impl Serialize) {
        let Some(a) = self.active.as_mut() else { return };
        let line = match serde_json::to_string(value) {
            Ok(v) => format!("{{\"t\":{t},\"{key}\":{v}}}\n"),
            Err(_) => return,
        };
        // written straight through, so the file is complete up to the last line even after a crash
        match a.file.write_all(line.as_bytes()) {
            Ok(()) => {
                self.status.lines += 1;
                self.status.bytes += line.len() as u64;
            }
            Err(e) => {
                self.status.error = Some(e.to_string());
                self.stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_utc() {
        assert_eq!(stamp(0), "1970-01-01_00-00-00");
        assert_eq!(stamp(1_791_641_683_466), "2026-10-10_14-14-43");
    }

    #[test]
    fn records_selected_packs_with_interval() {
        let dir = std::env::temp_dir().join(format!("openbms-rec-{}", std::process::id()));
        let mut r = Recorder::default();
        r.start(&dir, RecordOptions { packs: vec![1], system: false, interval_s: 60 }, "can").unwrap();
        r.pack(0, &1); // not selected
        r.pack(1, &2);
        r.pack(1, &3); // within the interval
        r.system(&4); // system off
        let st = r.stop();
        assert_eq!(st.lines, 2); // start line + one pack line
        let text = std::fs::read_to_string(st.path.unwrap()).unwrap();
        assert!(text.lines().nth(1).unwrap().ends_with("\"pack\":2}"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
