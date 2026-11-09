use openbms_flash::{
    ehex::{Header, Image},
    session::{self, Options},
    sim::BootSim,
};
use std::time::Duration;

fn image() -> Image {
    let code = vec![0x12; 300];
    Image {
        header: Header {
            mcu: "SWM181RCT6_64_M0".into(),
            device: "EMU1101_10D_HW16".into(),
            interface: "RS485Y19200N9600".into(),
            raw: vec![0x55; 256],
        },
        code_sha256: String::new(),
        file_sha256: String::new(),
        code,
        start: [0, 0, 0, 0xc1],
        stack_top: 0x2000_1000,
        reset_vector: 0xc1,
    }
}

fn fast() -> Options {
    Options {
        boot_delay: Duration::from_millis(50),
        min_frame_gap: Duration::ZERO,
        ack_timeout: Duration::from_millis(50),
        post_timeout: Duration::from_secs(2),
    }
}

#[test]
fn dry_run_pads_last_block_and_advances_address_by_16() {
    let frames = session::dry_run(&image()).unwrap();
    assert_eq!(frames.len(), 6); // RFL, WFE, three data blocks, WFG
    let first = openbms_flash::boot::decode(&frames[2]).unwrap();
    let last = openbms_flash::boot::decode(&frames[4]).unwrap();
    assert_eq!(first.address, 0);
    assert_eq!(last.address, 32);
    assert_eq!(last.data.len(), 256);
    assert_eq!(last.data[48], 0); // header 256 + code 300 + start 4 => 48 bytes in block 2
}

#[test]
fn probe_and_flash_complete_against_bootloader_simulator() {
    let mut probe_sim = BootSim::new();
    let probe = session::probe(&mut probe_sim, 0, &mut Vec::new(), fast()).unwrap();
    assert_eq!(probe.packet_size, 256);
    assert!(probe.app_returned);
    assert!(!probe_sim.wrote_flash());

    let mut flash_sim = BootSim::new();
    let result = session::flash(&mut flash_sim, 0, &image(), &mut Vec::new(), fast()).unwrap();
    assert_eq!(result.blocks, 3);
    assert!(flash_sim.wrote_flash());
    assert!(result.app_returned);
}

#[test]
fn the_frame_capture_pauses_from_rfl_through_wfg_only() {
    let mut sim = BootSim::new();
    session::flash(&mut sim, 0, &image(), &mut Vec::new(), fast()).unwrap();
    let writes = sim.capture_writes();
    // every bootloader frame while paused, every ASCII frame (preflight, reset, check afterwards) captured
    assert!(writes.iter().all(|(boot, paused)| boot == paused), "{writes:?}");
    assert_eq!(writes.iter().filter(|(boot, _)| *boot).count(), 6);
    assert!(writes.last().is_some_and(|(boot, _)| !boot));
    assert!(!sim.capture_paused());

    // a failure inside the bootloader part takes the link in again
    let mut sim = BootSim::new();
    sim.reject_data_block(1);
    assert!(session::flash(&mut sim, 0, &image(), &mut Vec::new(), fast()).is_err());
    assert!(!sim.capture_paused());

    let mut sim = BootSim::new();
    session::probe(&mut sim, 0, &mut Vec::new(), fast()).unwrap();
    assert!(sim.capture_writes().iter().all(|(boot, paused)| boot == paused));
    assert!(!sim.capture_paused());
}

#[test]
fn flash_stops_on_nak_without_retrying_block() {
    let mut sim = BootSim::new();
    sim.reject_data_block(1);
    assert!(session::flash(&mut sim, 0, &image(), &mut Vec::new(), fast()).is_err());
    assert_eq!(sim.data_writes(), 2);
}

#[test]
fn probe_preserves_transport_error_after_quiet_check() {
    use openbms_flash::{link::Link, Error, Result};
    struct LinkFailsOnInfo {
        reads: usize,
    }
    impl Link for LinkFailsOnInfo {
        fn clear_input(&mut self) -> Result<()> {
            Ok(())
        }
        fn write_raw(&mut self, _: &[u8]) -> Result<()> {
            Ok(())
        }
        fn read_raw(&mut self, _: &mut [u8]) -> Result<usize> {
            self.reads += 1;
            if self.reads > 20 {
                Err(Error::Transport("disconnected".into()))
            } else {
                Ok(0)
            }
        }
        fn wait(&mut self, _: Duration) {}
    }
    let err = session::probe(
        &mut LinkFailsOnInfo { reads: 0 },
        0,
        &mut Vec::new(),
        fast(),
    )
    .err()
    .unwrap();
    assert!(matches!(err, Error::Transport(message) if message == "disconnected"));
}

