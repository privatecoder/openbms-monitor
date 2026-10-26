//! The one place every request passes before it leaves (`Connection::send`). An application installs one rule for the
//! whole process: per bus, whether it may send there at all, and whether it listens first. Only one master per RS485
//! bus: where an inverter polls, or a Seplos master polls its slaves, a request of ours would collide. Without a rule
//! (command-line tools, tests) every request goes out as before.

use crate::Bus;
use std::sync::OnceLock;

/// What the rule allows on a bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permit {
    Send,
    /// Listen for one polling cycle before the connection's first request (`timing::BUS_LISTEN`): any byte heard means
    /// another master on the bus, and nothing is sent.
    ListenFirst,
}

/// A refusal is a short key the application explains (e.g. "bus-inverter").
pub type Rule = dyn Fn(Bus) -> Result<Permit, String> + Send + Sync;

static RULE: OnceLock<Box<Rule>> = OnceLock::new();

/// Set the rule once, at start-up; a second call is ignored.
pub fn install(rule: Box<Rule>) {
    let _ = RULE.set(rule);
}

/// The rule's answer for a connection on `bus`. With a rule installed, a connection that never said its bus
/// (`Connection::set_bus`) sends nothing.
pub fn permit(bus: Option<Bus>) -> Result<Permit, String> {
    let Some(rule) = RULE.get() else { return Ok(Permit::Send) };
    rule(bus.ok_or_else(|| "bus-unknown".to_string())?)
}
