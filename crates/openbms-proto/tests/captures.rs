//! Tests against frames captured from a real 12-pack system (firmware 16.06).

use openbms_proto::{frame, IntraPackRecord, Status, Telemetry};

#[test]
fn telemetry_frame_pack1() {
    let raw = b"~2001460010960001100D050D050D060D050D050D070D060D060D060D060D060D060D060D060D070D07060B270B200B210B270B580B30000014D6569A0A6D6003176D60000703E814D9FFF3FF770359027FDDB1\r";
    let f = frame::parse(raw).unwrap();
    assert_eq!((f.address, f.rtn), (0x01, 0x00));
    let t = Telemetry::parse(&f.info).unwrap();
    assert_eq!(t.cell_voltages.len(), 16);
    assert_eq!(t.cell_voltages[0], 3.333);
    assert_eq!(t.cell_temperatures, vec![12.4, 11.7, 11.8, 12.4]);
    assert_eq!(t.ambient_temperature, 17.3);
    assert_eq!(t.power_temperature, 13.3);
    assert_eq!(t.current, 0.0);
    assert_eq!(t.pack_voltage, 53.34);
    assert_eq!(t.soc, 79.1);
    assert_eq!(t.full_capacity_ah, 280.0);
    assert_eq!(t.cycles, 7);
    assert_eq!(t.soh, 100.0);
    assert_eq!(t.current_offset_ma, Some(-13));
    assert_eq!(t.idle_current_ma, Some(-137));
    assert_eq!(t.effective_current(), -0.137);
    assert_eq!(t.energy_charged_kwh, Some(85.7));
    assert_eq!(t.energy_discharged_kwh, Some(63.9));
}

#[test]
fn intra_pack_record_charging_above_3a() {
    let raw = b"~200B465AC04000010D4F0D4C0B2E0B1B015715496B7C6D6003D6154A00110000000003028082F00A\r";
    let f = frame::parse(raw).unwrap();
    assert_eq!((f.address, f.rtn), (0x0B, 0x5A));
    let r = IntraPackRecord::parse(&f.info).unwrap();
    assert_eq!(r.highest_cell_voltage, 3.407);
    assert_eq!(r.current, 3.43);
    assert_eq!(r.soc, 98.2);
    assert_eq!(r.alarm_events, [0x00, 0x11, 0x00, 0x00, 0x00, 0x00]);
    assert_eq!(r.system_status, 0x82); // charging + charge current >= 3 A
}

#[test]
fn status_frame_charging_no_alarms() {
    let raw = b"~20014600806200011000000000000000000000000000000000060000000000000000140000000000000300000200000000000000000002EB2F\r";
    let f = frame::parse(raw).unwrap();
    let s = Status::parse(&f.info).unwrap();
    assert_eq!(s.cell_limits.len(), 16);
    assert!(s.discharge_mosfet_on() && s.charge_mosfet_on() && !s.heater_on());
    assert_eq!(s.system_status, 0x02);
    assert_eq!(s.balancing, 0);
    assert!(s.alarms.is_empty());
}

#[test]
fn a_late_telemetry_answer_never_passes_as_status_nor_the_other_way() {
    let tele = frame::parse(b"~2001460010960001100D050D050D060D050D050D070D060D060D060D060D060D060D060D060D070D07060B270B200B210B270B580B30000014D6569A0A6D6003176D60000703E814D9FFF3FF770359027FDDB1\r").unwrap();
    let status = frame::parse(b"~20014600806200011000000000000000000000000000000000060000000000000000140000000000000300000200000000000000000002EB2F\r").unwrap();
    assert!(Status::parse(&tele.info).is_err());
    assert!(Telemetry::parse(&status.info).is_err());
    // a status cut short or with a byte too many no longer parses either
    assert!(Status::parse(&status.info[..status.info.len() - 1]).is_err());
    let mut longer = status.info.clone();
    longer.push(0);
    assert!(Status::parse(&longer).is_err());
}

/// A 0x42 INFO as documented for every version 16.04.06–16.06.13: M cells, 6 temperatures, the fixed part with 10
/// user-defined values, then `reserved` bytes (documented: 8).
fn telemetry_info(m: usize, reserved: usize) -> Vec<u8> {
    let mut i = vec![0x00, 0x01, m as u8];
    i.extend(std::iter::repeat_n([0x0D, 0x05], m).flatten());
    i.push(6);
    i.extend(std::iter::repeat_n([0x0B, 0x27], 6).flatten());
    i.extend([0x00, 0x00, 0x14, 0xD6, 0x56, 0x9A, 0x0A, 0x6D, 0x60, 0x03, 0x17, 0x6D, 0x60, 0x00, 0x07, 0x03, 0xE8, 0x14, 0xD9]);
    i.extend(std::iter::repeat_n(0u8, reserved));
    i
}

/// A 0x44 INFO as documented: M cell bytes, 6 temperature bytes, current/voltage, 20 bit bytes, all set to `fill`.
fn status_info(m: usize, fill: u8) -> Vec<u8> {
    let mut i = vec![0x00, 0x01, m as u8];
    i.extend(std::iter::repeat_n(fill, m));
    i.push(6);
    i.extend(std::iter::repeat_n(fill, 8));
    i.push(20);
    i.extend(std::iter::repeat_n(fill, 20));
    i
}

#[test]
fn every_documented_0x42_length_passes_and_no_0x44_passes_as_0x42() {
    for m in 1..=32 {
        let t = Telemetry::parse(&telemetry_info(m, 8)).unwrap_or_else(|e| panic!("{m} cells: {e}"));
        assert_eq!(t.cell_voltages.len(), m);
        // reserve: fewer or more reserved fields up to the limit
        assert!(Telemetry::parse(&telemetry_info(m, 0)).is_ok());
        assert!(Telemetry::parse(&telemetry_info(m, openbms_proto::telemetry::RESERVED_MAX)).is_ok());
        assert!(Telemetry::parse(&telemetry_info(m, openbms_proto::telemetry::RESERVED_MAX + 2)).is_err());
        for fill in [0x00, 0x01, 0x02, 0x0A, 0xF0, 0xFF] {
            assert!(Telemetry::parse(&status_info(m, fill)).is_err(), "0x44 with {m} cells, bytes {fill:02X}");
            assert!(Status::parse(&status_info(m, fill)).is_ok());
            assert!(Status::parse(&telemetry_info(m, 8)).is_err(), "0x42 with {m} cells as 0x44");
        }
    }
}
