//! Code hashes of unmodified vendor `.ehex` releases. Hashing decoded code permits harmless
//! CRLF/LF conversion while the strict parser still validates every record and the header.

use crate::{ehex::Image, Error, Result};
use openbms_proto::ExtInfo;

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

/// What an EMU1101-EXT master or pack offers. Every supported build (`ExtInfo::supported`) has all of these; functions
/// that differ between builds come from the image flags. The app asks these, never a version number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// format of the `0x64/03` identity (5: `EXT05`, 21 bytes; TEMP(maint-1.6): 3, the `MNT` format 3; no runtime state)
    pub ext_format: u8,
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
pub fn pack_caps_of(info: &ExtInfo) -> Option<PackCaps> {
    caps_of(info).map(|c| c.pack())
}

/// The lowest patch version the app knows functions of (`openbms_proto::access::EXT_MIN`); which builds are supported
/// decides `ExtInfo::supported`.
pub const MIN_PATCH: [u8; 2] = openbms_proto::access::EXT_MIN;

/// From this patch version on, image flag bit 2 announces the stored pack cutoff (Group0 bit 4 extended off, bit 5 the
/// lock state). Below it the bits are not read that way, whatever the flag says.
pub const PACK_CUTOFF_MIN: [u8; 2] = openbms_proto::access::CUTOFF_MIN;

/// The stored pack cutoff of an identity with patch `patch` and image flag bit 2 `flag`: both needed.
pub use openbms_proto::access::pack_cutoff_of;

/// The capabilities of a supported `0x64/03` identity (`ExtInfo::supported`): every supported build has them all;
/// None for another or an older build (its patch functions are not used).
pub fn caps_of(info: &ExtInfo) -> Option<Caps> {
    info.supported().then_some(PROOF_CAPS).map(|c| Caps { ext_format: info.format, ..c })
}

/// The capabilities a stored proof stands for (a supported build; it carries no format or image flags).
pub const PROOF_CAPS: Caps = Caps { ext_format: 5, status_split: true, uart1_params: true, uart1_identity: true, write_gate_bit0: true, start_gate_bit2: true };

/// A listed EMU1101-EXT build: modified firmware, accepted only like custom images; listed so a flash image is recognised
/// (code hash) and the master's build shown by name. Capabilities come from its identity (`caps_of`), not from the
/// listing.
#[derive(Debug, PartialEq, Eq)]
pub struct PatchBuild {
    /// shown name („EMU1101-EXT Full 1.6.5“, `ExtInfo::version`), also the release a flash records
    pub name: &'static str,
    /// base firmware major, minor, patch as `0x64/03` reports it
    pub base: [u8; 3],
    /// patch version major, minor and (format 5) revision as `0x64/03` reports it
    pub patch: [u8; 2],
    pub patch_rev: u8,
    /// the `0x64/03` format of this build (builds are told apart by format and build ID)
    pub ext_format: u8,
    /// the image flag byte of its `0x64/03`
    pub flags: u8,
    /// format 5: the read-capability mask of its `0x64/03`; 0 with format 3
    pub read_caps: u16,
    /// the build ID of `0x64/03`, 16 lower-case hex digits
    pub build_id: &'static str,
    /// SHA-256 of the decoded application code (`ehex::Image::code_sha256`)
    pub code_sha256: &'static str,
    /// SHA-256 of the `.ehex` file as published (reference only; recognition uses the code hash)
    pub ehex_sha256: &'static str,
}

