//! How the packs reach their inverter (installation setting "wiring"), and from it the bus rule every request of the
//! app passes (`openbms_transport::gate`): one master per RS485 bus.
//!
//! - The inverter polls RS485-1/2 (UART0) or the CAN socket's RS485 pair (UART1): the app sends nothing there, unless
//!   the user said the inverter's communication is paused or cut for now (`paused`, this session only).
//! - RS485-1/2 always listens for one polling cycle before a connection's first request: a Seplos master polling its
//!   slaves (pack 0 with slaves, until its maintenance marker or DIP 5–8 halt it) or any other sender refuses there.
//! - An installation without the setting: nothing is sent, the UI asks for it (never taken for one without inverter).
//! - No installation („Nur ansehen“, „Platine flashen“): the user chose a free port; the connection listens briefly on
//!   that port before its first request, whichever bus it is.

use openbms_transport::{gate::Permit, Bus};
use serde::Deserialize;
use std::sync::Mutex;

/// Where the inverter is connected.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Inverter {
    /// none, or a computer that is the app itself
    None,
    /// CAN-H/L (pins 4/5 of the CAN socket); with several packs pack 0 polls its slaves on RS485-1/2
    Can,
    /// RS485-1/2: the inverter polls the packs itself
    Uart0,
    /// the CAN socket's RS485 pair: system values at pack 0 or every pack itself (the same bus taken either way; a
    /// master polling its slaves on RS485-1/2 is heard there by the first listen)
    #[serde(alias = "uart1-master", alias = "uart1-each")]
    Uart1,
}

