//! Flashing one board outside an installation („Platine flashen“): a search on any RS485-1/2 port, each pack with its
//! String 2, and optionally the board's own CAN socket for the master marker. What the app knows about a board in an
//! installation (patch proof, hard-stop sighting) is found by its String 2, not by a place.

use crate::maint::{self, MaintError, MasterView};
use crate::maint_store::Store;
use crate::AppState;
use openbms_flash::known::ImageKind;
use openbms_flash::maint::dialog::{self, Action, Budgets};
use openbms_flash::maint::policy::Master;
use openbms_flash::maint::{bus, SystemClock};
use openbms_proto::{DeviceInfo, MarkerWord};
use openbms_transport::{Bus, Connection, Endpoint};
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

#[derive(Serialize, Clone, Debug)]
pub struct BenchPack {
    address: u8,
    info: DeviceInfo,
    /// String 2 as read now (None: not readable or not set)
    serial: Option<String>,
}

#[derive(Serialize)]
pub struct BenchScan {
    /// bytes heard while listening before the search; not 0: nothing was searched
    traffic: usize,
    packs: Vec<BenchPack>,
    /// stopped by „Abbrechen“: `packs` holds what answered until then
    cancelled: bool,
    /// the master on the given CAN socket, when address 0 answered and the CAN socket shows the same board
    master: Option<MasterView>,
    /// a CAN socket was given and address 0 answered: whether its String 2 there is the one read on RS485-1/2
    same_board: Option<bool>,
}

/// The board's CAN socket as an endpoint key (`serial:…` at 9600 baud, or `tcp:…`).
pub fn open_control(key: &str) -> Result<Connection, MaintError> {
    let key = maint::pack_key(key)?;
    let ep = match key.split_once(':') {
        Some(("tcp", addr)) => Endpoint::Tcp { addr: addr.into() },
        Some((_, path)) => Endpoint::Serial { path: path.into(), baud: Bus::CanSocketBus.baud() },
        None => return Err(maint::refused("no-port")),
    };
    let mut c = Connection::open(&ep).map_err(|e| MaintError::Connection { message: e.to_string() })?;
    c.set_label(crate::capture::label(&ep, Bus::CanSocketBus));
    c.set_bus(Bus::CanSocketBus);
    Ok(c)
}

/// The two ports must differ: the pack bus and the CAN socket are two buses.
pub fn distinct(pack: &str, control: Option<&str>) -> Result<(), MaintError> {
    match control {
        Some(c) if maint::pack_key(c)? == maint::pack_key(pack)? => Err(maint::refused("bench-same-port")),
        _ => Ok(()),
    }
}

/// String 2 of address 0 over its CAN socket, canonical.
fn can_string2(control: &mut Connection) -> Option<String> {
    control.uart1_serial(Bus::CanSocketBus, 0, openbms_proto::uart1::CMD_STRING2).ok().and_then(|raw| openbms_proto::serial::canonical(&raw))
}

/// Whether the CAN socket belongs to the board found at address 0 on the pack bus: both canonical String 2 read and
/// equal. Anything else ties nothing.
pub fn same_board(pack: Option<&str>, can: Option<&str>) -> bool {
    matches!((pack, can), (Some(p), Some(c)) if p == c)
}

