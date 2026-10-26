//! Code hashes of unmodified vendor `.ehex` releases. Hashing decoded code permits harmless
//! CRLF/LF conversion while the strict parser still validates every record and the header.

use crate::{ehex::Image, Error, Result};
use openbms_proto::MaintInfo;

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

/// What a patched master or pack offers. Every supported build (`MaintInfo::supported`) has all of these; functions
/// that differ between builds come from the image flags. The app asks these, never a version number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// layout of the `0x64/03` identity (3: 18 bytes with image flags; 4: 19 bytes with a third patch byte; no runtime
    /// state)
    pub mnt_format: u8,
    /// `0x64/02` and start/stop answer with the 5-byte state (marker word, effective slaves)
    pub status_split: bool,
    /// binary 0x47/0xA1 on the CAN socket
    pub uart1_params: bool,
    /// binary String 1/2 (0xA2/0xA4) on the CAN socket
    pub uart1_identity: bool,
    /// Group0 bit 0 allows a binary 0xA1 (E3 when off)
    pub write_gate_bit0: bool,
    /// Group0 bit 2 allows a maintenance start; a binary 0xA1 must not change it (E3)
    pub start_gate_bit2: bool,
}

/// What every pack with the patch offers at its own address: the binary parameter and identity service and the local
/// write release (Group0 bit 0). The Group0 bit 2 a pack stores is a master setting: it acts only at address 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackCaps {
    pub uart1_params: bool,
    pub uart1_identity: bool,
    pub write_gate_bit0: bool,
    /// a binary 0xA1 must not change Group0 bit 2 (E3), whether or not the pack is the master
    pub keeps_start_bit2: bool,
}

/// What only the master (address 0) offers: the 5-byte `0x64/02` state, start/stop, and the start release Group0 bit 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasterCaps {
    pub status_split: bool,
    pub start_gate_bit2: bool,
}

impl Caps {
    pub fn pack(&self) -> PackCaps {
        PackCaps { uart1_params: self.uart1_params, uart1_identity: self.uart1_identity, write_gate_bit0: self.write_gate_bit0, keeps_start_bit2: self.start_gate_bit2 }
    }

    pub fn master(&self) -> MasterCaps {
        MasterCaps { status_split: self.status_split, start_gate_bit2: self.start_gate_bit2 }
    }
}

/// The pack capabilities of a `0x64/03` identity read at any address (None: not a supported build).
pub fn pack_caps_of(info: &MaintInfo) -> Option<PackCaps> {
    caps_of(info).map(|c| c.pack())
}

/// The lowest patch version the app knows functions of (`openbms_proto::access::MAINT_MIN`); which builds are supported
/// decides `MaintInfo::supported`.
pub const MIN_PATCH: [u8; 2] = openbms_proto::access::MAINT_MIN;

/// From this patch version on, image flag bit 2 announces the stored pack cutoff (Group0 bit 4 extended off, bit 5 the
/// lock state). Below it the bits are not read that way, whatever the flag says.
pub const PACK_CUTOFF_MIN: [u8; 2] = openbms_proto::access::CUTOFF_MIN;

/// The stored pack cutoff of an identity with patch `patch` and image flag bit 2 `flag`: both needed.
pub use openbms_proto::access::pack_cutoff_of;

/// The `0x64/03` formats the app decodes; all carry the same identity fields (format 3 adds the image flags, format 4 a
/// third patch byte). Only format 4 can be supported (TEMP(maint-1.6): and the first 1.6 in format 3, `maint::temp16`);
/// formats 2 and 3 stay decoded after it, so older patch builds show as unsupported instead of unknown.
pub const MNT_FORMATS: [u8; 3] = [2, 3, 4];

/// The capabilities of a supported `0x64/03` identity (`MaintInfo::supported`): every supported build has them all;
/// None for another or an older build (its patch functions are not used).
pub fn caps_of(info: &MaintInfo) -> Option<Caps> {
    info.supported().then_some(PROOF_CAPS).map(|c| Caps { mnt_format: info.format, ..c })
}

/// The capabilities a stored proof stands for (a supported build; it carries no format or image flags).
pub const PROOF_CAPS: Caps = Caps { mnt_format: 4, status_split: true, uart1_params: true, uart1_identity: true, write_gate_bit0: true, start_gate_bit2: true };

