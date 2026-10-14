//! CID2 0x47: read all parameters (60 x u16, 27 x u8, 8 function switch bytes, device name); CID2 0xA1 writes the
//! same 169-byte block back (RS485-1/2 only).
//!
//! 0xA1 refuses a wrong marker with RTN 04 and an implausible block with RTN 06 (the app checks the firmware's rules
//! before sending). The function switches and the name are taken into RAM before that check, so after RTN 06 they
//! are live but not stored.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

pub const CID2_READ: u8 = 0x47;
pub const CID2_WRITE: u8 = 0xA1;
/// Length of the parameter block (INFO of the 0x47 answer and of 0xA1).
pub const BLOCK_LEN: usize = 169;
/// Parameters P60.. are single bytes.
const FIRST_U8: usize = 60;

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
    /// The block as read, the base of a write (keeps `[0]`, the markers and the device name).
    #[serde(skip)]
    pub block: Vec<u8>,
}

/// What a write changes: the raw values P0..P86 as stored (u16, currents two's complement) and the 8 switch bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Values {
    pub raws: Vec<u16>,
    pub switches: [u8; 8],
}

/// Whether a block has the 0x47 layout.
fn check_block(block: &[u8]) -> Result<()> {
    if block.len() < BLOCK_LEN || block[1] != 0x3C || block[122] != 0x1B || block[150] != 0x08 {
        return Err(Error::Layout("unexpected 0x47 markers/length"));
    }
    Ok(())
}

impl Values {
    fn of_block(block: &[u8]) -> Self {
        let mut raws: Vec<u16> = (0..FIRST_U8).map(|i| u16::from_be_bytes([block[2 + 2 * i], block[3 + 2 * i]])).collect();
        raws.extend(block[123..150].iter().map(|b| u16::from(*b)));
        Self { raws, switches: block[151..159].try_into().unwrap() }
    }

    /// The block to send: `base` (a fresh 0x47 read) with these values; `[0]`, markers and name stay as read.
    pub fn apply(&self, base: &[u8]) -> Result<Vec<u8>> {
        check_block(base)?;
        if self.raws.len() != DEFINITIONS.len() {
            return Err(Error::Layout("expected 87 parameter values"));
        }
        if self.raws[FIRST_U8..].iter().any(|r| *r > 0xFF) {
            return Err(Error::Layout("P60..P86 are single bytes"));
        }
        let mut out = base[..BLOCK_LEN].to_vec();
        for (i, r) in self.raws[..FIRST_U8].iter().enumerate() {
            out[2 + 2 * i..4 + 2 * i].copy_from_slice(&r.to_be_bytes());
        }
        for (i, r) in self.raws[FIRST_U8..].iter().enumerate() {
            out[123 + i] = *r as u8;
        }
        out[151..159].copy_from_slice(&self.switches);
        Ok(out)
    }

    /// The edit on top of a fresh read: what the user changed (`edited` against `base`, the read the edit started
    /// from) wins, everything else comes from `fresh`. `None` if the pack changed meanwhile in a value or switch bit
    /// the user did not touch: writing would silently undo that change.
    pub fn rebase(base: &Values, edited: &Values, fresh: &Values) -> Option<Values> {
        let n = DEFINITIONS.len();
        if [base, edited, fresh].iter().any(|v| v.raws.len() != n) {
            return None;
        }
        let mut raws = Vec::with_capacity(n);
        for i in 0..n {
            let touched = edited.raws[i] != base.raws[i];
            if !touched && fresh.raws[i] != base.raws[i] {
                return None;
            }
            raws.push(if touched { edited.raws[i] } else { fresh.raws[i] });
        }
        let mut switches = [0u8; 8];
        for g in 0..8 {
            let touched = edited.switches[g] ^ base.switches[g];
            if (fresh.switches[g] ^ base.switches[g]) & !touched != 0 {
                return None;
            }
            switches[g] = (edited.switches[g] & touched) | (fresh.switches[g] & !touched);
        }
        Some(Values { raws, switches })
    }
}

/// 0xA1 with a complete block (see `Values::apply`).
pub fn write_request(address: u8, block: &[u8]) -> Result<Vec<u8>> {
    check_block(block)?;
    crate::frame::encode_request(address, CID2_WRITE, &block[..BLOCK_LEN])
}

impl Parameters {
    pub fn request(address: u8) -> crate::Result<Vec<u8>> {
        crate::frame::encode_request(address, CID2_READ, &[])
    }

