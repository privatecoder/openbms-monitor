//! Serial numbers: CID2 0xA2 (battery serial number, "Battery_SN") and 0xA4 (BMS serial number, "BMS_SN").
//!
//! Both are persistent 30-byte ASCII strings, right-padded with spaces (older images: NUL). The two commands
//! exist only on RS485-1/2. On the CAN socket bus the master exposes the first 22 characters of the battery
//! serial number in Modbus input registers 0x0000.. (FC04, count exactly 21; firmware before 16.06.11 answers
//! with an exception).

use crate::error::{Error, Result};
use serde::Serialize;

/// CID2 that reads the battery serial number.
pub const CID2_BATTERY: u8 = 0xA2;
/// CID2 that reads the BMS (board) serial number.
pub const CID2_BMS: u8 = 0xA4;
/// Length of each stored string.
pub const LEN: usize = 30;
/// Untouched images carry this placeholder (followed by a date and a counter).
pub const PLACEHOLDER_PREFIX: &str = "EMU110110S-XXXX";

/// Modbus input registers of the master that hold the battery serial number (CAN socket bus).
pub const MODBUS_START: u16 = 0x0000;
/// The firmware accepts only this count (other counts: exception 03).
pub const MODBUS_COUNT: u16 = 21;
/// Registers 0..=10 hold two characters each; the rest are temperatures.
const MODBUS_SERIAL_REGS: usize = 11;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SerialValue {
    /// `truncated`: only the first 22 characters were readable (Modbus path) and more may follow.
    Set { value: String, truncated: bool },
    /// The factory placeholder; `value` is the raw text.
    Placeholder { value: String },
    Empty,
    /// The command is not supported or not reachable on this bus.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SerialNumbers {
    pub battery: SerialValue,
    pub bms: SerialValue,
}

impl SerialNumbers {
    pub const UNAVAILABLE: Self = Self { battery: SerialValue::Unavailable, bms: SerialValue::Unavailable };
}

pub fn battery_request(address: u8) -> Result<Vec<u8>> {
    crate::frame::encode_request(address, CID2_BATTERY, &[])
}

pub fn bms_request(address: u8) -> Result<Vec<u8>> {
    crate::frame::encode_request(address, CID2_BMS, &[])
}

/// Classify a stored text (padding is trimmed here).
pub fn classify(text: &str) -> SerialValue {
    let t = text.trim_matches(|c: char| c == '\0' || c == ' ');
    if t.is_empty() {
        SerialValue::Empty
    } else if t.starts_with(PLACEHOLDER_PREFIX) {
        SerialValue::Placeholder { value: t.to_string() }
    } else {
        SerialValue::Set { value: t.to_string(), truncated: false }
    }
}

/// INFO of a 0xA2/0xA4 answer: 30 bytes of ASCII.
pub fn parse_ascii_serial(info: &[u8]) -> Result<SerialValue> {
    if info.len() < LEN {
        return Err(Error::Layout("serial number answer shorter than 30 bytes"));
    }
    Ok(classify(&String::from_utf8_lossy(&info[..LEN])))
}

