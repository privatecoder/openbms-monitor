//! CID2 0x5A: intra-pack record (master polls slaves on RS485-1/2). The answer carries 0x5A
//! instead of an RTN code and 32 bytes of INFO.

use crate::error::{Error, Result};
use crate::reader::{kelvin10_to_c, Reader};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntraPackRecord {
    pub highest_cell_voltage: f64,
    pub lowest_cell_voltage: f64,
    pub highest_cell_temperature: f64,
    pub lowest_cell_temperature: f64,
    pub current: f64,
    pub pack_voltage: f64,
    pub remaining_capacity_ah: f64,
    pub full_capacity_ah: f64,
    pub soc: f64,
    pub port_voltage: f64,
    /// Alarm events 1..6 (event 6 bit 7 masked).
    pub alarm_events: [u8; 6],
    /// On-off state: bit0 discharge MOSFET, bit1 charge MOSFET, bit2 current limiter, bit3 heater.
    pub switch_state: u8,
    pub alarm_event_7: u8,
    pub alarm_event_8: u8,
    /// System status incl. bits masked in 0x44 (bit2 charger detected, bit6/7 current >= 3 A).
    pub system_status: u8,
}

impl IntraPackRecord {
    pub fn parse(info: &[u8]) -> Result<Self> {
        if info.len() != 32 {
            return Err(Error::Layout("0x5A record must be 32 bytes"));
        }
        let mut r = Reader::new(info);
        let _flag = r.u16()?;
        let v = |x: u16| f64::from(x);
        Ok(Self {
            highest_cell_voltage: v(r.u16()?) / 1000.0,
            lowest_cell_voltage: v(r.u16()?) / 1000.0,
            highest_cell_temperature: kelvin10_to_c(r.u16()?),
            lowest_cell_temperature: kelvin10_to_c(r.u16()?),
            current: f64::from(r.i16()?) / 100.0,
            pack_voltage: v(r.u16()?) / 100.0,
            remaining_capacity_ah: v(r.u16()?) / 100.0,
            full_capacity_ah: v(r.u16()?) / 100.0,
            soc: v(r.u16()?) / 10.0,
            port_voltage: v(r.u16()?) / 100.0,
            alarm_events: r.bytes(6)?.try_into().unwrap(),
            switch_state: r.u8()?,
            alarm_event_7: r.u8()?,
            alarm_event_8: r.u8()?,
            system_status: r.u8()?,
        })
    }
}
