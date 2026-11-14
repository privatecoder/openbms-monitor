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
    boot_ack_delay: Duration,
    boot_reply_ready_at: Duration,
    capture_paused: bool,
    /// every write: bootloader framing, and whether the frame capture was paused then
    writes: Vec<(bool, bool)>,
    /// W F G was acknowledged: the new application answers from now on
    flashed: bool,
    /// INFO of an ASCII answer by CID2: before and after the flash (none: an empty INFO, as before)
    answers: Vec<(u8, Vec<u8>, Vec<u8>)>,
    /// time of each ASCII request by CID2
    asked: Vec<(u8, Duration)>,
    /// EMU1101-EXT Full: the time the SOC save takes before the reset; the bootloader listens only after it
    soc_save: Duration,
    /// answer the reset with this RTN and do not restart (Full: RTN 08, the SOC could not be saved)
    refuse_reset: Option<u8>,
    resets: usize,
    /// the board starts in its bootloader (no application): it answers R F L at any time
    in_bootloader: bool,
    /// a second bootloader on the line answers R F L as well
    rfl_twice: bool,
    /// the application answers only at this address (None: at any)
    app_address: Option<u8>,
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
            boot_ack_delay: Duration::ZERO,
            boot_reply_ready_at: Duration::ZERO,
            capture_paused: false,
            writes: Vec::new(),
            flashed: false,
            answers: Vec::new(),
            asked: Vec::new(),
            soc_save: Duration::ZERO,
            refuse_reset: None,
            resets: 0,
            in_bootloader: false,
            rfl_twice: false,
            app_address: None,
        }
    }
    /// The bootloader listens only this long after the reset request (a SOC save before the reset).
    pub fn set_soc_save(&mut self, d: Duration) {
        self.soc_save = d;
    }
    /// Answer the reset with RTN `rtn` and stay in the application.
    pub fn refuse_reset(&mut self, rtn: u8) {
        self.refuse_reset = Some(rtn);
    }
    /// The board waits in its bootloader: no application answers until a flash, R F L is answered at any time.
    pub fn start_in_bootloader(&mut self) {
        self.in_bootloader = true;
        self.app_resume_at = Duration::MAX;
    }
    /// Another bootloader on the same line answers R F L too.
    pub fn answer_rfl_twice(&mut self) {
        self.rfl_twice = true;
    }
    /// The application answers ASCII only at this address.
    pub fn set_app_address(&mut self, address: u8) {
        self.app_address = Some(address);
    }
    /// How many reset requests came.
    pub fn resets(&self) -> usize {
        self.resets
    }
    /// Answer `cid2` with INFO `before` until the flash, `after` from then on.
    pub fn set_answer(&mut self, cid2: u8, before: Vec<u8>, after: Vec<u8>) {
        self.answers.retain(|(c, _, _)| *c != cid2);
        self.answers.push((cid2, before, after));
    }
    /// Every ASCII request as (CID2, simulated time).
    pub fn asked(&self) -> &[(u8, Duration)] {
        &self.asked
    }
    pub fn set_boot_ack_delay(&mut self, delay: Duration) {
        self.boot_ack_delay = delay;
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
    /// Every write as (bootloader framing, capture paused at the time).
    pub fn capture_writes(&self) -> &[(bool, bool)] {
        &self.writes
    }
    pub fn capture_paused(&self) -> bool {
        self.capture_paused
    }
    fn queue(&mut self, bytes: Vec<u8>) {
        self.pending.extend(bytes);
    }

    fn ascii_answer(address: u8, info: &[u8]) -> Vec<u8> {
        Self::reply(address, 0, info)
    }

    fn reply(address: u8, rtn: u8, info: &[u8]) -> Vec<u8> {
        let info_hex = info.iter().map(|b| format!("{b:02X}")).collect::<String>();
        let len = frame::length_field(info_hex.len()).unwrap();
        let body = format!("20{address:02X}46{rtn:02X}{len:04X}{info_hex}");
        let chk = frame::checksum(body.as_bytes());
        format!("~{body}{chk:04X}\r").into_bytes()
    }

    fn handle_ascii(&mut self, bytes: &[u8]) {
        let Ok(request) = frame::parse(bytes) else {
            return;
        };
        if request.rtn == 0x45 && request.info == [1, 6, 0] {
            self.resets += 1;
            if let Some(rtn) = self.refuse_reset {
                self.queue(Self::reply(request.address, rtn, &[]));
                return;
            }
            self.reset_at = Some(self.now + self.soc_save);
            self.app_resume_at = self.now + Duration::from_millis(500);
            self.locked = false;
            self.enabled = false;
            return;
        }
        self.asked.push((request.rtn, self.now));
        if self.now < self.app_resume_at || self.app_address.is_some_and(|a| a != request.address) {
            return;
        }
        let set = self.answers.iter().find(|(c, _, _)| *c == request.rtn);
        let info = if let Some((_, before, after)) = set {
            if self.flashed { after.clone() } else { before.clone() }
        } else if request.rtn == 0x51 {
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
        // an answer already on its way keeps its time; a frame sent meanwhile does not hold it back
        let was_empty = self.pending.is_empty();
        let window = self
            .reset_at
            .is_some_and(|t| self.now >= t && self.now - t <= Duration::from_millis(800));
        match (request.cid1, request.cid3) {
            (b'R', b'L') if window || self.in_bootloader => {
                self.locked = true;
                self.queue(boot::encode(b'R', b'B', 0, &[1, 0]).unwrap());
                if self.rfl_twice {
                    self.queue(boot::encode(b'R', b'B', 0, &[1, 0]).unwrap());
                }
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
                self.flashed = true;
                self.queue(boot::encode(b'W', b'B', 0, &[]).unwrap());
            }
            _ => {}
        }
        if was_empty && !self.pending.is_empty() {
            self.boot_reply_ready_at = self.now + self.boot_ack_delay;
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
        self.writes.push((bytes.starts_with(b"::::"), self.capture_paused));
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
        if self.pending.is_empty() || self.now < self.boot_reply_ready_at {
            // the simulated clock runs at least as fast as the real one: the session's deadlines are real time, and a
            // 1 ms sleep can take several on a loaded machine (CI). A pending answer still comes exactly at its time, so
            // the measured gaps do not grow with the machine's load.
            let slept = std::time::Instant::now();
            std::thread::sleep(Duration::from_millis(1));
            let next = self.now + slept.elapsed().max(Duration::from_millis(1));
            self.now = if self.pending.is_empty() { next } else { next.min(self.boot_reply_ready_at) };
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
    fn pause_capture(&mut self, paused: bool) {
        self.capture_paused = paused;
    }
}
