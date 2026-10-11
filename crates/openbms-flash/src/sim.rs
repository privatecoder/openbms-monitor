//! Deterministic bootloader simulator for protocol tests; it never opens a hardware port.

use crate::{boot, link::Link, Error, Result};
use openbms_proto::frame;
use std::collections::VecDeque;
use std::time::Duration;

pub struct BootSim {
    now: Duration,
    reset_at: Option<Duration>,
    app_resume_at: Duration,
    locked: bool,
    enabled: bool,
    pending: VecDeque<u8>,
    data_writes: usize,
    reject_block: Option<usize>,
    boot_writes: Vec<(u8, Duration)>,
}

impl BootSim {
    pub fn new() -> Self {
        Self {
            now: Duration::ZERO,
            reset_at: None,
            app_resume_at: Duration::ZERO,
            locked: false,
            enabled: false,
            pending: VecDeque::new(),
            data_writes: 0,
            reject_block: None,
            boot_writes: Vec::new(),
        }
    }
    pub fn reject_data_block(&mut self, index: usize) {
        self.reject_block = Some(index);
    }
    pub fn wrote_flash(&self) -> bool {
        self.enabled
    }
    pub fn data_writes(&self) -> usize {
        self.data_writes
    }
    pub fn boot_writes(&self) -> &[(u8, Duration)] {
        &self.boot_writes
    }
    fn queue(&mut self, bytes: Vec<u8>) {
        self.pending.extend(bytes);
    }

    fn ascii_answer(address: u8, info: &[u8]) -> Vec<u8> {
        let info_hex = info.iter().map(|b| format!("{b:02X}")).collect::<String>();
        let len = frame::length_field(info_hex.len()).unwrap();
        let body = format!("20{address:02X}4600{len:04X}{info_hex}");
        let chk = frame::checksum(body.as_bytes());
        format!("~{body}{chk:04X}\r").into_bytes()
    }

    fn handle_ascii(&mut self, bytes: &[u8]) {
        let Ok(request) = frame::parse(bytes) else {
            return;
        };
        if request.rtn == 0x45 && request.info == [1, 6, 0] {
            self.reset_at = Some(self.now);
            self.app_resume_at = self.now + Duration::from_millis(500);
            self.locked = false;
            self.enabled = false;
            return;
        }
        if self.now < self.app_resume_at {
            return;
        }
        let info = if request.rtn == 0x51 {
            let mut data = Vec::new();
            data.extend_from_slice(b"1101-SP85 ");
            data.extend_from_slice(&[16, 6]);
            data.extend_from_slice(b"CAN:PNG_DYE_Luxp_TBB");
            data
        } else {
            Vec::new()
        };
        self.queue(Self::ascii_answer(request.address, &info));
    }

    fn handle_boot(&mut self, bytes: &[u8]) {
        let Ok(request) = boot::decode(bytes) else {
            return;
        };
        self.boot_writes.push((request.cid3, self.now));
        let window = self
            .reset_at
            .is_some_and(|t| self.now.saturating_sub(t) <= Duration::from_millis(800));
        match (request.cid1, request.cid3) {
            (b'R', b'L') if window => {
                self.locked = true;
                self.queue(boot::encode(b'R', b'B', 0, &[1, 0]).unwrap());
            }
            (b'W', b'E') if self.locked => {
                self.enabled = true;
                self.queue(boot::encode(b'W', b'B', 0, &[]).unwrap());
            }
            (b'W', b'D') if self.enabled => {
                let index = self.data_writes;
                self.data_writes += 1;
                if self.reject_block == Some(index) {
                    self.queue(boot::encode(b'R', b'N', 0, &[]).unwrap());
                } else {
                    self.queue(boot::encode(b'W', b'B', 0, &[]).unwrap());
                }
            }
            (b'W', b'G') if self.enabled => {
                self.app_resume_at = self.now + Duration::from_millis(100);
                self.queue(boot::encode(b'W', b'B', 0, &[]).unwrap());
            }
            _ => {}
        }
    }
}

impl Default for BootSim {
    fn default() -> Self {
        Self::new()
    }
}

impl Link for BootSim {
    fn clear_input(&mut self) -> Result<()> {
        self.pending.clear();
        Ok(())
    }
    fn write_raw(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.starts_with(b"~") {
            self.handle_ascii(bytes);
        } else if bytes.starts_with(b"::::") {
            self.handle_boot(bytes);
        } else {
            return Err(Error::Proto("simulator received unknown framing".into()));
        }
        Ok(())
    }
    fn read_raw(&mut self, out: &mut [u8]) -> Result<usize> {
        if self.pending.is_empty() {
            std::thread::sleep(Duration::from_millis(1));
            self.now += Duration::from_millis(1);
            return Ok(0);
        }
        let n = out.len().min(self.pending.len()).min(11);
        for byte in &mut out[..n] {
            *byte = self.pending.pop_front().unwrap();
        }
        Ok(n)
    }
    fn wait(&mut self, duration: Duration) {
        self.now += duration;
    }
}
