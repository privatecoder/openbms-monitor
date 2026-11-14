//! CID2 0x64 INFO 07/08: the CCL decrease step of EMU1101-EXT Full 1.6.7 and later (read-capability bit 4,
//! `maint::CAP_CCL_STEP`). A persistent numeric EXT setting outside the 87 OEM parameters: the downward step of the bank
//! charge-current limit in A per online pack per aggregate update (1–20, default 1); the upward step stays 1. Only the
//! active master's value acts on the bank.
//!
//! Read: INFO `07`. Set and persist: INFO `08 vv` with `vv` the binary value (10 → `080A`). Both answer RTN 00 with
//! exactly five bytes `[01, value, 01, 01, 14]`; a refusal is RTN 05 (invalid argument), 04 (write denied over the CAN
//! socket: Group0 bit 0 off) or 08 (persistence failed), each without INFO. Never part of 0x47/0xA1.

use crate::error::{Error, Result};
use crate::frame::encode_request;
use serde::Serialize;

const CID2: u8 = 0x64;
const READ: u8 = 0x07;
const SET: u8 = 0x08;
/// The answer's format byte.
const FORMAT: u8 = 0x01;

/// The bounds of the setting and the fixed upward step, as the firmware reports them.
pub const MIN: u8 = 1;
pub const MAX: u8 = 20;
pub const UP: u8 = 1;
/// The documented reset value (the original ramp); never a stand-in for a value that could not be read.
pub const DEFAULT: u8 = 1;

/// A successful answer to a read or a set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CclStep {
    /// the effective downward step, A per online pack per aggregate update
    pub down: u8,
    /// the fixed upward step (1)
    pub up: u8,
    pub min: u8,
    pub max: u8,
}

impl CclStep {
    pub const LEN: usize = 5;
}

/// A definite refusal: RTN 05, 04 or 08 with empty INFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Refusal {
    /// RTN 05: the firmware did not take the argument
    InvalidArgument,
    /// RTN 04: a write over the CAN socket with Group0 bit 0 off
    WriteDenied,
    /// RTN 08: storing or verifying the value failed (the prior value stays active)
    Persistence,
}

impl Refusal {
    pub fn of_rtn(rtn: u8) -> Option<Self> {
        match rtn {
            0x05 => Some(Self::InvalidArgument),
            0x04 => Some(Self::WriteDenied),
            0x08 => Some(Self::Persistence),
            _ => None,
        }
    }
}

fn check_address(address: u8) -> Result<()> {
    if address > 21 {
        return Err(Error::Layout("0x64 address must be 0..=21"));
    }
    Ok(())
}

/// The value as a step: an integer 1..=20, never rounded or clamped.
pub fn step(value: u8) -> Result<u8> {
    if (MIN..=MAX).contains(&value) {
        Ok(value)
    } else {
        Err(Error::Layout("the CCL decrease step must be 1..=20"))
    }
}

/// Read the effective step of the pack at `address` (changes nothing).
pub fn read_request(address: u8) -> Result<Vec<u8>> {
    check_address(address)?;
    encode_request(address, CID2, &[READ])
}

/// Set and persist `value` (1..=20) at `address`; an invalid value builds no frame.
pub fn set_request(address: u8, value: u8) -> Result<Vec<u8>> {
    check_address(address)?;
    encode_request(address, CID2, &[SET, step(value)?])
}

/// Exactly `[01, value, 01, 01, 14]` with `value` in 1..=20.
pub fn parse(info: &[u8]) -> Result<CclStep> {
    match *info {
        [FORMAT, down, UP, MIN, MAX] if (MIN..=MAX).contains(&down) => Ok(CclStep { down, up: UP, min: MIN, max: MAX }),
        _ => Err(Error::Layout("0x64/07 answer is not [01, 1..=20, 01, 01, 14]")),
    }
}