/// A listed patched build: modified firmware, accepted only like custom images; listed so a flash image is recognised
/// (code hash) and the master's build shown by name. Capabilities come from its identity (`caps_of`), not from the
/// listing.
#[derive(Debug, PartialEq, Eq)]
pub struct PatchBuild {
    /// shown name, also the release a flash records
    pub name: &'static str,
    /// base firmware major, minor, patch as `0x64/03` reports it
    pub base: [u8; 3],
    /// patch version major, minor and (format 4) revision as `0x64/03` reports it
    pub patch: [u8; 2],
    pub patch_rev: u8,
    /// the `0x64/03` format of this build (builds are told apart by format and build ID)
    pub mnt_format: u8,
    /// the image flag byte of its `0x64/03`
    pub flags: u8,
    /// the build ID of `0x64/03`, 16 lower-case hex digits
    pub build_id: &'static str,
    /// SHA-256 of the decoded application code (`ehex::Image::code_sha256`)
    pub code_sha256: &'static str,
    /// SHA-256 of the `.ehex` file as published (reference only; recognition uses the code hash)
    pub ehex_sha256: &'static str,
}

impl PatchBuild {
    /// The `0x64/03` identity this build answers.
    pub fn info(&self) -> MaintInfo {
        let mut v = b"MNT".to_vec();
        v.push(self.mnt_format);
        v.extend_from_slice(&self.base);
        v.extend_from_slice(&self.patch);
        if self.mnt_format == 4 {
            v.push(self.patch_rev);
        }
        v.extend((0..8).map(|i| u8::from_str_radix(&self.build_id[2 * i..2 * i + 2], 16).unwrap_or(0)));
        if self.mnt_format != 2 {
            v.push(self.flags);
        }
        MaintInfo::parse(&v).expect("a listed build has a valid identity")
    }

    pub fn caps(&self) -> Option<Caps> {
        caps_of(&self.info())
    }

    /// Image flag bit 1: power, restart and clock over the CAN socket.
    pub fn power_rtc(&self) -> bool {
        self.info().power_rtc
    }

    /// The stored pack cutoff (`pack_cutoff_of`).
    pub fn pack_cutoff(&self) -> bool {
        pack_cutoff_of(self.patch, self.info().pack_cutoff)
    }
}

/// The listed patched builds (the supported ones). A later build: append one entry.
pub const PATCH_BUILDS: &[PatchBuild] = &[
    // MAINT 1.5.3 DIAGNOSTICS_POSITIVE, MNT format 4 with image flags 3A: power and clock (bit 3 at 1 allows, as 1.6.3),
    // RS485-1/2 marker and identity, the 64/04 diagnosis; fixed soft stop (56016 app bytes, 220 update blocks)
    PatchBuild {
        name: "16.06.13 MAINT 1.5.3",
        base: [16, 6, 13],
        patch: [1, 5],
        patch_rev: 3,
        mnt_format: 4,
        flags: 0x3A,
        build_id: "2b89286ed6c14604",
        code_sha256: "27be05dc5f9ab0f648861e2d8e9feaffc4685ce98d59c02cccb7a3d114a38cd0",
        ehex_sha256: "dee768334ad4227999b41efd0c4232c0b5c9f323c973cd392b5651a1b5f29325",
    },
    // MAINT 1.6.3 CUTOFF_POSITIVE, MNT format 4 with image flags FE: positive bits 3/4, stored pack cutoff, diagnosis,
    // SOC checkpoint before a planned restart, configurable hard stop with soft as default (57948 app bytes, 228 blocks)
    PatchBuild {
        name: "16.06.13 MAINT 1.6.3",
        base: [16, 6, 13],
        patch: [1, 6],
        patch_rev: 3,
        mnt_format: 4,
        flags: 0xFE,
        build_id: "baa01ecd5af4eb28",
        code_sha256: "fe04ff9a3263ee0fea0340a71d741c770c4056f10ceb534546fcb9b0181cf33f",
        ehex_sha256: "99ea2cc5496358ec127300b55a4ca718acb1c4f6ee16f13edbe346f7aa4db062",
    },
    // TEMP(maint-1.6): the first MAINT 1.6 (V1_6_CUTOFF), MNT format 3 with image flags 06: power and clock (bit 3 at 0
    // allows, as 1.5.2), the stored pack cutoff; hard stop off. Supported until the installation runs 1.6.3, then remove
    // this entry.
    PatchBuild {
        name: "16.06.13 MAINT 1.6",
        base: [16, 6, 13],
        patch: openbms_proto::maint::temp16::PATCH,
        patch_rev: 0,
        mnt_format: openbms_proto::maint::temp16::FORMAT,
        flags: openbms_proto::maint::temp16::FLAGS,
        build_id: openbms_proto::maint::temp16::BUILD,
        code_sha256: "be2ecc7549c7ee16d46eed1f10c256d7231cc5ed961ec90f0bae87e0cfc40d2d",
        ehex_sha256: "32f27f3f128a591759bd2e205579b2dfdccf50c3717e50c7092c1e30f44d146f",
    },
];

