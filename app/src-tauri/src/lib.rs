use openbms_proto::{DeviceInfo, Parameters, Status, SystemValues, Telemetry};
use openbms_transport::{list_serial_ports, Bus, Connection, Endpoint};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

mod recorder;
use recorder::{RecordOptions, RecordingStatus, SharedRecorder};

#[derive(Default)]
struct AppState {
    conn: Arc<Mutex<Option<Connection>>>,
    bus: Mutex<Option<Bus>>,
    /// Run flag of the current polling thread; each start gets a fresh one, so an old thread cannot stop a new one.
    polling: Mutex<Arc<AtomicBool>>,
    recorder: SharedRecorder,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum EndpointArg {
    Serial { path: String, bus: BusArg },
    // The baud rate of a TCP gateway is configured on the gateway itself.
    Tcp { addr: String, bus: BusArg },
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum BusArg {
    Pack,
    Can,
}

impl From<BusArg> for Bus {
    fn from(b: BusArg) -> Self {
        match b {
            BusArg::Pack => Bus::PackBus,
            BusArg::Can => Bus::CanSocketBus,
        }
    }
}

/// Snapshot of one pack, sent to the UI as event "pack".
#[derive(Serialize, Clone)]
struct PackUpdate {
    address: u8,
    telemetry: Option<Telemetry>,
    status: Option<Status>,
    error: Option<String>,
}

/// System values of the master, sent as event "system".
#[derive(Serialize, Clone)]
struct SystemUpdate {
    values: Option<SystemValues>,
    error: Option<String>,
}

type Shared = Arc<Mutex<Option<Connection>>>;

fn lock(m: &Mutex<Option<Connection>>) -> Result<std::sync::MutexGuard<'_, Option<Connection>>, String> {
    m.lock().map_err(|e| e.to_string())
}

fn with_conn<T>(conn: &Shared, f: impl FnOnce(&mut Connection) -> openbms_transport::Result<T>) -> Result<T, String> {
    let mut guard = lock(conn)?;
    let c = guard.as_mut().ok_or("not connected")?;
    f(c).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_ports() -> Vec<String> {
    list_serial_ports()
}

#[tauri::command]
async fn connect(state: State<'_, AppState>, endpoint: EndpointArg) -> Result<(), String> {
    if let Ok(p) = state.polling.lock() {
        p.store(false, Ordering::SeqCst);
    }
    let (ep, bus) = match endpoint {
        EndpointArg::Serial { path, bus } => (Endpoint::Serial { path, baud: Bus::from(bus).baud() }, Bus::from(bus)),
        EndpointArg::Tcp { addr, bus } => (Endpoint::Tcp { addr }, Bus::from(bus)),
    };
    let conn = Connection::open(&ep).map_err(|e| e.to_string())?;
    *lock(&state.conn)? = Some(conn);
    *state.bus.lock().map_err(|e| e.to_string())? = Some(bus);
    Ok(())
}

#[tauri::command]
fn disconnect(state: State<AppState>) -> Result<(), String> {
    if let Ok(p) = state.polling.lock() {
        p.store(false, Ordering::SeqCst);
    }
    *lock(&state.conn)? = None;
    Ok(())
}

#[tauri::command]
async fn scan(state: State<'_, AppState>) -> Result<Vec<(u8, DeviceInfo)>, String> {
    with_conn(&state.conn, |c| Ok(c.scan(0..=15)))
}

#[tauri::command]
async fn telemetry(state: State<'_, AppState>, address: u8) -> Result<Telemetry, String> {
    with_conn(&state.conn, |c| c.telemetry(address))
}

/// All parameters and function switches of one pack (0x47). Only RS485-1/2 knows this command; there the
/// master of a multi-pack bank does not answer.
#[tauri::command]
async fn parameters(state: State<'_, AppState>, address: u8) -> Result<Parameters, String> {
    if *state.bus.lock().map_err(|e| e.to_string())? == Some(Bus::CanSocketBus) {
        return Err("parameters can only be read on RS485-1/2".into());
    }
    with_conn(&state.conn, |c| c.parameters(address))
}

/// Poll the given packs in a loop and emit "pack" (and on the CAN socket bus "system") events.
#[tauri::command]
fn start_polling(app: AppHandle, state: State<AppState>, addresses: Vec<u8>, interval_ms: u64) -> Result<(), String> {
    let bus = state.bus.lock().map_err(|e| e.to_string())?.ok_or("not connected")?;
    let running = Arc::new(AtomicBool::new(true));
    {
        // stop the previous thread and install the new flag
        let mut current = state.polling.lock().map_err(|e| e.to_string())?;
        current.store(false, Ordering::SeqCst);
        *current = running.clone();
    }
    std::thread::sleep(Duration::from_millis(50));
    let conn = state.conn.clone();
    let rec = state.recorder.clone();
    std::thread::spawn(move || {
        while running.load(Ordering::SeqCst) {
            let started = std::time::Instant::now();
            if bus == Bus::CanSocketBus {
                let r = with_conn(&conn, |c| c.system_values());
                let update = SystemUpdate { values: r.as_ref().ok().cloned(), error: r.err() };
                if let Ok(mut rec) = rec.lock() {
                    rec.system(&update);
                }
                let _ = app.emit("system", update);
            }
            for &address in &addresses {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                let telemetry = with_conn(&conn, |c| c.telemetry(address));
                let status = with_conn(&conn, |c| c.status(address));
                let error = telemetry.as_ref().err().or(status.as_ref().err()).cloned();
                // A dead link (gateway closed the socket, broken pipe) does not heal by retrying: drop it and
                // stop polling. The UI then sees no fresh data and reconnects after its timeout.
                if error.as_deref().is_some_and(|e| e == "not connected" || e.starts_with("I/O") || e.starts_with("connection closed")) {
                    if running.load(Ordering::SeqCst) {
                        if let Ok(mut c) = conn.lock() {
                            *c = None;
                        }
                    }
                    running.store(false, Ordering::SeqCst);
                    break;
                }
                let update = PackUpdate { address, telemetry: telemetry.ok(), status: status.ok(), error };
                if let Ok(mut rec) = rec.lock() {
                    rec.pack(address, &update);
                }
                let _ = app.emit("pack", update);
            }
            let wait = Duration::from_millis(interval_ms).saturating_sub(started.elapsed());
            std::thread::sleep(wait.max(Duration::from_millis(100)));
        }
    });
    Ok(())
}

#[tauri::command]
fn stop_polling(state: State<AppState>) {
    if let Ok(p) = state.polling.lock() {
        p.store(false, Ordering::SeqCst);
    }
}

/// User cell entries (own and overridden models) as one JSON array in the app data directory.
fn user_cells_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("cells.user.json"))
}

