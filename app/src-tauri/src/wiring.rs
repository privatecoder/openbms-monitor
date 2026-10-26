//! How the packs reach their inverter (installation setting "wiring", for „Nur ansehen“ the session's choice), and
//! from it the bus rule every request of the app passes (`openbms_transport::gate`): one master per RS485 bus.
//!
//! - The inverter polls RS485-1/2 (UART0) or the CAN socket's RS485 pair (UART1): the app sends nothing there, unless
//!   the user said the inverter's communication is paused or cut for now (`paused`, this session only).
//! - RS485-1/2 always listens for one polling cycle before a connection's first request: a Seplos master polling its
//!   slaves (pack 0 with slaves, until its maintenance marker or DIP 5–8 halt it) or any other sender refuses there.
//! - Not set: nothing is sent, the UI asks first (an existing installation is never taken for one without inverter).

use openbms_transport::{gate::Permit, Bus};
use serde::Deserialize;
use std::sync::Mutex;

/// Where the inverter is connected.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Inverter {
    /// none, or a computer that is the app itself
    None,
    /// CAN (pins 4/5 of the CAN socket); with several packs pack 0 polls its slaves on RS485-1/2
    Can,
    /// RS485-1/2, the inverter polls every pack itself
    Uart0,
    /// the CAN socket's RS485 pair at pack 0, system values (Modbus, 0x60–0x63); with several packs pack 0 polls too
    Uart1Master,
    /// the CAN socket's RS485 pair, the inverter polls every pack itself
    Uart1Each,
}

impl Inverter {
    /// The RS485 bus the inverter polls.
    pub fn polls(self) -> Option<Bus> {
        match self {
            Inverter::Uart0 => Some(Bus::PackBus),
            Inverter::Uart1Master | Inverter::Uart1Each => Some(Bus::CanSocketBus),
            Inverter::None | Inverter::Can => None,
        }
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Wiring {
    pub inverter: Option<Inverter>,
    /// the user said the inverter's communication is paused or cut (this session only)
    pub paused: bool,
}

static NOW: Mutex<Wiring> = Mutex::new(Wiring { inverter: None, paused: false });
/// A change given while an action holds the bus (`held`): it takes effect once no action holds it, so the rule never
/// changes between the frames of one action (a flash erased halfway, a write without its read-back).
static PENDING: Mutex<Option<Wiring>> = Mutex::new(None);
static HELD: std::sync::OnceLock<Box<dyn Fn() -> bool + Send + Sync>> = std::sync::OnceLock::new();

fn held() -> bool {
    HELD.get().is_some_and(|h| h())
}

/// The bus rule for `bus` under `w`.
pub fn decide(w: Wiring, bus: Bus) -> Result<Permit, String> {
    let inverter = w.inverter.ok_or("wiring-unset")?;
    if inverter.polls() == Some(bus) && !w.paused {
        return Err("bus-inverter".into());
    }
    Ok(if bus == Bus::PackBus { Permit::ListenFirst } else { Permit::Send })
}

/// The rule in force: a waiting change once no action holds the bus.
pub fn now() -> Wiring {
    let mut now = NOW.lock().unwrap_or_else(|e| e.into_inner());
    if !held() {
        if let Some(w) = PENDING.lock().ok().and_then(|mut p| p.take()) {
            *now = w;
        }
    }
    *now
}

/// The rule of this app, once at start-up: an action holds the bus while a flash runs, the port is reserved or a pack
/// action runs.
pub fn install(state: &crate::AppState) {
    let (flashing, port, ops) = (state.flashing.clone(), state.port_busy.clone(), state.ops.clone());
    let _ = HELD.set(Box::new(move || {
        use std::sync::atomic::Ordering::SeqCst;
        flashing.load(SeqCst) || port.load(SeqCst) || crate::control::any_pack_action(&ops).is_some()
    }));
    openbms_transport::gate::install(Box::new(|bus| decide(now(), bus)));
}

/// The wiring of the installation the UI shows (or the session's choice), and whether the user said the inverter is
/// paused. While an action holds the bus the change waits for its end (`PENDING`).
#[tauri::command]
pub fn set_wiring(inverter: Option<Inverter>, paused: bool) -> Result<(), String> {
    *PENDING.lock().map_err(|e| e.to_string())? = Some(Wiring { inverter, paused });
    now();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(inverter: Inverter, paused: bool) -> Wiring {
        Wiring { inverter: Some(inverter), paused }
    }

    #[test]
    fn the_bus_the_inverter_polls_stays_silent_unless_paused() {
        for (inv, busy) in [(Inverter::Uart0, Some(Bus::PackBus)), (Inverter::Uart1Master, Some(Bus::CanSocketBus)), (Inverter::Uart1Each, Some(Bus::CanSocketBus)), (Inverter::Can, None), (Inverter::None, None)] {
            for bus in [Bus::PackBus, Bus::CanSocketBus] {
                let blocked = busy == Some(bus);
                assert_eq!(decide(w(inv, false), bus).is_err(), blocked, "{inv:?} {bus:?}");
                assert!(decide(w(inv, true), bus).is_ok(), "{inv:?} {bus:?} paused");
            }
        }
    }

    #[test]
    fn rs485_1_2_always_listens_first_and_unset_sends_nothing() {
        assert_eq!(decide(w(Inverter::Can, false), Bus::PackBus), Ok(Permit::ListenFirst));
        assert_eq!(decide(w(Inverter::Uart0, true), Bus::PackBus), Ok(Permit::ListenFirst));
        assert_eq!(decide(w(Inverter::Can, false), Bus::CanSocketBus), Ok(Permit::Send));
        assert_eq!(decide(Wiring::default(), Bus::CanSocketBus), Err("wiring-unset".into()));
    }
}