#[test]
fn default_session_sends_next_frame_as_soon_as_ack_arrives() {
    let options = Options::default();
    assert_eq!(options.min_frame_gap, Duration::ZERO);
    let mut sim = BootSim::new();
    session::flash(&mut sim, 0, &image(), &mut Vec::new(), options).unwrap();
    let times = sim.boot_writes();
    assert_eq!(times[0].0, b'L');
    assert_eq!(times[1].0, b'E');
    for pair in times.windows(2) {
        assert!(pair[1].1 - pair[0].1 < Duration::from_millis(350));
    }
}

#[test]
fn probe_reports_missed_bootloader_window() {
    use openbms_flash::Error;
    let mut options = fast();
    options.boot_delay = Duration::from_millis(900);
    let err = session::probe(&mut BootSim::new(), 0, &mut Vec::new(), options)
        .err()
        .unwrap();
    assert!(matches!(err, Error::NoBootloader(_)));
}

#[test]
fn delayed_ack_at_230_ms_succeeds_without_frame_gap() {
    let mut sim = BootSim::new();
    sim.set_boot_ack_delay(Duration::from_millis(230));
    let options = Options {
        ack_timeout: Duration::from_secs(1),
        ..Options::default()
    };
    let mut log = Vec::new();
    let result = session::flash(&mut sim, 0, &image(), &mut log, options).unwrap();
    assert_eq!(result.blocks, 3);
    assert!(result.wfd_reply_times.min >= Duration::from_millis(230));
    assert!(result.wfd_reply_times.min <= result.wfd_reply_times.median);
    assert!(result.wfd_reply_times.median <= result.wfd_reply_times.max);
    // after the R F L probes (one every 120 ms while no answer has begun) each frame waits for its acknowledgement
    let writes: Vec<_> = sim.boot_writes().iter().filter(|w| w.0 != b'L').collect();
    for pair in writes.windows(2) {
        let separation = pair[1].1 - pair[0].1;
        assert!(separation >= Duration::from_millis(230));
        assert!(separation < Duration::from_millis(300));
    }
    assert!(sim.boot_writes().iter().filter(|w| w.0 == b'L').count() > 1, "a late bootloader is probed again");
    let log = String::from_utf8(log).unwrap();
    assert_eq!(log.matches("TIMING command=WFD").count(), 3);
    assert!(log.contains("TIMING command=RFL"));
    assert!(log.contains("TIMING command=WFE"));
    assert!(log.contains("TIMING command=WFG"));
}

#[test]
fn tcp_probe_reassembles_split_boot_reply_and_keeps_reset_delay() {
    use openbms_flash::boot;
    use openbms_proto::frame;
    use openbms_transport::{Connection, Endpoint};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Instant;

    fn request(stream: &mut TcpStream) -> Vec<u8> {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        let mut out = vec![byte[0]];
        if byte[0] == b'~' {
            loop {
                stream.read_exact(&mut byte).unwrap();
                out.push(byte[0]);
                if byte[0] == b'\r' {
                    break;
                }
            }
        } else {
            assert_eq!(byte[0], b':');
            let mut rest = [0u8; 16];
            stream.read_exact(&mut rest).unwrap();
            out.extend_from_slice(&rest);
        }
        out
    }
    fn answer(info: &[u8]) -> Vec<u8> {
        let info_hex = info.iter().map(|b| format!("{b:02X}")).collect::<String>();
        let len = frame::length_field(info_hex.len()).unwrap();
        let body = format!("20004600{len:04X}{info_hex}");
        let check = frame::checksum(body.as_bytes());
        format!("~{body}{check:04X}\r").into_bytes()
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = Endpoint::Tcp {
        addr: listener.local_addr().unwrap().to_string(),
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        for command in [0x51, 0x47] {
            let received = request(&mut stream);
            assert_eq!(frame::parse(&received).unwrap().rtn, command);
            let info: &[u8] = if command == 0x51 {
                b"1101-SP85 \x10\x06CAN:PNG_DYE_Luxp_TBB"
            } else {
                b""
            };
            stream.write_all(&answer(info)).unwrap();
        }
        let reset = request(&mut stream);
        let reset_at = Instant::now();
        let reset = frame::parse(&reset).unwrap();
        assert_eq!(reset.rtn, 0x45);
        assert_eq!(reset.info, [1, 6, 0]);
        let rfl = request(&mut stream);
        assert!(reset_at.elapsed() >= Duration::from_millis(50));
        let rfl = boot::decode(&rfl).unwrap();
        assert_eq!((rfl.cid1, rfl.cid2, rfl.cid3), (b'R', b'F', b'L'));
        let reply = boot::encode(b'R', b'B', 0, &[1, 0]).unwrap();
        stream.write_all(&reply[..3]).unwrap();
        thread::sleep(Duration::from_millis(20));
        stream.write_all(&reply[3..reply.len() - 2]).unwrap();
        thread::sleep(Duration::from_millis(20));
        stream.write_all(&reply[reply.len() - 2..]).unwrap();
        let app_query = request(&mut stream);
        assert_eq!(frame::parse(&app_query).unwrap().rtn, 0x51);
        stream
            .write_all(&answer(b"1101-SP85 \x10\x06CAN:PNG_DYE_Luxp_TBB"))
            .unwrap();
    });
    let mut link = Connection::open(&endpoint).unwrap();
    let options = Options {
        ack_timeout: Duration::from_secs(1),
        post_timeout: Duration::from_secs(2),
        ..Options::default()
    };
    let result = session::probe(&mut link, 0, &mut Vec::new(), options).unwrap();
    assert_eq!(result.packet_size, 256);
    server.join().unwrap();
}

