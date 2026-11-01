//! Addressed read-only diagnostics of builds with MNT format 5 (`MaintInfo::cap`): the calibration pages (0x64/05) and
//! the stored capacity records with the live energy counters (0x64/06). Both answer at the pack's own address on
//! RS485-1/2 and the CAN socket and change nothing (no repair, no save). The history over the CAN socket is the
//! ordinary 0x4B layout (`memory`).
//!
//! Live values are big endian; the raw stored records stay in the firmware's little-endian flash layout and are decoded
//! only when their strict class is "valid" (expected marker, length and CRC). A record the firmware's own CRC check
//! accepts can still be invalid in shape: its raw bytes are kept, its fields are not guessed.

use crate::error::{Error, Result};
use crate::frame::encode_request;
use serde::Serialize;

const CID2: u8 = 0x64;

fn check_address(address: u8) -> Result<()> {
    if address > 21 {
        return Err(Error::Layout("0x64 address must be 0..=21"));
    }
    Ok(())
}

fn class(b: u8) -> &'static str {
    match b {
        0 => "erased",
        1 => "valid",
        2 => "invalid",
        _ => "unknown",
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}
fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Which calibration a 0x64/05 page holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CalPage {
    /// the values the firmware works with now (RAM)
    Active = 0,
    /// the stored primary record (flash)
    Primary = 1,
    /// the stored backup record (flash)
    Backup = 2,
}

/// The coefficients common to the active and the stored calibration. Gains are raw factors, zero bases raw ADC counts
/// (not amperes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Coefficients {
    /// the 16 cell voltage gains
    pub cells: Vec<u16>,
    /// the two pack voltage gains
    pub voltage: [u16; 2],
    /// current gain of channel A, B
    pub current_gain: [u16; 2],
    /// raw zero base of channel A, B
    pub zero: [i32; 2],
}

/// Page 00: the active calibration (72 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActiveCalibration {
    /// event 8 & 0x1C: the firmware's invalid-voltage, invalid-current-gain and invalid-current-zero bits
    pub invalid_bits: u8,
    pub cell_count: u8,
    pub coefficients: Coefficients,
    /// primary conversion reference of channel A, B
    pub reference: [u16; 2],
    /// raw sample of channel A, B
    pub raw_sample: [i32; 2],
    /// converted current of channel A, B in mA
    pub current_ma: [i32; 2],
}

/// Page 01/02: a stored calibration record (58 bytes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoredCalibration {
    pub page: CalPage,
    /// strict shape: "erased", "valid" (expected marker, length and CRC), "invalid", "unknown"
    pub class: &'static str,
    /// the firmware's own acceptance (declared length ≤ 250 and CRC); separate from `class`
    pub crc_ok: bool,
    pub declared_len: u8,
    /// the 53 raw bytes as stored (hex)
    pub raw: String,
    /// decoded only for a strictly valid record
    pub coefficients: Option<Coefficients>,
    /// the stored invalid flags (record byte 2), only for a strictly valid record
    pub invalid_bits: Option<u8>,
}

/// An answer to 0x64/05.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CalibrationPage {
    Active(ActiveCalibration),
    Stored(StoredCalibration),
}

pub const CAL_ACTIVE_LEN: usize = 72;
pub const CAL_STORED_LEN: usize = 58;

/// Ask the pack at `address` for calibration page `page` (INFO `05 pp`).
pub fn calibration_request(address: u8, page: CalPage) -> Result<Vec<u8>> {
    check_address(address)?;
    encode_request(address, CID2, &[0x05, page as u8])
}

