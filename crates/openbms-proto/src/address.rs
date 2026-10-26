//! Pack addresses, named once. A DIP address (switches 1–4) is 0..=15, 0 the master; scan, control and flash ask
//! exactly these. The CAN socket's addressed services (UART1, `uart1.rs`) take up to 21, the most slaves a master
//! counts by itself. Broadcast 0xFF is no pack address.

use std::ops::RangeInclusive;

/// The highest DIP address.
pub const DIP_MAX: u8 = 15;
/// Every DIP address, in order.
pub const DIP: RangeInclusive<u8> = 0..=DIP_MAX;
/// The highest address of the CAN socket's addressed services.
pub const SERVICE_MAX: u8 = 21;

/// A single pack by its DIP switches.
pub fn is_dip(address: u8) -> bool {
    address <= DIP_MAX
}
