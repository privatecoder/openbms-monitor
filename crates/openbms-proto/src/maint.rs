//! CID2 0x64: identity and maintenance mode of a patched master firmware (16.06.13 `_MAINT`), over the CAN socket bus;
//! builds with image flag bit 4 (MNT format 4, MAINT 1.5.1/1.6.1) also answer status, clear and identity over RS485-1/2
//! at their own address.
//!
//! The patch keeps a persistent marker in flash. While it is set, the master treats its slave count as 0: it stops
//! polling the pack bus (RS485-1/2) and answers there itself, so its parameters can be read and it can be updated.
//! Original firmware answers 0x64 with RTN 04 (unknown command); slaves never answer 0x64.
//!
//! `INFO=03` reads the identity ([`MaintInfo`], no runtime state; newer builds answer it at every pack's own address, the
//! address only in the frame): format 2 with 17 bytes, format 3 with 18 bytes and
//! an image flag byte (bit 0: the hard-stop patch is compiled in), format 4 with 19 bytes (a third patch byte) or format 5 with
//! 21 bytes (format 4 plus a big-endian read-capability mask, MAINT 1.5.4/1.6.4). `INFO=02` reads the state
//! ([`MaintState`], 5 bytes); start and stop answer with the same 5 bytes. The 5-byte layout holds only once a format-2
//! identity of a supported build was read on the same connection.

use crate::access::{FLAG_CHECKPOINT, FLAG_CUTOFF, FLAG_DIAGNOSIS, FLAG_HARD_STOP, FLAG_HARD_STOP_ON, FLAG_POSITIVE, FLAG_POWER, FLAG_UART0};
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

/// Answer to `0x64 INFO=04` (MAINT 1.5.3/1.6.3, image flag bit 5): the stored parameter and SOC copies as read straight
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

/// The lowest supported build of the 1.5 line and of the 1.6 line and later (major, minor, revision).
pub const SUPPORTED_15: [u8; 3] = [1, 5, 3];
pub const SUPPORTED_16: [u8; 3] = [1, 6, 3];

/// Whether the app supports a patch version (major, minor, revision): from 1.5.3 in the 1.5 line, from 1.6.3 on. One
/// rule for a live identity (`MaintInfo::supported`) and a stored proof's release name (`profile::release_patch`).
pub fn supported_version(v: [u8; 3]) -> bool {
    v >= SUPPORTED_16 || (SUPPORTED_15 <= v && v < [1, 6, 0])
}

/// TEMP(maint-1.6): the first MAINT 1.6 (V1_6_CUTOFF), the one build below the support boundary the app still supports,
/// until the installation runs 1.6.3. Every exception for it asks here. To drop it: this module and its uses, its entry
/// in `openbms_flash::known::PATCH_BUILDS` and every place marked TEMP(maint-1.6) (tests, doc comments, the help texts in
/// `tools/gen_help.py` and `tools/param_help.py`). The format-3 decoding stays: it shows older builds as unsupported.
pub mod temp16 {
    use super::MaintInfo;

    /// MNT format, patch version, build ID and image flags (06: power/clock and the pack cutoff; bit 3 at 0 allows power
    /// and clock, unlike 1.5.3/1.6.3)
    pub const FORMAT: u8 = 3;
    pub const PATCH: [u8; 2] = [1, 6];
    pub const BUILD: &str = "18daf61d8a93671d";
    pub const BUILD_ID: [u8; 8] = [0x18, 0xDA, 0xF6, 0x1D, 0x8A, 0x93, 0x67, 0x1D];
    pub const FLAGS: u8 = 0x06;

    /// This build's live identity, by its build ID (no other format-3 1.6).
    pub fn is(info: &MaintInfo) -> bool {
        info.format == FORMAT && info.patch == PATCH && info.build_id == BUILD_ID
    }

    /// The version a stored proof's release names for it: exactly „1.6“ („… MAINT 1.6“), never „1.6 positive“ or „1.6.0“.
    pub fn is_release_version(version: &str) -> bool {
        version.trim() == "1.6"
    }
}

