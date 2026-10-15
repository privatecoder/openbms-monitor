//! Pack control (CID2 0x45) and the remaining capacity (CID2 0x49 index 59). Like the calibration both exist only on
//! RS485-1/2 and always go to one pack: the app sends no broadcast (address 0xFF).
//!
//! 0x45 INFO: `00 <type> <param>`, answer empty with RTN 00, an unknown type gives RTN 04. Type 6 (restart) is never
//! answered: the board resets before it could send. 0x49 INFO: `00 <index> <u16 BE>`; index 59 sets the coulomb
//! counter (10 mAh) when the full capacity is at least the value, else RTN 06. RAM only until the BMS saves its state.

use crate::error::{Error, Result};
use serde::Serialize;

pub const CID2_CONTROL: u8 = 0x45;
pub const CID2_SET_VALUE: u8 = 0x49;

/// 0x45 types.
pub const TYPE_HEATER: u8 = 3;
pub const TYPE_ON_OFF: u8 = 4;
pub const TYPE_CLEAR: u8 = 5;
pub const TYPE_RESTART: u8 = 6;

/// Parameter of type 4: set or clear system status bit 5 („switched off“).
pub const ON: u8 = 0x10;
pub const OFF: u8 = 0x1F;
/// Parameter of type 3: 0x10 forces the heater for 3600 s, anything else stops it.
pub const HEATER_ON: u8 = 0x10;
pub const HEATER_OFF: u8 = 0x00;

/// 0x49 index of the remaining capacity (10 mAh).
pub const INDEX_REMAINING: u8 = 59;

fn control(address: u8, kind: u8, param: u8) -> Result<Vec<u8>> {
    one_pack(address)?;
    crate::frame::encode_request(address, CID2_CONTROL, &[0x00, kind, param])
}

fn one_pack(address: u8) -> Result<()> {
    if address > 15 {
        return Err(Error::Layout("pack control goes to one address 0..=15"));
    }
    Ok(())
}

/// Type 6: watchdog restart. Expect no answer.
pub fn restart_request(address: u8) -> Result<Vec<u8>> {
    control(address, TYPE_RESTART, 0x00)
}

/// Type 4: „on“ clears, „off“ sets system status bit 5; off blocks only the discharge MOSFET.
pub fn on_off_request(address: u8, on: bool) -> Result<Vec<u8>> {
    control(address, TYPE_ON_OFF, if on { ON } else { OFF })
}

/// Type 3: force the heater for one hour (skipping every heater condition), or stop it.
pub fn heater_request(address: u8, on: bool) -> Result<Vec<u8>> {
    control(address, TYPE_HEATER, if on { HEATER_ON } else { HEATER_OFF })
}

/// Type 5: history header and count, energy counters and cycle count to 0, stored at once. Not reversible.
pub fn clear_request(address: u8) -> Result<Vec<u8>> {
    control(address, TYPE_CLEAR, 0x00)
}

/// 0x49 index 59: the remaining capacity in 10 mAh.
pub fn set_remaining_request(address: u8, raw: u16) -> Result<Vec<u8>> {
    one_pack(address)?;
    let [hi, lo] = raw.to_be_bytes();
    crate::frame::encode_request(address, CID2_SET_VALUE, &[0x00, INDEX_REMAINING, hi, lo])
}

/// What 0x51 showed after a restart, the silence counted from sending the 0x45.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Comeback {
    /// At least one try went unanswered, then the pack answered again `after_ms` after the restart was sent.
    Back { after_ms: u64 },
    /// Every try was answered: no restart seen.
    NoInterruption,
    /// No answer up to the end of the watch.
    NoAnswer,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    #[test]
    fn control_frames() {
        assert_eq!(restart_request(0).unwrap(), b"~20004645A006000600FC6E\r".to_vec());
        assert_eq!(on_off_request(0, true).unwrap(), b"~20004645A006000410FC6F\r".to_vec());
        assert_eq!(on_off_request(0, false).unwrap(), b"~20004645A00600041FFC59\r".to_vec());
        assert_eq!(heater_request(1, true).unwrap(), b"~20014645A006000310FC6F\r".to_vec());
        assert_eq!(heater_request(1, false).unwrap(), b"~20014645A006000300FC70\r".to_vec());
        assert_eq!(clear_request(2).unwrap(), b"~20024645A006000500FC6D\r".to_vec());
    }

    #[test]
    fn remaining_capacity_frame() {
        // 280.00 Ah = 28000 × 10 mAh
        assert_eq!(set_remaining_request(3, 28000).unwrap(), b"~200346498008003B6D60FBDF\r".to_vec());
        let f = frame::parse(&set_remaining_request(5, 0).unwrap()).unwrap();
        assert_eq!((f.address, f.rtn, f.info.as_slice()), (5, 0x49, &[0, 59, 0, 0][..]));
    }

    #[test]
    fn no_broadcast() {
        for a in [16, 0xFF] {
            assert!(restart_request(a).is_err(), "{a}");
            assert!(on_off_request(a, false).is_err(), "{a}");
            assert!(set_remaining_request(a, 1).is_err(), "{a}");
        }
        assert!(clear_request(15).is_ok());
    }

    #[test]
    fn comeback_serde_shape() {
        assert_eq!(serde_json::to_string(&Comeback::Back { after_ms: 3200 }).unwrap(), r#"{"kind":"back","after_ms":3200}"#);
        assert_eq!(serde_json::to_string(&Comeback::NoInterruption).unwrap(), r#"{"kind":"no_interruption"}"#);
        assert_eq!(serde_json::to_string(&Comeback::NoAnswer).unwrap(), r#"{"kind":"no_answer"}"#);
    }
}
