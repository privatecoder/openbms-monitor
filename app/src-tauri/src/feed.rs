//! The Seplos BMS add-on's OpenBMS Monitor feed (`docs/exchange/openbms-feed-v1.md`): its MQTT messages decoded into
//! the types a bus read gives, and the state of the source (current boot, duplicates, its clock). Pure: no I/O.

use openbms_proto::device_info::DeviceInfo;
use openbms_proto::modbus::SystemValues;
use openbms_proto::serial::{parse_ascii_serial, SerialValue};
use openbms_proto::status::Status;
use openbms_proto::telemetry::Telemetry;
use serde_json::Value;

pub const VERSION: u64 = 1;
/// The canonical 0x47 INFO: address byte + 168 data bytes.
pub const PARAMS_LEN: usize = 169;
/// A battery or BMS serial: 30 string bytes.
pub const SERIAL_LEN: usize = 30;

/// One part of a message: its read time and the decoded value, or why it failed (`busy`, `failed`, `unsupported`
/// from the add-on, or a parse error here).
#[derive(Debug, Clone, PartialEq)]
pub struct Part<T> {
    pub t: i64,
    pub value: Result<T, String>,
}

#[derive(Debug, Clone)]
pub struct FeedPack {
    pub boot: String,
    pub seq: u64,
    pub sent_at: i64,
    pub address: u8,
    pub telemetry: Option<Part<Telemetry>>,
    pub status: Option<Part<Status>>,
    /// the 169-byte 0x47 block, only in the poll that read it
    pub params: Option<Part<Vec<u8>>>,
}

#[derive(Debug, Clone)]
pub struct FeedSystem {
    pub boot: String,
    pub seq: u64,
    pub values: Option<Part<SystemValues>>,
}

#[derive(Debug, Clone)]
pub struct MetaPack {
    pub address: u8,
    pub device: Option<Part<DeviceInfo>>,
    pub battery_serial: Option<Part<SerialValue>>,
    pub bms_serial: Option<Part<SerialValue>>,
    pub params: Option<Part<Vec<u8>>>,
}

#[derive(Debug, Clone)]
pub struct FeedMeta {
    pub on: bool,
    pub boot: String,
    pub packs: Vec<MetaPack>,
}

fn version(v: &Value) -> Result<(), String> {
    match v["v"].as_u64() {
        Some(VERSION) => Ok(()),
        Some(other) => Err(format!("feed version {other} not supported")),
        None => Err("feed version missing".into()),
    }
}

fn text(v: &Value, key: &str) -> Result<String, String> {
    v[key].as_str().map(str::to_owned).ok_or_else(|| format!("{key} missing"))
}

fn number(v: &Value, key: &str) -> Result<u64, String> {
    v[key].as_u64().ok_or_else(|| format!("{key} missing"))
}

/// A part `{"t", "hex"}` or `{"t", "err"}`; None when absent (not attempted). Bytes that don't decode make it failed.
fn part<T>(v: &Value, decode: impl FnOnce(&[u8]) -> Result<T, String>) -> Option<Part<T>> {
    if !v.is_object() {
        return None;
    }
    let t = v["t"].as_i64().unwrap_or(0);
    let value = match (v["hex"].as_str(), v["err"].as_str()) {
        (Some(hex), _) => from_hex(hex).and_then(|b| decode(&b)),
        (None, Some(err)) => Err(err.to_owned()),
        (None, None) => Err("neither hex nor err".into()),
    };
    Some(Part { t, value })
}

fn from_hex(s: &str) -> Result<Vec<u8>, String> {
    if !s.is_ascii() {
        return Err("invalid hex".into());
    }
    if !s.len().is_multiple_of(2) {
        return Err("odd hex length".into());
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "invalid hex".to_string())).collect()
}

fn params(b: &[u8]) -> Result<Vec<u8>, String> {
    if b.len() == PARAMS_LEN { Ok(b.to_vec()) } else { Err(format!("parameter block of {} bytes, not {PARAMS_LEN}", b.len())) }
}

fn serial(b: &[u8]) -> Result<SerialValue, String> {
    if b.len() != SERIAL_LEN {
        return Err(format!("serial of {} bytes, not {SERIAL_LEN}", b.len()));
    }
    parse_ascii_serial(b).map_err(|e| e.to_string())
}

pub fn decode_pack(v: &Value) -> Result<FeedPack, String> {
    version(v)?;
    let address = u8::try_from(number(v, "addr")?).map_err(|_| "addr out of range".to_string())?;
    Ok(FeedPack {
        boot: text(v, "boot")?,
        seq: number(v, "seq")?,
        sent_at: v["sent_at"].as_i64().unwrap_or(0),
        address,
        telemetry: part(&v["r42"], |b| Telemetry::parse(b).map_err(|e| e.to_string())),
        status: part(&v["r44"], |b| Status::parse(b).map_err(|e| e.to_string())),
        params: part(&v["r47"], params),
    })
}

