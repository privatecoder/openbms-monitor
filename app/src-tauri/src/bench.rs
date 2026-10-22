//! Flashing one board outside an installation („Platine flashen“): a search on any RS485-1/2 port, each pack with its
//! String 2 and, where the build shows it there (MAINT 1.5.1/1.6.1), its maintenance marker. What the app knows about a
//! board in an installation (patch proof, hard-stop sighting) is found by its String 2, not by a place. An older MAINT
//! build shows its marker over the CAN socket only: „Steuern“ clears it there.

use crate::maint::{self, MaintError};
use crate::maint_store::Store;
use crate::marker::{rs485_kind, Found};
use crate::AppState;
use openbms_flash::known::ImageKind;
use openbms_flash::maint::dialog::{self, Action, Budgets};
use openbms_flash::maint::{bus, SystemClock};
use openbms_proto::{DeviceInfo, MarkerWord};
use openbms_transport::Connection;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

#[derive(Serialize, Clone, Debug)]
pub struct BenchPack {
    address: u8,
    info: DeviceInfo,
    /// String 2 as read now (None: not readable or not set)
    serial: Option<String>,
    /// the patched build's identity over RS485-1/2 (None: no 64/03 answer there, older builds and the original)
    version: Option<String>,
    /// "on" | "off" | "foreign" where the build shows its marker over RS485-1/2; None: not shown there
    marker: Option<&'static str>,
}

#[derive(Serialize)]
pub struct BenchScan {
    /// bytes heard while listening before the search; not 0: nothing was searched
    traffic: usize,
    packs: Vec<BenchPack>,
    /// stopped by „Abbrechen“: `packs` holds what answered until then
    cancelled: bool,
}

/// The board the user chose is the one flashed: with a String 2 shown at the search (`expected`), the fresh read must be
/// that one; an unreadable or other value refuses. Without one (none readable at the search) nothing is compared.
pub fn chosen_board(expected: Option<&str>, read: Option<&str>) -> Result<(), MaintError> {
    match expected {
        Some(e) if read != Some(e) => Err(maint::refused("bench-board-changed")),
        _ => Ok(()),
    }
}

/// The identity and marker of the pack at `address` over RS485-1/2: a build with image flag bit 4 shows its state,
/// anything else none (`trusted` false).
pub fn rs485_marker(link: &mut Connection, address: u8) -> Found {
    let clock = SystemClock::new();
    let budget = crate::profile::identity_budget(link);
    let (kind, info) = rs485_kind(dialog::probe_pack(link, &clock, address, budget).unwrap_or(dialog::PackAnswer::Undecided));
    if kind != "patched" {
        return Found { kind, info, state: None, trusted: false };
    }
    let state = dialog::dialog_at(link, &clock, address, Action::Status, Budgets::default()).ok();
    Found { kind, info, state, trusted: true }
}

/// One search on a board's RS485-1/2 port, without an installation: listen first (a polling master or other traffic:
/// nothing is searched), then 0x51 on 0–15 with String 2 and the RS485-1/2 marker each. A pending pack that answers as
/// the board flashed is cleared.
#[tauri::command]
pub async fn bench_scan(app: AppHandle, state: State<'_, AppState>, port: String) -> Result<BenchScan, MaintError> {
    let key = maint::pack_key(&port)?;
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
        return Ok(BenchScan { traffic: heard.bytes, packs: vec![], cancelled: false });
    }
    let found = link.scan(maint::scan_addresses(&state.scan_cancel)).map_err(|e| MaintError::Connection { message: e.to_string() })?;
    // „Abbrechen“ counts between every exchange: no String 2, marker or pending read after it
    let stop = || state.scan_cancel.load(Ordering::SeqCst);
    let mut packs = vec![];
    for (address, info) in found {
        let serial = if stop() { None } else { maint::string2(&mut link, address) };
        let f = if stop() { None } else { Some(rs485_marker(&mut link, address)) };
        let version = f.and_then(|f| f.info).map(|i| i.version());
        let marker = f.and_then(|f| f.state).map(|s| crate::maint::marker_name(s.marker()));
        packs.push(BenchPack { address, info, serial, version, marker });
    }
    if !stop() {
        let addresses: Vec<u8> = packs.iter().map(|p| p.address).collect();
        let pending = maint::with_store(&state, |s| s.pending_on(&key))?;
        let answered = maint::answered_with_serial(&pending, &addresses, &mut link);
        maint::with_store(&state, |s| s.clear_answered(&key, &answered, crate::history::now_ms()))?;
    }
    let cancelled = state.scan_cancel.swap(false, Ordering::SeqCst);
    Ok(BenchScan { traffic: 0, packs, cancelled })
}

