//! Every connection the app opens: the endpoint with the bus's baud rate, the bus for the bus rule (`wiring.rs`) and
//! its name in a frame capture. One place, so no connection goes out without its bus.

use openbms_transport::{Bus, Connection, Endpoint};

/// A USB adapter on `bus`, at that bus's baud rate.
pub(crate) fn serial(path: impl Into<String>, bus: Bus) -> Endpoint {
    Endpoint::Serial { path: path.into(), baud: bus.baud() }
}

/// Open `endpoint` on `bus`, named in a capture by both.
pub(crate) fn open(endpoint: &Endpoint, bus: Bus) -> openbms_transport::Result<Connection> {
    let mut c = Connection::open_on(endpoint, bus)?;
    c.set_label(crate::capture::label(endpoint, bus));
    Ok(c)
}
