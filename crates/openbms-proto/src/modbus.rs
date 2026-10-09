//! Modbus RTU (only the master pack answers, slave id 1, RS485 bus of the CAN socket, 9600 baud).

use crate::error::{Error, Result};
use serde::Serialize;

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for b in data {
        crc ^= u16::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

/// Read request (FC 03 or 04). The firmware accepts only fixed start/count combinations.
pub fn read_request(function: u8, start: u16, count: u16) -> Vec<u8> {
    let mut f = vec![0x01, function, (start >> 8) as u8, start as u8, (count >> 8) as u8, count as u8];
    let crc = crc16(&f);
    f.extend_from_slice(&crc.to_le_bytes());
    f
}

/// Expected response length in bytes for a successful read of `count` registers.
pub fn response_len(count: u16) -> usize {
    5 + 2 * usize::from(count)
}

/// Parse a read response into registers.
pub fn parse_read_response(function: u8, raw: &[u8]) -> Result<Vec<u16>> {
    if raw.len() >= 5 && raw[1] == function | 0x80 {
        if crc16(&raw[..3]) != u16::from_le_bytes([raw[3], raw[4]]) {
            return Err(Error::ModbusCrc);
        }
        return Err(Error::ModbusException(raw[2]));
    }
    if raw.len() < 5 || raw[0] != 0x01 || raw[1] != function {
        return Err(Error::Layout("unexpected Modbus response header"));
    }
    let n = usize::from(raw[2]);
    if raw.len() < 5 + n {
        return Err(Error::Layout("Modbus response too short"));
    }
    if crc16(&raw[..3 + n]) != u16::from_le_bytes([raw[3 + n], raw[4 + n]]) {
        return Err(Error::ModbusCrc);
    }
    Ok(raw[3..3 + n].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect())
}

/// System values of the master (FC03 0x1000 x 21, firmware 16.06.04+).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemValues {
    /// 0x01 discharging, 0x02 charging, 0x10 idle, 0x20 off
    pub state: u16,
    pub charge_allowed: bool,
    pub discharge_allowed: bool,
    pub alarm_word_current_voltage: u16,
    pub alarm_word_temperature_misc: u16,
    pub pack_communication_fault: bool,
    pub voltage: f64,
    /// + = charging (as reported by the firmware)
    pub current: f64,
    pub soc: u16,
    pub highest_cell_temperature: f64,
    /// 0-based pack number (0 = master)
    pub highest_cell_temperature_pack: Option<u16>,
    pub lowest_cell_temperature: f64,
    pub lowest_cell_temperature_pack: Option<u16>,
    pub highest_cell_voltage: f64,
    pub highest_cell_voltage_pack: Option<u16>,
    pub lowest_cell_voltage: f64,
    pub lowest_cell_voltage_pack: Option<u16>,
    pub charge_voltage_limit: f64,
    pub charge_current_limit: f64,
    pub discharge_current_limit: f64,
    pub discharge_voltage_limit: f64,
    pub total_capacity_ah: u16,
}

impl SystemValues {
    pub const START: u16 = 0x1000;
    pub const COUNT: u16 = 21;

    pub fn request() -> Vec<u8> {
        read_request(0x03, Self::START, Self::COUNT)
    }

    pub fn from_registers(r: &[u16]) -> Result<Self> {
        if r.len() != usize::from(Self::COUNT) {
            return Err(Error::Layout("0x1000 block must have 21 registers"));
        }
        let s = |v: u16| f64::from(v as i16);
        let pack = |v: u16| if v > 0 { Some(v - 1) } else { None };
        Ok(Self {
            state: r[0],
            charge_allowed: r[1] & 0x02 != 0,
            discharge_allowed: r[1] & 0x01 != 0,
            alarm_word_current_voltage: r[2],
            alarm_word_temperature_misc: r[3],
            pack_communication_fault: r[3] & 0x8000 != 0,
            voltage: f64::from(r[4]) / 10.0,
            current: s(r[5]) / 10.0,
            soc: r[6],
            highest_cell_temperature: s(r[8]) / 10.0,
            highest_cell_temperature_pack: pack(r[9]),
            lowest_cell_temperature: s(r[10]) / 10.0,
            lowest_cell_temperature_pack: pack(r[11]),
            highest_cell_voltage: f64::from(r[12]) / 1000.0,
            highest_cell_voltage_pack: pack(r[13]),
            lowest_cell_voltage: f64::from(r[14]) / 1000.0,
            lowest_cell_voltage_pack: pack(r[15]),
            charge_voltage_limit: f64::from(r[16]) / 10.0,
            charge_current_limit: f64::from(r[17]) / 10.0,
            discharge_current_limit: f64::from(r[18]) / 10.0,
            discharge_voltage_limit: f64::from(r[19]) / 10.0,
            total_capacity_ah: r[20],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_request_bytes() {
        assert_eq!(SystemValues::request(), vec![0x01, 0x03, 0x10, 0x00, 0x00, 0x15, 0x80, 0xC5]);
    }

    #[test]
    fn exception_response() {
        let mut f = vec![0x01, 0x83, 0x02];
        let crc = crc16(&f);
        f.extend_from_slice(&crc.to_le_bytes());
        assert_eq!(parse_read_response(0x03, &f), Err(Error::ModbusException(2)));
    }
}
