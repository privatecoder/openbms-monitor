//! CID2 0x47: read all parameters (60 x u16, 27 x u8, 8 function switch bytes, device name).

use crate::error::{Error, Result};
use serde::Serialize;

/// Raw unit of a parameter as stored in the BMS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RawUnit {
    /// mV -> V
    MilliVolt,
    /// 10 mV -> V
    CentiVolt,
    /// 0.1 K -> °C
    DeciKelvin,
    /// 10 mA (signed) -> A
    CentiAmp,
    /// 10 mAh -> Ah
    CentiAmpHour,
    /// 0.1 mOhm -> mOhm
    DeciMilliOhm,
    /// value as is (count, %, s, min, h, ms, cell number)
    Plain,
}

impl RawUnit {
    pub fn convert(self, raw: u16) -> f64 {
        match self {
            RawUnit::MilliVolt => f64::from(raw) / 1000.0,
            RawUnit::CentiVolt => f64::from(raw) / 100.0,
            RawUnit::DeciKelvin => (f64::from(raw) - 2731.0) / 10.0,
            RawUnit::CentiAmp => f64::from(raw as i16) / 100.0,
            RawUnit::CentiAmpHour => f64::from(raw) / 100.0,
            RawUnit::DeciMilliOhm => f64::from(raw) / 10.0,
            RawUnit::Plain => f64::from(raw),
        }
    }
}