/// One search on a board's RS485-1/2 port, without an installation: listen first (a polling master or other traffic:
/// nothing is searched), then 0x51 on 0–15 with String 2 each. A pending pack that answers as the board flashed is
/// cleared. With `control` and address 0 found, its master state over the CAN socket, read only.
#[tauri::command]
pub async fn bench_scan(app: AppHandle, state: State<'_, AppState>, port: String, control: Option<String>) -> Result<BenchScan, MaintError> {
    let key = maint::pack_key(&port)?;
    distinct(&key, control.as_deref())?;
    if state.flashing.load(Ordering::SeqCst) {
        return Err(MaintError::Busy);
    }
    state.scan_cancel.store(false, Ordering::SeqCst);
    let _op = crate::control::reserve_bus_wait(&state).ok_or(MaintError::Busy)?;
    let _a = maint::begin_action(&state, "scan");
    crate::stop_monitor_and_tell(&state, &app).map_err(|m| MaintError::Connection { message: m })?;
    let mut link = maint::open_pack_for(&state, &app, &key)?;
    let heard = bus::listen(&mut link, &SystemClock::new(), maint::LISTEN).map_err(|e| MaintError::Connection { message: e.to_string() })?;
    if !heard.quiet() {
        return Ok(BenchScan { traffic: heard.bytes, packs: vec![], cancelled: false, master: None, same_board: None });
    }
    let found = link.scan(maint::scan_addresses(&state.scan_cancel)).map_err(|e| MaintError::Connection { message: e.to_string() })?;
    let cancelled = state.scan_cancel.swap(false, Ordering::SeqCst);
    let mut packs = vec![];
    for (address, info) in found {
        packs.push(BenchPack { address, info, serial: maint::string2(&mut link, address) });
    }
    let addresses: Vec<u8> = packs.iter().map(|p| p.address).collect();
    let pending = maint::with_store(&state, |s| s.pending_on(&key))?;
    let answered = maint::answered_with_serial(&pending, &addresses, &mut link);
    maint::with_store(&state, |s| s.clear_answered(&key, &answered, crate::history::now_ms()))?;
    let pack0 = if addresses.contains(&0) && !cancelled { maint::canonical_string2(&mut link, 0) } else { None };
    drop(link);
    let (mut master, mut same) = (None, None);
    if let (Some(c), true) = (control.as_deref(), addresses.contains(&0) && !cancelled) {
        let mut can = open_control(c)?;
        let tied = same_board(pack0.as_deref(), can_string2(&mut can).as_deref());
        same = Some(tied);
        if tied {
            master = dialog::read(&mut can, &SystemClock::new(), Budgets::default()).ok().map(|m| maint::master_view(&m));
        }
    }
    Ok(BenchScan { traffic: 0, packs, cancelled, master, same_board: same })
}

/// What a flash of address 0 outside an installation does with the master marker, over the board's own CAN socket:
/// "unchecked" (no CAN socket given, another board there, no readable state), "checked" (off, or on for an image
/// with the maintenance functions, which starts halted then), "released" (on, cleared before an image without them:
/// an original image ignores the marker and would keep it hidden for a later patched one).
pub fn bench_marker(control: Option<&mut Connection>, pack0: Option<&str>, image: ImageKind) -> Result<&'static str, MaintError> {
    let Some(can) = control else { return Ok("unchecked") };
    if !same_board(pack0, can_string2(can).as_deref()) {
        return Ok("unchecked");
    }
    let clock = SystemClock::new();
    let m = match dialog::read(can, &clock, Budgets::default()) {
        Ok(m) => m,
        Err(_) => return Ok("unchecked"),
    };
    marker_step(&m, image, || dialog::dialog(can, &clock, Action::Clear, Budgets::default()).map(|_| ()).map_err(|e| e.to_string()))
}

/// `bench_marker` once the board is tied and read: `clear` runs only for a trusted marker on before an image without
/// the maintenance functions; a failed clear refuses the flash.
pub fn marker_step(m: &Master, image: ImageKind, clear: impl FnOnce() -> Result<(), String>) -> Result<&'static str, MaintError> {
    match m.marker() {
        Some(MarkerWord::On) if image != ImageKind::Maintenance => {
            if !m.may_release() {
                return Err(maint::refused("release-master-first-unread"));
            }
            clear().map_err(|message| MaintError::Connection { message })?;
            Ok("released")
        }
        Some(MarkerWord::On | MarkerWord::Off) => Ok("checked"),
        _ => Ok("unchecked"),
    }
}