#[test]
fn configured_gap_is_added_after_delayed_ack() {
    let mut sim = BootSim::new();
    sim.set_boot_ack_delay(Duration::from_millis(230));
    let options = Options {
        ack_timeout: Duration::from_secs(1),
        min_frame_gap: Duration::from_millis(100),
        ..Options::default()
    };
    session::flash(&mut sim, 0, &image(), &mut Vec::new(), options).unwrap();
    let writes: Vec<_> = sim.boot_writes().iter().filter(|w| w.0 != b'L').collect();
    for pair in writes.windows(2) {
        assert!(pair[1].1 - pair[0].1 >= Duration::from_millis(330));
    }
}

#[test]
fn flash_reports_progress_in_order_and_returns_device_after() {
    use session::Progress::*;
    let mut sim = BootSim::new();
    let mut seen = Vec::new();
    let result =
        session::flash_with_progress(&mut sim, 0, &image(), &mut Vec::new(), fast(), &mut |p| {
            seen.push(p)
        })
        .unwrap();
    assert_eq!(
        seen,
        vec![
            Preflight,
            Bootloader,
            Erase,
            Block { index: 0, total: 3 },
            Block { index: 1, total: 3 },
            Block { index: 2, total: 3 },
            Finalize,
            AppRestart,
            PostCheck,
        ]
    );
    assert_eq!(result.after.device_name, result.before.device_name);
    assert!(!result.after.firmware_version.is_empty());
}

#[test]
fn progress_stops_at_rejected_block() {
    let mut sim = BootSim::new();
    sim.reject_data_block(1);
    let mut seen = Vec::new();
    let err =
        session::flash_with_progress(&mut sim, 0, &image(), &mut Vec::new(), fast(), &mut |p| {
            seen.push(p)
        });
    assert!(err.is_err());
    assert_eq!(
        seen.last(),
        Some(&session::Progress::Block { index: 1, total: 3 })
    );
}

#[test]
fn flash_with_progress_sends_the_same_frames_as_flash() {
    fn tx(log: Vec<u8>) -> Vec<String> {
        String::from_utf8(log)
            .unwrap()
            .lines()
            .filter_map(|l| l.split_once(" TX ").map(|(_, hex)| hex.to_string()))
            .collect()
    }
    let mut plain = Vec::new();
    session::flash(&mut BootSim::new(), 0, &image(), &mut plain, fast()).unwrap();
    let mut with = Vec::new();
    session::flash_with_progress(
        &mut BootSim::new(),
        0,
        &image(),
        &mut with,
        fast(),
        &mut |_| {},
    )
    .unwrap();
    let plain = tx(plain);
    // an empty log on both sides would compare equal without testing anything
    assert!(!plain.is_empty());
    assert_eq!(plain, tx(with));
}

/// A log that fails every write once `after` is in it and a line has ended behind it (flush never fails); with an
/// empty `after` from the first write.
struct FailingLog {
    text: String,
    after: &'static str,
}

