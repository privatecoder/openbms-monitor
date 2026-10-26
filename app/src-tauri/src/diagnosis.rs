//! „Speicherdiagnose“: the pack's own memory diagnosis (0x64/04, MAINT 1.5.3/1.6.3, image flag bit 5) over RS485-1/2 or
//! the CAN socket, at any pack's own address. It reads the stored parameter and SOC copies straight from flash and
//! changes nothing, so it needs no installation and no release.

use crate::bms_memory::on_bus;
use crate::control::PackAction;
use crate::{with_link, AppState};
use openbms_proto::access::Action;
use openbms_proto::MemoryDiagnosis;
use tauri::State;

/// The image flags of the pack at `address` from its own 0x64/03 on the current link, for the functions that depend on
/// them (hard stop choice, diagnosis lock fields) on either bus. None: no answer, or a build the app does not support.
#[tauri::command]
pub async fn pack_image_flags(state: State<'_, AppState>, address: u8) -> Result<Option<u8>, String> {
    let PackAction { guard: _guard, link, .. } = PackAction::read(&state, address, "identity")?;
    let info = with_link(&state.conn, &state.flashing, &state.link, &link, |c| Ok(c.maint_info(address).ok()))?;
    Ok(info.filter(|i| i.supported()).map(|i| i.image_flags()))
}

#[tauri::command]
pub async fn pack_diagnosis(state: State<'_, AppState>, address: u8) -> Result<MemoryDiagnosis, String> {
    let PackAction { guard: _guard, link, bus } = PackAction::read(&state, address, "diagnosis")?;
    on_bus(Action::ReadDiagnosis, bus)?;
    with_link(&state.conn, &state.flashing, &state.link, &link, |c| c.memory_diagnosis(address))
}
