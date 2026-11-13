//! „Speicherdiagnose“: the pack's own memory diagnosis (0x64/04, image flag bit 5) over RS485-1/2 or
//! the CAN socket, at any pack's own address. It reads the stored parameter and SOC copies straight from flash and
//! changes nothing, so it needs no installation and no release.

use crate::bms_memory::on_bus;
use crate::control::PackAction;
use crate::{with_link, AppState};
use openbms_proto::access::Action;
use openbms_proto::maint::{CAP_CALIBRATION, CAP_ENERGY, CAP_SOC_STORE};
use openbms_proto::{CalPage, CalibrationPage, MemoryDiagnosis, SocStore};
use serde::Serialize;
use tauri::State;

/// The image flags and read capabilities of a pack's own 0x64/03.
#[derive(Serialize)]
pub struct ImageBits {
    pub flags: u8,
    /// the read-capability mask of the EXT05 identity (0: none)
    pub read_caps: u16,
}

/// The image flags and read capabilities of the pack at `address` from its own 0x64/03 on the current link, for the
/// functions that depend on them (hard stop choice, diagnosis, the reads of the read-capability bits) on either bus. None: no answer,
/// or a build the app does not support.
#[tauri::command]
pub async fn pack_image_flags(state: State<'_, AppState>, address: u8) -> Result<Option<ImageBits>, String> {
    let PackAction { guard: _guard, link, .. } = PackAction::read(&state, address, "identity")?;
    let info = with_link(&state.conn, &state.flashing, &state.link, &link, |c| Ok(c.maint_info(address).ok()))?;
    Ok(info.filter(|i| i.supported()).map(|i| ImageBits { flags: i.image_flags(), read_caps: i.read_caps }))
}

#[tauri::command]
pub async fn pack_diagnosis(state: State<'_, AppState>, address: u8) -> Result<MemoryDiagnosis, String> {
    let PackAction { guard: _guard, link, bus } = PackAction::read(&state, address, "diagnosis")?;
    on_bus(Action::ReadDiagnosis, bus)?;
    with_link(&state.conn, &state.flashing, &state.link, &link, |c| c.memory_diagnosis(address))
}

/// The read-capability mask of the pack at `address` from its own fresh 0x64/03 (EXT05): 0 for an older or an
/// unsupported build, an error only when the link fails.
fn read_caps(c: &mut openbms_transport::Connection, address: u8) -> u16 {
    c.maint_info(address).ok().filter(|i| i.supported()).map_or(0, |i| i.read_caps)
}

const NO_CAP: &str = "no-cap: the pack did not report this read in its identity (EMU1101-EXT Light 1.5.4 / Full 1.6.4 or later)";

/// One page of `pack_calibration`: its value, or why it is missing (the others are still shown).
#[derive(Serialize)]
pub struct CalPageRead {
    pub page: CalPage,
    pub value: Option<CalibrationPage>,
    pub error: Option<String>,
}

/// „Kalibrierung lesen“: the active calibration and both stored copies (0x64/05, read-capability bit 1), three reads in
/// a row under one pack reservation. Changes nothing; needs no installation.
#[tauri::command]
pub async fn pack_calibration(state: State<'_, AppState>, address: u8) -> Result<Vec<CalPageRead>, String> {
    let PackAction { guard: _guard, link, bus } = PackAction::read(&state, address, "cal_read")?;
    on_bus(Action::ReadCalibration, bus)?;
    with_link(&state.conn, &state.flashing, &state.link, &link, |c| {
        if read_caps(c, address) & (1 << CAP_CALIBRATION) == 0 {
            return Ok(Err(NO_CAP.to_string()));
        }
        Ok(Ok([CalPage::Active, CalPage::Primary, CalPage::Backup]
            .into_iter()
            .map(|page| match c.calibration_page(address, page) {
                Ok(v) => CalPageRead { page, value: Some(v), error: None },
                Err(e) => CalPageRead { page, value: None, error: Some(e.to_string()) },
            })
            .collect()))
    })?
}

/// What 0x64/06 gave, with the bits that say which part of it the build vouches for, and whether a planned restart
/// first saves the SOC (image flag bit 6): then the stored state shown here is not the one a restart starts from.
#[derive(Serialize)]
pub struct SocStoreRead {
    pub store: SocStore,
    /// read-capability bit 2: the stored records
    pub records: bool,
    /// read-capability bit 3: the live energy counters
    pub energy: bool,
    pub checkpoint: bool,
}

fn soc_store_read(c: &mut openbms_transport::Connection, address: u8) -> Result<SocStoreRead, String> {
    let info = c.maint_info(address).ok().filter(|i| i.supported());
    let caps = info.map_or(0, |i| i.read_caps);
    if caps & ((1 << CAP_SOC_STORE) | (1 << CAP_ENERGY)) == 0 {
        return Err(NO_CAP.into());
    }
    let store = c.soc_store(address).map_err(|e| e.to_string())?;
    Ok(SocStoreRead {
        store,
        records: caps & (1 << CAP_SOC_STORE) != 0,
        energy: caps & (1 << CAP_ENERGY) != 0,
        checkpoint: info.is_some_and(|i| i.checkpoint()),
    })
}

/// „Gespeicherter Stand“: the stored capacity records against the live values, and the full-width energy counters
/// (0x64/06). Changes nothing; needs no installation.
#[tauri::command]
pub async fn pack_soc_store(state: State<'_, AppState>, address: u8) -> Result<SocStoreRead, String> {
    let PackAction { guard: _guard, link, bus } = PackAction::read(&state, address, "soc_store")?;
    on_bus(Action::ReadSocStore, bus)?;
    with_link(&state.conn, &state.flashing, &state.link, &link, |c| Ok(soc_store_read(c, address)))?
}

/// The same read on the flash target (port and address of the firmware view, not the monitor's connection), while the
/// bus is quiet, like `firmware::firmware_group0`. None: not readable now (no capability, a polling master, no answer);
/// only information next to the flash, never a condition for it.
#[tauri::command]
pub async fn firmware_soc_store(app: tauri::AppHandle, state: State<'_, AppState>, port: String, address: u8) -> Result<Option<SocStoreRead>, String> {
    if state.flashing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("a firmware update is running".into());
    }
    let _port_slot = crate::control::reserve_bus_wait(&state).ok_or("the port is busy with another operation")?;
    let key = crate::maint::pack_key(&port).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    let mut link = crate::maint::open_pack_for(&state, &app, &key).map_err(|e| serde_json::to_string(&e).unwrap_or_default())?;
    let quiet = openbms_flash::maint::bus::listen(&mut link, &openbms_flash::maint::SystemClock::new(), crate::maint::LISTEN).map(|t| t.quiet()).unwrap_or(false);
    Ok(if quiet { soc_store_read(&mut link, address).ok() } else { None })
}
