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
fn default_session_keeps_vendor_gap_after_each_boot_frame() {
    let mut sim = BootSim::new();
    session::flash(&mut sim, 0, &image(), &mut Vec::new(), Options::default()).unwrap();
    let times = sim.boot_writes();
    assert_eq!(times[0].0, b'L');
    assert_eq!(times[1].0, b'E');
    for pair in times.windows(2) {
        assert!(pair[1].1 - pair[0].1 >= Duration::from_millis(350));
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