#[tauri::command]
fn load_user_cells(app: AppHandle) -> Result<serde_json::Value, String> {
    let path = user_cells_path(&app)?;
    if !path.exists() {
        return Ok(serde_json::Value::Array(vec![]));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[tauri::command]
fn save_user_cells(app: AppHandle, cells: serde_json::Value) -> Result<(), String> {
    if !cells.is_array() {
        return Err("expected an array of cells".into());
    }
    // one write at a time, each through its own temporary file, renamed into place, so neither a
    // crash nor two overlapping saves can leave a half-written or mixed database
    static WRITE: Mutex<()> = Mutex::new(());
    let _guard = WRITE.lock().map_err(|e| e.to_string())?;
    let path = user_cells_path(&app)?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = path.with_extension(format!("json.{}.{nanos}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(&cells).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

fn recordings_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("recordings"))
}

/// Start recording the polled data to a new file (see recorder.rs for the format).
#[tauri::command]
fn start_recording(app: AppHandle, state: State<AppState>, options: RecordOptions) -> Result<RecordingStatus, String> {
    let bus = match *state.bus.lock().map_err(|e| e.to_string())? {
        Some(Bus::CanSocketBus) => "can",
        Some(Bus::PackBus) => "pack",
        None => "",
    };
    let dir = recordings_dir(&app)?;
    state.recorder.lock().map_err(|e| e.to_string())?.start(&dir, options, bus)
}

#[tauri::command]
fn stop_recording(state: State<AppState>) -> Result<RecordingStatus, String> {
    Ok(state.recorder.lock().map_err(|e| e.to_string())?.stop())
}

#[tauri::command]
fn recording_status(state: State<AppState>) -> Result<RecordingStatus, String> {
    Ok(state.recorder.lock().map_err(|e| e.to_string())?.status())
}

fn parameters_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("parameters"))
}

/// Save a parameter export (BatteryMonitor XML, built by the UI) and return its path.
#[tauri::command]
fn save_parameters(app: AppHandle, name: String, content: String) -> Result<String, String> {
    // the name comes from the UI: keep it a plain file name
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return Err(format!("invalid file name: {name}"));
    }
    let dir = parameters_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

/// Show a file (or the folder itself) in the file manager. Only paths inside `dir`, so the UI cannot open
/// arbitrary files.
fn reveal(dir: &std::path::Path, path: Option<String>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let target = match path.map(std::path::PathBuf::from) {
        Some(p) if p.starts_with(dir) && p.exists() => p,
        _ => dir.to_path_buf(),
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(&target).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(format!("/select,{}", target.display())).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(if target.is_dir() { &target } else { dir }).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

/// Show a recording (or the recordings folder) in the file manager.
#[tauri::command]
fn reveal_recording(app: AppHandle, path: Option<String>) -> Result<(), String> {
    reveal(&recordings_dir(&app)?, path)
}

/// Show a saved parameter export (or the parameters folder) in the file manager.
#[tauri::command]
fn reveal_parameters(app: AppHandle, path: Option<String>) -> Result<(), String> {
    reveal(&parameters_dir(&app)?, path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![list_ports, connect, disconnect, scan, telemetry, start_polling, stop_polling, load_user_cells, save_user_cells, start_recording, stop_recording, recording_status, reveal_recording, parameters, save_parameters, reveal_parameters])
        .run(tauri::generate_context!())
        .expect("error while running OpenBMS Monitor");
}