/// Format-2, -3, -4 or -5 answer to `0x64 INFO=03`: which build the master runs. No runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintInfo {
    pub format: u8,
    /// OEM base major, minor, revision (16.06.13 = `[0x10, 0x06, 0x0D]`; revision `FF`: unknown)
    pub base: [u8; 3],
    /// patch version major, minor
    pub patch: [u8; 2],
    /// formats 4 and 5: the third patch byte (MAINT 1.6.1 → 1); 0 with formats 2 and 3
    pub patch_rev: u8,
    /// identifies the build (not an authentication of the device)
    pub build_id: [u8; 8],
    /// image flag bit 0: the hard-stop patch is compiled in, fixed by the image; None with format 2 (not reported) and
    /// with a configurable hard stop (flag bit 7, format 4: bit 0 is then only the default, `hard_stop_default`)
    pub hard_stop: Option<bool>,
    /// format 3, image flag bit 1: ein/aus, restart and the clock over the CAN socket (0x45/0x4D/0x4E) with the shared
    /// lock Group0 bit 3 are compiled in; false with format 2 (no such build)
    pub power_rtc: bool,
    /// format 3, image flag bit 2: the stored pack cutoff of MAINT 1.6 (Group0 bit 4 extended off, bit 5 the lock state)
    /// is compiled in; a capability, never the lock itself; false with format 2
    pub pack_cutoff: bool,
    /// image flag bit 4 (format 4): status `02`, clear `00` and identity `03` also over RS485-1/2 (UART0) at the pack's
    /// own address; the start stays on the CAN socket
    pub uart0: bool,
    /// image flag bits 3 and 5–7 as reported, and bit 0 where it is only the default of a configurable hard stop (bit 3:
    /// the positive Group0 bits 3/4; format 4: bit 5 the 64/04 diagnosis, bit 6 the SOC checkpoint before a planned
    /// restart, bit 7 the configurable hard stop; read through the methods below); 0 with format 2
    pub reserved_flags: u8,
    /// format 5: the read-capability mask (bit n = `CAP_*`), only known bits (`CAPS_KNOWN`); 0 with formats 2–4
    pub read_caps: u16,
}

/// Read-capability bits of the format-5 mask: the history 0x4B over the CAN socket, the calibration pages 64/05, the
/// stored SOC records in 64/06, the full-width live energy counters in 64/06.
pub const CAP_HISTORY: u8 = 0;
pub const CAP_CALIBRATION: u8 = 1;
pub const CAP_SOC_STORE: u8 = 2;
pub const CAP_ENERGY: u8 = 3;
/// The bits this app knows; a mask with any other bit is a format it does not know (never guessed).
pub const CAPS_KNOWN: u16 = 0x000F;

/// What an RTN 00 answer to `0x64 INFO=03` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoAnswer {
    /// the 17-byte format-2, the 18-byte format-3, the 19-byte format-4 or the 21-byte format-5 identity
    Known(MaintInfo),
    /// `MNT` with another format or length (e.g. the historic 22-byte format 1), or a format-5 mask with unknown bits:
    /// a patch the app does not
    /// know, never decoded
    OtherFormat,
    /// no `MNT` signature: not an identity
    Broken,
}

impl MaintInfo {
    /// format 2: 17 bytes; format 3: 18 bytes (the image flags after the build ID); format 4: 19 bytes (a third patch
    /// byte before the build ID)
    pub const LEN: usize = 17;
    pub const LEN3: usize = 18;
    pub const LEN4: usize = 19;
    pub const LEN5: usize = 21;
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

    /// Exactly the 17-byte format 2, the 18-byte format 3, the 19-byte format 4 or the 21-byte format 5.
    pub fn parse(info: &[u8]) -> Result<Self> {
        match Self::classify(info) {
            InfoAnswer::Known(i) => Ok(i),
            _ => Err(Error::Layout("0x64/03 answer is none of the MNT formats 2 (17 bytes), 3 (18), 4 (19) or 5 (21)")),
        }
    }

