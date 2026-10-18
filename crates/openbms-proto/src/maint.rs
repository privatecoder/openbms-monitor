//! CID2 0x64: identity and maintenance mode of a patched master firmware (16.06.13 `_MAINT`), CAN socket bus only.
//!
//! The patch keeps a persistent marker in flash. While it is set, the master treats its slave count as 0: it stops
//! polling the pack bus (RS485-1/2) and answers there itself, so its parameters can be read and it can be updated.
//! Original firmware answers 0x64 with RTN 04 (unknown command); slaves never answer 0x64.
//!
//! `INFO=03` reads the identity ([`MaintInfo`], no runtime state; newer builds answer it at every pack's own address, the
//! address only in the frame): format 2 with 17 bytes, or format 3 with 18 bytes and
//! an image flag byte (bit 0: the hard-stop patch is compiled in). `INFO=02` reads the state
//! ([`MaintState`], 5 bytes); start and stop answer with the same 5 bytes. The 5-byte layout holds only once a format-2
//! identity of a supported build was read on the same connection.

use crate::error::{Error, Result};
use crate::frame::encode_request;
use serde::Serialize;

/// Release value that guards start and stop against accidental frames (not an authentication).
const TOKEN: [u8; 2] = [0xC3, 0x5A];
const SIGNATURE: &[u8; 3] = b"MNT";

/// The raw marker word, as read from flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "word", rename_all = "kebab-case")]
pub enum MarkerWord {
    /// `FFFFFFFF`: erased, the master polls its slaves
    Off,
    /// `00000000`: set, the master polls no slaves
    On,
    /// anything else: content the patch did not write; never counts as maintenance on (start/stop refuse it)
    Foreign(u32),
}

impl MarkerWord {
    pub fn from_raw(raw: u32) -> Self {
        match raw {
            0xFFFF_FFFF => Self::Off,
            0 => Self::On,
            w => Self::Foreign(w),
        }
    }
}

/// Answer to `0x64 INFO=02`, and to a successful start or stop: the raw marker word and the slave count the master works
/// with (the DIP count, or 0 while the marker is set, from its next one-second cycle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MaintState {
    pub marker_word: u32,
    pub effective_slaves: u8,
}

impl MaintState {
    pub const LEN: usize = 5;

    /// Read the state (changes nothing). Only the master at address 0 answers.
    pub fn request() -> Result<Vec<u8>> {
        encode_request(0, 0x64, &[0x02])
    }

    /// Set (`true`) or clear (`false`) the marker. The reply may still show the slave count of the previous cycle; read
    /// the state again afterwards.
    pub fn set_request(on: bool) -> Result<Vec<u8>> {
        encode_request(0, 0x64, &[u8::from(on), TOKEN[0], TOKEN[1]])
    }

    pub fn parse(info: &[u8]) -> Result<Self> {
        match info {
            [a, b, c, d, slaves] => Ok(Self { marker_word: u32::from_le_bytes([*a, *b, *c, *d]), effective_slaves: *slaves }),
            _ => Err(Error::Layout("0x64 state answer must have 5 bytes")),
        }
    }

    pub fn marker(&self) -> MarkerWord {
        MarkerWord::from_raw(self.marker_word)
    }
}

/// Format-2 or format-3 answer to `0x64 INFO=03`: which build the master runs. No runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintInfo {
    pub format: u8,
    /// OEM base major, minor, revision (16.06.13 = `[0x10, 0x06, 0x0D]`; revision `FF`: unknown)
    pub base: [u8; 3],
    /// patch version major, minor
    pub patch: [u8; 2],
    /// identifies the build (not an authentication of the device)
    pub build_id: [u8; 8],
    /// format 3, image flag bit 0: the hard-stop patch is compiled in; None with format 2 (not reported)
    pub hard_stop: Option<bool>,
}

/// What an RTN 00 answer to `0x64 INFO=03` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoAnswer {
    /// the 17-byte format-2 or the 18-byte format-3 identity
    Known(MaintInfo),
    /// `MNT` with another format or length (e.g. the historic 22-byte format 1, or format 4): a patch the app does not
    /// know, never decoded
    OtherFormat,
    /// no `MNT` signature: not an identity
    Broken,
}

impl MaintInfo {
    /// format 2: 17 bytes; format 3: 18 bytes (the image flags after the build ID)
    pub const LEN: usize = 17;
    pub const LEN3: usize = 18;
    /// the revision byte of an unknown OEM revision
    pub const REVISION_UNKNOWN: u8 = 0xFF;

