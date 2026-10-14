//! Calibration (CID2 0xA0) and the CAN inverter protocol (CID2 0xA6). Both exist only on RS485-1/2 and are stored
//! by the firmware at once.
//!
//! 0xA0 INFO: sub, reference u16/s16 BE. The firmware computes the new factor from the old one and the ratio
//! reference/measured, and refuses (RTN 06) when its own condition is not met; an unknown sub gives RTN 04. No
//! command reads the factors back: a success answer proves the value in RAM, not the stored one.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

pub const CID2_CALIBRATE: u8 = 0xA0;
pub const CID2_SET_CAN_PROTOCOL: u8 = 0xA6;

/// Protocols the app offers: 1 Pylontech/Deye/Luxpower/TBB … 6 Studer. 7 (MUST) is left out on purpose: 06.04/06.05
/// store it but run protocol 1, and a later update to ≥ 06.11 then switches the CAN speed; 0x51 cannot tell them apart.
pub const CAN_PROTOCOLS: std::ops::RangeInclusive<u8> = 1..=6;

/// Smallest current reference in 0.01 A. The firmware has no minimum and only checks |measured − reference| < 3.01 A,
/// so a tiny reference against a small current ruins the gain beyond one-step repair.
pub const MIN_CURRENT_CENTI_A: i16 = 1000;

/// One calibration step of 0xA0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Calibration {
    /// Sub 0: the momentary ADC bases become the zero point; a current flowing now stays as an offset.
    ZeroPoint,
    /// Sub 1: current gain; reference in 0.01 A, + = charging.
    Current { centi_amps: i16 },
    /// Sub 2: voltage; reference is the mean cell voltage in mV (pack voltage at P+/P− ÷ cells).
    Voltage { cell_mv: u16 },
}

impl Calibration {
    pub fn sub(self) -> u8 {
        match self {
            Calibration::ZeroPoint => 0,
            Calibration::Current { .. } => 1,
            Calibration::Voltage { .. } => 2,
        }
    }

    /// The 3 INFO bytes: sub, reference BE.
    pub fn to_bytes(self) -> [u8; 3] {
        let [hi, lo] = match self {
            Calibration::ZeroPoint => [0, 0],
            Calibration::Current { centi_amps } => centi_amps.to_be_bytes(),
            Calibration::Voltage { cell_mv } => cell_mv.to_be_bytes(),
        };
        [self.sub(), hi, lo]
    }
}

pub fn calibrate_request(address: u8, cal: Calibration) -> Result<Vec<u8>> {
    if let Calibration::Current { centi_amps } = cal {
        if centi_amps.unsigned_abs() < MIN_CURRENT_CENTI_A.unsigned_abs() {
            return Err(Error::Layout("current reference below 10 A"));
        }
    }
    crate::frame::encode_request(address, CID2_CALIBRATE, &cal.to_bytes())
}

pub fn set_can_protocol_request(address: u8, protocol: u8) -> Result<Vec<u8>> {
    if !CAN_PROTOCOLS.contains(&protocol) {
        return Err(Error::Layout("CAN protocol outside 1..=6"));
    }
    crate::frame::encode_request(address, CID2_SET_CAN_PROTOCOL, &[protocol])
}

/// Why the BMS did not take a write, from its RTN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// RTN 06: the firmware's condition is not met (deviation too large, cells too far apart, port at 0 V …).
    Refused,
    /// RTN 04: unknown command or value.
    Invalid,
}

impl Refusal {
    pub fn from_error(e: &Error) -> Option<Self> {
        match e {
            Error::Rtn(0x06) => Some(Refusal::Refused),
            Error::Rtn(0x04) => Some(Refusal::Invalid),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    #[test]
    fn can_protocol_frame_matches_the_documented_example() {
        assert_eq!(set_can_protocol_request(1, 1).unwrap(), b"~200146A6E00201FD24\r".to_vec());
        let f = frame::parse(&set_can_protocol_request(3, 6).unwrap()).unwrap();
        assert_eq!((f.address, f.rtn, f.info.as_slice()), (3, 0xA6, &[6][..]));
    }

    #[test]
    fn can_protocol_outside_one_to_six_is_not_sent() {
        for n in [0, 7, 8, 20, 255] {
            assert!(set_can_protocol_request(1, n).is_err(), "{n}");
        }
    }

    #[test]
    fn calibration_frames() {
        let zero = calibrate_request(1, Calibration::ZeroPoint).unwrap();
        assert_eq!(zero, b"~200146A0A006000000FC6B\r".to_vec());
        let f = frame::parse(&zero).unwrap();
        assert_eq!((f.rtn, f.info.as_slice()), (0xA0, &[0, 0, 0][..]));

        // -12.34 A discharging: s16 BE two's complement
        let f = frame::parse(&calibrate_request(2, Calibration::Current { centi_amps: -1234 }).unwrap()).unwrap();
        assert_eq!((f.address, f.info.as_slice()), (2, &[1, 0xFB, 0x2E][..]));
        let f = frame::parse(&calibrate_request(2, Calibration::Current { centi_amps: 2500 }).unwrap()).unwrap();
        assert_eq!(f.info, vec![1, 0x09, 0xC4]);

        // 3.301 V per cell
        let f = frame::parse(&calibrate_request(4, Calibration::Voltage { cell_mv: 3301 }).unwrap()).unwrap();
        assert_eq!(f.info, vec![2, 0x0C, 0xE5]);
    }

    #[test]
    fn current_reference_needs_ten_amps() {
        for c in [0, 1, 999, -999] {
            assert!(calibrate_request(1, Calibration::Current { centi_amps: c }).is_err(), "{c}");
        }
        assert!(calibrate_request(1, Calibration::Current { centi_amps: 1000 }).is_ok());
        assert!(calibrate_request(1, Calibration::Current { centi_amps: -1000 }).is_ok());
        assert!(calibrate_request(1, Calibration::Current { centi_amps: i16::MIN }).is_ok());
    }

    #[test]
    fn refusal_mapping() {
        assert_eq!(Refusal::from_error(&Error::Rtn(0x06)), Some(Refusal::Refused));
        assert_eq!(Refusal::from_error(&Error::Rtn(0x04)), Some(Refusal::Invalid));
        assert_eq!(Refusal::from_error(&Error::Rtn(0x02)), None);
        assert_eq!(Refusal::from_error(&Error::ModbusCrc), None);
    }

    #[test]
    fn calibration_serde_shape() {
        let c: Calibration = serde_json::from_str(r#"{"kind":"current","centi_amps":-1500}"#).unwrap();
        assert_eq!(c, Calibration::Current { centi_amps: -1500 });
        let v: Calibration = serde_json::from_str(r#"{"kind":"voltage","cell_mv":3301}"#).unwrap();
        assert_eq!(v, Calibration::Voltage { cell_mv: 3301 });
        assert_eq!(serde_json::from_str::<Calibration>(r#"{"kind":"zero_point"}"#).unwrap(), Calibration::ZeroPoint);
    }
}