/// What a flash outside an installation does with the board's marker, over RS485-1/2: "unchecked" (the build does not
/// show it there: an older MAINT build or the original), "checked" (off, or on for an image with the maintenance
/// functions, which starts halted then), "released" (on, cleared before an image without them: an original image
/// ignores the marker and would keep it hidden for a later patched one).
pub fn bench_marker(link: &mut Connection, address: u8, image: ImageKind) -> Result<&'static str, MaintError> {
    let f = rs485_marker(link, address);
    let clock = SystemClock::new();
    marker_step(&f, image, || dialog::dialog_at(link, &clock, address, Action::Clear, Budgets::default()).map(|s| s.effective_slaves).map_err(|e| e.to_string()))
}

/// `bench_marker` once the board is read: `clear` runs only for a trusted marker on before an image without the
/// maintenance functions and returns the slave count the board polls one DIP cycle later. A failed clear refuses the
/// flash; so does a master that polls slaves then (DIP 5–8 not OFF): the marker is gone, the bus would not be quiet.
pub fn marker_step(f: &Found, image: ImageKind, clear: impl FnOnce() -> Result<u8, String>) -> Result<&'static str, MaintError> {
    match f.state.map(|s| s.marker()) {
        Some(MarkerWord::On) if image != ImageKind::Maintenance => {
            if !f.may_clear() {
                return Err(maint::refused("release-master-first-unread"));
            }
            match clear().map_err(|message| MaintError::Connection { message })? {
                0 => Ok("released"),
                _ => Err(maint::refused("bench-dip-on")),
            }
        }
        Some(MarkerWord::On | MarkerWord::Off) if f.trusted => Ok("checked"),
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
    use openbms_proto::{MaintInfo, MaintState};

    fn found(word: Option<u32>, trusted: bool) -> Found {
        let info = MaintInfo { format: 4, base: [16, 6, 13], patch: [1, 6], patch_rev: 1, build_id: [1; 8], hard_stop: Some(false), power_rtc: true, pack_cutoff: true, uart0: true, reserved_flags: 0 };
        Found { kind: if trusted { "patched" } else { "no-uart0" }, info: Some(info), state: word.map(|w| MaintState { marker_word: w, effective_slaves: 0 }), trusted }
    }

    #[test]
    fn a_marker_on_is_cleared_before_an_image_without_maintenance_only() {
        let (on, off) = (found(Some(0), true), found(Some(u32::MAX), true));
        let mut cleared = 0;
        assert_eq!(marker_step(&on, ImageKind::Original, || { cleared += 1; Ok(0) }).unwrap(), "released");
        assert_eq!(marker_step(&on, ImageKind::Custom, || { cleared += 1; Ok(0) }).unwrap(), "released");
        assert_eq!(cleared, 2);
        // cleared, but DIP 5–8 not OFF: the master polls, the flash does not start
        assert!(matches!(marker_step(&on, ImageKind::Original, || Ok(1)), Err(MaintError::Refused { reason }) if reason == "bench-dip-on"));
        assert_eq!(marker_step(&on, ImageKind::Maintenance, || panic!("kept")).unwrap(), "checked");
        assert_eq!(marker_step(&off, ImageKind::Original, || panic!("off")).unwrap(), "checked");
        // a failed clear refuses the flash
        assert!(marker_step(&on, ImageKind::Original, || Err("timeout".into())).is_err());
        // a foreign word, no state, or a build that does not show it on RS485-1/2: nothing to say
        assert_eq!(marker_step(&found(Some(0x1234_5678), true), ImageKind::Original, || panic!("foreign")).unwrap(), "unchecked");
        assert_eq!(marker_step(&found(None, true), ImageKind::Original, || panic!("unread")).unwrap(), "unchecked");
        assert_eq!(marker_step(&found(None, false), ImageKind::Original, || panic!("older")).unwrap(), "unchecked");
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
    fn only_the_board_shown_at_the_search_is_flashed() {
        assert!(chosen_board(Some("SYN-A"), Some("SYN-A")).is_ok());
        assert!(chosen_board(Some("SYN-A"), Some("SYN-B")).is_err());
        assert!(chosen_board(Some("SYN-A"), None).is_err());
        assert!(chosen_board(None, Some("SYN-B")).is_ok());
        assert!(chosen_board(None, None).is_ok());
    }
}
