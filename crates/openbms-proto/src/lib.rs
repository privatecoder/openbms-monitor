//! Protocol library for the Seplos BMS V2.0 (10C/10E, module EMU1101, SH Energy ETECH firmware).
//!
//! Protocol details: <https://github.com/privatecoder/seplos-emu1101-docs>.

pub mod access;
pub mod address;
pub mod calibration;
pub mod ccl;
pub mod control;
pub mod device_info;
pub mod error;
pub mod frame;
pub mod intra_pack;
pub mod maint;
pub mod memory;
pub mod modbus;
pub mod params;
pub mod remote;
mod reader;
pub mod serial;
pub mod status;
pub mod telemetry;
pub mod uart1;

pub use calibration::{Calibration, Refusal};
pub use ccl::CclStep;
pub use control::Comeback;
pub use device_info::DeviceInfo;
pub use error::{Error, Result};
pub use frame::Frame;
pub use intra_pack::IntraPackRecord;
pub use memory::{BmsTime, HistoryRecord};
pub use maint::{InfoAnswer, ExtInfo, MaintState, MarkerWord, MemoryDiagnosis};
pub use modbus::SystemValues;
pub use params::{Parameters, Values};
pub use remote::{CalPage, CalibrationPage, SocStore};
pub use serial::{SerialNumbers, SerialValue};
pub use status::Status;
pub use telemetry::Telemetry;