/// The places in installations whose current board has this String 2: where a flash of it outside an installation
/// ends the patch proof and the hard-stop sighting, and a finished patched flash sets the proof again.
pub fn places_of(store: &Store, serial: &str) -> Result<Vec<(String, u8)>, String> {
    let mut out: Vec<(String, u8)> = vec![];
    for p in store.serial_places(serial)? {
        if let Some(b) = store.current_board(&p.installation, p.address)? {
            let here = (p.installation.clone(), p.address);
            if (b.bms_serial == serial || b.aliases.iter().any(|a| a == serial)) && !out.contains(&here) {
                out.push(here);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbms_flash::maint::policy::Patch;
    use openbms_proto::{MaintInfo, MaintState};

    fn master(word: u32) -> Master {
        let info = MaintInfo { format: 2, base: [16, 6, 13], patch: [1, 5], build_id: [0x90, 0x9A, 0x60, 0x4E, 0x60, 0xE9, 0x22, 0x5B], hard_stop: None, power_rtc: false, pack_cutoff: false, reserved_flags: 0 };
        Master::Patched { patch: Patch::recognise(info).unwrap(), state: Some(MaintState { marker_word: word, effective_slaves: 0 }) }
    }

    #[test]
    fn the_can_socket_counts_only_for_the_same_board() {
        assert!(same_board(Some("SYN-0"), Some("SYN-0")));
        assert!(!same_board(Some("SYN-0"), Some("SYN-1")));
        assert!(!same_board(None, Some("SYN-0")));
        assert!(!same_board(Some("SYN-0"), None));
        assert!(!same_board(None, None));
    }

    #[test]
    fn a_marker_on_is_cleared_before_an_image_without_maintenance_only() {
        let (on, off) = (master(0), master(u32::MAX));
        let mut cleared = 0;
        assert_eq!(marker_step(&on, ImageKind::Original, || { cleared += 1; Ok(()) }).unwrap(), "released");
        assert_eq!(marker_step(&on, ImageKind::Custom, || { cleared += 1; Ok(()) }).unwrap(), "released");
        assert_eq!(cleared, 2);
        assert_eq!(marker_step(&on, ImageKind::Maintenance, || panic!("kept")).unwrap(), "checked");
        assert_eq!(marker_step(&off, ImageKind::Original, || panic!("off")).unwrap(), "checked");
        // a failed clear refuses the flash
        assert!(marker_step(&on, ImageKind::Original, || Err("timeout".into())).is_err());
        // no valid state (a foreign word) or no 0x64: nothing to say
        assert_eq!(marker_step(&master(0x1234_5678), ImageKind::Original, || panic!("foreign")).unwrap(), "unchecked");
        assert_eq!(marker_step(&Master::No0x64, ImageKind::Original, || panic!("none")).unwrap(), "unchecked");
    }

    #[test]
    fn a_board_is_found_only_where_it_is_the_current_one() {
        use openbms_proto::SerialValue;
        let dir = std::env::temp_dir().join(format!("openbms-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = Store::open(&dir.join("maintenance.sqlite")).unwrap();
        let set = |v: &str| SerialValue::Set { value: v.into(), truncated: false };
        let (a, b) = (s.create_installation("A", None, vec![]).unwrap(), s.create_installation("B", None, vec![]).unwrap());
        s.board_seen(&a.id, 2, &set("SYN-X"), &SerialValue::NoAnswer, 10).unwrap();
        // moved to B, and another board took its place in A
        s.board_seen(&b.id, 5, &set("SYN-X"), &SerialValue::NoAnswer, 20).unwrap();
        s.board_seen(&a.id, 2, &set("SYN-Y"), &SerialValue::NoAnswer, 30).unwrap();
        assert_eq!(places_of(&s, "SYN-X").unwrap(), vec![(b.id.clone(), 5)]);
        assert_eq!(places_of(&s, "SYN-Y").unwrap(), vec![(a.id.clone(), 2)]);
        assert!(places_of(&s, "SYN-Z").unwrap().is_empty());
    }

    #[test]
    fn the_two_ports_must_differ() {
        assert!(distinct("serial:/dev/a", Some("serial:/dev/a")).is_err());
        assert!(distinct("serial:/dev/a", Some("serial:/dev/b")).is_ok());
        assert!(distinct("tcp:192.0.2.1:4196", None).is_ok());
    }
}
