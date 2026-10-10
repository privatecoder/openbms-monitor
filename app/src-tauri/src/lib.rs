use openbms_proto::{DeviceInfo, Status, SystemValues, Telemetry};
use openbms_transport::{list_serial_ports, Bus, Connection, Endpoint};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    conn: Arc<Mutex<Option<Connection>>>,
    bus: Mutex<Option<Bus>>,
    /// Run flag of the current polling thread; each start gets a fresh one, so an old thread cannot stop a new one.
    polling: Mutex<Arc<AtomicBool>>,
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
    // Diagnostics: with OPENBMS_POLL_LOG=<file>, every event is appended there as one JSON line.
    let mut log = std::env::var_os("OPENBMS_POLL_LOG")
        .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok());
    std::thread::spawn(move || {
        while running.load(Ordering::SeqCst) {
            let started = std::time::Instant::now();
            if bus == Bus::CanSocketBus {
                let r = with_conn(&conn, |c| c.system_values());
                let _ = app.emit("system", SystemUpdate { values: r.as_ref().ok().cloned(), error: r.err() });
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
                if let Some(f) = log.as_mut() {
                    use std::io::Write;
                    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                    let _ = writeln!(f, "{{\"t\":{ms},\"pack\":{}}}", serde_json::to_string(&update).unwrap_or_default());
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![list_ports, connect, disconnect, scan, telemetry, start_polling, stop_polling, load_user_cells, save_user_cells])
        .run(tauri::generate_context!())
        .expect("error while running OpenBMS Monitor");
}