impl PatchBuild {
    /// The `0x64/03` identity this build answers.
    pub fn info(&self) -> ExtInfo {
        // TEMP(maint-1.6): the MNT format 3 has no third patch byte and no mask
        let ext = self.ext_format == 5;
        let mut v = if ext { b"EXT".to_vec() } else { b"MNT".to_vec() };
        v.push(self.ext_format);
        v.extend_from_slice(&self.base);
        v.extend_from_slice(&self.patch);
        if ext {
            v.push(self.patch_rev);
        }
        v.extend((0..8).map(|i| u8::from_str_radix(&self.build_id[2 * i..2 * i + 2], 16).unwrap_or(0)));
        v.push(self.flags);
        if ext {
            v.extend_from_slice(&self.read_caps.to_be_bytes());
        }
        ExtInfo::parse(&v).expect("a listed build has a valid identity")
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

/// The listed builds (the supported ones): the four EMU1101-EXT releases, all on OEM 16.06.13 with soft stop as default
/// and read caps 000F, and the TEMP(maint-1.6) build (no read caps). A later build: append one entry. .4 and .5 (the atomic scheduler fix) differ only by version and build ID.
pub const PATCH_BUILDS: &[PatchBuild] = &[
    // EMU1101-EXT Light 1.5.4: flags 3A (power and clock with Group0 bit 3 at 1, RS485-1/2 marker and identity, the 64/04
    // diagnosis, fixed soft stop)
    PatchBuild {
        name: "EMU1101-EXT Light 1.5.4",
        base: [16, 6, 13],
        patch: [1, 5],
        patch_rev: 4,
        ext_format: 5,
        flags: 0x3A,
        read_caps: 0x000F,
        build_id: "ffe3acf7e52bed80",
        code_sha256: "927e178e0b4140185e974a786525091b49a37f5618e86729981697f4f90d5003",
        ehex_sha256: "6b4549fa89145db7755dde90b72511501b2b5425006028e5f6968e8bac530c1c",
    },
    // EMU1101-EXT Light 1.5.5: 1.5.4 plus the atomic scheduler fix
    PatchBuild {
        name: "EMU1101-EXT Light 1.5.5",
        base: [16, 6, 13],
        patch: [1, 5],
        patch_rev: 5,
        ext_format: 5,
        flags: 0x3A,
        read_caps: 0x000F,
        build_id: "082cb731c006aac5",
        code_sha256: "a6bd1f959b4fd7baaf84ed2c65207a54f6225203c86e0257953e4d486c42d8fd",
        ehex_sha256: "20ee87a808314781479821a107dbad350e165db6a63c88241bec444e02556911",
    },
    // EMU1101-EXT Full 1.6.4: flags FE (Light plus the stored pack cutoff, the SOC checkpoint before a planned restart and
    // the configurable hard stop with soft as default)
    PatchBuild {
        name: "EMU1101-EXT Full 1.6.4",
        base: [16, 6, 13],
        patch: [1, 6],
        patch_rev: 4,
        ext_format: 5,
        flags: 0xFE,
        read_caps: 0x000F,
        build_id: "4f4d2715c9ee553c",
        code_sha256: "b3ab4856a2be678801cf1c7061c6daacb8f9b44b6f6a5aee1020ad64a4c4424a",
        ehex_sha256: "bfd2ef0bd7a47f02d9dcd11aac88a34a7095d4f5f4286ba7c89cbbcb946e1172",
    },
    // EMU1101-EXT Full 1.6.5: 1.6.4 plus the atomic scheduler fix
    PatchBuild {
        name: "EMU1101-EXT Full 1.6.5",
        base: [16, 6, 13],
        patch: [1, 6],
        patch_rev: 5,
        ext_format: 5,
        flags: 0xFE,
        read_caps: 0x000F,
        build_id: "13464fc91a6e6a20",
        code_sha256: "20e26bf8a0a0cd9d9b21c9018fc2c5b4e2325cf510f0c01b48c72da1c684265d",
        ehex_sha256: "fae20016130c950ee12e9a90ad0aa0e042b41eda3e46aef1f95469378d800b2f",
    },
    // TEMP(maint-1.6): the first MAINT 1.6 (V1_6_CUTOFF), MNT format 3 with image flags 06: power and clock (bit 3 at 0
    // allows), the stored pack cutoff; hard stop off. Supported until the installation runs EMU1101-EXT Full, then
    // remove this entry.
    PatchBuild {
        name: "16.06.13 MAINT 1.6",
        base: [16, 6, 13],
        patch: openbms_proto::maint::temp16::PATCH,
        patch_rev: 0,
        ext_format: openbms_proto::maint::temp16::FORMAT,
        flags: openbms_proto::maint::temp16::FLAGS,
        read_caps: 0,
        build_id: openbms_proto::maint::temp16::BUILD,
        code_sha256: "be2ecc7549c7ee16d46eed1f10c256d7231cc5ed961ec90f0bae87e0cfc40d2d",
        ehex_sha256: "32f27f3f128a591759bd2e205579b2dfdccf50c3717e50c7092c1e30f44d146f",
    },
];

/// The entry `0x64/03` describes in `table`: format, base, patch and build ID all match.
pub fn patch_in<'a>(table: &'a [PatchBuild], format: u8, base: [u8; 3], patch: [u8; 2], build_id: &str) -> Option<&'a PatchBuild> {
    table.iter().find(|b| b.ext_format == format && b.base == base && b.patch == patch && b.build_id == build_id)
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
    /// a known EMU1101-EXT build (`PATCH_BUILDS`)
    Maintenance,
    /// anything else that passes the format checks
    Custom,
}

