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