    pub fn classify(info: &[u8]) -> InfoAnswer {
        if info.len() < 4 || &info[0..3] != SIGNATURE {
            return InfoAnswer::Broken;
        }
        // format 4 has a third patch byte, so its build ID and flags sit one byte later; format 5 appends the mask
        let (rev, id) = match (info[3], info.len()) {
            (2, Self::LEN) | (3, Self::LEN3) => (0, 9),
            (4, Self::LEN4) | (5, Self::LEN5) => (info[9], 10),
            _ => return InfoAnswer::OtherFormat,
        };
        let read_caps = if info[3] == 5 { u16::from_be_bytes([info[19], info[20]]) } else { 0 };
        if read_caps & !CAPS_KNOWN != 0 {
            return InfoAnswer::OtherFormat;
        }
        let f4 = info[3] >= 4;
        let flags = if info[3] == 2 { None } else { Some(info[id + 8]) };
        let bit = |b: u8| flags.is_some_and(|f| f & (1 << b) != 0);
        // bit 4 only from format 4; bit 3 and 5–7 kept as reported
        let uart0 = f4 && bit(FLAG_UART0);
        // a configurable hard stop (from format 4, bit 7) keeps bit 0 as its default, not as a compiled-in hard stop
        let configurable = f4 && bit(FLAG_HARD_STOP);
        let reserved_flags = flags.map_or(0, |f| f & if f4 { 0xE8 | u8::from(configurable) } else { 0xF8 });
        let mut build_id = [0u8; 8];
        build_id.copy_from_slice(&info[id..id + 8]);
        InfoAnswer::Known(Self {
            format: info[3], base: [info[4], info[5], info[6]], patch: [info[7], info[8]], patch_rev: rev, build_id,
            hard_stop: flags.filter(|_| !configurable).map(|f| f & (1 << FLAG_HARD_STOP_ON) != 0), power_rtc: bit(FLAG_POWER), pack_cutoff: bit(FLAG_CUTOFF), uart0, reserved_flags,
            read_caps,
        })
    }

    /// The image flag byte as reported (formats 3 and 4); a format-2 identity reports none: 0.
    pub fn image_flags(&self) -> u8 {
        (u8::from(self.hard_stop == Some(true)) << FLAG_HARD_STOP_ON) | (u8::from(self.power_rtc) << FLAG_POWER) | (u8::from(self.pack_cutoff) << FLAG_CUTOFF)
            | (u8::from(self.uart0) << FLAG_UART0)
            | self.reserved_flags
    }

    fn format4_flag(&self, bit: u8) -> bool {
        self.format >= 4 && self.reserved_flags & (1 << bit) != 0
    }

    /// Image flag bit 3: Group0 bits 3/4 positive (bit 3 at 1 allows power and clock, bit 4 at 1 the extended off).
    pub fn positive(&self) -> bool {
        self.reserved_flags & (1 << FLAG_POSITIVE) != 0
    }

    /// Format 4, image flag bit 5: the memory diagnosis `0x64/04` over RS485-1/2 and the CAN socket (MAINT 1.5.3/1.6.3).
    pub fn diagnostics(&self) -> bool {
        self.format4_flag(FLAG_DIAGNOSIS)
    }

    /// Format 4, image flag bit 6: a planned restart (0x45/06) first saves the SOC record verified into both copies; a
    /// memory error answers RTN 08 and does not restart (MAINT 1.6.3).
    pub fn checkpoint(&self) -> bool {
        self.format4_flag(FLAG_CHECKPOINT)
    }

    /// Format 4, image flag bit 7: the hard stop at a recharge pause is chosen by Group0 bits 6/7 (MAINT 1.6.3).
    pub fn hard_stop_configurable(&self) -> bool {
        self.format4_flag(FLAG_HARD_STOP)
    }

    /// The default a configurable hard stop takes while Group0 bit 7 is 1 (image flag bit 0); None: not configurable.
    pub fn hard_stop_default(&self) -> Option<bool> {
        self.hard_stop_configurable().then_some(self.reserved_flags & (1 << FLAG_HARD_STOP_ON) != 0)
    }

    /// Format 5, read-capability bit `bit` (`CAP_*`): the read is offered only with its own bit.
    pub fn cap(&self, bit: u8) -> bool {
        self.read_caps & (1 << bit) != 0
    }