/// The kind of an image and its name (a vendor release or an EMU1101-EXT build).
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

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn kind_of_hash_tells_originals_ext_builds_and_custom_apart() {
        assert_eq!(kind_of_hash(RELEASES[0].code_sha256), (ImageKind::Original, Some(RELEASES[0].name)));
        assert_eq!(kind_of_hash("20e26bf8a0a0cd9d9b21c9018fc2c5b4e2325cf510f0c01b48c72da1c684265d"), (ImageKind::Maintenance, Some("EMU1101-EXT Full 1.6.5")));
        // TEMP(maint-1.6)
        assert_eq!(kind_of_hash("be2ecc7549c7ee16d46eed1f10c256d7231cc5ed961ec90f0bae87e0cfc40d2d"), (ImageKind::Maintenance, Some("16.06.13 MAINT 1.6")));
        assert_eq!(kind_of_hash(&"0".repeat(64)), (ImageKind::Custom, None));
        // the builds before the EXT signature (1.5 … 1.5.4, the positive 1.6, 1.6.1 … 1.6.4 with MNT) are custom images now
        for old in ["5cd0f08fdbed8adce2a340b8ca842a02f4f2de61eee3f6bdb5b268fa5ec78c12", "58f45b854822d1f06eabb663fb2cd8f55d41485dd003eb6d52c2585477d0ca7a",
            "619cdf77811e05b085d37e997b3c469f35e83ddd5dd351fcf2ab6fd63b3a0437", "010ac5446b644a739611ccaffda85542365dbfccbe5753ede1c1e6b2bc776ae8",
            "fec91208a5553726bb5c4da905dc5fadb0955011eb6f337b05b67ffefe62437f", "9c1d8a912222d6f8ec9329a17b332776803f5d2a955a6028ae17e3cf0f1d1e1f",
            "27be05dc5f9ab0f648861e2d8e9feaffc4685ce98d59c02cccb7a3d114a38cd0", "fe04ff9a3263ee0fea0340a71d741c770c4056f10ceb534546fcb9b0181cf33f",
            "0e10295aac3f3c88ea5f77f1deaf431a67f6f720c7e82a7e805bf4e1ecb34ef3", "cd4ce64e20bf04c37e81da37daa170be53cf9380a589dd3d76c12972374d2f14"] {
            assert_eq!(kind_of_hash(old), (ImageKind::Custom, None));
        }
    }

    #[test]
    fn the_listed_builds_answer_exactly_their_release_identity() {
        for (name, info_hex, code) in [
            ("EMU1101-EXT Light 1.5.4", "4558540510060D010504FFE3ACF7E52BED803A000F", "927e178e0b4140185e974a786525091b49a37f5618e86729981697f4f90d5003"),
            ("EMU1101-EXT Light 1.5.5", "4558540510060D010505082CB731C006AAC53A000F", "a6bd1f959b4fd7baaf84ed2c65207a54f6225203c86e0257953e4d486c42d8fd"),
            ("EMU1101-EXT Full 1.6.4", "4558540510060D0106044F4D2715C9EE553CFE000F", "b3ab4856a2be678801cf1c7061c6daacb8f9b44b6f6a5aee1020ad64a4c4424a"),
            ("EMU1101-EXT Full 1.6.5", "4558540510060D01060513464FC91A6E6A20FE000F", "20e26bf8a0a0cd9d9b21c9018fc2c5b4e2325cf510f0c01b48c72da1c684265d"),
        ] {
            let b = PATCH_BUILDS.iter().find(|b| b.name == name).unwrap();
            let live = ExtInfo::parse(&hex(info_hex)).unwrap();
            assert_eq!(b.info(), live, "{name}");
            assert_eq!(live.version(), name, "the display name is the name the identity gives");
            assert_eq!((b.code_sha256, patch_image(code)), (code, Some(name)));
            assert_eq!(patch_build(5, live.base, live.patch, &live.build_id_hex()).map(|b| b.name), Some(name));
            assert_eq!(b.caps(), Some(Caps { ext_format: 5, ..PROOF_CAPS }));
        }
        let full = PATCH_BUILDS.iter().find(|b| b.name == "EMU1101-EXT Full 1.6.5").unwrap();
        assert_eq!((full.power_rtc(), full.pack_cutoff(), full.info().checkpoint(), full.info().hard_stop_configurable()), (true, true, true, true));
        let light = PATCH_BUILDS.iter().find(|b| b.name == "EMU1101-EXT Light 1.5.5").unwrap();
        assert_eq!((light.power_rtc(), light.pack_cutoff(), light.info().positive(), light.info().diagnostics(), light.info().checkpoint(), light.info().hard_stop), (true, false, true, true, false, Some(false)));
        assert!(patch_build(3, [16, 6, 13], [1, 6], "13464fc91a6e6a20").is_none(), "other format");
        assert!(patch_build(5, [16, 6, 12], [1, 6], "13464fc91a6e6a20").is_none(), "other base");
        assert!(patch_build(5, [16, 6, 13], [1, 6], "0000000000000000").is_none(), "other build");
        // every listed build is supported
        assert!(PATCH_BUILDS.iter().all(|b| b.caps().is_some()));
    }

    #[test]
    fn capabilities_come_with_a_supported_build_only() {
        let info = |patch: [u8; 2], patch_rev: u8, flags: u8| ExtInfo::parse(&{
            let mut v = b"EXT".to_vec();
            v.extend_from_slice(&[5, 16, 6, 13, patch[0], patch[1], patch_rev]);
            v.extend_from_slice(&[1; 8]);
            v.extend_from_slice(&[flags, 0, 0x0F]);
            v
        }).unwrap();
        assert_eq!(caps_of(&info([1, 5], 4, 0x3A)), Some(PROOF_CAPS), "Light 1.5.4 with another build ID");
        assert_eq!(caps_of(&info([1, 6], 5, 0xFE)), Some(PROOF_CAPS));
        assert_eq!(caps_of(&info([1, 7], 0, 0xFE)), Some(PROOF_CAPS), "a later version");
        assert_eq!(caps_of(&info([1, 5], 3, 0x3A)), None, "below Light 1.5.4");
        assert_eq!(caps_of(&info([1, 6], 3, 0xFE)), None, "below Full 1.6.4");
        // TEMP(maint-1.6): the first 1.6 (V1_6_CUTOFF) by its build, not another format-3 1.6 (not the positive one)
        let temp = PATCH_BUILDS.iter().find(|b| b.name == "16.06.13 MAINT 1.6").unwrap();
        assert_eq!(temp.caps(), Some(Caps { ext_format: 3, ..PROOF_CAPS }));
        assert_eq!(temp.info().version(), "16.06.13 MAINT 1.6");
        let f3 = |flags: u8| ExtInfo::parse(&[b"MNT".as_slice(), &[3, 16, 6, 13, 1, 6], &[1; 8], &[flags]].concat()).unwrap();
        assert_eq!(caps_of(&f3(0x06)), None);
        assert_eq!(caps_of(&f3(0x0E)), None);
        let c = PROOF_CAPS;
        assert_eq!(c.pack(), PackCaps { uart1_params: true, uart1_identity: true, write_gate_bit0: true, keeps_start_bit2: true });
        assert_eq!(c.master(), MasterCaps { status_split: true, start_gate_bit2: true });
        assert_eq!(pack_caps_of(&info([1, 5], 4, 0x3A)), Some(c.pack()));
        assert_eq!(pack_caps_of(&info([1, 5], 3, 0x3A)), None);
        // the flag alone is not enough for the pack cutoff, nor the version alone
        assert!(pack_cutoff_of([1, 6], true));
        assert!(!pack_cutoff_of([1, 5], true));
        assert!(!pack_cutoff_of([1, 6], false));
    }

    #[test]
    fn a_later_build_is_one_more_table_entry() {
        let later = PatchBuild { name: "EMU1101-EXT Full 1.6.6", patch_rev: 6, build_id: "0123456789abcdef", code_sha256: "c0de", ehex_sha256: "f11e", ..PATCH_BUILDS[3] };
        let table = [PatchBuild { ..PATCH_BUILDS[3] }, later];
        let found = patch_in(&table, 5, [16, 6, 13], [1, 6], "0123456789abcdef").unwrap();
        assert_eq!((found.name, found.caps()), ("EMU1101-EXT Full 1.6.6", PATCH_BUILDS[3].caps()));
    }

    #[test]
    fn identify_still_accepts_only_vendor_releases() {
        for m in PATCH_BUILDS {
            assert!(RELEASES.iter().all(|v| v.code_sha256 != m.code_sha256));
        }
    }
}