/// (key, raw unit, display unit) for parameters 0..86 in BMS order.
pub const DEFINITIONS: [(&str, RawUnit, &str); 87] = {
    use RawUnit::*;
    [
        ("cell_high_voltage_alarm", MilliVolt, "V"), ("cell_high_voltage_recovery", MilliVolt, "V"),
        ("cell_low_voltage_alarm", MilliVolt, "V"), ("cell_low_voltage_recovery", MilliVolt, "V"),
        ("cell_overvoltage_protection", MilliVolt, "V"), ("cell_overvoltage_recovery", MilliVolt, "V"),
        ("cell_undervoltage_protection", MilliVolt, "V"), ("cell_undervoltage_recovery", MilliVolt, "V"),
        ("balancing_start_voltage", MilliVolt, "V"), ("cell_low_voltage_charging_forbidden", MilliVolt, "V"),
        ("pack_high_voltage_alarm", CentiVolt, "V"), ("pack_high_voltage_recovery", CentiVolt, "V"),
        ("pack_low_voltage_alarm", CentiVolt, "V"), ("pack_low_voltage_recovery", CentiVolt, "V"),
        ("pack_overvoltage_protection", CentiVolt, "V"), ("pack_overvoltage_recovery", CentiVolt, "V"),
        ("pack_undervoltage_protection", CentiVolt, "V"), ("pack_undervoltage_recovery", CentiVolt, "V"),
        ("charger_overvoltage_protection", CentiVolt, "V"), ("charger_overvoltage_recovery", CentiVolt, "V"),
        ("charging_high_temperature_alarm", DeciKelvin, "°C"), ("charging_high_temperature_recovery", DeciKelvin, "°C"),
        ("charging_low_temperature_alarm", DeciKelvin, "°C"), ("charging_low_temperature_recovery", DeciKelvin, "°C"),
        ("charging_over_temperature_protection", DeciKelvin, "°C"), ("charging_over_temperature_recovery", DeciKelvin, "°C"),
        ("charging_under_temperature_protection", DeciKelvin, "°C"), ("charging_under_temperature_recovery", DeciKelvin, "°C"),
        ("discharging_high_temperature_alarm", DeciKelvin, "°C"), ("discharging_high_temperature_recovery", DeciKelvin, "°C"),
        ("discharging_low_temperature_alarm", DeciKelvin, "°C"), ("discharging_low_temperature_recovery", DeciKelvin, "°C"),
        ("discharging_over_temperature_protection", DeciKelvin, "°C"), ("discharging_over_temperature_recovery", DeciKelvin, "°C"),
        ("discharging_under_temperature_protection", DeciKelvin, "°C"), ("discharging_under_temperature_recovery", DeciKelvin, "°C"),
        ("cell_heating_on", DeciKelvin, "°C"), ("cell_heating_off", DeciKelvin, "°C"),
        ("ambient_high_temperature_alarm", DeciKelvin, "°C"), ("ambient_high_temperature_recovery", DeciKelvin, "°C"),
        ("ambient_low_temperature_alarm", DeciKelvin, "°C"), ("ambient_low_temperature_recovery", DeciKelvin, "°C"),
        ("ambient_over_temperature_protection", DeciKelvin, "°C"), ("ambient_over_temperature_recovery", DeciKelvin, "°C"),
        ("ambient_under_temperature_protection", DeciKelvin, "°C"), ("ambient_under_temperature_recovery", DeciKelvin, "°C"),
        ("power_high_temperature_alarm", DeciKelvin, "°C"), ("power_high_temperature_recovery", DeciKelvin, "°C"),
        ("power_over_temperature_protection", DeciKelvin, "°C"), ("power_over_temperature_recovery", DeciKelvin, "°C"),
        ("charging_overcurrent_alarm", CentiAmp, "A"), ("charging_overcurrent_recovery", CentiAmp, "A"),
        ("discharging_overcurrent_alarm", CentiAmp, "A"), ("discharging_overcurrent_recovery", CentiAmp, "A"),
        ("charging_overcurrent_protection", CentiAmp, "A"), ("discharging_overcurrent_protection", CentiAmp, "A"),
        ("transient_overcurrent_protection", CentiAmp, "A"), ("output_soft_start_delay", Plain, "ms"),
        ("rated_capacity", CentiAmpHour, "Ah"), ("remaining_capacity_setting", CentiAmpHour, "Ah"),
        ("cell_difference_fault", CentiVolt, "V"), ("cell_difference_fault_recovery", CentiVolt, "V"),
        ("balancing_start_difference", MilliVolt, "V"), ("balancing_stop_difference", MilliVolt, "V"),
        ("static_balancing_time", Plain, "h"), ("cells_in_series", Plain, ""),
        ("charging_overcurrent_delay", Plain, "s"), ("discharging_overcurrent_delay", Plain, "s"),
        ("transient_overcurrent_delay", Plain, "ms"), ("overcurrent_recovery_delay", Plain, "s"),
        ("overcurrent_lock_events", Plain, ""), ("charge_current_limit_duration", Plain, "min"),
        ("charge_activation_window", Plain, "min"), ("charge_activation_interval", Plain, "h"),
        ("charge_activation_attempts", Plain, ""), ("work_record_interval", Plain, "min"),
        ("standby_record_interval", Plain, "min"), ("standby_shutdown_delay", Plain, "h"),
        ("remaining_capacity_alarm", Plain, "%"), ("remaining_capacity_protection", Plain, "%"),
        ("intermittent_recharge_below", Plain, "%"), ("cycle_cumulative_capacity", Plain, "%"),
        ("connection_fault_impedance", DeciMilliOhm, "mΩ"), ("compensation_1_cell", Plain, ""),
        ("compensation_1_resistance", DeciMilliOhm, "mΩ"), ("compensation_2_cell", Plain, ""),
        ("compensation_2_resistance", DeciMilliOhm, "mΩ"),
    ]
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Parameter {
    /// Index 0..86 (P0..P86).
    pub index: usize,
    pub key: &'static str,
    pub raw: u16,
    pub value: f64,
    pub unit: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Parameters {
    pub address: u8,
    pub parameters: Vec<Parameter>,
    /// BitGroup0..7
    pub function_switches: [u8; 8],
    pub device_name: String,
}

impl Parameters {
    pub fn request(address: u8) -> crate::Result<Vec<u8>> {
        crate::frame::encode_request(address, 0x47, &[])
    }

    pub fn parse(info: &[u8]) -> Result<Self> {
        if info.len() < 169 || info[1] != 0x3C || info[122] != 0x1B || info[150] != 0x08 {
            return Err(Error::Layout("unexpected 0x47 markers/length"));
        }
        let mut raws: Vec<u16> = (0..60).map(|i| u16::from_be_bytes([info[2 + 2 * i], info[3 + 2 * i]])).collect();
        raws.extend(info[123..150].iter().map(|b| u16::from(*b)));
        let parameters = DEFINITIONS
            .iter()
            .zip(raws)
            .enumerate()
            .map(|(index, ((key, unit, display), raw))| Parameter { index, key, raw, value: unit.convert(raw), unit: display })
            .collect();
        Ok(Self {
            address: info[0],
            parameters,
            function_switches: info[151..159].try_into().unwrap(),
            device_name: String::from_utf8_lossy(&info[159..169]).trim_matches(|c: char| c == '\0' || c == ' ').to_string(),
        })
    }

    pub fn get(&self, key: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.key == key)
    }
}
