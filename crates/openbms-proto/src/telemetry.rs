//! CID2 0x42: telemetry of one pack.

use crate::error::{Error, Result};
use crate::reader::{kelvin10_to_c, Reader};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Telemetry {
    /// Address echoed by the BMS (second INFO byte).
    pub address: u8,
    /// Cell voltages in V.
    pub cell_voltages: Vec<f64>,
    /// Cell temperature sensors 1..4 in °C.
    pub cell_temperatures: Vec<f64>,
    pub ambient_temperature: f64,
    /// Power stage / MOSFET temperature.
    pub power_temperature: f64,
    /// Current in A (+ = charging). The firmware reports 0 while idle; see `idle_current_ma`.
    pub current: f64,
    pub pack_voltage: f64,
    pub remaining_capacity_ah: f64,
    /// Learned full capacity.
    pub full_capacity_ah: f64,
    /// State of charge in %.
    pub soc: f64,
    /// Rated capacity (parameter 58).
    pub rated_capacity_ah: f64,
    pub cycles: u16,
    /// Always 100.0 in firmware 16.06.
    pub soh: f64,
    /// Voltage at P+/P-.
    pub port_voltage: f64,
    /// "Reserved 1": reference/offset channel of the current measurement (mA).
    pub current_offset_ma: Option<i16>,
    /// "Reserved 2": offset-corrected current with 1 mA resolution, only while idle (else 0).
    pub idle_current_ma: Option<i16>,
    /// "Reserved 3": charged energy in kWh (the firmware sends 650.0 once the counter reaches 6500 kWh).
    pub energy_charged_kwh: Option<f64>,
    /// "Reserved 4": discharged energy in kWh (same limit).
    pub energy_discharged_kwh: Option<f64>,
}

impl Telemetry {
    /// Best available current: the regular value, or the idle current while idle.
    pub fn effective_current(&self) -> f64 {
        if self.current != 0.0 {
            self.current
        } else {
            self.idle_current_ma.map_or(0.0, |ma| f64::from(ma) / 1000.0)
        }
    }

    /// Request INFO for pack `address` (`0x42` with INFO = address).
    pub fn request(address: u8) -> crate::Result<Vec<u8>> {
        crate::frame::encode_request(address, 0x42, &[address])
    }

    pub fn parse(info: &[u8]) -> Result<Self> {
        let mut r = Reader::new(info);
        let _data_flag = r.u8()?;
        let address = r.u8()?;
        let cells = usize::from(r.u8()?);
        if cells == 0 || cells > 32 {
            return Err(Error::Layout("implausible cell count"));
        }
        let cell_voltages = (0..cells).map(|_| r.u16().map(|v| f64::from(v) / 1000.0)).collect::<Result<_>>()?;
        let temps = usize::from(r.u8()?);
        if temps < 2 {
            return Err(Error::Layout("expected at least ambient and power temperature"));
        }
        let mut t = (0..temps).map(|_| r.u16().map(kelvin10_to_c)).collect::<Result<Vec<_>>>()?;
        let power_temperature = t.pop().unwrap();
        let ambient_temperature = t.pop().unwrap();
        let current = f64::from(r.i16()?) / 100.0;
        let pack_voltage = f64::from(r.u16()?) / 100.0;
        let remaining_capacity_ah = f64::from(r.u16()?) / 100.0;
        let _custom = r.u8()?;
        let full_capacity_ah = f64::from(r.u16()?) / 100.0;
        let soc = f64::from(r.u16()?) / 10.0;
        let rated_capacity_ah = f64::from(r.u16()?) / 100.0;
        let cycles = r.u16()?;
        let soh = f64::from(r.u16()?) / 10.0;
        let port_voltage = f64::from(r.u16()?) / 100.0;
        let mut reserved = || -> Option<u16> { if r.remaining() >= 2 { r.u16().ok() } else { None } };
        let r1 = reserved();
        let r2 = reserved();
        let r3 = reserved();
        let r4 = reserved();
        Ok(Self {
            address,
            cell_voltages,
            cell_temperatures: t,
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
            current_offset_ma: r1.map(|v| v as i16),
            idle_current_ma: r2.map(|v| v as i16),
            energy_charged_kwh: r3.map(|v| f64::from(v) / 10.0),
            energy_discharged_kwh: r4.map(|v| f64::from(v) / 10.0),
        })
    }
}