/// The entry `0x64/03` describes in `table`: format, base, patch and build ID all match.
pub fn patch_in<'a>(table: &'a [PatchBuild], format: u8, base: [u8; 3], patch: [u8; 2], build_id: &str) -> Option<&'a PatchBuild> {
    table.iter().find(|b| b.mnt_format == format && b.base == base && b.patch == patch && b.build_id == build_id)
}

/// `patch_in` over the listed builds.
pub fn patch_build(format: u8, base: [u8; 3], patch: [u8; 2], build_id: &str) -> Option<&'static PatchBuild> {
    patch_in(PATCH_BUILDS, format, base, patch, build_id)
}

/// The listed build whose code this is.
pub fn patch_by_code(code_sha256: &str) -> Option<&'static PatchBuild> {
    PATCH_BUILDS.iter().find(|b| b.code_sha256 == code_sha256)
}

/// A flash image that gives the pack the patch functions (a listed build with capabilities): its name.
pub fn patch_image(code_sha256: &str) -> Option<&'static str> {
    patch_by_code(code_sha256).filter(|b| b.caps().is_some()).map(|b| b.name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// unmodified vendor release (allowlist above)
    Original,
    /// a known patched build (`PATCH_BUILDS`)
    Maintenance,
    /// anything else that passes the format checks
    Custom,
}

/// The kind of an image and its name (a vendor release or a patched build).
pub fn kind_of_hash(code_sha256: &str) -> (ImageKind, Option<&'static str>) {
    if let Some(r) = RELEASES.iter().find(|r| r.code_sha256 == code_sha256) {
        return (ImageKind::Original, Some(r.name));
    }
    if let Some(b) = patch_by_code(code_sha256) {
        return (ImageKind::Maintenance, Some(b.name));
    }
    (ImageKind::Custom, None)
}

