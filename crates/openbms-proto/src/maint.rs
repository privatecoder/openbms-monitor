//! CID2 0x64: identity and maintenance mode of the EMU1101-EXT firmware (Light 1.5.x, Full 1.6.x on OEM 16.06.13), over
//! the CAN socket bus; with image flag bit 4 also status, clear and identity over RS485-1/2 at the pack's own address.
//!
//! The patch keeps a persistent marker in flash. While it is set, the master treats its slave count as 0: it stops
//! polling the pack bus (RS485-1/2) and answers there itself, so its parameters can be read and it can be updated.
//! Original firmware answers 0x64 with RTN 04 (unknown command). Over the CAN socket status, start and stop answer only
//! at the master (address 0); identity and the reads 04–06 at every pack's own address.
//!
//! `INFO=03` reads the identity ([`ExtInfo`], no runtime state; answered at every pack's own address, the address only
//! in the frame): signature `EXT`, format 05, 21 bytes (OEM base, patch version, build ID, image flags, big-endian
//! read-capability mask). TEMP(maint-1.6): the first MAINT 1.6 answers signature `MNT`, format 3 with 18 bytes.
//! `INFO=02` reads the state ([`MaintState`], 5 bytes); start and stop answer with the same 5 bytes. The 5-byte layout
//! holds only once the identity of a supported build was read on the same connection.

use crate::access::{FLAG_CHECKPOINT, FLAG_CUTOFF, FLAG_DIAGNOSIS, FLAG_HARD_STOP, FLAG_HARD_STOP_ON, FLAG_POSITIVE, FLAG_POWER, FLAG_UART0};
use crate::error::{Error, Result};
use crate::frame::encode_request;
use serde::Serialize;

