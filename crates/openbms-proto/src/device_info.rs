//! CID2 0x51: device name, firmware version, active CAN protocol.

use crate::error::{Error, Result};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceInfo {
    pub device_name: String,
    /// Major.minor as reported by the firmware, e.g. "16.06". The patch level is not reported.
    pub firmware_version: String,
    /// Name of the active CAN inverter protocol, e.g. "CAN:PNG_DYE_Luxp_TBB".
    pub can_protocol: String,
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim_matches(|c: char| c == '\0' || c == ' ').to_string()
}

impl DeviceInfo {
    pub fn request(address: u8) -> crate::Result<Vec<u8>> {
        crate::frame::encode_request(address, 0x51, &[])
    }

    pub fn parse(info: &[u8]) -> Result<Self> {
        if info.len() < 32 {
            return Err(Error::Layout("0x51 answer shorter than 32 bytes"));
        }
        Ok(Self {
            device_name: text(&info[0..10]),
            firmware_version: format!("{}.{:02}", info[10], info[11]),
            can_protocol: text(&info[12..32]),
        })
    }
}