    /// Whether the app supports this build's patch functions: MNT format 4 or 5 from MAINT 1.5.3 in the 1.5 line and
    /// from 1.6.3 on. Below, the patch is shown but treated like the original firmware.
    pub fn supported(&self) -> bool {
        // TEMP(maint-1.6)
        temp16::is(self) || (matches!(self.format, 4 | 5) && supported_version([self.patch[0], self.patch[1], self.patch_rev]))
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

    /// "1.5", or with format 4 "1.6.1"
    pub fn patch_text(&self) -> String {
        if self.format >= 4 { format!("{}.{}.{}", self.patch[0], self.patch[1], self.patch_rev) } else { format!("{}.{}", self.patch[0], self.patch[1]) }
    }

    /// "16.06.13 MAINT 1.5", "16.06.13 MAINT 1.6.1"
    pub fn version(&self) -> String {
        format!("{} MAINT {}", self.base_text(), self.patch_text())
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
        // other bits do not set it
        assert_eq!(MaintInfo::parse(&info3(0x0D, 0xFE)).unwrap().hard_stop, Some(false));
        // format 2 does not report it
        assert_eq!(MaintInfo::parse(&info15()).unwrap().hard_stop, None);
        // bit 1: power and clock over the CAN socket, independent of the hard stop
        assert!(!on.power_rtc);
        let pr = MaintInfo::parse(&info3(0x0D, 0x02)).unwrap();
        assert_eq!((pr.hard_stop, pr.power_rtc), (Some(false), true));
        assert!(MaintInfo::parse(&info3(0x0D, 0x03)).unwrap().power_rtc);
        assert!(!MaintInfo::parse(&info3(0x0D, 0xFD)).unwrap().power_rtc);
        assert!(!MaintInfo::parse(&info15()).unwrap().power_rtc);
        // through a frame: LENGTH A024 carries the 36 hex digits
        let frame = crate::frame::encode_request(0, 0, &info3(0x0D, 0x01)).unwrap();
        assert_eq!(&frame[9..13], b"A024");
        assert_eq!(MaintInfo::classify(&crate::frame::parse(&frame).unwrap().info), InfoAnswer::Known(on));
    }

    #[test]
    fn image_flag_bit_2_is_the_pack_cutoff_capability() {
        // the measured 1.6 identity: 16.06.13, patch 1.6, flags 06 (power and clock, pack cutoff; no hard stop)
        let raw: Vec<u8> = (0..18).map(|i| u8::from_str_radix(&"4D4E540310060D010618DAF61D8A93671D06"[2 * i..2 * i + 2], 16).unwrap()).collect();
        let i = MaintInfo::parse(&raw).unwrap();
        assert_eq!((i.format, i.patch, i.hard_stop, i.power_rtc, i.pack_cutoff), (3, [1, 6], Some(false), true, true));
        assert_eq!((i.build_id_hex(), i.version()), ("18daf61d8a93671d".to_string(), "16.06.13 MAINT 1.6".to_string()));
        // with the hard stop 07; other bits do not set it
        assert!(MaintInfo::parse(&info3(0x0D, 0x07)).unwrap().pack_cutoff);
        assert!(MaintInfo::parse(&info3(0x0D, 0x04)).unwrap().pack_cutoff);
        assert!(!MaintInfo::parse(&info3(0x0D, 0xFB)).unwrap().pack_cutoff);
        assert!(!MaintInfo::parse(&info3(0x0D, 0x02)).unwrap().pack_cutoff);
        assert!(!MaintInfo::parse(&info15()).unwrap().pack_cutoff, "format 2 has no image flags");
    }

    #[test]
    fn an_unknown_oem_revision_shows_as_question_mark() {
        let i = MaintInfo::parse(&info3(0xFF, 0x00)).unwrap();
        assert_eq!((i.base_text(), i.version()), ("16.06.?".to_string(), "16.06.? MAINT 1.5".to_string()));
    }

    #[test]
    fn wrong_lengths_and_other_formats_are_rejected() {
        // format 3 with 17 bytes, format 2 with 18 bytes, format 4 with 18 bytes, format 5
        assert_eq!(MaintInfo::classify(&info3(0x0D, 1)[..17]), InfoAnswer::OtherFormat);
        assert_eq!(MaintInfo::classify(&[info15(), vec![1]].concat()), InfoAnswer::OtherFormat);
        let mut f4 = info3(0x0D, 1);
        f4[3] = 4;
        assert_eq!(MaintInfo::classify(&f4), InfoAnswer::OtherFormat);
        assert!(MaintInfo::parse(&f4).is_err());
        let mut f5 = info4(1, 0x16);
        f5[3] = 5;
        assert_eq!(MaintInfo::classify(&f5), InfoAnswer::OtherFormat, "format 5 with 19 bytes");
        assert_eq!(MaintInfo::classify(&[info4(1, 0x16), vec![0, 0]].concat()), InfoAnswer::OtherFormat, "format 4 with 21 bytes");
    }

    #[test]
    fn format_5_appends_the_read_capabilities() {
        // the 1.5.4 and 1.6.4 releases: the format-4 offsets, then mask 000F
        let a = MaintInfo::parse(&hex("4D4E540510060D01050480112D861B8329C43A000F")).unwrap();
        assert_eq!((a.format, a.patch_text(), a.build_id_hex(), a.image_flags(), a.read_caps), (5, "1.5.4".to_string(), "80112d861b8329c4".to_string(), 0x3A, 0x0F));
        assert_eq!((a.diagnostics(), a.checkpoint(), a.positive(), a.uart0, a.hard_stop, a.supported()), (true, false, true, true, Some(false), true));
        let b = MaintInfo::parse(&hex("4D4E540510060D010604A8676B52BAE281FFFE000F")).unwrap();
        assert_eq!((b.patch_text(), b.image_flags(), b.hard_stop_default(), b.checkpoint(), b.supported()), ("1.6.4".to_string(), 0xFE, Some(false), true, true));
        assert!([CAP_HISTORY, CAP_CALIBRATION, CAP_SOC_STORE, CAP_ENERGY].iter().all(|&c| b.cap(c)));
        // a partial mask names only its bits
        let only = MaintInfo::parse(&hex("4D4E540510060D010604A8676B52BAE281FFFE0005")).unwrap();
        assert_eq!((only.cap(CAP_HISTORY), only.cap(CAP_CALIBRATION), only.cap(CAP_SOC_STORE), only.cap(CAP_ENERGY)), (true, false, true, false));
        // an unknown bit: a format the app does not know, never partly read
        assert_eq!(MaintInfo::classify(&hex("4D4E540510060D010604A8676B52BAE281FFFE001F")), InfoAnswer::OtherFormat);
        assert_eq!(MaintInfo::classify(&hex("4D4E540510060D010604A8676B52BAE281FFFE800F")), InfoAnswer::OtherFormat);
        // below the support boundary format 5 stays unsupported; format 4 has no capabilities
        assert!(!MaintInfo::parse(&hex("4D4E540510060D010602A8676B52BAE281FFFE000F")).unwrap().supported());
        assert_eq!(MaintInfo::parse(&hex("4D4E540410060D010603BAA01ECD5AF4EB28FE")).unwrap().read_caps, 0);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn the_1_5_3_and_1_6_3_identities_and_their_new_flags() {
        // MAINT 1.5.3 (planned, flags 3A): diagnosis, positive bit 3, no checkpoint, fixed soft stop
        let a = MaintInfo::parse(&hex("4D4E540410060D0105030102030405060708 3A".replace(' ', "").as_str())).unwrap();
        assert_eq!((a.patch_text(), a.image_flags(), a.hard_stop, a.hard_stop_default()), ("1.5.3".to_string(), 0x3A, Some(false), None));
        assert_eq!((a.diagnostics(), a.checkpoint(), a.hard_stop_configurable(), a.positive(), a.uart0, a.supported()), (true, false, false, true, true, true));
        // the published 1.5.2 (flags 32, bit 3 at 0 allows) is no longer supported
        assert!(!MaintInfo::parse(&hex("4D4E540410060D01050289D85C61B40E03DA32")).unwrap().supported());
        // MAINT 1.6.3 (flags FE): configurable hard stop with soft as default, checkpoint, positive bits
        let b = MaintInfo::parse(&hex("4D4E540410060D010603BAA01ECD5AF4EB28FE")).unwrap();
        assert_eq!((b.patch_text(), b.image_flags(), b.hard_stop, b.hard_stop_default()), ("1.6.3".to_string(), 0xFE, None, Some(false)));
        assert_eq!((b.diagnostics(), b.checkpoint(), b.hard_stop_configurable(), b.positive(), b.supported()), (true, true, true, true, true));
        // FF: the default is hard; the flag byte comes back unchanged
        let mut ff = hex("4D4E540410060D010603BAA01ECD5AF4EB28FF");
        assert_eq!(MaintInfo::parse(&ff).unwrap().hard_stop_default(), Some(true));
        assert_eq!(MaintInfo::parse(&ff).unwrap().image_flags(), 0xFF);
        // older format-4 builds and 1.6.2 are not supported; a later version is
        ff[9] = 2;
        assert!(!MaintInfo::parse(&ff).unwrap().supported());
        assert!(!MaintInfo::parse(&info4(1, 0x1E)).unwrap().supported());
        ff[8] = 7;
        ff[9] = 0;
        assert!(MaintInfo::parse(&ff).unwrap().supported());
        // TEMP(maint-1.6): the first 1.6 (V1_6_CUTOFF, flags 06) yes, the positive 1.6 (flags 0E) and 1.5 no
        assert!(MaintInfo::parse(&hex("4D4E540310060D010618DAF61D8A93671D06")).unwrap().supported());
        assert!(!MaintInfo::parse(&hex("4D4E540310060D010639E0C5D9CCF133960E")).unwrap().supported());
        assert!(!MaintInfo::parse(&info15()).unwrap().supported());
    }

    #[test]
    fn the_memory_diagnosis_reads_format_01_with_10_bytes() {
        assert_eq!(MemoryDiagnosis::request_at(3).unwrap(), crate::frame::encode_request(3, 0x64, &[0x04]).unwrap());
        assert!(MemoryDiagnosis::request_at(22).is_err());
        // the contract's example: both copies valid and equal, both locks off, cache valid and consistent
        let d = MemoryDiagnosis::parse(&hex("01010101010101000006")).unwrap();
        assert_eq!((d.params, d.params_compare, d.soc, d.soc_compare), (["valid", "valid"], "same", ["valid", "valid"], "same"));
        assert_eq!((d.lock, d.cache), ([Some(false), Some(false)], 0x06));
        // 1.5.3: no lock or cache support (FF FF 00); an empty backup, differing SOC copies
        let d = MemoryDiagnosis::parse(&hex("01010000010102FFFF00")).unwrap();
        assert_eq!((d.params[1], d.params_compare, d.soc_compare, d.lock), ("empty", "not-comparable", "different", [None, None]));
        assert!(MemoryDiagnosis::parse(&hex("020101010101010000")).is_err());
        assert!(MemoryDiagnosis::parse(&hex("0201010101010100000600")).is_err());
    }

    /// The 19 INFO bytes of a format-4 identity: 16.06.13, patch 1.6.`rev`, image flags `flags`.
    fn info4(rev: u8, flags: u8) -> Vec<u8> {
        let mut v = b"MNT".to_vec();
        v.extend_from_slice(&[0x04, 0x10, 0x06, 0x0D, 0x01, 0x06, rev, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, flags]);
        v
    }

    #[test]
    fn format_4_has_19_bytes_a_third_patch_byte_and_the_uart0_flag() {
        // 1.6.1 with positive bits and the cutoff, power and UART0 marker: flags 0x1E
        let i = MaintInfo::parse(&info4(1, 0x1E)).unwrap();
        assert_eq!((i.format, i.base, i.patch, i.patch_rev), (4, [16, 6, 13], [1, 6], 1));
        assert_eq!((i.hard_stop, i.power_rtc, i.pack_cutoff, i.uart0, i.reserved_flags), (Some(false), true, true, true, 0x08));
        assert_eq!((i.build_id_hex(), i.version()), ("0123456789abcdef".to_string(), "16.06.13 MAINT 1.6.1".to_string()));
        assert_eq!(i.image_flags(), 0x1E);
        assert!(!MaintInfo::parse(&info4(1, 0xEF)).unwrap().uart0);
        // bit 4 means nothing in format 3
        let f3 = MaintInfo::parse(&info3(0x0D, 0x10)).unwrap();
        assert_eq!((f3.uart0, f3.reserved_flags, f3.image_flags()), (false, 0x10, 0x10));
        // through a frame: LENGTH 8026 carries the 38 hex digits
        let frame = crate::frame::encode_request(0, 0, &info4(1, 0x1E)).unwrap();
        assert_eq!(&frame[9..13], b"8026");
        assert_eq!(MaintInfo::classify(&crate::frame::parse(&frame).unwrap().info), InfoAnswer::Known(i));
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