    /// Read the master's identity (changes nothing): `request_at(0)`.
    pub fn request() -> Result<Vec<u8>> {
        Self::request_at(0)
    }

    /// Read the identity of the pack at `address` (0–21): a build with addressed 64/03 answers at its own address; older
    /// slaves stay silent or answer an error.
    pub fn request_at(address: u8) -> Result<Vec<u8>> {
        if address > 21 {
            return Err(Error::Layout("0x64/03 address must be 0..=21"));
        }
        encode_request(address, 0x64, &[0x03])
    }

    /// Exactly the 17-byte format 2 or the 18-byte format 3.
    pub fn parse(info: &[u8]) -> Result<Self> {
        match Self::classify(info) {
            InfoAnswer::Known(i) => Ok(i),
            _ => Err(Error::Layout("0x64/03 answer is neither the 17-byte MNT format 2 nor the 18-byte format 3")),
        }
    }

    pub fn classify(info: &[u8]) -> InfoAnswer {
        if info.len() < 4 || &info[0..3] != SIGNATURE {
            return InfoAnswer::Broken;
        }
        let hard_stop = match (info[3], info.len()) {
            (2, Self::LEN) => None,
            // bits 1–7 are reserved
            (3, Self::LEN3) => Some(info[17] & 0x01 != 0),
            _ => return InfoAnswer::OtherFormat,
        };
        let mut build_id = [0u8; 8];
        build_id.copy_from_slice(&info[9..17]);
        InfoAnswer::Known(Self { format: info[3], base: [info[4], info[5], info[6]], patch: [info[7], info[8]], build_id, hard_stop })
    }

