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

/// What a patched master or pack offers. Patch functions are cumulative: a later patch version keeps every function of
/// the earlier ones unchanged, and a new OEM base gets the same functions. So each capability has a minimum patch version
/// (`CAP_MIN`), and a format-2 identity with a patch version at or above it has it, whatever its base or build ID. The
/// app asks these, never a version number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// layout of the `0x64/03` identity (2: 17 bytes; 3: 18 bytes with image flags; neither has runtime state)
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

/// The pack capabilities of a `0x64/03` identity read at any address (None: another format or below `MIN_PATCH`).
pub fn pack_caps_of(format: u8, patch: [u8; 2]) -> Option<PackCaps> {
    caps_of(format, patch).map(|c| c.pack())
}

/// The lowest patch version of each capability: (status_split, uart1_params, uart1_identity, write_gate_bit0,
/// start_gate_bit2). The app supports the patch functions as a whole from `MIN_PATCH` on.
pub const CAP_MIN: CapMin = CapMin { status_split: [1, 5], uart1_params: [1, 5], uart1_identity: [1, 5], write_gate_bit0: [1, 5], start_gate_bit2: [1, 5] };
/// Below this a format-2 identity is an unknown patch (no patch functions): 1.4 has format 2 but no release bits.
pub const MIN_PATCH: [u8; 2] = [1, 5];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapMin {
    pub status_split: [u8; 2],
    pub uart1_params: [u8; 2],
    pub uart1_identity: [u8; 2],
    pub write_gate_bit0: [u8; 2],
    pub start_gate_bit2: [u8; 2],
}

/// From this patch version on, image flag bit 2 announces the stored pack cutoff (Group0 bit 4 extended off, bit 5 the
/// lock state). Below it the bits are not read that way, whatever the flag says.
pub const PACK_CUTOFF_MIN: [u8; 2] = [1, 6];

/// The stored pack cutoff of an identity with patch `patch` and image flag bit 2 `flag`: both needed.
pub fn pack_cutoff_of(patch: [u8; 2], flag: bool) -> bool {
    flag && patch >= PACK_CUTOFF_MIN
}

/// The `0x64/03` formats the app decodes; both carry the same identity fields (format 3 adds the image flags).
pub const MNT_FORMATS: [u8; 2] = [2, 3];

/// The capabilities of a `0x64/03` identity (`format`, `patch`): None for another format or a patch below `MIN_PATCH`.
pub fn caps_of(format: u8, patch: [u8; 2]) -> Option<Caps> {
    if !MNT_FORMATS.contains(&format) || patch < MIN_PATCH {
        return None;
    }
    Some(Caps {
        mnt_format: format,
        status_split: patch >= CAP_MIN.status_split,
        uart1_params: patch >= CAP_MIN.uart1_params,
        uart1_identity: patch >= CAP_MIN.uart1_identity,
        write_gate_bit0: patch >= CAP_MIN.write_gate_bit0,
        start_gate_bit2: patch >= CAP_MIN.start_gate_bit2,
    })
}

/// A listed patched build: modified firmware, accepted only like custom images; listed so a flash image is recognised
/// (code hash) and the master's build shown by name. Capabilities come from its patch version (`caps_of`), not from the
/// listing.
#[derive(Debug, PartialEq, Eq)]
pub struct PatchBuild {
    /// shown name, also the release a flash records
    pub name: &'static str,
    /// base firmware major, minor, patch as `0x64/03` reports it
    pub base: [u8; 3],
    /// patch version major, minor as `0x64/03` reports it
    pub patch: [u8; 2],
    /// the `0x64/03` format of this build (builds are told apart by format and build ID)
    pub mnt_format: u8,
    /// format 3: the hard-stop patch is compiled in; None for a format-2 build (not reported)
    pub hard_stop: Option<bool>,
    /// format 3, image flag bit 1: ein/aus, restart and the clock over the CAN socket with the lock Group0 bit 3
    pub power_rtc: bool,
    /// format 3, image flag bit 2 with patch 1.6: the stored pack cutoff (Group0 bits 4/5)
    pub pack_cutoff: bool,
    /// the build ID of `0x64/03`, 16 lower-case hex digits
    pub build_id: &'static str,
    /// SHA-256 of the decoded application code (`ehex::Image::code_sha256`)
    pub code_sha256: &'static str,
    /// SHA-256 of the `.ehex` file as published (reference only; recognition uses the code hash)
    pub ehex_sha256: &'static str,
}

impl PatchBuild {
    pub fn caps(&self) -> Option<Caps> {
        caps_of(self.mnt_format, self.patch)
    }
}

