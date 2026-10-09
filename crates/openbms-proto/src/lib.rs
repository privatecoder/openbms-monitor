//! Protocol library for the Seplos BMS V2.0 (10C/10E, module EMU1101, SH Energy ETECH firmware).
//!
//! Protocol details: <https://github.com/privatecoder/seplos-emu1101-docs>.

pub mod device_info;
pub mod error;
pub mod frame;
pub mod intra_pack;
pub mod modbus;
pub mod params;
mod reader;
pub mod telemetry;

pub use device_info::DeviceInfo;
pub use error::{Error, Result};
pub use frame::Frame;
pub use intra_pack::IntraPackRecord;
pub use modbus::SystemValues;
pub use params::Parameters;
pub use telemetry::Telemetry;
