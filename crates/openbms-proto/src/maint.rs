//! CID2 0x64: maintenance mode of the patched master firmware (16.06.13 `_MAINT`), CAN socket bus only.
//!
//! The patch keeps a persistent marker in flash. While it is set, the master treats its slave count as 0:
//! it stops polling the pack bus (RS485-1/2) and answers there itself, so its parameters can be read and
//! it can be updated. Original firmware answers 0x64 with RTN 04 (unknown command).
//!
//! MAINT 1.1 adds the read-only identity query `INFO=03` (22 bytes, [`MaintInfo`]); MAINT 1.0 answers it with
//! RTN 05 and keeps only the two-byte status `INFO=02`, which 1.1 still answers too.

use crate::error::{Error, Result};
use crate::frame::encode_request;
use serde::Serialize;

/// Release value that guards start and stop against accidental frames (not an authentication).
const TOKEN: [u8; 2] = [0xC3, 0x5A];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MaintStatus {
    /// Maintenance marker in flash is set.
    pub marker: bool,
    /// Slave count the master works with: the DIP count, or 0 while the marker is set
    /// (takes effect at the master's next one-second cycle).
    pub effective_slaves: u8,
}

impl MaintStatus {
    /// Read the state (changes nothing). Only the master at address 0 answers.
    pub fn request() -> Result<Vec<u8>> {
        encode_request(0, 0x64, &[0x02])
    }

    /// Set (`true`) or clear (`false`) the marker. The reply still shows the slave count of the previous
    /// cycle; read the state again about a second later.
    pub fn set_request(on: bool) -> Result<Vec<u8>> {
        encode_request(0, 0x64, &[u8::from(on), TOKEN[0], TOKEN[1]])
    }

    pub fn parse(info: &[u8]) -> Result<Self> {
        match info {
            [marker, slaves] => Ok(Self { marker: *marker != 0, effective_slaves: *slaves }),
            _ => Err(Error::Layout("0x64 answer must have 2 bytes")),
        }
    }
}

/// The raw marker word of MAINT 1.1, as read from flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "word", rename_all = "kebab-case")]
pub enum MarkerWord {
    /// `FFFFFFFF`: erased, the master polls its slaves
    Off,
    /// `00000000`: set, the master polls no slaves
    On,
    /// anything else: the sector holds content the patch did not write (start/stop refuse it)
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

/// Answer of MAINT 1.1 to `0x64 INFO=03` (identity and raw state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintInfo {
    /// base firmware major, minor, patch (16.06.13 = `[0x10, 0x06, 0x0D]`)
    pub base: [u8; 3],
    /// maintenance patch version major, minor
    pub patch: [u8; 2],
    /// raw marker word (little endian in the answer)
    pub marker_word: u32,
    /// slave count the master works with (0 while the marker is set, from its next one-second cycle)
    pub effective_slaves: u8,
    /// first 8 bytes of the SHA-256 over the hooked image with a fixed placeholder in these bytes (not an
    /// authentication of the device)
    pub build_id: [u8; 8],
}

impl MaintInfo {
    pub const LEN: usize = 22;
    const SIGNATURE: &'static [u8; 3] = b"MNT";
    const FORMAT: u8 = 1;

    /// Read the identity (changes nothing). Only the master at address 0 answers.
    pub fn request() -> Result<Vec<u8>> {
        encode_request(0, 0x64, &[0x03])
    }

    pub fn parse(info: &[u8]) -> Result<Self> {
        if info.len() != Self::LEN {
            return Err(Error::Layout("0x64/03 answer must have 22 bytes"));
        }
        if &info[0..3] != Self::SIGNATURE || info[3] != Self::FORMAT {
            return Err(Error::Layout("0x64/03 answer without MNT signature or with an unknown format"));
        }
        let mut build_id = [0u8; 8];
        build_id.copy_from_slice(&info[14..22]);
        Ok(Self {
            base: [info[4], info[5], info[6]],
            patch: [info[7], info[8]],
            marker_word: u32::from_le_bytes([info[9], info[10], info[11], info[12]]),
            effective_slaves: info[13],
            build_id,
        })
    }

    pub fn marker(&self) -> MarkerWord {
        MarkerWord::from_raw(self.marker_word)
    }

    /// The build ID as 16 lower-case hex digits (as the catalog lists it).
    pub fn build_id_hex(&self) -> String {
        self.build_id.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// "16.06.13 MAINT 1.1"
    pub fn version(&self) -> String {
        let [a, b, c] = self.base;
        format!("{a}.{b:02}.{c:02} MAINT {}.{}", self.patch[0], self.patch[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_match_the_patch_documentation() {
        assert_eq!(MaintStatus::request().unwrap(), b"~20004664E00202FD31\r");
        assert_eq!(MaintStatus::set_request(true).unwrap(), b"~20004664A00601C35AFC46\r");
        assert_eq!(MaintStatus::set_request(false).unwrap(), b"~20004664A00600C35AFC47\r");
    }

    #[test]
    fn parses_status() {
        assert_eq!(MaintStatus::parse(&[0, 11]).unwrap(), MaintStatus { marker: false, effective_slaves: 11 });
        assert_eq!(MaintStatus::parse(&[1, 0]).unwrap(), MaintStatus { marker: true, effective_slaves: 0 });
        assert!(MaintStatus::parse(&[1]).is_err());
    }

    fn info_of(frame: &[u8]) -> Vec<u8> {
        crate::frame::parse(frame).unwrap().info
    }

    #[test]
    fn identity_request_and_measured_answers() {
        assert_eq!(MaintInfo::request().unwrap(), b"~20004664E00203FD30\r");
        let off = MaintInfo::parse(&info_of(b"~20004600202C4D4E540110060D0101FFFFFFFF00FE381985BFFF8AD0F387")).unwrap();
        assert_eq!(off.base, [16, 6, 13]);
        assert_eq!(off.patch, [1, 1]);
        assert_eq!(off.marker(), MarkerWord::Off);
        assert_eq!(off.effective_slaves, 0);
        assert_eq!(off.build_id_hex(), "fe381985bfff8ad0");
        assert_eq!(off.version(), "16.06.13 MAINT 1.1");
        let on = MaintInfo::parse(&info_of(b"~20004600202C4D4E540110060D01010000000000FE381985BFFF8AD0F437")).unwrap();
        assert_eq!((on.marker(), on.effective_slaves), (MarkerWord::On, 0));
        // MAINT 1.0 answers 0x64/03 with RTN 05 and no INFO
        let f = crate::frame::parse(b"~200046050000FDAF").unwrap();
        assert_eq!((f.address, f.rtn, f.info.len()), (0, 5, 0));
    }

    #[test]
    fn identity_rejects_other_layouts() {
        let mut good = info_of(b"~20004600202C4D4E540110060D0101FFFFFFFF00FE381985BFFF8AD0F387");
        assert!(MaintInfo::parse(&good[..21]).is_err());
        assert!(MaintInfo::parse(&[good.clone(), vec![0]].concat()).is_err());
        good[3] = 2;
        assert!(MaintInfo::parse(&good).is_err(), "unknown format version");
        good[3] = 1;
        good[0] = b'X';
        assert!(MaintInfo::parse(&good).is_err(), "no MNT signature");
        assert_eq!(MarkerWord::from_raw(0x1234_5678), MarkerWord::Foreign(0x1234_5678));
    }
}