impl Inverter {
    /// The RS485 bus the inverter polls.
    pub fn polls(self) -> Option<Bus> {
        match self {
            Inverter::Uart0 => Some(Bus::PackBus),
            Inverter::Uart1 => Some(Bus::CanSocketBus),
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

/// The rule in force and the installation it is of (None: „Nur ansehen“).
struct Rule {
    scope: Option<String>,
    wiring: Wiring,
}

static NOW: Mutex<Rule> = Mutex::new(Rule { scope: None, wiring: Wiring { inverter: None, paused: false } });
/// A change given while an action holds the bus (`held`): it takes effect once no action holds it (`settle`, at every
/// begin and end of an action), so the rule never changes between the frames of one action (a flash erased halfway, a
/// write without its read-back).
static PENDING: Mutex<Option<Rule>> = Mutex::new(None);
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

/// The bus rule in force for `bus`: an installation's wiring (`decide`), or without one only the brief listen.
pub fn permit(bus: Bus) -> Result<Permit, String> {
    settle();
    let now = NOW.lock().unwrap_or_else(|e| e.into_inner());
    match now.scope {
        Some(_) => decide(now.wiring, bus),
        None => Ok(Permit::ListenFirst),
    }
}

/// Take a waiting change unless an action holds the bus. Called where an action begins (before it holds the bus) and
/// ends, so a change never waits behind the next action.
pub fn settle() {
    if held() {
        return;
    }
    // the locks in one order everywhere: NOW, PENDING, BOUND. Taking a change and putting it in force is one step, so
    // a connect's `bind` comes either before (and drops it) or after (and replaces it).
    let mut now = NOW.lock().unwrap_or_else(|e| e.into_inner());
    let waiting = PENDING.lock().ok().and_then(|mut p| p.take());
    if let Some(r) = waiting {
        *now = r;
    }
}

/// Right after a connect for `installation` reserved the bus (so no other action holds the rule), before its first
/// request: the rule must be that installation's as stored now, not the one the UI showed before (its update may still
/// be on its way, and a change made meanwhile, e.g. while connected over MQTT, must count even for the same
/// installation). The inverter is always read from the store; only the session's pause is kept, and only from a rule of
/// this installation. Without an installation only the brief listen applies (`permit`). A connect refused for a busy
/// bus never gets here, so it never changes a running action's rule.
pub fn bind(state: &crate::AppState, installation: Option<&str>) -> Bound {
    let stored = installation
        .and_then(|id| crate::maint::with_store(state, |s| s.wiring(id)).ok().flatten())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| serde_json::from_value(v["inverter"].clone()).ok());
    let mut now = NOW.lock().unwrap_or_else(|e| e.into_inner());
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    *BOUND.lock().unwrap_or_else(|e| e.into_inner()) = Some(installation.map(String::from));
    rebind(&mut now, &mut pending, installation, stored);
    Bound(true)
}

/// The installation whose rule an RS485 connection bound (`bind`), from its connect until it ends (`unbind`): a change
/// for another installation arriving meanwhile (a late update of the one the UI showed before) is dropped, so it never
/// takes over once the port is free.
static BOUND: Mutex<Option<Option<String>>> = Mutex::new(None);

/// The connection whose rule was bound ended (disconnect, or another source): changes for any installation count again.
pub fn unbind() {
    *BOUND.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Held by a connect after `bind`: a connect that fails or is cancelled unbinds again when it is dropped; one that
/// worked `keep`s the binding until the connection ends.
pub struct Bound(bool);
impl Bound {
    pub fn keep(mut self) {
        self.0 = false;
    }
}
impl Drop for Bound {
    fn drop(&mut self) {
        if self.0 {
            unbind();
        }
    }
}

/// Whether a change for `scope` is taken while `bound` (the installation of a running RS485 connection, if any).
fn accepts(bound: Option<&Option<String>>, scope: Option<&str>) -> bool {
    bound.map_or(true, |b| b.as_deref() == scope)
}

/// `bind` without the store: the stored inverter, the pause of a waiting or current rule of the same installation. Every
/// waiting change is used up: one of this installation gave its pause, one of another (a late update of the
/// installation shown before) must not replace this connection's rule once the port is free.
fn rebind(now: &mut Rule, pending: &mut Option<Rule>, installation: Option<&str>, stored: Option<Inverter>) {
    let same = |r: &Rule| r.scope.as_deref() == installation;
    let paused = match pending.as_ref() {
        Some(p) if same(p) => p.wiring.paused,
        _ => same(now) && now.wiring.paused,
    };
    *pending = None;
    *now = Rule { scope: installation.map(String::from), wiring: Wiring { inverter: stored, paused } };
}

/// The rule of this app, once at start-up: an action holds the bus while a flash runs, the port is reserved or a pack
/// action runs.
pub fn install(state: &crate::AppState) {
    let (flashing, port, ops) = (state.flashing.clone(), state.port_busy.clone(), state.ops.clone());
    let _ = HELD.set(Box::new(move || {
        use std::sync::atomic::Ordering::SeqCst;
        flashing.load(SeqCst) || port.load(SeqCst) || crate::control::any_pack_action(&ops).is_some()
    }));
    openbms_transport::gate::install(Box::new(permit));
}

/// The wiring of the installation the UI shows (None: no installation, the brief listen only), and whether the user
/// said the inverter is paused. While an action holds the bus the change waits for its end (`PENDING`).
#[tauri::command]
pub fn set_wiring(installation: Option<String>, inverter: Option<Inverter>, paused: bool) -> Result<(), String> {
    {
        // checked and queued under the same locks, in `bind`'s order (PENDING, then BOUND): a connect binding meanwhile
        // either comes first (and this change of another installation is dropped) or clears it from PENDING after
        let mut pending = PENDING.lock().map_err(|e| e.to_string())?;
        let bound = BOUND.lock().map_err(|e| e.to_string())?;
        if !accepts(bound.as_ref(), installation.as_deref()) {
            return Ok(());
        }
        *pending = Some(Rule { scope: installation, wiring: Wiring { inverter, paused } });
    }
    settle();
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
        for (inv, busy) in [(Inverter::Uart0, Some(Bus::PackBus)), (Inverter::Uart1, Some(Bus::CanSocketBus)), (Inverter::Can, None), (Inverter::None, None)] {
            for bus in [Bus::PackBus, Bus::CanSocketBus] {
                let blocked = busy == Some(bus);
                assert_eq!(decide(w(inv, false), bus).is_err(), blocked, "{inv:?} {bus:?}");
                assert!(decide(w(inv, true), bus).is_ok(), "{inv:?} {bus:?} paused");
            }
        }
    }

    #[test]
    fn a_change_takes_effect_once_no_action_holds_the_bus() {
        // no action here (no hold installed): the change is the rule at once
        set_wiring(Some("A".into()), Some(Inverter::Uart1), false).unwrap();
        assert_eq!(NOW.lock().unwrap().wiring.inverter, Some(Inverter::Uart1));
        assert_eq!(NOW.lock().unwrap().scope.as_deref(), Some("A"));
        assert_eq!(permit(Bus::CanSocketBus), Err("bus-inverter".into()));
        // no installation: the brief listen on the chosen port, nothing else
        set_wiring(None, None, false).unwrap();
        assert_eq!((permit(Bus::CanSocketBus), permit(Bus::PackBus)), (Ok(Permit::ListenFirst), Ok(Permit::ListenFirst)));
        // the two earlier names of the CAN socket's RS485 pair read as that one value
        for old in ["uart1-master", "uart1-each"] {
            assert_eq!(serde_json::from_value::<Inverter>(serde_json::json!(old)).unwrap(), Inverter::Uart1);
        }
    }

    #[test]
    fn a_connect_binds_the_stored_inverter_even_for_the_same_installation() {
        let rule = |scope: Option<&str>, inverter, paused| Rule { scope: scope.map(String::from), wiring: Wiring { inverter, paused } };
        // same installation, changed meanwhile (e.g. over MQTT): the stored value counts, the session's pause stays
        let mut now = rule(Some("A"), Some(Inverter::Uart1), true);
        let mut pending = None;
        rebind(&mut now, &mut pending, Some("A"), Some(Inverter::Can));
        assert_eq!((now.scope.as_deref(), now.wiring.inverter, now.wiring.paused), (Some("A"), Some(Inverter::Can), true));
        // another installation: no pause carried over
        rebind(&mut now, &mut pending, Some("B"), Some(Inverter::Uart0));
        assert_eq!((now.scope.as_deref(), now.wiring.inverter, now.wiring.paused), (Some("B"), Some(Inverter::Uart0), false));
        // a waiting change of this installation gives its pause and is used up
        pending = Some(rule(Some("B"), Some(Inverter::Uart1), true));
        rebind(&mut now, &mut pending, Some("B"), Some(Inverter::Uart0));
        assert_eq!((now.wiring.inverter, now.wiring.paused, pending.is_none()), (Some(Inverter::Uart0), true, true));
        // one of another installation is dropped: freeing the port later never puts it in force, nor its pause
        now.wiring.paused = false;
        pending = Some(rule(Some("C"), Some(Inverter::Can), true));
        rebind(&mut now, &mut pending, Some("B"), Some(Inverter::Uart1));
        // what `settle` does once the port is free: take a waiting change (none is left)
        if let Some(r) = pending.take() {
            now = r;
        }
        assert_eq!((now.scope.as_deref(), now.wiring.inverter, now.wiring.paused), (Some("B"), Some(Inverter::Uart1), false));
        assert_eq!(decide(now.wiring, Bus::CanSocketBus), Err("bus-inverter".into()));
        let mut now = rule(Some("B"), Some(Inverter::Uart1), false);
        let mut pending = None;
        // no installation: nothing stored, no pause
        rebind(&mut now, &mut pending, None, None);
        assert_eq!((now.scope.as_deref(), now.wiring.inverter, now.wiring.paused), (None, None, false));
    }

    #[test]
    fn while_a_connection_is_bound_only_its_installation_may_change_the_rule() {
        let b = Some("B".to_string());
        // B's connection: a late change of C is dropped, B's own (e.g. the pause) is taken
        assert!(!accepts(Some(&b), Some("C")));
        assert!(!accepts(Some(&b), None));
        assert!(accepts(Some(&b), Some("B")));
        // a „Nur ansehen“ connection: no installation's change replaces its brief listen
        assert!(!accepts(Some(&None), Some("C")));
        assert!(accepts(Some(&None), None));
        // no connection bound (disconnected, failed connect, MQTT): any installation
        assert!(accepts(None, Some("C")));
    }

    #[test]
    fn rs485_1_2_always_listens_first_and_unset_sends_nothing() {
        assert_eq!(decide(w(Inverter::Can, false), Bus::PackBus), Ok(Permit::ListenFirst));
        assert_eq!(decide(w(Inverter::Uart0, true), Bus::PackBus), Ok(Permit::ListenFirst));
        assert_eq!(decide(w(Inverter::Can, false), Bus::CanSocketBus), Ok(Permit::Send));
        assert_eq!(decide(Wiring::default(), Bus::CanSocketBus), Err("wiring-unset".into()));
    }
}