    pub fn parse(info: &[u8]) -> Result<Self> {
        check_block(info)?;
        let raws = Values::of_block(info).raws;
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
            block: info[..BLOCK_LEN].to_vec(),
        })
    }

    pub fn values(&self) -> Values {
        Values::of_block(&self.block)
    }

    pub fn get(&self, key: &str) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    /// A synthetic block: P0..P59 = 1000 + i, P60.. = i, switches 0xA0 + g, name "SYN-TEST".
    fn block() -> Vec<u8> {
        let mut b = vec![0x07, 0x3C];
        for i in 0..60u16 {
            b.extend((1000 + i).to_be_bytes());
        }
        b.push(0x1B);
        b.extend(60..87u8);
        b.push(0x08);
        b.extend((0..8u8).map(|g| 0xA0 + g));
        b.extend(b"SYN-TEST  ");
        assert_eq!(b.len(), BLOCK_LEN);
        b
    }

    #[test]
    fn parses_the_block_and_keeps_it() {
        let p = Parameters::parse(&block()).unwrap();
        assert_eq!((p.address, p.device_name.as_str()), (7, "SYN-TEST"));
        assert_eq!(p.parameters[0].raw, 1000);
        assert_eq!(p.parameters[59].raw, 1059);
        assert_eq!(p.parameters[60].raw, 60);
        assert_eq!(p.parameters[86].raw, 86);
        assert_eq!(p.function_switches[7], 0xA7);
        assert_eq!(p.block, block());
        assert_eq!(p.values().raws.len(), 87);
        let mut bad = block();
        bad[122] = 0;
        assert!(Parameters::parse(&bad).is_err());
    }

    #[test]
    fn signed_currents_convert() {
        assert_eq!(RawUnit::CentiAmp.convert((-15000i16) as u16), -150.0);
        assert_eq!(RawUnit::DeciKelvin.convert(2731), 0.0);
    }

    #[test]
    fn apply_writes_values_and_keeps_byte_zero_markers_and_name() {
        let base = block();
        let mut v = Values::of_block(&base);
        v.raws[0] = 3650;
        v.raws[50] = (-1500i16) as u16;
        v.raws[59] = 0xBEEF;
        v.raws[86] = 200;
        v.switches[3] = 0x3F;
        let out = v.apply(&base).unwrap();
        assert_eq!(out.len(), BLOCK_LEN);
        assert_eq!((out[0], out[1], out[122], out[150]), (0x07, 0x3C, 0x1B, 0x08));
        assert_eq!(&out[2..4], &3650u16.to_be_bytes());
        assert_eq!(&out[102..104], &[0xFA, 0x24]);
        assert_eq!(&out[120..122], &[0xBE, 0xEF]);
        assert_eq!(out[149], 200);
        assert_eq!(out[154], 0x3F);
        assert_eq!(&out[159..], b"SYN-TEST  ");
        assert_eq!(Parameters::parse(&out).unwrap().values(), v);
    }

    #[test]
    fn apply_refuses_bad_input() {
        let base = block();
        let mut v = Values::of_block(&base);
        v.raws[60] = 256;
        assert!(v.apply(&base).is_err());
        let short = Values { raws: vec![0; 86], switches: [0; 8] };
        assert!(short.apply(&base).is_err());
        let ok = Values::of_block(&base);
        assert!(ok.apply(&base[..168]).is_err());
        let mut bad = base.clone();
        bad[150] = 0x09;
        assert!(ok.apply(&bad).is_err());
    }

    #[test]
    fn write_frame_carries_the_whole_block() {
        let raw = write_request(3, &block()).unwrap();
        let f = frame::parse(&raw).unwrap();
        assert_eq!((f.address, f.rtn), (3, CID2_WRITE));
        assert_eq!(f.info, block());
        assert!(write_request(3, &block()[..100]).is_err());
    }

    #[test]
    fn rebase_keeps_the_users_changes_on_a_fresh_read() {
        let base = Values::of_block(&block());
        let mut edited = base.clone();
        edited.raws[12] = 4640;
        edited.switches[1] ^= 0b0000_0100;
        // unchanged pack: the edit as is
        assert_eq!(Values::rebase(&base, &edited, &base), Some(edited.clone()));
        // the pack changed a value the user also changed: the user's value wins
        let mut fresh = base.clone();
        fresh.raws[12] = 4500;
        fresh.switches[1] ^= 0b0000_0100;
        assert_eq!(Values::rebase(&base, &edited, &fresh), Some(edited.clone()));
        // the pack changed an untouched value or switch bit: stop
        let mut fresh = base.clone();
        fresh.raws[13] += 1;
        assert_eq!(Values::rebase(&base, &edited, &fresh), None);
        let mut fresh = base.clone();
        fresh.switches[1] ^= 0b0000_0001;
        assert_eq!(Values::rebase(&base, &edited, &fresh), None);
        let mut fresh = base.clone();
        fresh.switches[5] ^= 0x80;
        assert_eq!(Values::rebase(&base, &edited, &fresh), None);
        let short = Values { raws: vec![0; 3], switches: [0; 8] };
        assert_eq!(Values::rebase(&base, &edited, &short), None);
    }
}
