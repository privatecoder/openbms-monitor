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
    for pair in sim.boot_writes().windows(2) {
        let separation = pair[1].1 - pair[0].1;
        assert!(separation >= Duration::from_millis(230));
        assert!(separation < Duration::from_millis(300));
    }
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
    for pair in sim.boot_writes().windows(2) {
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
    assert_eq!(tx(plain), tx(with));
}
