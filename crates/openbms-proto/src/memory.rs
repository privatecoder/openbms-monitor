//! The BMS clock (CID2 0x4D read, 0x4E set) and its internal history (CID2 0x4B). All three exist only on
//! RS485-1/2; the CAN socket bus answers RTN 04.
//!
//! The history is a ring of up to 500 records in an I²C EEPROM, written every parameter 75 (working) or 76
//! (standby) minutes. 0x4B with INFO `[0x00, 0x00]` returns the oldest record, `[0x00, n≠0]` the next one;
//! reading only moves the firmware's read index. An empty answer means there are no records.

use crate::error::{Error, Result};
use crate::reader::{kelvin10_to_c, Reader};
use crate::status::{active_alarms, ActiveAlarm};
use serde::{Deserialize, Serialize};

pub const CID2_HISTORY: u8 = 0x4B;
pub const CID2_CLOCK_READ: u8 = 0x4D;
pub const CID2_CLOCK_SET: u8 = 0x4E;
/// Length of a 0x4B answer with a record.
pub const HISTORY_LEN: usize = 97;

/// Calendar time as the BMS stores it: no time zone, no weekday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BmsTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl BmsTime {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self { year: r.u16()?, month: r.u8()?, day: r.u8()?, hour: r.u8()?, minute: r.u8()?, second: r.u8()? })
    }

    /// The 7 bytes of 0x4D/0x4E: year u16 BE, month, day, hour, minute, second.
    pub fn to_bytes(self) -> [u8; 7] {
        let [hi, lo] = self.year.to_be_bytes();
        [hi, lo, self.month, self.day, self.hour, self.minute, self.second]
    }

    /// Plausible calendar fields. The firmware checks nothing (a year ≤ 2000 becomes 2022), so a sender must.
    pub fn is_valid(&self) -> bool {
        let days = match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if self.year.is_multiple_of(4) && (!self.year.is_multiple_of(100) || self.year.is_multiple_of(400)) => 29,
            2 => 28,
            _ => return false,
        };
        (2001..=2099).contains(&self.year) && (1..=days).contains(&self.day) && self.hour < 24 && self.minute < 60 && self.second < 60
    }
}

pub fn clock_request(address: u8) -> Result<Vec<u8>> {
    crate::frame::encode_request(address, CID2_CLOCK_READ, &[])
}

pub fn set_clock_request(address: u8, time: BmsTime) -> Result<Vec<u8>> {
    if !time.is_valid() {
        return Err(Error::Layout("implausible clock time"));
    }
    crate::frame::encode_request(address, CID2_CLOCK_SET, &time.to_bytes())
}

/// Answer of 0x4D.
pub fn parse_clock(info: &[u8]) -> Result<BmsTime> {
    if info.len() < 7 {
        return Err(Error::Layout("0x4D answer shorter than 7 bytes"));
    }
    BmsTime::read(&mut Reader::new(info))
}

/// `next = false`: the oldest record (and the read index back to the start); `true`: the record after the last one read.
pub fn history_request(address: u8, next: bool) -> Result<Vec<u8>> {
    crate::frame::encode_request(address, CID2_HISTORY, &[0x00, u8::from(next)])
}

/// One record of the internal history, converted like the telemetry (0x42).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryRecord {
    /// BMS clock when the record was written.
    pub time: BmsTime,
    /// Bit0 discharging, bit1 charging, bit4 standby, bit5 off (& 0x33 as in 0x44).
    pub system_status: u8,
    /// Alarm events 1..8 masked as in 0x44 (event 6 & 0x7F, event 7 & 0x30, event 8 & 0x3F).
    pub events: [u8; 8],
    pub alarms: Vec<ActiveAlarm>,
    /// Cell voltages in V.
    pub cell_voltages: Vec<f64>,
    /// Cell temperature sensors 1..4 in °C.
    pub cell_temperatures: Vec<f64>,
    pub ambient_temperature: f64,
    pub power_temperature: f64,
    /// A, + = charging.
    pub current: f64,
    pub pack_voltage: f64,
    pub remaining_capacity_ah: f64,
    pub full_capacity_ah: f64,
    /// %.
    pub soc: f64,
    pub rated_capacity_ah: f64,
    pub cycles: u16,
    /// %.
    pub soh: f64,
    pub port_voltage: f64,
}

