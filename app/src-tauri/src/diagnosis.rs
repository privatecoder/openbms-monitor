//! „Speicherdiagnose“: the pack's own memory diagnosis (0x64/04, MAINT 1.5.2/1.6.3, image flag bit 5) over RS485-1/2 or
//! the CAN socket, at any pack's own address. It reads the stored parameter and SOC copies straight from flash and
//! changes nothing, so it needs no installation and no release.

use crate::bms_memory::{bus_of, on_bus};
use crate::control::reserve_pack_wait;
use crate::{link_now, with_link, AppState};
use openbms_proto::access::Action;
use openbms_proto::MemoryDiagnosis;
use tauri::State;

#[tauri::command]
pub async fn pack_diagnosis(state: State<'_, AppState>, address: u8) -> Result<MemoryDiagnosis, String> {
    let _guard = reserve_pack_wait(&state, address, "diagnosis")?;
    let link = link_now(&state);
    let bus = bus_of(&state, &link)?;
    on_bus(Action::ReadDiagnosis, bus)?;
    with_link(&state.conn, &state.flashing, &state.link, &link, |c| c.memory_diagnosis(address))
}