/// Exactly format 01 with the page asked for: 72 bytes active, 58 bytes stored.
pub fn parse_calibration(info: &[u8], page: CalPage) -> Result<CalibrationPage> {
    let want = if page == CalPage::Active { CAL_ACTIVE_LEN } else { CAL_STORED_LEN };
    if info.len() != want || info[0] != 0x01 || info[1] != page as u8 {
        return Err(Error::Layout("0x64/05 answer is not format 01 of the page asked for"));
    }
    if page == CalPage::Active {
        let pair16 = |at: usize| [be16(info, at), be16(info, at + 2)];
        let pair32 = |at: usize| [be32(info, at) as i32, be32(info, at + 4) as i32];
        return Ok(CalibrationPage::Active(ActiveCalibration {
            invalid_bits: info[2],
            cell_count: info[3],
            coefficients: Coefficients {
                cells: (0..16).map(|i| be16(info, 4 + 2 * i)).collect(),
                voltage: pair16(36),
                current_gain: pair16(40),
                zero: pair32(44),
            },
            reference: pair16(52),
            raw_sample: pair32(56),
            current_ma: pair32(64),
        }));
    }
    let raw = &info[5..];
    let valid = info[2] == 1;
    Ok(CalibrationPage::Stored(StoredCalibration {
        page,
        class: class(info[2]),
        crc_ok: info[3] == 1,
        declared_len: info[4],
        raw: hex(raw),
        coefficients: valid.then(|| Coefficients {
            cells: (0..16).map(|i| le16(raw, 3 + 2 * i)).collect(),
            voltage: [le16(raw, 35), le16(raw, 37)],
            current_gain: [le16(raw, 39), le16(raw, 45)],
            zero: [le32(raw, 41) as i32, le32(raw, 47) as i32],
        }),
        invalid_bits: valid.then_some(raw[2]),
    }))
}

/// One stored capacity record (22 bytes, little endian), decoded.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SocRecord {
    pub remaining_ah: f64,
    pub full_ah: f64,
    pub charged_kwh: f64,
    pub discharged_kwh: f64,
    pub cycles: u16,
    /// cycle bookkeeping: SOC reference and accumulated SOC decrease, in %
    pub soc_reference: f64,
    pub soc_decrease: f64,
}

impl SocRecord {
    /// remaining / full in %, None without a full capacity
    pub fn soc(&self) -> Option<f64> {
        (self.full_ah > 0.0).then(|| self.remaining_ah / self.full_ah * 100.0)
    }
}

/// An answer to 0x64/06 (68 bytes): the two stored capacity records and a coherent live snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SocStore {
    /// strict class of primary, backup: "erased", "valid", "invalid", "unknown"
    pub class: [&'static str; 2],
    /// "not-comparable" (not both valid), "same", "different", "unknown"
    pub compare: &'static str,
    /// the firmware's own CRC acceptance of primary, backup
    pub crc_ok: [bool; 2],
    /// which copy a start would take as things stand: "fallback", "primary", "backup", "unknown"
    pub boot: &'static str,
    /// live parameters 58 (full) and 59 (remaining capacity) in Ah: today's values, not a promise for the next start
    pub live_p58_ah: f64,
    pub live_p59_ah: f64,
    /// live energy counters, full 32 bit, in kWh
    pub live_charged_kwh: f64,
    pub live_discharged_kwh: f64,
    pub live_remaining_ah: f64,
    pub live_full_ah: f64,
    /// live SOC in %
    pub live_soc: f64,
    /// raw primary, backup (hex)
    pub raw: [String; 2],
    /// decoded only for a strictly valid record
    pub records: [Option<SocRecord>; 2],
}

impl SocStore {
    pub const LEN: usize = 68;

    /// The decoded record a start would take now (primary or backup), if valid.
    pub fn boot_record(&self) -> Option<&SocRecord> {
        match self.boot {
            "primary" => self.records[0].as_ref(),
            "backup" => self.records[1].as_ref(),
            _ => None,
        }
    }
}

/// Ask the pack at `address` for its stored capacity records and live energy (INFO `06`).
pub fn soc_store_request(address: u8) -> Result<Vec<u8>> {
    check_address(address)?;
    encode_request(address, CID2, &[0x06])
}

fn soc_record(r: &[u8]) -> SocRecord {
    SocRecord {
        remaining_ah: f64::from(le16(r, 2)) / 100.0,
        full_ah: f64::from(le16(r, 4)) / 100.0,
        charged_kwh: f64::from(le32(r, 6)) / 10.0,
        discharged_kwh: f64::from(le32(r, 10)) / 10.0,
        cycles: le16(r, 14),
        soc_reference: f64::from(le16(r, 16)) / 10.0,
        soc_decrease: f64::from(le16(r, 18)) / 10.0,
    }
}

