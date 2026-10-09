use openbms_proto::{DeviceInfo, Telemetry};
use openbms_transport::{list_serial_ports, Bus, Connection, Endpoint};
use serde::Deserialize;
use std::sync::Mutex;

#[derive(Default)]
struct AppState {
    conn: Mutex<Option<Connection>>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum EndpointArg {
    Serial { path: String, bus: BusArg },
    // The baud rate of a TCP gateway is configured on the gateway itself.
    Tcp {
        addr: String,
        #[allow(dead_code)]
        bus: BusArg,
    },
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

fn with_conn<T>(state: &tauri::State<AppState>, f: impl FnOnce(&mut Connection) -> openbms_transport::Result<T>) -> Result<T, String> {
    let mut guard = state.conn.lock().map_err(|e| e.to_string())?;
    let conn = guard.as_mut().ok_or("not connected")?;
    f(conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_ports() -> Vec<String> {
    list_serial_ports()
}

#[tauri::command]
async fn connect(state: tauri::State<'_, AppState>, endpoint: EndpointArg) -> Result<(), String> {
    let ep = match endpoint {
        EndpointArg::Serial { path, bus } => Endpoint::Serial { path, baud: Bus::from(bus).baud() },
        EndpointArg::Tcp { addr, .. } => Endpoint::Tcp { addr },
    };
    let conn = Connection::open(&ep).map_err(|e| e.to_string())?;
    *state.conn.lock().map_err(|e| e.to_string())? = Some(conn);
    Ok(())
}

#[tauri::command]
fn disconnect(state: tauri::State<AppState>) -> Result<(), String> {
    *state.conn.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

#[tauri::command]
async fn scan(state: tauri::State<'_, AppState>) -> Result<Vec<(u8, DeviceInfo)>, String> {
    with_conn(&state, |c| Ok(c.scan(0..=15)))
}

#[tauri::command]
async fn telemetry(state: tauri::State<'_, AppState>, address: u8) -> Result<Telemetry, String> {
    with_conn(&state, |c| c.telemetry(address))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![list_ports, connect, disconnect, scan, telemetry])
        .run(tauri::generate_context!())
        .expect("error while running OpenBMS Monitor");
}
