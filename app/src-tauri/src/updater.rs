//! App updates from the project's GitHub releases (Tauri updater plugin).
//!
//! Every release carries `latest.json` (written by the release workflow) with one signed download per platform: the
//! macOS app as `.app.tar.gz`, the Windows NSIS installers. The plugin checks the minisign signature against the public
//! key in `tauri.conf.json` before it installs anything. The releases are pre-releases, so GitHub's "latest release"
//! link would never find them: the newest release with a `latest.json` is looked up through the GitHub API instead.
//!
//! The check only reads; installing is the user's click. Before the first install step the updater takes the bus
//! reservation (`control::reserve_bus`: only while no pack action runs, under the same mutex as the pack reservations)
//! and holds it until the process ends or the install fails: a running pack action, firmware update, maintenance
//! operation or installation import refuses the install, and none of them can start meanwhile. The recording needs no
//! care: every row is committed when written.

use crate::AppState;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const RELEASES: &str = "https://api.github.com/repos/privatecoder/openbms-monitor/releases?per_page=20";
const MANIFEST: &str = "latest.json";

/// The update found by the last check; installing uses exactly that one.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

#[derive(Serialize, Clone)]
pub struct UpdateInfo {
    version: String,
    current: String,
    notes: Option<String>,
    date: Option<String>,
}

#[derive(Serialize, Clone)]
struct Progress {
    done: u64,
    total: Option<u64>,
}

/// The download link of `latest.json` of the newest release that has one. The API lists releases newest first;
/// drafts are skipped (they are not public).
fn manifest_url(releases: &serde_json::Value) -> Option<String> {
    releases.as_array()?.iter().filter(|r| !r["draft"].as_bool().unwrap_or(false)).find_map(|r| {
        r["assets"].as_array()?.iter().find(|a| a["name"] == MANIFEST)?["browser_download_url"].as_str().map(str::to_owned)
    })
}

/// Why the bus reservation was refused (or a structural data operation runs), for the UI.
pub(crate) fn busy(state: &AppState) -> Option<&'static str> {
    if state.flashing.load(Ordering::SeqCst) {
        return Some("flash");
    }
    if state.port_busy.load(Ordering::SeqCst) || state.maint_action.lock().map(|a| a.is_some()).unwrap_or(true) {
        return Some("maintenance");
    }
    if state.memory_busy.load(Ordering::SeqCst) {
        return Some("history");
    }
    if state.params_pause.load(Ordering::SeqCst) > 0 {
        return Some("write");
    }
    if crate::control::any_pack_action(&state.ops).is_some() {
        return Some("pack");
    }
    if crate::control::any_installation_operation(&state.ops) {
        return Some("installations");
    }
    None
}

/// Network limits: the API and the manifest are small; the package may take minutes on a slow line, but a stalled
/// connection ends after `READ_STALL`.
const CONNECT: std::time::Duration = std::time::Duration::from_secs(20);
const READ_STALL: std::time::Duration = std::time::Duration::from_secs(60);
const DOWNLOAD_MAX: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// What the install holds until the process ends: the bus (no pack action, flash or maintenance operation runs or
/// starts) and every installation (no merge, delete, export or import runs or starts). Refused with the reason.
pub(crate) struct InstallSlot {
    _bus: crate::PortSlot,
    _installations: crate::control::InstallationGuard,
}

pub(crate) fn install_slot(state: &AppState) -> Result<InstallSlot, &'static str> {
    let Some(bus) = crate::control::reserve_bus(state) else {
        return Err(busy(state).unwrap_or("maintenance"));
    };
    let installations = crate::control::reserve_installations(&state.ops, &[crate::control::ALL], "update").map_err(|_| "installations")?;
    Ok(InstallSlot { _bus: bus, _installations: installations })
}