pub fn kind(image: &Image) -> (ImageKind, Option<&'static str>) {
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
    fn kind_of_hash_tells_originals_patched_builds_and_custom_apart() {
        assert_eq!(kind_of_hash(RELEASES[0].code_sha256), (ImageKind::Original, Some(RELEASES[0].name)));
        assert_eq!(kind_of_hash("fe04ff9a3263ee0fea0340a71d741c770c4056f10ceb534546fcb9b0181cf33f"), (ImageKind::Maintenance, Some("16.06.13 MAINT 1.6.3")));
        // TEMP(maint-1.6)
        assert_eq!(kind_of_hash("be2ecc7549c7ee16d46eed1f10c256d7231cc5ed961ec90f0bae87e0cfc40d2d"), (ImageKind::Maintenance, Some("16.06.13 MAINT 1.6")));
        assert_eq!(kind_of_hash(&"0".repeat(64)), (ImageKind::Custom, None));
        // older patch builds (1.5, 1.5.1, 1.5.2, the positive 1.6, 1.6.1) are custom images now
        for old in ["5cd0f08fdbed8adce2a340b8ca842a02f4f2de61eee3f6bdb5b268fa5ec78c12", "58f45b854822d1f06eabb663fb2cd8f55d41485dd003eb6d52c2585477d0ca7a",
            "619cdf77811e05b085d37e997b3c469f35e83ddd5dd351fcf2ab6fd63b3a0437", "010ac5446b644a739611ccaffda85542365dbfccbe5753ede1c1e6b2bc776ae8",
            "fec91208a5553726bb5c4da905dc5fadb0955011eb6f337b05b67ffefe62437f", "9c1d8a912222d6f8ec9329a17b332776803f5d2a955a6028ae17e3cf0f1d1e1f"] {
            assert_eq!(kind_of_hash(old), (ImageKind::Custom, None));
        }
    }

    #[test]
    fn the_listed_builds_are_recognised_by_their_exact_identity_and_carry_their_flags() {
        let b = patch_build(4, [16, 6, 13], [1, 6], "baa01ecd5af4eb28").unwrap();
        assert_eq!((b.name, b.info().patch_text(), b.info().image_flags()), ("16.06.13 MAINT 1.6.3", "1.6.3".to_string(), 0xFE));
        assert_eq!((b.power_rtc(), b.pack_cutoff(), b.info().checkpoint(), b.info().hard_stop_configurable()), (true, true, true, true));
        let a = patch_build(4, [16, 6, 13], [1, 5], "2b89286ed6c14604").unwrap();
        assert_eq!((a.name, a.info().patch_text(), a.info().image_flags()), ("16.06.13 MAINT 1.5.3", "1.5.3".to_string(), 0x3A));
        assert_eq!((a.power_rtc(), a.pack_cutoff(), a.info().positive(), a.info().diagnostics(), a.info().checkpoint(), a.info().hard_stop), (true, false, true, true, false, Some(false)));
        assert_eq!(kind_of_hash("27be05dc5f9ab0f648861e2d8e9feaffc4685ce98d59c02cccb7a3d114a38cd0"), (ImageKind::Maintenance, Some("16.06.13 MAINT 1.5.3")));
        // 1.5.2 is no longer listed
        assert!(patch_build(4, [16, 6, 13], [1, 5], "89d85c61b40e03da").is_none());
        assert!(patch_build(3, [16, 6, 13], [1, 6], "baa01ecd5af4eb28").is_none(), "other format");
        assert!(patch_build(4, [16, 6, 12], [1, 6], "baa01ecd5af4eb28").is_none(), "other base");
        assert!(patch_build(4, [16, 6, 13], [1, 6], "0000000000000000").is_none(), "other build");
        assert_eq!(patch_image(b.code_sha256), Some("16.06.13 MAINT 1.6.3"));
        // every listed build is supported
        assert!(PATCH_BUILDS.iter().all(|b| b.caps().is_some()));
    }

    #[test]
    fn capabilities_come_with_a_supported_build_only() {
        let info = |format: u8, patch: [u8; 2], patch_rev: u8, flags: u8| MaintInfo::parse(&{
            let mut v = b"MNT".to_vec();
            v.extend_from_slice(&[format, 16, 6, 13, patch[0], patch[1]]);
            if format == 4 {
                v.push(patch_rev);
            }
            v.extend_from_slice(&[1; 8]);
            if format > 2 {
                v.push(flags);
            }
            v
        }).unwrap();
        assert_eq!(caps_of(&info(4, [1, 5], 3, 0x3A)), Some(PROOF_CAPS), "1.5.3, not listed yet");
        assert_eq!(caps_of(&info(4, [1, 5], 2, 0x32)), None, "1.5.2");
        assert_eq!(caps_of(&info(4, [1, 6], 3, 0xFE)), Some(PROOF_CAPS));
        assert_eq!(caps_of(&info(4, [1, 7], 0, 0xFE)), Some(PROOF_CAPS), "a later version");
        assert_eq!(caps_of(&info(4, [1, 5], 1, 0x12)), None, "1.5.1");
        assert_eq!(caps_of(&info(4, [1, 6], 2, 0x7E)), None, "1.6.2");
        assert_eq!(caps_of(&info(3, [1, 5], 0, 0x02)), None, "1.5");
        assert_eq!(caps_of(&info(2, [1, 5], 0, 0)), None, "1.5 format 2");
        // TEMP(maint-1.6): the first 1.6 (V1_6_CUTOFF) by its build, not another format-3 1.6 (not the positive one)
        assert_eq!(PATCH_BUILDS[2].caps(), Some(Caps { mnt_format: 3, ..PROOF_CAPS }));
        assert_eq!(caps_of(&info(3, [1, 6], 0, 0x06)), None);
        assert_eq!(caps_of(&info(3, [1, 6], 0, 0x0E)), None);
        let c = PROOF_CAPS;
        assert_eq!(c.pack(), PackCaps { uart1_params: true, uart1_identity: true, write_gate_bit0: true, keeps_start_bit2: true });
        assert_eq!(c.master(), MasterCaps { status_split: true, start_gate_bit2: true });
        assert_eq!(pack_caps_of(&info(4, [1, 5], 3, 0x3A)), Some(c.pack()));
        assert_eq!(pack_caps_of(&info(4, [1, 5], 1, 0x12)), None);
        // the flag alone is not enough for the pack cutoff, nor the version alone
        assert!(pack_cutoff_of([1, 6], true));
        assert!(!pack_cutoff_of([1, 5], true));
        assert!(!pack_cutoff_of([1, 6], false));
    }

    #[test]
    fn a_later_build_is_one_more_table_entry() {
        let later = PatchBuild { name: "16.06.14 MAINT 1.6.4", base: [16, 6, 14], patch_rev: 4, build_id: "0123456789abcdef", code_sha256: "c0de", ehex_sha256: "f11e", ..PATCH_BUILDS[1] };
        let table = [PatchBuild { ..PATCH_BUILDS[1] }, later];
        let found = patch_in(&table, 4, [16, 6, 14], [1, 6], "0123456789abcdef").unwrap();
        assert_eq!((found.name, found.caps()), ("16.06.14 MAINT 1.6.4", PATCH_BUILDS[1].caps()));
    }

    #[test]
    fn identify_still_accepts_only_vendor_releases() {
        for m in PATCH_BUILDS {
            assert!(RELEASES.iter().all(|v| v.code_sha256 != m.code_sha256));
        }
    }
}