/// `r1000` only (21 registers, big endian); `r0074` is not used in v1.
pub fn decode_system(v: &Value) -> Result<FeedSystem, String> {
    version(v)?;
    Ok(FeedSystem {
        boot: text(v, "boot")?,
        seq: number(v, "seq")?,
        values: part(&v["r1000"], |b| {
            let regs: Vec<u16> = b.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            if b.len() % 2 != 0 {
                return Err("odd register byte count".into());
            }
            SystemValues::from_registers(&regs).map_err(|e| e.to_string())
        }),
    })
}

pub fn decode_meta(v: &Value) -> Result<FeedMeta, String> {
    version(v)?;
    let packs = v["packs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let address = u8::try_from(p["addr"].as_u64()?).ok()?;
                    Some(MetaPack {
                        address,
                        device: part(&p["r51"], |b| DeviceInfo::parse(b).map_err(|e| e.to_string())),
                        battery_serial: part(&p["a2"], serial),
                        bms_serial: part(&p["a4"], serial),
                        params: part(&p["r47"], params),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(FeedMeta {
        on: v["on"].as_bool().unwrap_or(false),
        boot: text(v, "boot")?,
        packs,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accept {
    Yes,
    Duplicate,
    OtherBoot,
    NoMeta,
}

/// What the Monitor knows of the source: the boot of the latest meta and the last accepted `seq`.
#[derive(Debug, Default)]
pub struct SourceState {
    boot: Option<String>,
    last_seq: u64,
}

impl SourceState {
    /// A meta arrived; true when its boot differs from the current one (the caller starts over).
    pub fn meta(&mut self, m: &FeedMeta) -> bool {
        if self.boot.as_deref() == Some(m.boot.as_str()) {
            return false;
        }
        let changed = self.boot.is_some();
        self.boot = Some(m.boot.clone());
        self.last_seq = 0;
        changed
    }

    pub fn boot(&self) -> Option<&str> {
        self.boot.as_deref()
    }

    /// Whether a pack or system message of `boot` with `seq` is new for the current boot.
    pub fn accept(&mut self, boot: &str, seq: u64) -> Accept {
        match self.boot.as_deref() {
            None => Accept::NoMeta,
            Some(b) if b != boot => Accept::OtherBoot,
            _ if seq <= self.last_seq => Accept::Duplicate,
            _ => {
                self.last_seq = seq;
                Accept::Yes
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn telemetry_info(m: usize) -> Vec<u8> {
        let mut i = vec![0x00, 0x01, m as u8];
        i.extend(std::iter::repeat_n([0x0D, 0x05], m).flatten());
        i.push(6);
        i.extend(std::iter::repeat_n([0x0B, 0x27], 6).flatten());
        i.extend([0x00, 0x00, 0x14, 0xD6, 0x56, 0x9A, 0x0A, 0x6D, 0x60, 0x03, 0x17, 0x6D, 0x60, 0x00, 0x07, 0x03, 0xE8, 0x14, 0xD9]);
        i.extend(std::iter::repeat_n(0u8, 8));
        i
    }

    fn status_info(m: usize) -> Vec<u8> {
        let mut i = vec![0x00, 0x01, m as u8];
        i.extend(std::iter::repeat_n(0u8, m));
        i.push(6);
        i.extend(std::iter::repeat_n(0u8, 8));
        i.push(20);
        i.extend(std::iter::repeat_n(0u8, 20));
        i
    }

    fn serial30(s: &str) -> Vec<u8> {
        let mut b = s.as_bytes().to_vec();
        b.resize(30, 0);
        b
    }

    fn pack(extra: Value) -> Value {
        let mut v = json!({"v": 1, "boot": "b1", "seq": 3, "sent_at": 1000, "addr": 0});
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        v
    }

    #[test]
    fn pack_with_both_parts() {
        let p = decode_pack(&pack(json!({"r42": {"t": 10, "hex": hex(&telemetry_info(16))}, "r44": {"t": 11, "hex": hex(&status_info(16))}}))).unwrap();
        let t = p.telemetry.unwrap();
        assert_eq!((t.t, t.value.unwrap().cell_voltages.len()), (10, 16));
        assert_eq!(p.status.unwrap().t, 11);
        assert!(p.params.is_none());
        assert_eq!((p.boot.as_str(), p.seq, p.sent_at, p.address), ("b1", 3, 1000, 0));
    }

    #[test]
    fn pack_r42_failed_has_no_status() {
        let p = decode_pack(&pack(json!({"r42": {"t": 10, "err": "failed"}}))).unwrap();
        assert_eq!(p.telemetry.unwrap().value.unwrap_err(), "failed");
        assert!(p.status.is_none());
    }

    #[test]
    fn bad_hex_fails_only_that_part() {
        let p = decode_pack(&pack(json!({"r42": {"t": 10, "hex": "zz"}, "r44": {"t": 11, "hex": hex(&status_info(16))}, "r47": {"t": 5, "hex": "00"}}))).unwrap();
        assert!(p.telemetry.unwrap().value.is_err());
        assert!(p.status.unwrap().value.is_ok());
        assert!(p.params.unwrap().value.unwrap_err().contains("169"));
    }

    #[test]
    fn wrong_version_is_rejected() {
        let mut v = pack(json!({}));
        v["v"] = json!(2);
        assert_eq!(decode_pack(&v).unwrap_err(), "feed version 2 not supported");
    }

    #[test]
    fn unknown_address_decodes() {
        let mut v = pack(json!({"r42": {"t": 1, "hex": hex(&telemetry_info(16))}}));
        v["addr"] = json!(15);
        assert_eq!(decode_pack(&v).unwrap().address, 15);
    }

    #[test]
    fn system_ignores_r0074_and_its_error() {
        let regs: Vec<u8> = (0..21u16).flat_map(|r| r.to_be_bytes()).collect();
        let base = json!({"v": 1, "boot": "b1", "seq": 4, "sent_at": 9, "r1000": {"t": 7, "hex": hex(&regs)}});
        for r0074 in [json!({"t": 7, "hex": "00c0"}), json!({"t": 7, "err": "unsupported"}), Value::Null] {
            let mut v = base.clone();
            v["r0074"] = r0074;
            let s = decode_system(&v).unwrap();
            assert_eq!(s.values.unwrap().t, 7);
        }
        let mut short = base.clone();
        short["r1000"] = json!({"t": 7, "hex": "0001"});
        assert!(decode_system(&short).unwrap().values.unwrap().value.is_err());
    }

    fn meta(boot: &str, packs: Value) -> Value {
        json!({"v": 1, "on": true, "auto_off": null, "boot": boot, "sent_at": 1, "bus": "uart1", "addon": "x", "packs": packs, "sets": []})
    }

    #[test]
    fn meta_parses_device_serials_params() {
        let mut params = vec![0u8];
        params.extend(std::iter::repeat_n(0u8, 168));
        let m = decode_meta(&meta("b1", json!([{"addr": 0, "r51": null, "a2": {"t": 3, "hex": hex(&serial30("BAT-1"))},
            "a4": null, "maint": null, "r47": {"t": 4, "hex": hex(&params)}}]))).unwrap();
        assert!(m.on && m.packs.len() == 1);
        let p = &m.packs[0];
        assert_eq!(p.battery_serial.as_ref().unwrap().value, Ok(SerialValue::Set { value: "BAT-1".into(), truncated: false }));
        assert!(p.device.is_none() && p.bms_serial.is_none());
        assert_eq!(p.params.as_ref().unwrap().t, 4);
    }

    #[test]
    fn meta_a2_wrong_length_is_a_failed_part() {
        let m = decode_meta(&meta("b1", json!([{"addr": 0, "a2": {"t": 3, "hex": "4142"}}]))).unwrap();
        assert!(m.packs[0].battery_serial.as_ref().unwrap().value.is_err());
    }

    #[test]
    fn meta_off() {
        let mut v = meta("b1", json!([]));
        v["on"] = json!(false);
        assert!(!decode_meta(&v).unwrap().on);
    }

    #[test]
    fn no_meta_rejects_packs() {
        assert_eq!(SourceState::default().accept("b1", 1), Accept::NoMeta);
    }

    fn state(boot: &str) -> SourceState {
        let mut s = SourceState::default();
        s.meta(&decode_meta(&meta(boot, json!([]))).unwrap());
        s
    }

    #[test]
    fn duplicate_and_older_seq_rejected() {
        let mut s = state("b1");
        assert_eq!(s.accept("b1", 5), Accept::Yes);
        assert_eq!(s.accept("b1", 5), Accept::Duplicate);
        assert_eq!(s.accept("b1", 4), Accept::Duplicate);
        assert_eq!(s.accept("b1", 6), Accept::Yes);
    }

    #[test]
    fn new_boot_via_meta_accepts_seq_one_and_reports_change() {
        let mut s = state("b1");
        assert_eq!(s.accept("b1", 40), Accept::Yes);
        assert!(!s.meta(&decode_meta(&meta("b1", json!([]))).unwrap()));
        assert!(s.meta(&decode_meta(&meta("b2", json!([]))).unwrap()));
        assert_eq!(s.accept("b2", 1), Accept::Yes);
    }

    #[test]
    fn late_old_boot_message_rejected() {
        let mut s = state("b1");
        s.meta(&decode_meta(&meta("b2", json!([]))).unwrap());
        assert_eq!(s.accept("b1", 99), Accept::OtherBoot);
    }

    #[test]
    fn non_ascii_hex_fails_only_that_part() {
        let p = decode_pack(&pack(json!({"r42": {"t": 10, "hex": "€0"}, "r44": {"t": 11, "hex": hex(&status_info(16))}}))).unwrap();
        assert!(p.telemetry.unwrap().value.is_err());
        assert!(p.status.unwrap().value.is_ok());
    }
}