/// Look for a newer version. None: this one is current (or no release carries update data yet).
#[tauri::command]
pub async fn update_check(app: AppHandle, pending: State<'_, Pending>) -> Result<Option<UpdateInfo>, String> {
    // the updater plugin installs the same provider before its own requests; this request comes first
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("openbms-monitor/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(CONNECT)
        .timeout(CONNECT)
        .build()
        .map_err(|e| e.to_string())?;
    let releases: serde_json::Value = client
        .get(RELEASES)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let Some(url) = manifest_url(&releases) else {
        *pending.0.lock().map_err(|e| e.to_string())? = None;
        return Ok(None);
    };
    let endpoint = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
    let update = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| e.to_string())?
        .timeout(DOWNLOAD_MAX)
        .configure_client(|c| c.connect_timeout(CONNECT).read_timeout(READ_STALL))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    });
    *pending.0.lock().map_err(|e| e.to_string())? = update;
    Ok(info)
}

/// Download, verify and install the update of the last check, then restart. Progress as `update-progress` events.
/// Errors: `busy:<why>`, `no-update`, or the plugin's message (download, signature, install).
#[tauri::command]
pub async fn update_install(app: AppHandle, state: State<'_, AppState>, pending: State<'_, Pending>) -> Result<(), String> {
    if let Some(why) = busy(&state) {
        return Err(format!("busy:{why}"));
    }
    let update = pending.0.lock().map_err(|e| e.to_string())?.take().ok_or("no-update")?;
    let mut done = 0u64;
    let events = app.clone();
    let downloaded = update
        .download(
            move |chunk, total| {
                done += chunk as u64;
                let _ = events.emit("update-progress", Progress { done, total });
            },
            || {},
        )
        .await;
    let bytes = match downloaded {
        Ok(b) => b,
        Err(e) => return Err(e.to_string()),
    };
    // from here to the end of the process: no pack action, flash, maintenance operation or import starts, and none runs
    let refused = |why: &'static str| -> Result<(), String> {
        *pending.0.lock().map_err(|e| e.to_string())? = Some(update.clone());
        Err(format!("busy:{why}"))
    };
    let _bus = match install_slot(&state) {
        Ok(slot) => slot,
        Err(why) => return refused(why),
    };
    // Windows: the installer takes over and the plugin ends the app; macOS: the bundle is replaced, then restart
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_install_holds_the_bus_and_waits_for_running_pack_actions() {
        let state = AppState::default();
        let pack = crate::control::reserve_pack(&state, 3, "write").unwrap();
        assert_eq!(install_slot(&state).err(), Some("pack"));
        drop(pack);
        let slot = install_slot(&state).expect("nothing runs");
        assert!(crate::control::reserve_pack(&state, 3, "write").is_err(), "no pack action starts while the install holds the bus");
        assert!(crate::control::reserve_bus(&state).is_none(), "no flash or maintenance operation either");
        assert!(crate::control::reserve_installations(&state.ops, &["a"], "merge").is_err(), "no installation operation starts either");
        drop(slot);
        assert!(crate::control::reserve_pack(&state, 3, "write").is_ok());
        let merge = crate::control::reserve_installations(&state.ops, &["a"], "merge").unwrap();
        assert_eq!(install_slot(&state).err(), Some("installations"));
        assert!(crate::control::reserve_bus(&state).is_some(), "a refused install frees the bus again");
        drop(merge);
        state.flashing.store(true, Ordering::SeqCst);
        let _flash_port = crate::control::reserve_bus(&state).unwrap();
        assert_eq!(install_slot(&state).err(), Some("flash"));
    }

    #[test]
    fn the_newest_published_release_with_update_data_is_used() {
        let releases = json!([
            { "draft": true, "assets": [{ "name": "latest.json", "browser_download_url": "https://x/draft/latest.json" }] },
            { "draft": false, "prerelease": true, "assets": [{ "name": "a.dmg", "browser_download_url": "https://x/new/a.dmg" }] },
            { "draft": false, "prerelease": true, "assets": [
                { "name": "a.dmg", "browser_download_url": "https://x/v2/a.dmg" },
                { "name": "latest.json", "browser_download_url": "https://x/v2/latest.json" }] },
            { "draft": false, "assets": [{ "name": "latest.json", "browser_download_url": "https://x/v1/latest.json" }] },
        ]);
        assert_eq!(manifest_url(&releases).as_deref(), Some("https://x/v2/latest.json"));
        assert_eq!(manifest_url(&json!([])), None);
        assert_eq!(manifest_url(&json!({ "message": "rate limited" })), None);
    }
}
