//! CID2 0x44: alarms and status of one pack.
//!
//! Every active bit is reported with a stable id (`alarm.ev<n>.b<bit>`); texts and help live in
//! the UI's language files, keyed by that id.

use crate::error::{Error, Result};
use crate::reader::Reader;
use serde::Serialize;

/// Per-measurement limit state ("byte alarms").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitState {
    Normal,
    BelowLimit,
    AboveLimit,
    Other(u8),
}

impl From<u8> for LimitState {
    fn from(b: u8) -> Self {
        match b {
            0x00 => LimitState::Normal,
            0x01 => LimitState::BelowLimit,
            0x02 => LimitState::AboveLimit,
            x => LimitState::Other(x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational state (e.g. heating requested, waiting for charge activation).
    Info,
    Warning,
    /// Protection active: a MOSFET is (or may be) switched off.
    Protection,
    /// Hardware or configuration fault.
    Fault,
}

/// (event 1..8, bit, key, severity). Event 7 and 8 contain only the bits visible in 0x44.
pub const ALARM_BITS: &[(u8, u8, &str, Severity)] = {
    use Severity::*;
    &[
        (1, 0, "voltage_sensor_fault", Fault),
        (1, 1, "temperature_sensor_fault", Fault),
        (1, 2, "current_sensor_fault", Fault),
        (1, 3, "button_fault", Fault),
        (1, 4, "cell_difference_fault", Fault),
        (1, 5, "charge_switch_fault", Fault),
        (1, 6, "discharge_switch_fault", Fault),
        (1, 7, "current_limiter_fault", Fault),
        (2, 0, "cell_high_voltage_warning", Warning),
        (2, 1, "cell_overvoltage_protection", Protection),
        (2, 2, "cell_low_voltage_warning", Warning),
        (2, 3, "cell_undervoltage_protection", Protection),
        (2, 4, "pack_high_voltage_warning", Warning),
        (2, 5, "pack_overvoltage_protection", Protection),
        (2, 6, "pack_low_voltage_warning", Warning),
        (2, 7, "pack_undervoltage_protection", Protection),
        (3, 0, "charge_high_temperature_warning", Warning),
        (3, 1, "charge_over_temperature_protection", Protection),
        (3, 2, "charge_low_temperature_warning", Warning),
        (3, 3, "charge_under_temperature_protection", Protection),
        (3, 4, "discharge_high_temperature_warning", Warning),
        (3, 5, "discharge_over_temperature_protection", Protection),
        (3, 6, "discharge_low_temperature_warning", Warning),
        (3, 7, "discharge_under_temperature_protection", Protection),
        (4, 0, "ambient_high_temperature_warning", Warning),
        (4, 1, "ambient_over_temperature_protection", Protection),
        (4, 2, "ambient_low_temperature_warning", Warning),
        (4, 3, "ambient_under_temperature_protection", Protection),
        (4, 4, "power_over_temperature_protection", Protection),
        (4, 5, "power_high_temperature_warning", Warning),
        (4, 6, "cell_heating_requested", Info),
        (4, 7, "severe_fault", Fault),
        (5, 0, "charge_overcurrent_warning", Warning),
        (5, 1, "charge_overcurrent_protection", Protection),
        (5, 2, "discharge_overcurrent_warning", Warning),
        (5, 3, "discharge_overcurrent_protection", Protection),
        (5, 4, "transient_overcurrent_protection", Protection),
        (5, 5, "short_circuit_protection", Protection),
        (5, 6, "transient_overcurrent_lock", Protection),
        (5, 7, "short_circuit_lock", Protection),
        (6, 0, "charger_overvoltage_protection", Protection),
        (6, 1, "intermittent_recharge_waiting", Info),
        (6, 2, "remaining_capacity_warning", Warning),
        (6, 3, "remaining_capacity_protection", Protection),
        (6, 4, "cell_too_low_to_charge", Protection),
        (6, 5, "reverse_polarity", Protection),
        (6, 6, "output_connection_fault", Fault),
        (7, 4, "charge_activation_automatic", Info),
        (7, 5, "charge_activation_manual", Info),
        (8, 0, "eeprom_fault", Fault),
        (8, 1, "rtc_fault", Fault),
        (8, 2, "voltage_not_calibrated", Fault),
        (8, 3, "current_not_calibrated", Fault),
        (8, 4, "zero_point_not_calibrated", Fault),
        (8, 5, "rtc_time_invalid", Warning),
    ]
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActiveAlarm {
    /// Stable id, e.g. "alarm.ev2.b1".
    pub id: String,
    pub key: &'static str,
    pub severity: Severity,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub address: u8,
    pub cell_limits: Vec<LimitState>,
    pub cell_temperature_limits: Vec<LimitState>,
    pub ambient_temperature_limit: LimitState,
    pub power_temperature_limit: LimitState,
    pub current_limit: LimitState,
    pub pack_voltage_limit: LimitState,
    /// Alarm events 1..8 (event 6 & 0x7F, event 7 & 0x30, event 8 & 0x3F as sent in 0x44).
    pub events: [u8; 8],
    /// On-off state: bit0 discharge MOSFET, bit1 charge MOSFET, bit2 current limiter, bit3 heater.
    pub switch_state: u8,
    /// Bit n = cell n+1 is being balanced.
    pub balancing: u32,
    /// System status & 0x33: bit0 discharging, bit1 charging, bit4 standby, bit5 off.
    pub system_status: u8,
    pub alarms: Vec<ActiveAlarm>,
}

impl Status {
    pub fn request(address: u8) -> crate::Result<Vec<u8>> {
        crate::frame::encode_request(address, 0x44, &[address])
    }

    pub fn discharge_mosfet_on(&self) -> bool { self.switch_state & 0x01 != 0 }
    pub fn charge_mosfet_on(&self) -> bool { self.switch_state & 0x02 != 0 }
    pub fn current_limiter_on(&self) -> bool { self.switch_state & 0x04 != 0 }
    pub fn heater_on(&self) -> bool { self.switch_state & 0x08 != 0 }

    pub fn parse(info: &[u8]) -> Result<Self> {
        let mut r = Reader::new(info);
        let _flag = r.u8()?;
        let address = r.u8()?;
        let cells = usize::from(r.u8()?);
        let cell_limits = r.bytes(cells)?.iter().map(|b| LimitState::from(*b)).collect();
        let temps = usize::from(r.u8()?);
        if temps < 2 {
            return Err(Error::Layout("expected at least ambient and power temperature"));
        }
        let mut t: Vec<LimitState> = r.bytes(temps)?.iter().map(|b| LimitState::from(*b)).collect();
        let power_temperature_limit = t.pop().unwrap();
        let ambient_temperature_limit = t.pop().unwrap();
        let current_limit = LimitState::from(r.u8()?);
        let pack_voltage_limit = LimitState::from(r.u8()?);
        let _bit_count = r.u8()?;
        let ev = r.bytes(6)?;
        let switch_state = r.u8()?;
        let bal_lo = r.u8()?;
        let bal_hi = r.u8()?;
        let system_status = r.u8()?;
        let _disconnection = r.bytes(2)?;
        let ev7 = r.u8()?;
        let ev8 = r.u8()?;
        let events = [ev[0], ev[1], ev[2], ev[3], ev[4], ev[5], ev7, ev8];
        let alarms = ALARM_BITS
            .iter()
            .filter(|(e, b, _, _)| events[usize::from(*e) - 1] & (1 << b) != 0)
            .map(|(e, b, key, severity)| ActiveAlarm { id: format!("alarm.ev{e}.b{b}"), key, severity: *severity })
            .collect();
        Ok(Self {
            address,
            cell_limits,
            cell_temperature_limits: t,
            ambient_temperature_limit,
            power_temperature_limit,
            current_limit,
            pack_voltage_limit,
            events,
            switch_state,
            balancing: u32::from(bal_lo) | (u32::from(bal_hi) << 8),
            system_status,
            alarms,
        })
    }
}