/// Exactly format 01 with 68 bytes.
pub fn parse_soc_store(info: &[u8]) -> Result<SocStore> {
    if info.len() != SocStore::LEN || info[0] != 0x01 {
        return Err(Error::Layout("0x64/06 answer is not format 01 with 68 bytes"));
    }
    let compare = match info[3] {
        0 => "not-comparable",
        1 => "same",
        2 => "different",
        _ => "unknown",
    };
    let boot = match info[5] {
        0 => "fallback",
        1 => "primary",
        2 => "backup",
        _ => "unknown",
    };
    let (p, b) = (&info[24..46], &info[46..68]);
    let ah = |at: usize| f64::from(be16(info, at)) / 100.0;
    Ok(SocStore {
        class: [class(info[1]), class(info[2])],
        compare,
        crc_ok: [info[4] & 1 != 0, info[4] & 2 != 0],
        boot,
        live_p58_ah: ah(6),
        live_p59_ah: ah(8),
        live_charged_kwh: f64::from(be32(info, 10)) / 10.0,
        live_discharged_kwh: f64::from(be32(info, 14)) / 10.0,
        live_remaining_ah: ah(18),
        live_full_ah: ah(20),
        live_soc: f64::from(be16(info, 22)) / 10.0,
        raw: [hex(p), hex(b)],
        records: [(info[1] == 1).then(|| soc_record(p)), (info[2] == 1).then(|| soc_record(b))],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    #[test]
    fn requests_carry_the_page_and_the_address() {
        let f = frame::parse(&calibration_request(3, CalPage::Backup).unwrap()).unwrap();
        assert_eq!((f.address, f.rtn, f.info.as_slice()), (3, 0x64, [0x05, 0x02].as_slice()));
        let f = frame::parse(&soc_store_request(11).unwrap()).unwrap();
        assert_eq!((f.address, f.info.as_slice()), (11, [0x06].as_slice()));
        assert!(soc_store_request(22).is_err());
    }

    fn active() -> Vec<u8> {
        let mut v = vec![0x01, 0x00, 0x04, 16];
        for i in 0..16u16 {
            v.extend_from_slice(&(60000 + i).to_be_bytes());
        }
        for x in [61000u16, 61001, 50000, 62000] {
            v.extend_from_slice(&x.to_be_bytes());
        }
        for x in [-1234i32, 5678] {
            v.extend_from_slice(&x.to_be_bytes());
        }
        for x in [4096u16, 4097] {
            v.extend_from_slice(&x.to_be_bytes());
        }
        for x in [-20i32, 30, -1500, 0] {
            v.extend_from_slice(&x.to_be_bytes());
        }
        v
    }

    #[test]
    fn the_active_page_is_big_endian_and_signed() {
        let info = active();
        assert_eq!(info.len(), CAL_ACTIVE_LEN);
        let CalibrationPage::Active(a) = parse_calibration(&info, CalPage::Active).unwrap() else { panic!() };
        assert_eq!((a.invalid_bits, a.cell_count), (0x04, 16));
        assert_eq!((a.coefficients.cells[0], a.coefficients.cells[15]), (60000, 60015));
        assert_eq!((a.coefficients.voltage, a.coefficients.current_gain, a.coefficients.zero), ([61000, 61001], [50000, 62000], [-1234, 5678]));
        assert_eq!((a.reference, a.raw_sample, a.current_ma), ([4096, 4097], [-20, 30], [-1500, 0]));
        // the wrong page or length is no answer
        assert!(parse_calibration(&info, CalPage::Primary).is_err());
        assert!(parse_calibration(&info[..71], CalPage::Active).is_err());
    }

    /// A stored page: class, CRC acceptance, declared length, then the 53 raw little-endian bytes.
    fn stored(page: u8, class: u8) -> Vec<u8> {
        let mut raw = vec![0xA0, 51, 0x00];
        for i in 0..16u16 {
            raw.extend_from_slice(&(60100 + i).to_le_bytes());
        }
        raw.extend_from_slice(&61000u16.to_le_bytes()); // 35
        raw.extend_from_slice(&61001u16.to_le_bytes()); // 37
        raw.extend_from_slice(&50000u16.to_le_bytes()); // 39 gain A
        raw.extend_from_slice(&(-777i32).to_le_bytes()); // 41 zero A
        raw.extend_from_slice(&61500u16.to_le_bytes()); // 45 gain B
        raw.extend_from_slice(&888i32.to_le_bytes()); // 47 zero B
        raw.extend_from_slice(&[0x12, 0x34]); // 51 CRC
        assert_eq!(raw.len(), 53);
        [vec![0x01, page, class, 1, 51], raw].concat()
    }

    #[test]
    fn a_stored_page_is_decoded_only_when_strictly_valid() {
        let CalibrationPage::Stored(s) = parse_calibration(&stored(1, 1), CalPage::Primary).unwrap() else { panic!() };
        assert_eq!((s.page, s.class, s.crc_ok, s.declared_len), (CalPage::Primary, "valid", true, 51));
        let c = s.coefficients.unwrap();
        assert_eq!((c.cells[0], c.cells[15], c.voltage, c.current_gain, c.zero), (60100, 60115, [61000, 61001], [50000, 61500], [-777, 888]));
        // CRC-accepted but of the wrong shape: raw kept, nothing decoded
        let CalibrationPage::Stored(s) = parse_calibration(&stored(2, 2), CalPage::Backup).unwrap() else { panic!() };
        assert_eq!((s.class, s.crc_ok, s.coefficients, s.invalid_bits), ("invalid", true, None, None));
        assert_eq!(s.raw.len(), 106);
        assert!(parse_calibration(&stored(1, 1), CalPage::Backup).is_err());
    }

    fn record(remaining: u16, full: u16, charged: u32, cycles: u16) -> Vec<u8> {
        let mut r = vec![0xA0, 20];
        r.extend_from_slice(&remaining.to_le_bytes());
        r.extend_from_slice(&full.to_le_bytes());
        r.extend_from_slice(&charged.to_le_bytes());
        r.extend_from_slice(&(charged - 100).to_le_bytes());
        r.extend_from_slice(&cycles.to_le_bytes());
        r.extend_from_slice(&500u16.to_le_bytes());
        r.extend_from_slice(&123u16.to_le_bytes());
        r.extend_from_slice(&[0xAB, 0xCD]);
        r
    }

    fn soc(primary: u8, backup: u8, boot: u8) -> Vec<u8> {
        let mut v = vec![0x01, primary, backup, if primary == 1 && backup == 1 { 2 } else { 0 }, 0x03, boot];
        v.extend_from_slice(&28000u16.to_be_bytes()); // P58
        v.extend_from_slice(&14000u16.to_be_bytes()); // P59
        v.extend_from_slice(&70000u32.to_be_bytes()); // 7000.0 kWh, beyond the 0x42 fallback
        v.extend_from_slice(&65000u32.to_be_bytes());
        v.extend_from_slice(&17640u16.to_be_bytes());
        v.extend_from_slice(&28000u16.to_be_bytes());
        v.extend_from_slice(&630u16.to_be_bytes());
        v.extend(record(11480, 28000, 69000, 300));
        v.extend(record(11200, 28000, 68900, 299));
        v
    }

    #[test]
    fn the_soc_store_keeps_live_and_stored_apart() {
        let info = soc(1, 1, 1);
        assert_eq!(info.len(), SocStore::LEN);
        let s = parse_soc_store(&info).unwrap();
        assert_eq!((s.class, s.compare, s.crc_ok, s.boot), (["valid", "valid"], "different", [true, true], "primary"));
        assert_eq!((s.live_p58_ah, s.live_p59_ah, s.live_remaining_ah, s.live_full_ah, s.live_soc), (280.0, 140.0, 176.4, 280.0, 63.0));
        assert_eq!((s.live_charged_kwh, s.live_discharged_kwh), (7000.0, 6500.0));
        let r = s.boot_record().unwrap();
        assert_eq!((r.remaining_ah, r.full_ah, r.charged_kwh, r.discharged_kwh, r.cycles), (114.8, 280.0, 6900.0, 6890.0, 300));
        assert_eq!((r.soc_reference, r.soc_decrease), (50.0, 12.3));
        assert!((r.soc().unwrap() - 41.0).abs() < 1e-9);
        // an invalid primary: boot falls to the backup, the primary stays raw
        let s = parse_soc_store(&soc(2, 1, 2)).unwrap();
        assert_eq!((s.records[0].is_none(), s.boot_record().unwrap().cycles), (true, 299));
        assert_eq!(s.raw[0].len(), 44);
        // fallback: no stored record to name
        assert!(parse_soc_store(&soc(0, 0, 0)).unwrap().boot_record().is_none());
        assert!(parse_soc_store(&info[..67]).is_err());
    }
}
