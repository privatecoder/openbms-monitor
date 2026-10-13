//! Code hashes of unmodified vendor `.ehex` releases. Hashing decoded code permits harmless
//! CRLF/LF conversion while the strict parser still validates every record and the header.

use crate::{ehex::Image, Error, Result};

#[derive(Debug, PartialEq, Eq)]
pub struct Release {
    pub name: &'static str,
    pub code_sha256: &'static str,
    /// maintenance builds from 1.1 on: the build ID the master reports in `0x64/03` (16 hex digits)
    pub build_id: Option<&'static str>,
}

pub const RELEASES: &[Release] = &[
    Release {
        name: "16.06.03",
        code_sha256: "689d3819eb9f73e12473764094843acfd619a58e5ec0c7b1da76724dac364706",
        build_id: None,
    },
    Release {
        name: "16.06.04",
        code_sha256: "390299c85aae21b1d26194d304a85bd31f26b9c51f28e74bf145ba232d5e8c2c",
        build_id: None,
    },
    Release {
        name: "16.06.05 SP80",
        code_sha256: "a2abc282abf9d34412c3b5e359077d81762187a0c9d5113a9c56c33b02e328a2",
        build_id: None,
    },
    Release {
        name: "16.06.11",
        code_sha256: "b686b69e69878390fddf24f54cb2ab6569ce305c2c52b300e9a67b533fd99747",
        build_id: None,
    },
    Release {
        name: "16.06.12",
        code_sha256: "f487ac4bc3b89c5f9afde647aa1acd8c1e7b260e6b20a1a286123abe4cf607a2",
        build_id: None,
    },
    Release {
        name: "16.06.13",
        code_sha256: "5c36a7e08fb881b3c2ca6a22888ecb63585c3eb6811fb67b67cc9d0d0ecdde95",
        build_id: None,
    },
];

/// Maintenance builds: patched 16.06.13 with CID2 0x64 on the CAN socket bus. Modified firmware, so
/// accepted only like custom images; known here so the app can plan a flash of the master.
pub const MAINT_BUILDS: &[Release] = &[
    Release {
        name: "16.06.13 MAINT 1.1",
        code_sha256: "734a4a4c73abbf991b2616b50958aff6c05cdcca93ebf3781867a179194e38f1",
        build_id: Some("fe381985bfff8ad0"),
    },
    // answers 0x64/03 with RTN 05: it reports no build ID
    Release {
        name: "16.06.13 MAINT 1.0",
        code_sha256: "4d76a96fa7f7ad0e1aa3de1ac4fbe51f0fc4ae03a19aea2d4e696c2812d8f55d",
        build_id: None,
    },
];

/// The known maintenance build with this build ID (as `MaintInfo::build_id_hex` gives it).
pub fn maint_build(build_id: &str) -> Option<&'static Release> {
    MAINT_BUILDS.iter().find(|r| r.build_id == Some(build_id))
}

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
        assert_eq!((k, r.map(|r| r.name)), (ImageKind::Maintenance, Some("16.06.13 MAINT 1.1")));
        assert_eq!(kind_of_hash(&"0".repeat(64)), (ImageKind::Custom, None));
    }

    #[test]
    fn maint_builds_1_1_and_1_0() {
        let (k, r) = kind_of_hash("734a4a4c73abbf991b2616b50958aff6c05cdcca93ebf3781867a179194e38f1");
        assert_eq!((k, r.and_then(|r| r.build_id)), (ImageKind::Maintenance, Some("fe381985bfff8ad0")));
        assert_eq!(maint_build("fe381985bfff8ad0").map(|r| r.name), Some("16.06.13 MAINT 1.1"));
        assert_eq!(maint_build("0000000000000000"), None);
        // 1.0: renamed, same code hash as before
        let (k, r) = kind_of_hash("4d76a96fa7f7ad0e1aa3de1ac4fbe51f0fc4ae03a19aea2d4e696c2812d8f55d");
        assert_eq!((k, r.map(|r| r.name), r.and_then(|r| r.build_id)), (ImageKind::Maintenance, Some("16.06.13 MAINT 1.0"), None));
    }

    #[test]
    fn identify_still_accepts_only_vendor_releases() {
        for m in MAINT_BUILDS {
            assert!(RELEASES.iter().all(|v| v.code_sha256 != m.code_sha256));
        }
    }
}