    /// The build ID as 16 lower-case hex digits (as the catalog lists it).
    pub fn build_id_hex(&self) -> String {
        self.build_id.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// "16.06.13", or "16.06.?" for an unknown revision
    pub fn base_text(&self) -> String {
        let [a, b, c] = self.base;
        if c == Self::REVISION_UNKNOWN { format!("{a}.{b:02}.?") } else { format!("{a}.{b:02}.{c:02}") }
    }

    /// "16.06.13 MAINT 1.5"
    pub fn version(&self) -> String {
        format!("{} MAINT {}.{}", self.base_text(), self.patch[0], self.patch[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 17 INFO bytes of the documented MAINT 1.5 identity.
    fn info15() -> Vec<u8> {
        let mut v = b"MNT".to_vec();
        v.extend_from_slice(&[0x02, 0x10, 0x06, 0x0D, 0x01, 0x05, 0x90, 0x9A, 0x60, 0x4E, 0x60, 0xE9, 0x22, 0x5B]);
        v
    }

    #[test]
    fn frames_match_the_patch_documentation() {
        assert_eq!(MaintState::request().unwrap(), b"~20004664E00202FD31\r");
        assert_eq!(MaintState::set_request(true).unwrap(), b"~20004664A00601C35AFC46\r");
        assert_eq!(MaintState::set_request(false).unwrap(), b"~20004664A00600C35AFC47\r");
        assert_eq!(MaintInfo::request().unwrap(), b"~20004664E00203FD30\r");
    }

    #[test]
    fn the_identity_has_17_bytes_format_2() {
        let i = MaintInfo::parse(&info15()).unwrap();
        assert_eq!((i.format, i.base, i.patch), (2, [16, 6, 13], [1, 5]));
        assert_eq!(i.build_id_hex(), "909a604e60e9225b");
        assert_eq!(i.version(), "16.06.13 MAINT 1.5");
        // through a frame: LENGTH C022 carries the 34 hex digits
        let f = crate::frame::parse(&crate::frame::encode_request(0, 0, &info15()).unwrap()).unwrap();
        assert_eq!(MaintInfo::classify(&f.info), InfoAnswer::Known(i));
    }

    #[test]
    fn other_layouts_are_never_read_as_format_2() {
        let good = info15();
        // the historic 22-byte format 1
        let mut f1 = b"MNT".to_vec();
        f1.extend_from_slice(&[1, 16, 6, 13, 1, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0xFE, 0x38, 0x19, 0x85, 0xBF, 0xFF, 0x8A, 0xD0]);
        assert_eq!(MaintInfo::classify(&f1), InfoAnswer::OtherFormat);
        assert_eq!(MaintInfo::classify(&good[..16]), InfoAnswer::OtherFormat);
        assert_eq!(MaintInfo::classify(&[good.clone(), vec![0]].concat()), InfoAnswer::OtherFormat);
        let mut f3 = good.clone();
        f3[3] = 3;
        assert_eq!(MaintInfo::classify(&f3), InfoAnswer::OtherFormat);
        let mut broken = good.clone();
        broken[0] = b'X';
        assert_eq!(MaintInfo::classify(&broken), InfoAnswer::Broken);
        assert_eq!(MaintInfo::classify(&[]), InfoAnswer::Broken);
        assert!(MaintInfo::parse(&f1).is_err());
    }

    /// The 18 INFO bytes of a format-3 identity (the documented example) with image flags `flags`.
    fn info3(revision: u8, flags: u8) -> Vec<u8> {
        let mut v = b"MNT".to_vec();
        v.extend_from_slice(&[0x03, 0x10, 0x06, revision, 0x01, 0x05, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, flags]);
        v
    }

    #[test]
    fn format_3_has_18_bytes_and_reports_the_hard_stop() {
        let on = MaintInfo::parse(&info3(0x0D, 0x01)).unwrap();
        assert_eq!((on.format, on.base, on.patch, on.hard_stop), (3, [16, 6, 13], [1, 5], Some(true)));
        assert_eq!(on.build_id_hex(), "0123456789abcdef");
        assert_eq!(on.version(), "16.06.13 MAINT 1.5");
        assert_eq!(MaintInfo::parse(&info3(0x0D, 0x00)).unwrap().hard_stop, Some(false));
        // reserved bits do not set it
        assert_eq!(MaintInfo::parse(&info3(0x0D, 0xFE)).unwrap().hard_stop, Some(false));
        // format 2 does not report it
        assert_eq!(MaintInfo::parse(&info15()).unwrap().hard_stop, None);
        // through a frame: LENGTH A024 carries the 36 hex digits
        let frame = crate::frame::encode_request(0, 0, &info3(0x0D, 0x01)).unwrap();
        assert_eq!(&frame[9..13], b"A024");
        assert_eq!(MaintInfo::classify(&crate::frame::parse(&frame).unwrap().info), InfoAnswer::Known(on));
    }

    #[test]
    fn an_unknown_oem_revision_shows_as_question_mark() {
        let i = MaintInfo::parse(&info3(0xFF, 0x00)).unwrap();
        assert_eq!((i.base_text(), i.version()), ("16.06.?".to_string(), "16.06.? MAINT 1.5".to_string()));
    }

    #[test]
    fn wrong_lengths_and_other_formats_are_rejected() {
        // format 3 with 17 bytes, format 2 with 18 bytes, format 4
        assert_eq!(MaintInfo::classify(&info3(0x0D, 1)[..17]), InfoAnswer::OtherFormat);
        assert_eq!(MaintInfo::classify(&[info15(), vec![1]].concat()), InfoAnswer::OtherFormat);
        let mut f4 = info3(0x0D, 1);
        f4[3] = 4;
        assert_eq!(MaintInfo::classify(&f4), InfoAnswer::OtherFormat);
        assert!(MaintInfo::parse(&f4).is_err());
    }

    #[test]
    fn the_identity_request_is_addressed() {
        let f = crate::frame::parse(&MaintInfo::request_at(11).unwrap()).unwrap();
        assert_eq!((f.address, f.cid1, f.rtn, f.info.as_slice()), (11, 0x46, 0x64, &[0x03][..]));
        assert_eq!(MaintInfo::request().unwrap(), MaintInfo::request_at(0).unwrap());
        assert!(MaintInfo::request_at(22).is_err());
    }

    #[test]
    fn the_state_has_5_bytes_with_the_marker_word_little_endian() {
        let off = MaintState::parse(&[0xFF, 0xFF, 0xFF, 0xFF, 11]).unwrap();
        assert_eq!((off.marker(), off.effective_slaves), (MarkerWord::Off, 11));
        let on = MaintState::parse(&[0, 0, 0, 0, 0]).unwrap();
        assert_eq!(on.marker(), MarkerWord::On);
        let foreign = MaintState::parse(&[0x78, 0x56, 0x34, 0x12, 0]).unwrap();
        assert_eq!(foreign.marker(), MarkerWord::Foreign(0x1234_5678));
        // the old two-byte status is not accepted
        assert!(MaintState::parse(&[1, 0]).is_err());
        assert!(MaintState::parse(&[0; 6]).is_err());
    }
}