/// Battery serial number from the master's input registers 0x0000.. (high byte first).
pub fn battery_from_input_regs(regs: &[u16]) -> Result<SerialValue> {
    if regs.len() < MODBUS_SERIAL_REGS {
        return Err(Error::Layout("fewer than 11 serial number registers"));
    }
    let bytes: Vec<u8> = regs[..MODBUS_SERIAL_REGS].iter().flat_map(|r| r.to_be_bytes()).collect();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    // the 22nd character is not padding: the stored string may go on beyond what Modbus shows
    let cut = !matches!(bytes.last(), Some(0 | b' '));
    Ok(match classify(&text) {
        SerialValue::Set { value, .. } => SerialValue::Set { value, truncated: cut },
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    /// A response frame has the request layout with RTN in place of CID2.
    fn answer(text: &[u8]) -> Vec<u8> {
        frame::encode_request(0x00, 0x00, text).unwrap()
    }

    fn padded(s: &str, pad: u8) -> Vec<u8> {
        let mut b = s.as_bytes().to_vec();
        b.resize(LEN, pad);
        b
    }

    #[test]
    fn requests_have_empty_info() {
        assert_eq!(battery_request(0x02).unwrap(), b"~200246A20000FD9F\r".to_vec());
        assert!(bms_request(0x02).unwrap().starts_with(b"~200246A40000"));
    }

    #[test]
    fn parses_a_full_answer_frame() {
        let raw = answer(b"SYN-BAT-2000-01-01-0000000042X");
        assert!(raw.starts_with(b"~20004600103C"));
        let f = frame::parse(&raw).unwrap();
        assert_eq!(f.rtn, 0);
        assert_eq!(
            parse_ascii_serial(&f.info).unwrap(),
            SerialValue::Set { value: "SYN-BAT-2000-01-01-0000000042X".into(), truncated: false }
        );
    }

    #[test]
    fn space_and_nul_padding() {
        let want = SerialValue::Set { value: "SYN-0001".into(), truncated: false };
        assert_eq!(parse_ascii_serial(&padded("SYN-0001", b' ')).unwrap(), want);
        assert_eq!(parse_ascii_serial(&padded("SYN-0001", 0)).unwrap(), want);
    }

    #[test]
    fn placeholder_and_empty() {
        assert_eq!(
            parse_ascii_serial(&padded("EMU110110S-XXXX-20000101-00000", b' ')).unwrap(),
            SerialValue::Placeholder { value: "EMU110110S-XXXX-20000101-00000".into() }
        );
        assert_eq!(parse_ascii_serial(&[b' '; LEN]).unwrap(), SerialValue::Empty);
        assert_eq!(parse_ascii_serial(&[0; LEN]).unwrap(), SerialValue::Empty);
    }

    #[test]
    fn short_info_is_an_error() {
        assert!(parse_ascii_serial(&[b'A'; 29]).is_err());
        assert!(parse_ascii_serial(&[]).is_err());
    }

    fn regs(text: &[u8]) -> Vec<u16> {
        let mut b = text.to_vec();
        b.resize(22, b' ');
        let mut r: Vec<u16> = b.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        r.extend([250, 251, 252, 253, 254, 255, 256, 257, 258, 259]); // temperatures, ignored
        r
    }

    #[test]
    fn modbus_registers_high_byte_first() {
        let r = regs(b"SYN-0001");
        assert_eq!(r[0], 0x5359); // "SY"
        assert_eq!(battery_from_input_regs(&r).unwrap(), SerialValue::Set { value: "SYN-0001".into(), truncated: false });
    }

    #[test]
    fn modbus_full_22_characters_may_be_cut() {
        let r = regs(b"SYN-BAT-2000-01-01-000");
        assert_eq!(battery_from_input_regs(&r).unwrap(), SerialValue::Set { value: "SYN-BAT-2000-01-01-000".into(), truncated: true });
    }

    #[test]
    fn modbus_placeholder_empty_and_short() {
        assert_eq!(
            battery_from_input_regs(&regs(b"EMU110110S-XXXX-200001")).unwrap(),
            SerialValue::Placeholder { value: "EMU110110S-XXXX-200001".into() }
        );
        assert_eq!(battery_from_input_regs(&regs(b"")).unwrap(), SerialValue::Empty);
        assert!(battery_from_input_regs(&[0x5359; 10]).is_err());
    }

    #[test]
    fn classify_trims_and_detects() {
        assert_eq!(classify("  \0"), SerialValue::Empty);
        assert_eq!(classify("EMU110110S-XXXX"), SerialValue::Placeholder { value: "EMU110110S-XXXX".into() });
        assert_eq!(classify("EMU110110S-1234 "), SerialValue::Set { value: "EMU110110S-1234".into(), truncated: false });
    }

    #[test]
    fn serializes_with_a_kind_tag() {
        let v = serde_json::to_value(SerialNumbers {
            battery: SerialValue::Set { value: "A".into(), truncated: true },
            bms: SerialValue::Unavailable,
        })
        .unwrap();
        assert_eq!(v, serde_json::json!({ "battery": { "kind": "set", "value": "A", "truncated": true }, "bms": { "kind": "unavailable" } }));
    }
}