/// The listed patched builds. A later build: append one entry.
pub const PATCH_BUILDS: &[PatchBuild] = &[PatchBuild {
    name: "16.06.13 MAINT 1.5",
    base: [16, 6, 13],
    patch: [1, 5],
    mnt_format: 2,
    hard_stop: None,
    power_rtc: false,
    pack_cutoff: false,
    build_id: "909a604e60e9225b",
    code_sha256: "5cd0f08fdbed8adce2a340b8ca842a02f4f2de61eee3f6bdb5b268fa5ec78c12",
    ehex_sha256: "346959edab55e857c2ae8a47cb2f4a7a2be2b279024857ce83ed612304060ee1",
},
// MNT format 3, 64/03 answered at every pack's own address; hard stop off (54584 app bytes, 215 update blocks)
PatchBuild {
    name: "16.06.13 MAINT 1.5",
    base: [16, 6, 13],
    patch: [1, 5],
    mnt_format: 3,
    hard_stop: Some(false),
    power_rtc: false,
    pack_cutoff: false,
    build_id: "297f9f2aa8402e9f",
    code_sha256: "02b50e2e4772ec787db86de5c6f69dcd364e484516f8cf2f04311c3478f59508",
    ehex_sha256: "31a6477f938bf2ac017deec3d0158f72039a3898d28686e428094946ae609fe0",
},
// MNT format 3 with image flags 02: ein/aus, restart and the clock over the CAN socket, lock Group0 bit 3; hard stop off
// (55124 app bytes, 217 update blocks)
PatchBuild {
    name: "16.06.13 MAINT 1.5",
    base: [16, 6, 13],
    patch: [1, 5],
    mnt_format: 3,
    hard_stop: Some(false),
    power_rtc: true,
    pack_cutoff: false,
    build_id: "d75a7b24cb6e8590",
    code_sha256: "58f45b854822d1f06eabb663fb2cd8f55d41485dd003eb6d52c2585477d0ca7a",
    ehex_sha256: "9b8f7b1535a2491e883b61f8bd7566d074842aadcfdc3095d715f2d968e49052",
},
// MAINT 1.6, MNT format 3 with image flags 06: power and clock as 1.5, plus the stored pack cutoff (Group0 bits 4/5) and
// the master's CCL clamp; hard stop off
PatchBuild {
    name: "16.06.13 MAINT 1.6",
    base: [16, 6, 13],
    patch: [1, 6],
    mnt_format: 3,
    hard_stop: Some(false),
    power_rtc: true,
    pack_cutoff: true,
    build_id: "18daf61d8a93671d",
    code_sha256: "be2ecc7549c7ee16d46eed1f10c256d7231cc5ed961ec90f0bae87e0cfc40d2d",
    ehex_sha256: "32f27f3f128a591759bd2e205579b2dfdccf50c3717e50c7092c1e30f44d146f",
}];

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
        assert_eq!(kind_of_hash("5cd0f08fdbed8adce2a340b8ca842a02f4f2de61eee3f6bdb5b268fa5ec78c12"), (ImageKind::Maintenance, Some("16.06.13 MAINT 1.5")));
        assert_eq!(kind_of_hash(&"0".repeat(64)), (ImageKind::Custom, None));
        // earlier unpublished patch builds are custom images now
        for old in ["734a4a4c73abbf991b2616b50958aff6c05cdcca93ebf3781867a179194e38f1", "4d76a96fa7f7ad0e1aa3de1ac4fbe51f0fc4ae03a19aea2d4e696c2812d8f55d",
            "bc1235ade82dba79b58ea6e96cbf8196e5a1dc6160d17b8434a5fbc5766efbf6", "e00ed8fcf75c696b23de37605752d9a961022d8a43e11466c89e5a8840511ccb"] {
            assert_eq!(kind_of_hash(old), (ImageKind::Custom, None));
        }
    }

    #[test]
    fn the_listed_build_is_recognised_only_by_its_exact_identity() {
        let b = patch_build(2, [16, 6, 13], [1, 5], "909a604e60e9225b").unwrap();
        assert_eq!(b.name, "16.06.13 MAINT 1.5");
        assert_eq!(b.ehex_sha256, "346959edab55e857c2ae8a47cb2f4a7a2be2b279024857ce83ed612304060ee1");
        assert!(patch_build(1, [16, 6, 13], [1, 5], "909a604e60e9225b").is_none(), "other format");
        assert!(patch_build(2, [16, 6, 13], [1, 4], "909a604e60e9225b").is_none(), "other patch");
        assert!(patch_build(2, [16, 6, 12], [1, 5], "909a604e60e9225b").is_none(), "other base");
        assert!(patch_build(2, [16, 6, 13], [1, 5], "0000000000000000").is_none(), "other build");
        assert_eq!(patch_image(b.code_sha256), Some("16.06.13 MAINT 1.5"));
    }

    #[test]
    fn capabilities_follow_the_patch_version_cumulatively() {
        let full = Caps { mnt_format: 2, status_split: true, uart1_params: true, uart1_identity: true, write_gate_bit0: true, start_gate_bit2: true };
        assert_eq!(caps_of(2, [1, 5]), Some(full));
        assert_eq!(caps_of(2, [1, 6]), Some(full));
        assert_eq!(caps_of(2, [2, 0]), Some(full));
        assert_eq!(caps_of(3, [1, 5]), Some(Caps { mnt_format: 3, ..full }), "format 3: the same patch functions");
        assert_eq!(caps_of(4, [1, 5]), None, "format 4");
        assert_eq!(caps_of(2, [1, 4]), None, "1.4: format 2 without release bits");
        assert_eq!(caps_of(1, [1, 5]), None, "format 1");
        assert_eq!(PATCH_BUILDS[0].caps(), Some(full));
    }

    #[test]
    fn the_power_rtc_build_is_listed_with_its_image_flag() {
        let b = patch_build(3, [16, 6, 13], [1, 5], "d75a7b24cb6e8590").unwrap();
        assert_eq!((b.name, b.hard_stop, b.power_rtc, b.caps()), ("16.06.13 MAINT 1.5", Some(false), true, caps_of(3, [1, 5])));
        assert_eq!(patch_image("58f45b854822d1f06eabb663fb2cd8f55d41485dd003eb6d52c2585477d0ca7a"), Some("16.06.13 MAINT 1.5"));
        assert_eq!(kind_of_hash("58f45b854822d1f06eabb663fb2cd8f55d41485dd003eb6d52c2585477d0ca7a").0, ImageKind::Maintenance);
        // the earlier builds have no power and clock service
        assert!(!patch_build(3, [16, 6, 13], [1, 5], "297f9f2aa8402e9f").unwrap().power_rtc);
        assert!(!patch_build(2, [16, 6, 13], [1, 5], "909a604e60e9225b").unwrap().power_rtc);
    }

    #[test]
    fn the_pack_cutoff_build_is_listed_and_the_capability_needs_patch_1_6() {
        let b = patch_build(3, [16, 6, 13], [1, 6], "18daf61d8a93671d").unwrap();
        assert_eq!((b.name, b.hard_stop, b.power_rtc, b.pack_cutoff), ("16.06.13 MAINT 1.6", Some(false), true, true));
        assert_eq!(b.caps(), caps_of(3, [1, 6]), "the 1.5 functions, cumulatively");
        assert_eq!(patch_image("be2ecc7549c7ee16d46eed1f10c256d7231cc5ed961ec90f0bae87e0cfc40d2d"), Some("16.06.13 MAINT 1.6"));
        assert_eq!(kind_of_hash("be2ecc7549c7ee16d46eed1f10c256d7231cc5ed961ec90f0bae87e0cfc40d2d").0, ImageKind::Maintenance);
        assert!(PATCH_BUILDS.iter().filter(|b| b.patch < [1, 6]).all(|b| !b.pack_cutoff));
        // the flag alone is not enough, nor the version alone
        assert!(pack_cutoff_of([1, 6], true));
        assert!(pack_cutoff_of([1, 7], true));
        assert!(!pack_cutoff_of([1, 5], true));
        assert!(!pack_cutoff_of([1, 6], false));
    }

    #[test]
    fn the_slave_addressed_build_is_listed_by_format_and_build_id() {
        let b = patch_build(3, [16, 6, 13], [1, 5], "297f9f2aa8402e9f").unwrap();
        assert_eq!((b.name, b.hard_stop, b.caps()), ("16.06.13 MAINT 1.5", Some(false), caps_of(3, [1, 5])));
        assert_eq!(patch_image("02b50e2e4772ec787db86de5c6f69dcd364e484516f8cf2f04311c3478f59508"), Some("16.06.13 MAINT 1.5"));
        assert_eq!(kind_of_hash("02b50e2e4772ec787db86de5c6f69dcd364e484516f8cf2f04311c3478f59508").0, ImageKind::Maintenance);
        // the format-2 build stays listed; the same build ID under another format is not this entry
        assert!(patch_build(2, [16, 6, 13], [1, 5], "909a604e60e9225b").is_some());
        assert!(patch_build(2, [16, 6, 13], [1, 5], "297f9f2aa8402e9f").is_none());
    }

    #[test]
    fn pack_and_master_capabilities_are_apart() {
        let c = caps_of(3, [1, 5]).unwrap();
        assert_eq!(c.pack(), PackCaps { uart1_params: true, uart1_identity: true, write_gate_bit0: true, keeps_start_bit2: true });
        assert_eq!(c.master(), MasterCaps { status_split: true, start_gate_bit2: true });
        assert_eq!(pack_caps_of(2, [1, 5]), Some(c.pack()));
        assert_eq!(pack_caps_of(2, [1, 4]), None);
        assert_eq!(pack_caps_of(1, [1, 5]), None);
    }

    #[test]
    fn a_later_build_is_one_more_table_entry() {
        let later = PatchBuild { name: "16.06.14 MAINT 1.6", base: [16, 6, 14], patch: [1, 6], build_id: "0123456789abcdef", code_sha256: "c0de", ehex_sha256: "f11e", ..PATCH_BUILDS[0] };
        let table = [PatchBuild { ..PATCH_BUILDS[0] }, later];
        let found = patch_in(&table, 2, [16, 6, 14], [1, 6], "0123456789abcdef").unwrap();
        assert_eq!((found.name, found.caps()), ("16.06.14 MAINT 1.6", PATCH_BUILDS[0].caps()));
    }

    #[test]
    fn identify_still_accepts_only_vendor_releases() {
        for m in PATCH_BUILDS {
            assert!(RELEASES.iter().all(|v| v.code_sha256 != m.code_sha256));
        }
    }
}
