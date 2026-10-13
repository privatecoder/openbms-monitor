//! CID2 0x64: maintenance mode of the patched master firmware (16.06.13 `_MAINT`), CAN socket bus only.
//!
//! The patch keeps a persistent marker in flash. While it is set, the master treats its slave count as 0:
//! it stops polling the pack bus (RS485-1/2) and answers there itself, so its parameters can be read and
//! it can be updated. Original firmware answers 0x64 with RTN 04 (unknown command).

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
}
