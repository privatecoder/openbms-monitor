//! The "Home Assistant (MQTT)" source: the Seplos BMS add-on's OpenBMS feed read from an MQTT broker instead of the
//! RS485 bus (`docs/exchange/openbms-feed-v1.md`, decoding in `feed.rs`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// The broker and the add-on's topic prefix (`mqtt_topic`, default `seplos`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttSettings {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub tls: bool,
    pub base: String,
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("mqtt.json"))
}

pub(crate) fn load_from(path: &Path) -> Option<MqttSettings> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Written readable by the user only: the file holds the broker password.
pub(crate) fn save_to(path: &Path, s: &MqttSettings) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(&text).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_mqtt_settings(app: AppHandle) -> Option<MqttSettings> {
    load_from(&settings_path(&app).ok()?)
}

#[tauri::command]
pub fn save_mqtt_settings(app: AppHandle, settings: MqttSettings) -> Result<(), String> {
    save_to(&settings_path(&app)?, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MqttSettings {
        MqttSettings { host: "broker.local".into(), port: 1883, user: "u".into(), password: "p".into(), tls: false, base: "seplos".into() }
    }

    #[test]
    fn settings_round_trip_and_mode_0600() {
        let dir = std::env::temp_dir().join(format!("openbms-mqtt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mqtt.json");
        assert_eq!(load_from(&path), None);
        save_to(&path, &sample()).unwrap();
        assert_eq!(load_from(&path), Some(sample()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