impl std::io::Write for FailingLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let failing = self.text.find(self.after).is_some_and(|i| self.after.is_empty() || self.text[i..].contains('\n'));
        if failing {
            return Err(std::io::Error::other("disk full"));
        }
        self.text.push_str(&String::from_utf8_lossy(buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_log_error_before_the_erase_stops_the_flash_before_the_board_changes() {
    let mut sim = BootSim::new();
    let mut log = FailingLog { text: String::new(), after: "" };
    assert!(session::flash(&mut sim, 0, &image(), &mut log, fast()).is_err());
    assert!(!sim.wrote_flash());
}

#[test]
fn a_log_error_on_the_erase_ack_no_longer_stops_the_flash() {
    // fails from the line after W F E went out: the RX and timing lines of its ACK
    let wfe: String = session::dry_run(&image()).unwrap()[1].iter().map(|b| format!("{b:02X}")).collect();
    let after: &'static str = Box::leak(format!("TX {wfe}").into_boxed_str());
    let mut sim = BootSim::new();
    let mut log = FailingLog { text: String::new(), after };
    let result = session::flash(&mut sim, 0, &image(), &mut log, fast()).unwrap();
    assert!(sim.wrote_flash());
    assert_eq!(result.blocks, 3);
    assert!(result.log_error.is_some());
    assert!(!log.text.contains("command=WFE"), "the ACK's timing line was not written");
}

#[test]
fn a_log_error_after_the_confirmed_erase_no_longer_stops_the_flash() {
    let mut sim = BootSim::new();
    let mut log = FailingLog { text: String::new(), after: "command=WFE" };
    let result = session::flash(&mut sim, 0, &image(), &mut log, fast()).unwrap();
    assert!(sim.wrote_flash());
    assert!(result.app_returned);
    assert_eq!(result.blocks, 3);
    assert!(result.log_error.as_deref().is_some_and(|e| e.contains("disk full")));
    // a flash with a working log reports none
    let ok = session::flash(&mut BootSim::new(), 0, &image(), &mut Vec::new(), fast()).unwrap();
    assert!(ok.log_error.is_none());
}

/// A synthetic 0x42 INFO: 16 cells, 6 temperatures, remaining/full capacity in 10 mAh, SOC in 0.1 %, cycles.
fn telemetry_info(address: u8, remaining: u16, soc: u16, cycles: u16) -> Vec<u8> {
    let mut i = vec![0x00, address, 16];
    i.extend(std::iter::repeat_n([0x0D, 0x05], 16).flatten());
    i.push(6);
    i.extend(std::iter::repeat_n([0x0B, 0x27], 6).flatten());
    i.extend([0x00, 0x00, 0x14, 0xD6]);
    i.extend(remaining.to_be_bytes());
    i.extend([0x0A, 0x6D, 0x60]);
    i.extend(soc.to_be_bytes());
    i.extend([0x6D, 0x60]);
    i.extend(cycles.to_be_bytes());
    i.extend([0x03, 0xE8, 0x14, 0xD9]);
    i.extend([0u8; 8]);
    i
}

/// A synthetic 0x47 INFO: P0..P59 = 1000 + i, P60.. = i, switches as given, name "SYN-TEST".
fn params_info(address: u8, p10: u16, switches: [u8; 8]) -> Vec<u8> {
    let mut b = vec![address, 0x3C];
    for i in 0..60u16 {
        b.extend(if i == 10 { p10 } else { 1000 + i }.to_be_bytes());
    }
    b.push(0x1B);
    b.extend(60..87u8);
    b.push(0x08);
    b.extend(switches);
    b.extend(b"SYN-TEST  ");
    b
}

#[test]
fn a_flash_keeps_the_state_before_and_after_for_the_comparison() {
    let mut sim = BootSim::new();
    sim.set_answer(0x42, telemetry_info(0, 22170, 791, 7), telemetry_info(0, 7500, 268, 7));
    sim.set_answer(0x47, params_info(0, 3500, [0x18, 0, 0, 0, 0, 0, 0, 0]), params_info(0, 3400, [0x38, 0, 0, 0, 0, 0, 0, 0]));
    let r = session::flash(&mut sim, 0, &image(), &mut Vec::new(), fast()).unwrap();
    let (b, a) = (r.state_before.telemetry.unwrap(), r.state_after.telemetry.unwrap());
    assert_eq!((b.soc, a.soc), (79.1, 26.8));
    assert_eq!((b.remaining_capacity_ah, a.remaining_capacity_ah), (221.7, 75.0));
    let (b, a) = (r.state_before.parameters.unwrap(), r.state_after.parameters.unwrap());
    assert_eq!((b.parameters[10].raw, a.parameters[10].raw), (3500, 3400));
    assert_eq!((b.function_switches[0], a.function_switches[0]), (0x18, 0x38));
    // the state after is read once the application has settled, not right at its first answer
    let first_answer = sim.asked().iter().rposition(|(c, _)| *c == 0x44).unwrap();
    let back = sim.asked().iter().take(first_answer).rfind(|(c, _)| *c == 0x51).unwrap().1;
    let read = sim.asked().iter().skip(first_answer).find(|(c, _)| *c == 0x47).unwrap().1;
    // (the session counts from the answer in real time, the simulator from the request in its own time)
    assert!(read - back >= openbms_transport::timing::FLASH_SETTLE - Duration::from_millis(50), "{:?}", read - back);
}

#[test]
fn a_missing_state_answer_is_not_compared_and_does_not_fail_the_flash() {
    // the simulator's default answers have an empty INFO: neither 0x42 nor 0x47 parse
    let mut sim = BootSim::new();
    let r = session::flash(&mut sim, 0, &image(), &mut Vec::new(), fast()).unwrap();
    assert_eq!(r.state_before, session::PackState::default());
    assert_eq!(r.state_after, session::PackState::default());
    assert!(r.app_returned);
}

#[test]
fn a_bootloader_late_after_the_soc_save_is_found_by_a_later_probe() {
    // MAINT 1.6.3 saves the SOC before it resets: the first R F L after 50 ms comes too early
    let mut sim = BootSim::new();
    sim.set_soc_save(Duration::from_millis(400));
    let options = Options { ack_timeout: Duration::from_secs(3), ..Options::default() };
    let result = session::flash(&mut sim, 0, &image(), &mut Vec::new(), options).unwrap();
    assert_eq!(result.blocks, 3);
    assert!(sim.boot_writes().iter().filter(|w| w.0 == b'L').count() > 1);
    assert_eq!(sim.resets(), 1, "the reset is never repeated");
}

#[test]
fn a_refused_restart_ends_the_entry_and_nothing_is_flashed() {
    use openbms_flash::Error;
    let mut sim = BootSim::new();
    sim.refuse_reset(0x08);
    let err = session::flash(&mut sim, 0, &image(), &mut Vec::new(), fast()).err().unwrap();
    assert!(matches!(err, Error::ResetRefused(0x08)), "{err:?}");
    assert_eq!(sim.resets(), 1);
    assert!(!sim.wrote_flash() && sim.boot_writes().iter().all(|w| w.0 == b'L'));
}

// ---- recovery: a board already in its bootloader ----

#[test]
fn recovery_programs_a_waiting_bootloader_without_any_reset_and_finds_the_app() {
    let mut sim = BootSim::new();
    sim.start_in_bootloader();
    sim.set_app_address(7);
    let r = session::recover_with_progress(&mut sim, &image(), &mut Vec::new(), fast(), &mut |_| {}).unwrap();
    assert_eq!((r.packet_size, r.blocks), (256, 3));
    assert_eq!(r.found.map(|(a, _)| a), Some(7));
    assert_eq!(sim.resets(), 0);
    assert_eq!(sim.data_writes(), 3);
    assert!(!sim.capture_paused());
}

#[test]
fn recovery_refuses_before_any_write_when_a_second_board_answers() {
    let mut sim = BootSim::new();
    sim.start_in_bootloader();
    sim.answer_rfl_twice();
    assert!(session::recover_with_progress(&mut sim, &image(), &mut Vec::new(), fast(), &mut |_| {}).is_err());
    assert!(!sim.wrote_flash());
    assert_eq!(sim.resets(), 0);
}

#[test]
fn recovery_sends_nothing_but_rfl_to_a_board_without_a_waiting_bootloader() {
    let mut sim = BootSim::new();
    assert!(session::probe_bootloader(&mut sim, &mut Vec::new(), fast()).is_err());
    assert!(session::recover_with_progress(&mut sim, &image(), &mut Vec::new(), fast(), &mut |_| {}).is_err());
    assert!(!sim.wrote_flash());
    assert_eq!(sim.resets(), 0);
    assert!(sim.boot_writes().iter().all(|(c, _)| *c == b'L'), "only R F L probes");
}

#[test]
fn a_recovery_whose_app_stays_silent_still_confirms_the_transfer() {
    let mut sim = BootSim::new();
    sim.start_in_bootloader();
    sim.set_app_address(99);    // never answers a DIP address
    let r = session::recover_with_progress(&mut sim, &image(), &mut Vec::new(), fast(), &mut |_| {}).unwrap();
    assert!(r.found.is_none() && sim.wrote_flash());
}