/// `parse` of a set's answer: it must report the value asked for.
pub fn parse_set(info: &[u8], value: u8) -> Result<CclStep> {
    let s = parse(info)?;
    if s.down != value {
        return Err(Error::Layout("0x64/08 answer reports another value than the one set"));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    #[test]
    fn the_set_frames_carry_the_value_as_one_binary_byte() {
        for (value, info) in [(1u8, "0801"), (10, "080A"), (20, "0814")] {
            let req = set_request(0, value).unwrap();
            let text = String::from_utf8(req.clone()).unwrap();
            // ~ 20 00 46 64 LENGTH INFO CHKSUM CR: two decoded INFO bytes, LENGTH C004
            assert_eq!(&text[1..13], "20004664C004", "{value}");
            assert_eq!(&text[13..17], info, "{value}");
            let f = frame::parse(&req).unwrap();
            assert_eq!((f.address, f.cid1, f.rtn, f.info.as_slice()), (0, 0x46, 0x64, [0x08, value].as_slice()));
        }
        assert_eq!(set_request(0, 1).unwrap(), b"~20004664C0040801FCCA\r");
        assert_eq!(set_request(0, 10).unwrap(), b"~20004664C004080AFCBA\r");
        assert_eq!(set_request(0, 20).unwrap(), b"~20004664C0040814FCC6\r");
        assert_eq!(read_request(0).unwrap(), b"~20004664E00207FD2C\r");
        // the decimal text and the hex of the decimal digits are not what goes out
        assert_ne!(&set_request(0, 10).unwrap()[13..17], b"0810");
    }

    #[test]
    fn invalid_values_and_addresses_build_no_frame() {
        for bad in [0u8, 21, 255] {
            assert!(set_request(0, bad).is_err(), "{bad}");
            assert!(step(bad).is_err(), "{bad}");
        }
        assert!(set_request(22, 10).is_err() && read_request(22).is_err());
        assert_eq!(step(1).unwrap(), 1);
        assert_eq!(step(20).unwrap(), 20);
    }

    #[test]
    fn only_the_exact_five_byte_answer_is_a_value() {
        assert_eq!(parse(&[1, 10, 1, 1, 20]).unwrap(), CclStep { down: 10, up: 1, min: 1, max: 20 });
        assert_eq!(parse(&[1, 1, 1, 1, 0x14]).unwrap().down, 1);
        for bad in [
            &[][..], &[1, 10, 1, 1][..], &[1, 10, 1, 1, 20, 0][..], &[2, 10, 1, 1, 20][..], &[1, 0, 1, 1, 20][..],
            &[1, 21, 1, 1, 20][..], &[1, 10, 2, 1, 20][..], &[1, 10, 1, 0, 20][..], &[1, 10, 1, 1, 21][..],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        // a set must report the value asked for
        assert_eq!(parse_set(&[1, 20, 1, 1, 20], 20).unwrap().down, 20);
        assert!(parse_set(&[1, 1, 1, 1, 20], 10).is_err());
        // through a frame: LENGTH 600A, 28 frame bytes
        let reply = crate::frame::encode_request(0, 0, &[1, 10, 1, 1, 20]).unwrap();
        assert_eq!((&reply[9..13], reply.len()), (&b"600A"[..], 28));
    }

    #[test]
    fn the_refusals_are_rtn_05_04_and_08() {
        assert_eq!(Refusal::of_rtn(5), Some(Refusal::InvalidArgument));
        assert_eq!(Refusal::of_rtn(4), Some(Refusal::WriteDenied));
        assert_eq!(Refusal::of_rtn(8), Some(Refusal::Persistence));
        assert_eq!(Refusal::of_rtn(2), None);
        assert_eq!(Refusal::of_rtn(0), None);
    }

    #[test]
    fn the_oem_block_has_no_room_for_it() {
        // 87 parameters and eight switch bytes; the ASCII A1 carries the 169-byte block, the binary A1 its 158 bytes
        assert_eq!((crate::params::DEFINITIONS.len(), crate::params::BLOCK_LEN, crate::uart1::WRITE_LEN), (87, 169, 158));
        let mut b = vec![0u8, 0x3C];
        b.extend([0u8; 120]);
        b.push(0x1B);
        b.extend([0u8; 27]);
        b.push(0x08);
        b.extend([0u8; 8]);
        b.extend(b"SYN       ");
        let ascii = crate::params::write_request(0, &b).unwrap();
        let binary = crate::uart1::write_request(0, &b).unwrap();
        // the same frames as before this setting existed: CID2 A1 with the whole block, binary A1 with its 158 bytes
        assert_eq!((&ascii[7..13], ascii.len()), (&b"A18152"[..], 18 + 2 * 169));
        assert_eq!((binary[2], binary[3], binary.len()), (0xA1, 158, 158 + 7));
        // nothing of the 64/08 frame rides in either
        assert!(!ascii.windows(4).any(|w| w == b"4664"));
    }
}
