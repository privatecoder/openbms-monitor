//! Code hashes of unmodified vendor `.ehex` releases. Hashing decoded code permits harmless
//! CRLF/LF conversion while the strict parser still validates every record and the header.

use crate::{ehex::Image, Error, Result};

#[derive(Debug, PartialEq, Eq)]
pub struct Release {
    pub name: &'static str,
    pub code_sha256: &'static str,
}

pub const RELEASES: &[Release] = &[
    Release {
        name: "16.06.03",
        code_sha256: "689d3819eb9f73e12473764094843acfd619a58e5ec0c7b1da76724dac364706",
    },
    Release {
        name: "16.06.04",
        code_sha256: "390299c85aae21b1d26194d304a85bd31f26b9c51f28e74bf145ba232d5e8c2c",
    },
    Release {
        name: "16.06.05 SP80",
        code_sha256: "a2abc282abf9d34412c3b5e359077d81762187a0c9d5113a9c56c33b02e328a2",
    },
    Release {
        name: "16.06.11",
        code_sha256: "b686b69e69878390fddf24f54cb2ab6569ce305c2c52b300e9a67b533fd99747",
    },
    Release {
        name: "16.06.12",
        code_sha256: "f487ac4bc3b89c5f9afde647aa1acd8c1e7b260e6b20a1a286123abe4cf607a2",
    },
    Release {
        name: "16.06.13",
        code_sha256: "5c36a7e08fb881b3c2ca6a22888ecb63585c3eb6811fb67b67cc9d0d0ecdde95",
    },
];

/// Maintenance builds: patched 16.06.13 with CID2 0x64 on the CAN socket bus. Modified firmware, so
/// accepted only like custom images; known here so the app can plan a flash of the master.
pub const MAINT_BUILDS: &[Release] = &[Release {
    name: "16.06.13 MAINT",
    code_sha256: "4d76a96fa7f7ad0e1aa3de1ac4fbe51f0fc4ae03a19aea2d4e696c2812d8f55d",
}];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// unmodified vendor release (allowlist above)
    Original,
    /// a known maintenance build
    Maintenance,
    /// anything else that passes the format checks
    Custom,
}

pub fn kind_of_hash(code_sha256: &str) -> (ImageKind, Option<&'static Release>) {
    if let Some(r) = RELEASES.iter().find(|r| r.code_sha256 == code_sha256) {
        return (ImageKind::Original, Some(r));
    }
    if let Some(r) = MAINT_BUILDS.iter().find(|r| r.code_sha256 == code_sha256) {
        return (ImageKind::Maintenance, Some(r));
    }
    (ImageKind::Custom, None)
}

pub fn kind(image: &Image) -> (ImageKind, Option<&'static Release>) {
    kind_of_hash(&image.code_sha256)
}

pub fn identify(image: &Image) -> Result<&'static Release> {
    RELEASES
        .iter()
        .find(|r| r.code_sha256 == image.code_sha256)
        .ok_or_else(|| {
            Error::Image(format!(
                "unknown or unsupported vendor code SHA-256 {}",
                image.code_sha256
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_of_hash_tells_originals_maintenance_builds_and_custom_apart() {
        let (k, r) = kind_of_hash(RELEASES[0].code_sha256);
        assert_eq!((k, r.map(|r| r.name)), (ImageKind::Original, Some(RELEASES[0].name)));
        let (k, r) = kind_of_hash(MAINT_BUILDS[0].code_sha256);
        assert_eq!((k, r.map(|r| r.name)), (ImageKind::Maintenance, Some("16.06.13 MAINT")));
        assert_eq!(kind_of_hash(&"0".repeat(64)), (ImageKind::Custom, None));
    }

    #[test]
    fn identify_still_accepts_only_vendor_releases() {
        let (_, r) = kind_of_hash(MAINT_BUILDS[0].code_sha256);
        assert!(RELEASES.iter().all(|v| v.code_sha256 != r.unwrap().code_sha256));
    }
}