/// Answer of 0x4B: `None` = the pack has no records; otherwise the record and whether it is the last one.
///
/// Layout: `01`, last flag, year u16 BE, then record bytes 1..93 (record byte 0, year − 2000, is replaced by the
/// full year). Record offset k ≥ 1 is INFO byte 3 + k.
pub fn parse_history(info: &[u8]) -> Result<Option<(HistoryRecord, bool)>> {
    if info.is_empty() {
        return Ok(None);
    }
    if info.len() < HISTORY_LEN || info[0] != 0x01 {
        return Err(Error::Layout("unexpected 0x4B layout"));
    }
    let last = info[1] != 0;
    let mut r = Reader::new(&info[2..]);
    let time = BmsTime::read(&mut r)?;
    let system_status = info[9];
    let e = &info[11..17];
    let events = [e[0], e[1], e[2], e[3], e[4], e[5], info[19], info[20]];
    let mut r = Reader::new(&info[27..]);
    let cell_voltages = (0..16).map(|_| r.u16().map(|v| f64::from(v) / 1000.0)).collect::<Result<_>>()?;
    let cell_temperatures = (0..4).map(|_| r.u16().map(kelvin10_to_c)).collect::<Result<_>>()?;
    let ambient_temperature = kelvin10_to_c(r.u16()?);
    let power_temperature = kelvin10_to_c(r.u16()?);
    let current = f64::from(r.i16()?) / 100.0;
    let pack_voltage = f64::from(r.u16()?) / 100.0;
    let remaining_capacity_ah = f64::from(r.u16()?) / 100.0;
    let full_capacity_ah = f64::from(r.u16()?) / 100.0;
    let soc = f64::from(r.u16()?) / 10.0;
    let rated_capacity_ah = f64::from(r.u16()?) / 100.0;
    let cycles = r.u16()?;
    let soh = f64::from(r.u16()?) / 10.0;
    let port_voltage = f64::from(r.u16()?) / 100.0;
    let record = HistoryRecord {
        time,
        system_status,
        events,
        alarms: active_alarms(&events),
        cell_voltages,
        cell_temperatures,
        ambient_temperature,
        power_temperature,
        current,
        pack_voltage,
        remaining_capacity_ah,
        full_capacity_ah,
        soc,
        rated_capacity_ah,
        cycles,
        soh,
        port_voltage,
    };
    Ok(Some((record, last)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    const T: BmsTime = BmsTime { year: 2026, month: 3, day: 9, hour: 7, minute: 5, second: 30 };

    /// A synthetic 0x4B answer INFO built from the record layout (record offsets as in the protocol notes).
    fn history_info(last: bool) -> Vec<u8> {
        let mut rec = [0u8; 94];
        rec[..6].copy_from_slice(&[26, 3, 9, 7, 5, 30]);
        rec[6] = 0x02; // charging
        rec[7] = 0x0F;
        rec[8..14].copy_from_slice(&[0x00, 0x03, 0x00, 0x00, 0x00, 0x00]); // cell high warning + OVP
        rec[16] = 0x10; // ev7 charge activation automatic
        rec[17] = 0x20; // ev8 rtc time invalid
        rec[23] = 0x23;
        let mut put = |off: usize, v: u16| rec[off..off + 2].copy_from_slice(&v.to_be_bytes());
        for c in 0..16 {
            put(24 + 2 * c, 3300 + c as u16);
        }
        for t in 0..4 {
            put(56 + 2 * t, 2981 + t as u16); // 25.0 °C ..
        }
        put(64, 2931); // ambient 20.0 °C
        put(66, 3031); // power 30.0 °C
        put(68, (-1234i16) as u16); // -12.34 A
        put(70, 5281);
        put(72, 14000);
        put(74, 28000);
        put(76, 500);
        put(78, 28000);
        put(80, 123);
        put(82, 1000);
        put(84, 5290);
        let mut info = vec![0x01, u8::from(last)];
        info.extend(2026u16.to_be_bytes());
        info.extend(&rec[1..]);
        info
    }

    #[test]
    fn clock_frames() {
        assert_eq!(clock_request(1).unwrap(), b"~2001464D0000FD9B\r".to_vec());
        let f = frame::parse(&set_clock_request(2, T).unwrap()).unwrap();
        assert_eq!((f.address, f.rtn, f.info.as_slice()), (2, 0x4E, &[0x07, 0xEA, 3, 9, 7, 5, 30][..]));
        assert_eq!(parse_clock(&f.info).unwrap(), T);
        assert!(parse_clock(&[0x07, 0xEA, 3]).is_err());
    }

    #[test]
    fn implausible_times_are_not_sent() {
        for bad in [
            BmsTime { year: 2000, ..T },
            BmsTime { month: 13, ..T },
            BmsTime { month: 2, day: 30, ..T },
            BmsTime { year: 2025, month: 2, day: 29, ..T },
            BmsTime { hour: 24, ..T },
        ] {
            assert!(set_clock_request(1, bad).is_err(), "{bad:?}");
        }
        assert!(set_clock_request(1, BmsTime { year: 2028, month: 2, day: 29, ..T }).is_ok());
    }

    #[test]
    fn history_requests() {
        let oldest = frame::parse(&history_request(3, false).unwrap()).unwrap();
        let next = frame::parse(&history_request(3, true).unwrap()).unwrap();
        assert_eq!((oldest.rtn, oldest.info.as_slice()), (0x4B, &[0, 0][..]));
        assert_eq!(next.info, vec![0, 1]);
    }

    #[test]
    fn history_record() {
        let info = history_info(false);
        assert_eq!(info.len(), HISTORY_LEN);
        let (r, last) = parse_history(&info).unwrap().unwrap();
        assert!(!last);
        assert_eq!(r.time, T);
        assert_eq!(r.system_status, 0x02);
        assert_eq!(r.events, [0, 0x03, 0, 0, 0, 0, 0x10, 0x20]);
        let ids: Vec<_> = r.alarms.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["alarm.ev2.b0", "alarm.ev2.b1", "alarm.ev7.b4", "alarm.ev8.b5"]);
        assert_eq!(r.cell_voltages.len(), 16);
        assert_eq!((r.cell_voltages[0], r.cell_voltages[15]), (3.3, 3.315));
        assert_eq!(r.cell_temperatures, vec![25.0, 25.1, 25.2, 25.3]);
        assert_eq!((r.ambient_temperature, r.power_temperature), (20.0, 30.0));
        assert_eq!((r.current, r.pack_voltage, r.port_voltage), (-12.34, 52.81, 52.9));
        assert_eq!((r.remaining_capacity_ah, r.full_capacity_ah, r.rated_capacity_ah), (140.0, 280.0, 280.0));
        assert_eq!((r.soc, r.cycles, r.soh), (50.0, 123, 100.0));
    }

    #[test]
    fn history_last_empty_and_broken() {
        assert!(parse_history(&history_info(true)).unwrap().unwrap().1);
        assert_eq!(parse_history(&[]).unwrap(), None);
        assert!(parse_history(&history_info(false)[..60]).is_err());
        let mut wrong = history_info(false);
        wrong[0] = 0x00;
        assert!(parse_history(&wrong).is_err());
    }

    #[test]
    fn history_answer_frame_roundtrip() {
        // an answer frame carries RTN 00 in place of CID2; 97 bytes = LENID 0x0C2, LENGTH 0x20C2
        let raw = frame::encode_request(4, 0x00, &history_info(true)).unwrap();
        assert_eq!(&raw[9..13], b"20C2");
        let f = frame::parse(&raw).unwrap();
        assert_eq!(f.info.len(), HISTORY_LEN);
        assert!(parse_history(&f.info).unwrap().is_some());
    }
}