/// Release value that guards start and stop against accidental frames (not an authentication).
const TOKEN: [u8; 2] = [0xC3, 0x5A];
const SIGNATURE: &[u8; 3] = b"EXT";
/// TEMP(maint-1.6): the signature of the first MAINT 1.6 (`temp16`)
const SIGNATURE_MNT: &[u8; 3] = b"MNT";

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

    /// Read the state (changes nothing). Over the CAN socket only the master at address 0 answers.
    pub fn request() -> Result<Vec<u8>> {
        Self::request_at(0)
    }

    /// Read the state of the pack at `address` (0–21): over RS485-1/2 a build with image flag bit 4 answers at its own
    /// address, also a slave.
    pub fn request_at(address: u8) -> Result<Vec<u8>> {
        check_address(address)?;
        encode_request(address, 0x64, &[0x02])
    }

    /// Set (`true`) or clear (`false`) the marker. The reply may still show the slave count of the previous cycle; read
    /// the state again afterwards.
    pub fn set_request(on: bool) -> Result<Vec<u8>> {
        Self::set_request_at(0, on)
    }

    /// `set_request` at `address` (0–21); over RS485-1/2 the build with image flag bit 4 only clears.
    pub fn set_request_at(address: u8, on: bool) -> Result<Vec<u8>> {
        check_address(address)?;
        encode_request(address, 0x64, &[u8::from(on), TOKEN[0], TOKEN[1]])
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

fn check_address(address: u8) -> Result<()> {
    if address > 21 {
        return Err(Error::Layout("0x64 address must be 0..=21"));
    }
    Ok(())
}

/// Answer to `0x64 INFO=04` (image flag bit 5): the stored parameter and SOC copies as read straight
/// from flash, nothing repaired or changed. Two valid copies that differ do not tell which is newer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MemoryDiagnosis {
    /// parameter record primary, backup: "empty" (256 bytes FF), "valid" (form and CRC), "invalid", "unknown" (another
    /// code)
    pub params: [&'static str; 2],
    /// "not-comparable", "same", "different", "unknown"
    pub params_compare: &'static str,
    pub soc: [&'static str; 2],
    pub soc_compare: &'static str,
    /// the individual pack lock stored in the parameter primary, backup: Some(true) locked, Some(false) unlocked, None
    /// unknown (FF: not supported by the build, or the record invalid)
    pub lock: [Option<bool>; 2],
    /// runtime cache flags: bit 0 latch not 0, bit 1 cache consistency value 1, bit 2 cache identifier valid (A6)
    pub cache: u8,
}

impl MemoryDiagnosis {
    pub const LEN: usize = 10;

    /// Ask the pack at `address` (0–21) for its memory diagnosis (changes nothing).
    pub fn request_at(address: u8) -> Result<Vec<u8>> {
        check_address(address)?;
        encode_request(address, 0x64, &[0x04])
    }

    /// Exactly format 01 with 10 bytes.
    pub fn parse(info: &[u8]) -> Result<Self> {
        if info.len() != Self::LEN || info[0] != 0x01 {
            return Err(Error::Layout("0x64/04 answer is not format 01 with 10 bytes"));
        }
        let record = |b: u8| match b {
            0 => "empty",
            1 => "valid",
            2 => "invalid",
            _ => "unknown",
        };
        let compare = |b: u8| match b {
            0 => "not-comparable",
            1 => "same",
            2 => "different",
            _ => "unknown",
        };
        let lock = |b: u8| match b {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        Ok(Self {
            params: [record(info[1]), record(info[2])],
            params_compare: compare(info[3]),
            soc: [record(info[4]), record(info[5])],
            soc_compare: compare(info[6]),
            lock: [lock(info[7]), lock(info[8])],
            cache: info[9],
        })
    }
}

/// The lowest supported build of the Light line (1.5) and of the Full line (1.6) and later (major, minor, revision):
/// the first EXT05 builds.
pub const SUPPORTED_15: [u8; 3] = [1, 5, 4];
pub const SUPPORTED_16: [u8; 3] = [1, 6, 4];

/// Whether the app supports a patch version (major, minor, revision): from Light 1.5.4, from Full 1.6.4 on. One
/// rule for a live identity (`ExtInfo::supported`) and a stored proof's release name (`profile::release_patch`).
pub fn supported_version(v: [u8; 3]) -> bool {
    v >= SUPPORTED_16 || (SUPPORTED_15 <= v && v < [1, 6, 0])
}

/// TEMP(maint-1.6): the first MAINT 1.6 (V1_6_CUTOFF), the one build below the support boundary the app still supports,
/// until the installation runs EMU1101-EXT Full. Every exception for it asks here. To drop it: this module and its uses, its entry
/// in `openbms_flash::known::PATCH_BUILDS` and every place marked TEMP(maint-1.6) (tests, doc comments, the help texts in
/// `tools/gen_help.py` and `tools/param_help.py`). With it go the `MNT` signature and the format-3 decoding
/// (`ExtInfo::classify`).
pub mod temp16 {
    use super::ExtInfo;

    /// MNT format, patch version, build ID and image flags (06: power/clock and the pack cutoff; bit 3 at 0 allows power
    /// and clock, unlike EMU1101-EXT)
    pub const FORMAT: u8 = 3;
    pub const PATCH: [u8; 2] = [1, 6];
    pub const BUILD: &str = "18daf61d8a93671d";
    pub const BUILD_ID: [u8; 8] = [0x18, 0xDA, 0xF6, 0x1D, 0x8A, 0x93, 0x67, 0x1D];
    pub const FLAGS: u8 = 0x06;

    /// This build's live identity, by its build ID (no other format-3 1.6).
    pub fn is(info: &ExtInfo) -> bool {
        info.format == FORMAT && info.patch == PATCH && info.build_id == BUILD_ID
    }

    /// The version a stored proof's release names for it: exactly „1.6“ („… MAINT 1.6“), never „1.6 positive“ or „1.6.0“.
    pub fn is_release_version(version: &str) -> bool {
        version.trim() == "1.6"
    }
}

/// The `EXT05` answer to `0x64 INFO=03` (TEMP(maint-1.6): or the `MNT` format 3): which build the pack runs. No runtime
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtInfo {
    pub format: u8,
    /// OEM base major, minor, revision (16.06.13 = `[0x10, 0x06, 0x0D]`; revision `FF`: unknown)
    pub base: [u8; 3],
    /// patch version major, minor
    pub patch: [u8; 2],
    /// format 5: the third patch byte (Full 1.6.5 → 5); 0 with format 3
    pub patch_rev: u8,
    /// identifies the build (not an authentication of the device)
    pub build_id: [u8; 8],
    /// image flag bit 0: the hard-stop patch is compiled in, fixed by the image; None with a configurable hard stop (flag
    /// bit 7, format 5: bit 0 is then only the default, `hard_stop_default`)
    pub hard_stop: Option<bool>,
    /// image flag bit 1: on/off, restart and the clock over the CAN socket (0x45/0x4D/0x4E) with the shared lock Group0
    /// bit 3 are compiled in
    pub power_rtc: bool,
    /// image flag bit 2: the stored pack cutoff of the Full line (Group0 bit 4 extended off, bit 5 the lock state) is
    /// compiled in; a capability, never the lock itself
    pub pack_cutoff: bool,
    /// image flag bit 4 (format 5): status `02`, clear `00` and identity `03` also over RS485-1/2 (UART0) at the pack's
    /// own address; the start stays on the CAN socket
    pub uart0: bool,
    /// image flag bits 3 and 5–7 as reported, and bit 0 where it is only the default of a configurable hard stop (bit 3:
    /// the Group0 bits 3/4 enable at 1; format 5: bit 5 the 64/04 diagnosis, bit 6 the SOC checkpoint before a planned
    /// restart, bit 7 the configurable hard stop; read through the methods below)
    pub reserved_flags: u8,
    /// format 5: the read-capability mask (bit n = `CAP_*`), only known bits (`CAPS_KNOWN`); 0 with format 3
    pub read_caps: u16,
}

/// Capability bits of the format-5 mask: the history 0x4B over the CAN socket, the calibration pages 64/05, the
/// stored SOC records in 64/06, the full-width live energy counters in 64/06, and (Full 1.6.7) the configurable CCL
/// decrease step 64/07–08 (`ccl`).
pub const CAP_HISTORY: u8 = 0;
pub const CAP_CALIBRATION: u8 = 1;
pub const CAP_SOC_STORE: u8 = 2;
pub const CAP_ENERGY: u8 = 3;
pub const CAP_CCL_STEP: u8 = 4;
/// The bits this app knows; a mask with any other bit is a format it does not know (never guessed).
pub const CAPS_KNOWN: u16 = 0x001F;

/// What an RTN 00 answer to `0x64 INFO=03` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoAnswer {
    /// the 21-byte `EXT05` identity (TEMP(maint-1.6): or the 18-byte `MNT` format 3)
    Known(ExtInfo),
    /// `EXT` (or `MNT`) with another format or length, or a mask with unknown bits: a patch the app does not know, never
    /// decoded
    OtherFormat,
    /// no `EXT` or `MNT` signature: not an identity
    Broken,
}

impl ExtInfo {
    /// `EXT05`: 21 bytes; TEMP(maint-1.6): `MNT` format 3, 18 bytes (no third patch byte, no mask)
    pub const LEN: usize = 21;
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
        check_address(address)?;
        encode_request(address, 0x64, &[0x03])
    }

    /// Exactly the 21-byte `EXT05` (TEMP(maint-1.6): or the 18-byte `MNT` format 3).
    pub fn parse(info: &[u8]) -> Result<Self> {
        match Self::classify(info) {
            InfoAnswer::Known(i) => Ok(i),
            _ => Err(Error::Layout("0x64/03 answer is not EXT format 05 with 21 bytes")),
        }
    }

    /// Whether `info` is shaped like a `64/03` identity at all: a signature, a format byte 01–1F and at most
    /// `IDENTITY_MAX` bytes. Other answers that only start with `EXT`/`MNT` never count as an identity, not even as an
    /// unknown one: a 0x51 device name has a printable character or NUL padding after them and 32 bytes in all.
    pub fn identity_shaped(info: &[u8]) -> bool {
        (4..=Self::IDENTITY_MAX).contains(&info.len()) && (&info[0..3] == SIGNATURE || &info[0..3] == SIGNATURE_MNT) && (0x01..0x20).contains(&info[3])
    }

    /// The longest INFO an unknown identity format may have to be taken as one (the known ones have 18 and 21 bytes).
    pub const IDENTITY_MAX: usize = 24;

    pub fn classify(info: &[u8]) -> InfoAnswer {
        if info.len() < 3 {
            return InfoAnswer::Broken;
        }
        let sig = &info[0..3];
        if info.len() == 3 {
            // a signature without a format byte
            return if sig == SIGNATURE || sig == SIGNATURE_MNT { InfoAnswer::OtherFormat } else { InfoAnswer::Broken };
        }
        // EXT05: a third patch byte, so the build ID starts at 10, the flags at 18, the mask at 19–20
        let ext = if sig == SIGNATURE && info[3] == 5 && info.len() == Self::LEN {
            true
        } else if sig == SIGNATURE_MNT && info[3] == 3 && info.len() == Self::LEN3 {
            // TEMP(maint-1.6): MNT format 3, build ID at 9, flags at 17
            false
        } else if sig == SIGNATURE || sig == SIGNATURE_MNT {
            return InfoAnswer::OtherFormat;
        } else {
            return InfoAnswer::Broken;
        };
        let (rev, id) = if ext { (info[9], 10) } else { (0, 9) };
        let read_caps = if ext { u16::from_be_bytes([info[19], info[20]]) } else { 0 };
        if read_caps & !CAPS_KNOWN != 0 {
            return InfoAnswer::OtherFormat;
        }
        let flags = info[id + 8];
        let bit = |b: u8| flags & (1 << b) != 0;
        // bit 4 only in EXT05; bit 3 and 5–7 kept as reported
        let uart0 = ext && bit(FLAG_UART0);
        // a configurable hard stop (EXT05, bit 7) keeps bit 0 as its default, not as a compiled-in hard stop
        let configurable = ext && bit(FLAG_HARD_STOP);
        let reserved_flags = flags & if ext { 0xE8 | u8::from(configurable) } else { 0xF8 };
        let mut build_id = [0u8; 8];
        build_id.copy_from_slice(&info[id..id + 8]);
        InfoAnswer::Known(Self {
            format: info[3], base: [info[4], info[5], info[6]], patch: [info[7], info[8]], patch_rev: rev, build_id,
            hard_stop: (!configurable).then_some(bit(FLAG_HARD_STOP_ON)), power_rtc: bit(FLAG_POWER), pack_cutoff: bit(FLAG_CUTOFF), uart0, reserved_flags,
            read_caps,
        })
    }

    /// The image flag byte as reported.
    pub fn image_flags(&self) -> u8 {
        (u8::from(self.hard_stop == Some(true)) << FLAG_HARD_STOP_ON) | (u8::from(self.power_rtc) << FLAG_POWER) | (u8::from(self.pack_cutoff) << FLAG_CUTOFF)
            | (u8::from(self.uart0) << FLAG_UART0)
            | self.reserved_flags
    }

    fn ext_flag(&self, bit: u8) -> bool {
        self.format == 5 && self.reserved_flags & (1 << bit) != 0
    }

    /// Image flag bit 3: the Group0 bits 3/4 enable at 1 (bit 3 at 1 allows power and clock, bit 4 at 1 the extended off).
    pub fn positive(&self) -> bool {
        self.reserved_flags & (1 << FLAG_POSITIVE) != 0
    }

    /// Image flag bit 5: the memory diagnosis `0x64/04` over RS485-1/2 and the CAN socket.
    pub fn diagnostics(&self) -> bool {
        self.ext_flag(FLAG_DIAGNOSIS)
    }

    /// Image flag bit 6 (Full): a planned restart (0x45/06) first saves the SOC record verified into both copies; a
    /// memory error answers RTN 08 and does not restart.
    pub fn checkpoint(&self) -> bool {
        self.ext_flag(FLAG_CHECKPOINT)
    }

    /// Image flag bit 7 (Full): the hard stop at a recharge pause is chosen by Group0 bits 6/7.
    pub fn hard_stop_configurable(&self) -> bool {
        self.ext_flag(FLAG_HARD_STOP)
    }

    /// The default a configurable hard stop takes while Group0 bit 7 is 1 (image flag bit 0); None: not configurable.
    pub fn hard_stop_default(&self) -> Option<bool> {
        self.hard_stop_configurable().then_some(self.reserved_flags & (1 << FLAG_HARD_STOP_ON) != 0)
    }

    /// Format 5, read-capability bit `bit` (`CAP_*`): the read is offered only with its own bit.
    pub fn cap(&self, bit: u8) -> bool {
        self.read_caps & (1 << bit) != 0
    }

    /// Whether the app supports this build's patch functions: EXT05 from Light 1.5.4 and from Full 1.6.4 on. Below, the
    /// patch is shown but treated like the original firmware.
    pub fn supported(&self) -> bool {
        // TEMP(maint-1.6)
        temp16::is(self) || (self.format == 5 && supported_version([self.patch[0], self.patch[1], self.patch_rev]))
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

    /// "1.5.5"; TEMP(maint-1.6): the format-3 build "1.6"
    pub fn patch_text(&self) -> String {
        if self.format == 5 { format!("{}.{}.{}", self.patch[0], self.patch[1], self.patch_rev) } else { format!("{}.{}", self.patch[0], self.patch[1]) }
    }

    /// The display name: "EMU1101-EXT Light 1.5.5", "EMU1101-EXT Full 1.6.5" (another line: "EMU1101-EXT 1.7.0"); the
    /// OEM base is `base_text`. TEMP(maint-1.6): the `MNT` build keeps "16.06.13 MAINT 1.6".
    pub fn version(&self) -> String {
        if self.format != 5 {
            return format!("{} MAINT {}", self.base_text(), self.patch_text());
        }
        match line_name(self.patch) {
            Some(line) => format!("EMU1101-EXT {line} {}", self.patch_text()),
            None => format!("EMU1101-EXT {}", self.patch_text()),
        }
    }
}

/// The line of a patch version: "Light" (1.5), "Full" (1.6); None for another.
pub fn line_name(patch: [u8; 2]) -> Option<&'static str> {
    match patch {
        [1, 5] => Some("Light"),
        [1, 6] => Some("Full"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    /// The INFO bytes of the four releases (EMU1101-EXT Light 1.5.4/1.5.5, Full 1.6.4/1.6.5).
    const LIGHT_154: &str = "4558540510060D010504FFE3ACF7E52BED803A000F";
    const LIGHT_155: &str = "4558540510060D010505082CB731C006AAC53A000F";
    const FULL_164: &str = "4558540510060D0106044F4D2715C9EE553CFE000F";
    const FULL_165: &str = "4558540510060D01060513464FC91A6E6A20FE000F";
    /// Light 1.5.6, Full 1.6.6 (early polling) and Full 1.6.7 (the CCL decrease step, mask 001F)
    const LIGHT_156: &str = "4558540510060D010506FE7CBA82F861F3373A000F";
    const FULL_166: &str = "4558540510060D0106062508B93F45C29F31FE000F";
    const FULL_167: &str = "4558540510060D010607F445666C205184A8FE001F";
    /// TEMP(maint-1.6): the first MAINT 1.6, MNT format 3
    const TEMP_16: &str = "4D4E540310060D010618DAF61D8A93671D06";

    #[test]
    fn frames_match_the_patch_documentation() {
        assert_eq!(MaintState::request().unwrap(), b"~20004664E00202FD31\r");
        assert_eq!(MaintState::set_request(true).unwrap(), b"~20004664A00601C35AFC46\r");
        assert_eq!(MaintState::set_request(false).unwrap(), b"~20004664A00600C35AFC47\r");
        assert_eq!(ExtInfo::request().unwrap(), b"~20004664E00203FD30\r");
    }

    #[test]
    fn the_ext05_identity_has_21_bytes() {
        let a = ExtInfo::parse(&hex(LIGHT_155)).unwrap();
        assert_eq!((a.format, a.base, a.patch, a.patch_rev), (5, [16, 6, 13], [1, 5], 5));
        assert_eq!((a.build_id_hex(), a.image_flags(), a.read_caps), ("082cb731c006aac5".to_string(), 0x3A, 0x0F));
        assert_eq!((a.version(), a.base_text()), ("EMU1101-EXT Light 1.5.5".to_string(), "16.06.13".to_string()));
        assert_eq!((a.diagnostics(), a.checkpoint(), a.hard_stop_configurable(), a.positive(), a.uart0), (true, false, false, true, true));
        assert_eq!((a.power_rtc, a.pack_cutoff, a.hard_stop, a.supported()), (true, false, Some(false), true));
        let b = ExtInfo::parse(&hex(FULL_165)).unwrap();
        assert_eq!((b.version(), b.build_id_hex(), b.image_flags()), ("EMU1101-EXT Full 1.6.5".to_string(), "13464fc91a6e6a20".to_string(), 0xFE));
        assert_eq!((b.diagnostics(), b.checkpoint(), b.hard_stop_configurable(), b.hard_stop, b.hard_stop_default()), (true, true, true, None, Some(false)));
        assert_eq!((b.pack_cutoff, b.positive(), b.supported()), (true, true, true));
        assert!([CAP_HISTORY, CAP_CALIBRATION, CAP_SOC_STORE, CAP_ENERGY].iter().all(|&c| a.cap(c) && b.cap(c)));
        // .4 and .5 differ only by version and build ID
        let (a4, b4) = (ExtInfo::parse(&hex(LIGHT_154)).unwrap(), ExtInfo::parse(&hex(FULL_164)).unwrap());
        assert_eq!((a4.version(), a4.build_id_hex()), ("EMU1101-EXT Light 1.5.4".to_string(), "ffe3acf7e52bed80".to_string()));
        assert_eq!((b4.version(), b4.build_id_hex()), ("EMU1101-EXT Full 1.6.4".to_string(), "4f4d2715c9ee553c".to_string()));
        assert_eq!((a4.image_flags(), a4.read_caps, b4.image_flags(), b4.read_caps), (0x3A, 0x0F, 0xFE, 0x0F));
        assert!(a4.supported() && b4.supported());
        // through a frame: LENGTH 402A carries the 42 hex digits, 60 frame bytes in all
        let frame = crate::frame::encode_request(0, 0, &hex(FULL_165)).unwrap();
        assert_eq!((&frame[9..13], frame.len()), (&b"402A"[..], 60));
        assert_eq!(ExtInfo::classify(&crate::frame::parse(&frame).unwrap().info), InfoAnswer::Known(b));
    }

    #[test]
    fn the_image_flags_and_the_read_caps_are_read_separately() {
        // FF: a configurable hard stop with hard as default; the flag byte comes back unchanged
        let mut ff = hex(FULL_165);
        ff[18] = 0xFF;
        let i = ExtInfo::parse(&ff).unwrap();
        assert_eq!((i.hard_stop_default(), i.image_flags()), (Some(true), 0xFF));
        // a Light build with a fixed hard stop: 3B
        let mut l = hex(LIGHT_155);
        l[18] = 0x3B;
        assert_eq!(ExtInfo::parse(&l).unwrap().hard_stop, Some(true));
        // a partial mask names only its bits
        let mut p = hex(FULL_165);
        p[20] = 0x05;
        let only = ExtInfo::parse(&p).unwrap();
        assert_eq!((only.cap(CAP_HISTORY), only.cap(CAP_CALIBRATION), only.cap(CAP_SOC_STORE), only.cap(CAP_ENERGY)), (true, false, true, false));
        // Full 1.6.7: bit 4 the CCL decrease step, apart from the four reads
        let mut c = hex(FULL_165);
        c[20] = 0x1F;
        let ccl = ExtInfo::parse(&c).unwrap();
        assert!(ccl.cap(CAP_CCL_STEP) && !b_ccl(&hex(FULL_165)));
        // an unknown bit: a format the app does not know, never partly read
        for (at, v) in [(20, 0x3F), (20, 0x2F), (19, 0x80), (19, 0x01)] {
            let mut u = hex(FULL_165);
            u[at] = v;
            assert_eq!(ExtInfo::classify(&u), InfoAnswer::OtherFormat);
        }
    }

    fn b_ccl(info: &[u8]) -> bool {
        ExtInfo::parse(info).unwrap().cap(CAP_CCL_STEP)
    }

    #[test]
    fn the_new_releases_report_their_identity_and_capabilities() {
        // Light 1.5.6 and Full 1.6.6: early polling, the same flags and mask 000F; Full 1.6.7 adds bit 4 (001F)
        for (info, name, build, caps) in [
            (LIGHT_156, "EMU1101-EXT Light 1.5.6", "fe7cba82f861f337", 0x000F),
            (FULL_166, "EMU1101-EXT Full 1.6.6", "2508b93f45c29f31", 0x000F),
            (FULL_167, "EMU1101-EXT Full 1.6.7", "f445666c205184a8", 0x001F),
        ] {
            let i = ExtInfo::parse(&hex(info)).unwrap();
            assert_eq!((i.version(), i.build_id_hex(), i.read_caps, i.supported()), (name.to_string(), build.to_string(), caps, true));
            assert_eq!(i.cap(CAP_CCL_STEP), caps & 0x10 != 0, "{name}");
            assert!([CAP_HISTORY, CAP_CALIBRATION, CAP_SOC_STORE, CAP_ENERGY].iter().all(|&c| i.cap(c)), "{name}");
        }
        assert_eq!(ExtInfo::parse(&hex(LIGHT_156)).unwrap().image_flags(), 0x3A);
        assert_eq!(ExtInfo::parse(&hex(FULL_167)).unwrap().image_flags(), 0xFE);
        // the version alone gives no capability: a 1.6.7 identity with mask 000F has no CCL step
        let mut v = hex(FULL_167);
        v[20] = 0x0F;
        assert!(!b_ccl(&v));
    }

    #[test]
    fn only_ext05_with_21_bytes_is_decoded() {
        let good = hex(LIGHT_155);
        assert_eq!(ExtInfo::classify(&good[..20]), InfoAnswer::OtherFormat);
        assert_eq!(ExtInfo::classify(&[good.clone(), vec![0]].concat()), InfoAnswer::OtherFormat);
        for format in [2, 3, 4, 6] {
            let mut f = good.clone();
            f[3] = format;
            assert_eq!(ExtInfo::classify(&f), InfoAnswer::OtherFormat, "EXT format {format}");
        }
        // the old MAINT identities (MNT formats 2, 4 and 5) are not decoded any more
        for old in ["4D4E540210060D0105909A604E60E9225B", "4D4E540410060D010603BAA01ECD5AF4EB28FE", "4D4E540510060D010604A8676B52BAE281FFFE000F"] {
            assert_eq!(ExtInfo::classify(&hex(old)), InfoAnswer::OtherFormat, "{old}");
        }
        // short answers: no signature is broken, a signature without the rest another format
        for sig in [b"EXT", b"MNT"] {
            for n in 0..3 {
                assert_eq!(ExtInfo::classify(&sig[..n]), InfoAnswer::Broken, "{n} bytes");
            }
            assert_eq!(ExtInfo::classify(sig), InfoAnswer::OtherFormat);
            assert_eq!(ExtInfo::classify(&[sig.as_slice(), &[5]].concat()), InfoAnswer::OtherFormat);
        }
        // identity-shaped: a signature and a binary format byte; a device name starting with EXT is not
        assert!(ExtInfo::identity_shaped(&good) && ExtInfo::identity_shaped(&hex(TEMP_16)));
        assert!(ExtInfo::identity_shaped(b"EXT\x09"));
        assert!(!ExtInfo::identity_shaped(b"EXT-PACK\0\0") && !ExtInfo::identity_shaped(b"EXT") && !ExtInfo::identity_shaped(b"MNTX"));
        // NUL padding after a three-letter name, a format byte 00, a 0x51-sized answer
        assert!(!ExtInfo::identity_shaped(b"EXT\0\0\0\0\0\0\0") && !ExtInfo::identity_shaped(b"MNT\0"));
        assert!(!ExtInfo::identity_shaped(&[b"EXT".as_slice(), &[5; 29]].concat()));
        let mut broken = good.clone();
        broken[0] = b'X';
        assert_eq!(ExtInfo::classify(&broken), InfoAnswer::Broken);
        assert_eq!(ExtInfo::classify(&[]), InfoAnswer::Broken);
        assert!(ExtInfo::parse(&broken).is_err());
    }

    #[test]
    fn the_support_boundary_is_light_1_5_4_and_full_1_6_4() {
        let at = |minor: u8, rev: u8| {
            let mut v = hex(FULL_165);
            v[8] = minor;
            v[9] = rev;
            ExtInfo::parse(&v).unwrap()
        };
        assert!(at(5, 4).supported() && at(5, 5).supported() && at(6, 4).supported() && at(6, 5).supported());
        assert!(!at(5, 3).supported() && !at(6, 3).supported() && !at(4, 9).supported());
        assert!(at(7, 0).supported(), "a later line");
        assert_eq!(at(7, 0).version(), "EMU1101-EXT 1.7.0");
        // an unknown OEM revision
        let mut q = hex(LIGHT_155);
        q[6] = 0xFF;
        assert_eq!(ExtInfo::parse(&q).unwrap().base_text(), "16.06.?");
    }

    #[test]
    fn temp16_the_first_maint_1_6_is_still_read() {
        // TEMP(maint-1.6): 16.06.13, patch 1.6, flags 06 (power and clock, pack cutoff; no hard stop)
        let i = ExtInfo::parse(&hex(TEMP_16)).unwrap();
        assert_eq!((i.format, i.patch, i.patch_rev, i.hard_stop, i.power_rtc, i.pack_cutoff, i.uart0, i.read_caps), (3, [1, 6], 0, Some(false), true, true, false, 0));
        assert_eq!((i.build_id_hex(), i.version(), i.supported()), ("18daf61d8a93671d".to_string(), "16.06.13 MAINT 1.6".to_string(), true));
        assert!(temp16::is(&i));
        // another format-3 build (flags 0E) is decoded but not supported
        assert!(!ExtInfo::parse(&hex("4D4E540310060D010639E0C5D9CCF133960E")).unwrap().supported());
        // bits 4–7 carry no function in format 3; the byte comes back unchanged
        let mut f = hex(TEMP_16);
        f[17] = 0xF6;
        let f = ExtInfo::parse(&f).unwrap();
        assert_eq!((f.uart0, f.diagnostics(), f.checkpoint(), f.hard_stop_configurable(), f.image_flags()), (false, false, false, false, 0xF6));
        // format 3 under the EXT signature is not this build
        let mut e = hex(TEMP_16);
        e[0..3].copy_from_slice(b"EXT");
        assert_eq!(ExtInfo::classify(&e), InfoAnswer::OtherFormat);
    }

    #[test]
    fn the_memory_diagnosis_reads_format_01_with_10_bytes() {
        assert_eq!(MemoryDiagnosis::request_at(3).unwrap(), crate::frame::encode_request(3, 0x64, &[0x04]).unwrap());
        assert!(MemoryDiagnosis::request_at(22).is_err());
        // the contract's example: both copies valid and equal, both locks off, cache valid and consistent
        let d = MemoryDiagnosis::parse(&hex("01010101010101000006")).unwrap();
        assert_eq!((d.params, d.params_compare, d.soc, d.soc_compare), (["valid", "valid"], "same", ["valid", "valid"], "same"));
        assert_eq!((d.lock, d.cache), ([Some(false), Some(false)], 0x06));
        // Light: no lock or cache support (FF FF 00); an empty backup, differing SOC copies
        let d = MemoryDiagnosis::parse(&hex("01010000010102FFFF00")).unwrap();
        assert_eq!((d.params[1], d.params_compare, d.soc_compare, d.lock), ("empty", "not-comparable", "different", [None, None]));
        assert!(MemoryDiagnosis::parse(&hex("020101010101010000")).is_err());
        assert!(MemoryDiagnosis::parse(&hex("0201010101010100000600")).is_err());
    }

    #[test]
    fn status_and_clear_are_addressed_for_rs485() {
        let f = crate::frame::parse(&MaintState::request_at(7).unwrap()).unwrap();
        assert_eq!((f.address, f.rtn, f.info.as_slice()), (7, 0x64, &[0x02][..]));
        let f = crate::frame::parse(&MaintState::set_request_at(7, false).unwrap()).unwrap();
        assert_eq!((f.address, f.info.as_slice()), (7, &[0x00, 0xC3, 0x5A][..]));
        assert_eq!(MaintState::set_request(true).unwrap(), MaintState::set_request_at(0, true).unwrap());
        assert!(MaintState::request_at(22).is_err() && MaintState::set_request_at(22, false).is_err());
    }

    #[test]
    fn the_identity_request_is_addressed() {
        let f = crate::frame::parse(&ExtInfo::request_at(11).unwrap()).unwrap();
        assert_eq!((f.address, f.cid1, f.rtn, f.info.as_slice()), (11, 0x46, 0x64, &[0x03][..]));
        assert_eq!(ExtInfo::request().unwrap(), ExtInfo::request_at(0).unwrap());
        assert!(ExtInfo::request_at(22).is_err());
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
