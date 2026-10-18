//! Binary parameter service of the patched firmware 16.06.13 MAINT 1.2 (`UART1_PARAMS`) on the CAN socket bus (UART1,
//! 9600 baud): `55 ADR CMD LEN DATA[LEN] CRC_HI CRC_LO AA`, CRC-16/XMODEM over ADR..DATA, big endian.
//!
//! CMD 0x47 (LEN 0) reads, the answer carries the 0x47 INFO without its first byte (168 bytes: markers, P0..P86, the
//! switches, the device name). CMD 0xA1 writes the first 158 of those bytes (no name) and is acknowledged with LEN 0.
//! A frame of the right pack can answer with an error code in place of CMD (LEN 0). Every pack answers only its own
//! address; a broken frame, a wrong length or a foreign address stays unanswered. Other firmware ignores the frame.

use crate::error::{Error, Result};
use crate::params::{self, Parameters};

pub const START: u8 = 0x55;
pub const END: u8 = 0xAA;
pub const CMD_READ: u8 = 0x47;
pub const CMD_WRITE: u8 = 0xA1;
/// DATA of a 0x47 answer: the 0x47 INFO without byte 0 (the pack address)
pub const READ_LEN: usize = params::BLOCK_LEN - 1;
/// DATA of a 0xA1 request: the read DATA without the ten name bytes
pub const WRITE_LEN: usize = READ_LEN - 10;
/// The firmware accepts addresses 0..=21 (DIP addresses 0..=15).
pub const MAX_ADDRESS: u8 = 21;
/// Header, CRC and end byte around DATA.
const OVERHEAD: usize = 7;

/// CRC-16/XMODEM: polynomial 0x1021, start 0, not reflected, no final XOR.
pub fn crc16_xmodem(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |crc, b| {
        (0..8).fold(crc ^ (u16::from(*b) << 8), |c, _| if c & 0x8000 != 0 { (c << 1) ^ 0x1021 } else { c << 1 })
    })
}

/// An error code in place of CMD (LEN 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// 0x81: command unknown
    UnknownCommand,
    /// 0x87: wrong length for the command
    WrongLength,
    /// 0x83: wrong block markers
    Markers,
    /// 0xE2: the firmware's parameter check failed; nothing was taken, not even the switches
    Rejected,
}

impl Code {
    pub fn of(cmd: u8) -> Option<Self> {
        match cmd {
            0x81 => Some(Code::UnknownCommand),
            0x87 => Some(Code::WrongLength),
            0x83 => Some(Code::Markers),
            0xE2 => Some(Code::Rejected),
            _ => None,
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            Code::UnknownCommand => 0x81,
            Code::WrongLength => 0x87,
            Code::Markers => 0x83,
            Code::Rejected => 0xE2,
        }
    }
}

/// The pack's answer to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// 0x47: the 168 DATA bytes, markers checked
    Params(Vec<u8>),
    /// 0xA1 taken
    Ack,
    Error(Code),
}

/// A request frame. Only 0x47 with no data and 0xA1 with exactly 158 bytes are built; a write without data never.
pub fn encode(address: u8, cmd: u8, data: &[u8]) -> Result<Vec<u8>> {
    if address > MAX_ADDRESS {
        return Err(Error::Layout("UART1 address must be 0..=21"));
    }
    match (cmd, data.len()) {
        (CMD_READ, 0) | (CMD_WRITE, WRITE_LEN) => {}
        _ => return Err(Error::Layout("UART1 request: 0x47 without data or 0xA1 with 158 bytes")),
    }
    Ok(frame(address, cmd, data))
}

fn frame(address: u8, cmd: u8, data: &[u8]) -> Vec<u8> {
    let mut body = vec![address, cmd, data.len() as u8];
    body.extend_from_slice(data);
    let crc = crc16_xmodem(&body);
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(START);
    out.extend(body);
    out.extend(crc.to_be_bytes());
    out.push(END);
    out
}

pub fn read_request(address: u8) -> Result<Vec<u8>> {
    encode(address, CMD_READ, &[])
}

/// 0xA1 with a block as `Values::apply` builds it (the 169-byte 0x47 INFO): its bytes 1..159, without address and name.
pub fn write_request(address: u8, block: &[u8]) -> Result<Vec<u8>> {
    params::check_block(block)?;
    encode(address, CMD_WRITE, write_data(block))
}

/// What a 0xA1 carries of a block, and what a read-back must show again.
pub fn write_data(block: &[u8]) -> &[u8] {
    &block[1..1 + WRITE_LEN]
}

/// The parameters of a 0x47 answer of `address`: the address (from the checked answer header) in front, as the ASCII
/// INFO has it, so every later step sees the pack's own address.
pub fn parameters(address: u8, data: &[u8]) -> Result<Parameters> {
    if data.len() != READ_LEN {
        return Err(Error::Layout("UART1 0x47 answer must carry 168 bytes"));
    }
    let mut info = Vec::with_capacity(params::BLOCK_LEN);
    info.push(address);
    info.extend_from_slice(data);
    Parameters::parse(&info)
}

/// The header shapes the service uses; anything else is not one of its frames.
fn known_shape(address: u8, cmd: u8, len: usize) -> bool {
    address <= MAX_ADDRESS
        && match cmd {
            CMD_READ => len == 0 || len == READ_LEN,
            CMD_WRITE => len == 0 || len == WRITE_LEN,
            c => Code::of(c).is_some() && len == 0,
        }
}

/// A complete, valid frame starting at `buf[0]` (end byte, CRC, known shape): its length.
fn frame_at(buf: &[u8]) -> Option<usize> {
    if buf.len() < OVERHEAD || buf[0] != START {
        return None;
    }
    let len = usize::from(buf[3]);
    let total = len + OVERHEAD;
    if buf.len() < total || buf[total - 1] != END || !known_shape(buf[1], buf[2], len) {
        return None;
    }
    let crc = u16::from_be_bytes([buf[total - 3], buf[total - 2]]);
    (crc16_xmodem(&buf[1..total - 3]) == crc).then_some(total)
}

/// Every valid frame in `bytes`, in order, as (address, command, data length): for the capture export.
pub fn headers(bytes: &[u8]) -> Vec<(u8, u8, usize)> {
    let (mut out, mut i) = (Vec::new(), 0);
    while i < bytes.len() {
        match frame_at(&bytes[i..]) {
            Some(n) => {
                out.push((bytes[i + 1], bytes[i + 2], n - OVERHEAD));
                i += n;
            }
            None => i += 1,
        }
    }
    out
}

/// Collects the received bytes of one exchange and finds the answer to `request` in them. Frames are cut by their LEN,
/// never at the first `AA` or `0D`; TCP may split or join them. A start byte whose frame is broken or incomplete does
/// not hold up a valid frame after it. An exact echo of the request is skipped, as is every frame of another address,
/// command or shape.
#[derive(Debug, Default)]
pub struct Framer {
    buf: Vec<u8>,
}

/// More than any answer plus noise; older bytes cannot begin a frame that still fits.
const KEEP: usize = 1024;

impl Framer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() > KEEP {
            self.buf.drain(..self.buf.len() - KEEP);
        }
    }

    /// The answer to `request` (a frame from `encode`), once a fitting frame has arrived completely.
    pub fn next(&mut self, request: &[u8]) -> Option<Reply> {
        let (address, cmd) = (*request.get(1)?, *request.get(2)?);
        loop {
            let (start, len) = self.first_frame()?;
            let f: Vec<u8> = self.buf.drain(..start + len).skip(start).collect();
            if f == request || f[1] != address {
                continue;
            }
            let data = &f[4..f.len() - 3];
            let reply = match (f[2], data.len()) {
                (CMD_READ, READ_LEN) if cmd == CMD_READ && markers_ok(data) => Reply::Params(data.to_vec()),
                (CMD_WRITE, 0) if cmd == CMD_WRITE => Reply::Ack,
                (c, 0) if Code::of(c).is_some() => Reply::Error(Code::of(c).unwrap()),
                _ => continue,
            };
            return Some(reply);
        }
    }
}

impl Framer {
    /// The first complete valid frame (start, length). A start byte with a plausible header protects its frame up to
    /// its LEN boundary: a valid-looking frame inside its data (switch bytes may read `55 00 E2 00 76 D0 AA`) is not
    /// taken while it is still arriving. Only when its end byte or CRC disprove it does the search go on at a later
    /// start byte; a start byte whose header is no known shape is skipped at once.
    fn first_frame(&self) -> Option<(usize, usize)> {
        for i in (0..self.buf.len()).filter(|i| self.buf[*i] == START) {
            let rest = &self.buf[i..];
            if rest.len() < 4 {
                return None;
            }
            if !known_shape(rest[1], rest[2], usize::from(rest[3])) {
                continue;
            }
            if rest.len() < usize::from(rest[3]) + OVERHEAD {
                return None;
            }
            if let Some(n) = frame_at(rest) {
                return Some((i, n));
            }
        }
        None
    }
}

fn markers_ok(data: &[u8]) -> bool {
    data[0] == 0x3C && data[121] == 0x1B && data[149] == 0x08
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// 168 DATA bytes: P0..P59 = 3000 + i, P60.. = i, switches 0x10 + g, name "SYN-UART1 ".
    fn data() -> Vec<u8> {
        let mut d = vec![0x3C];
        for i in 0..60u16 {
            d.extend((3000 + i).to_be_bytes());
        }
        d.push(0x1B);
        d.extend(60..87u8);
        d.push(0x08);
        d.extend((0..8u8).map(|g| 0x10 + g));
        d.extend(b"SYN-UART1 ");
        assert_eq!(d.len(), READ_LEN);
        d
    }

    fn answer(address: u8, cmd: u8, data: &[u8]) -> Vec<u8> {
        frame(address, cmd, data)
    }

    #[test]
    fn crc_and_the_documented_vectors() {
        assert_eq!(crc16_xmodem(b"123456789"), 0x31C3);
        assert_eq!(read_request(0).unwrap(), hex("55 00 47 00 94 5B AA"));
        assert_eq!(read_request(1).unwrap(), hex("55 01 47 00 A3 6B AA"));
        assert_eq!(answer(0, CMD_WRITE, &[]), hex("55 00 A1 00 2E 4F AA"));
        let mut f = Framer::new();
        f.push(&hex("55 00 A1 00 2E 4F AA"));
        let req = write_request(0, &[vec![0], data()].concat()).unwrap();
        assert_eq!(f.next(&req), Some(Reply::Ack));
    }

    #[test]
    fn requests_are_only_read_without_data_or_a_full_write() {
        assert!(read_request(21).is_ok());
        assert!(read_request(22).is_err());
        assert!(encode(0, CMD_WRITE, &[]).is_err(), "a write without data is never built");
        assert!(encode(0, CMD_WRITE, &[0; 157]).is_err());
        assert!(encode(0, CMD_READ, &[0]).is_err());
        assert!(encode(0, 0x42, &[]).is_err());
        let block = [vec![3], data()].concat();
        let w = write_request(3, &block).unwrap();
        assert_eq!(w.len(), WRITE_LEN + 7);
        assert_eq!((w[1], w[2], w[3]), (3, CMD_WRITE, 158));
        // the first 158 DATA bytes of the read, without the name
        assert_eq!(&w[4..4 + WRITE_LEN], &data()[..WRITE_LEN]);
        let mut bad = block.clone();
        bad[122] = 0;
        assert!(write_request(3, &bad).is_err());
    }

    #[test]
    fn the_answer_gets_its_own_address_in_front() {
        for adr in [0u8, 1, 11] {
            let p = parameters(adr, &data()).unwrap();
            assert_eq!(p.address, adr);
            assert_eq!(p.block[0], adr);
            assert_eq!(&p.block[1..], &data()[..]);
            assert_eq!(p.parameters[0].raw, 3000);
            assert_eq!(p.parameters[86].raw, 86);
            assert_eq!(p.function_switches[7], 0x17);
            assert_eq!(p.device_name, "SYN-UART1");
            // the write carries exactly the read's first 158 DATA bytes, for this address
            let w = write_request(adr, &p.block).unwrap();
            assert_eq!(w[1], adr);
            assert_eq!(write_data(&p.block), &data()[..WRITE_LEN]);
        }
        assert!(parameters(0, &data()[..167]).is_err());
    }

    #[test]
    fn a_read_answer_split_joined_or_after_an_echo() {
        let req = read_request(1).unwrap();
        let ans = answer(1, CMD_READ, &data());
        assert_eq!(ans.len(), 175);
        // one byte at a time
        let mut f = Framer::new();
        for (i, b) in ans.iter().enumerate() {
            f.push(&[*b]);
            let r = f.next(&req);
            assert_eq!(r.is_some(), i == ans.len() - 1, "byte {i}");
            if let Some(r) = r {
                assert_eq!(r, Reply::Params(data()));
            }
        }
        // echo and answer in one chunk; the echo is no answer
        let mut f = Framer::new();
        f.push(&[req.clone(), ans.clone()].concat());
        assert_eq!(f.next(&req), Some(Reply::Params(data())));
        // echo alone
        let mut f = Framer::new();
        f.push(&req);
        assert_eq!(f.next(&req), None);
        // noise, a stray start byte and a broken frame before the answer
        let mut f = Framer::new();
        f.push(&[0x0D, 0xAA, 0x55, 0x07, 0xFF, 0x55]);
        f.push(&ans[..100]);
        assert_eq!(f.next(&req), None);
        f.push(&ans[100..]);
        assert_eq!(f.next(&req), Some(Reply::Params(data())));
    }

    #[test]
    fn data_with_start_end_and_cr_bytes_is_cut_by_its_length() {
        let mut d = data();
        // P0 = 0x55AA, P1 = 0x0D0D, P2 = 0xAA55: a cut at the first AA or 0D would break the frame
        d[1..7].copy_from_slice(&[0x55, 0xAA, 0x0D, 0x0D, 0xAA, 0x55]);
        let req = read_request(2).unwrap();
        let ans = answer(2, CMD_READ, &d);
        let mut f = Framer::new();
        f.push(&ans[..5]);
        f.push(&ans[5..9]);
        assert_eq!(f.next(&req), None);
        f.push(&ans[9..]);
        assert_eq!(f.next(&req), Some(Reply::Params(d)));
    }

    #[test]
    fn a_frame_inside_the_data_of_an_arriving_answer_is_not_taken() {
        // switch bytes 0..7 read as a complete E2 answer of address 0
        let mut d = data();
        d[150..157].copy_from_slice(&hex("55 00 E2 00 76 D0 AA"));
        assert_eq!(&answer(0, 0xE2, &[])[..], &d[150..157]);
        let req = read_request(0).unwrap();
        let ans = answer(0, CMD_READ, &d);
        // two TCP chunks, the first one ends right after the inner frame (161 of 175 bytes)
        let mut f = Framer::new();
        f.push(&ans[..161]);
        assert_eq!(f.next(&req), None);
        f.push(&ans[161..]);
        assert_eq!(f.next(&req), Some(Reply::Params(d.clone())));
        // the same after the echo of the request
        let mut f = Framer::new();
        f.push(&[req.clone(), ans[..161].to_vec()].concat());
        assert_eq!(f.next(&req), None);
        f.push(&ans[161..]);
        assert_eq!(f.next(&req), Some(Reply::Params(d.clone())));
        // a broken outer frame (CRC) is disproved once complete: the search goes on and finds the valid frame after it
        let mut broken = ans.clone();
        broken[172] ^= 0xFF;
        let clean = answer(0, CMD_READ, &data());
        let mut f = Framer::new();
        f.push(&broken[..161]);
        assert_eq!(f.next(&req), None);
        f.push(&broken[161..]);
        f.push(&clean[..80]);
        // only the inner frame of the disproved one is complete so far: it is a frame of its own now
        assert_eq!(f.next(&req), Some(Reply::Error(Code::Rejected)));
        f.push(&clean[80..]);
        assert_eq!(f.next(&req), Some(Reply::Params(data())));
        // a broken outer end byte without a frame inside: the following answer is found
        let mut bad_end = answer(0, CMD_READ, &data());
        *bad_end.last_mut().unwrap() = 0x00;
        let mut f = Framer::new();
        f.push(&[bad_end, clean.clone()].concat());
        assert_eq!(f.next(&req), Some(Reply::Params(data())));
    }

    #[test]
    fn foreign_broken_and_unfitting_frames_are_skipped() {
        let req = read_request(1).unwrap();
        let good = answer(1, CMD_READ, &data());
        let mut bad_crc = good.clone();
        bad_crc[10] ^= 1;
        let mut bad_end = good.clone();
        *bad_end.last_mut().unwrap() = 0xAB;
        let mut bad_marker = data();
        bad_marker[121] = 0x1C;
        let skipped = [
            answer(2, CMD_READ, &data()),        // another address
            bad_crc,                             // CRC
            bad_end,                             // end byte
            answer(1, CMD_READ, &bad_marker),    // markers
            answer(1, CMD_WRITE, &[]),           // an ACK while reading
            answer(1, CMD_READ, &data()[..100]), // a length the service never uses
            answer(1, 0x42, &[]),                // an unknown command
            answer(22, CMD_READ, &data()),       // no valid address
        ];
        for s in &skipped {
            let mut f = Framer::new();
            f.push(s);
            assert_eq!(f.next(&req), None, "{s:02X?}");
        }
        let mut f = Framer::new();
        f.push(&skipped.concat());
        f.push(&good);
        assert_eq!(f.next(&req), Some(Reply::Params(data())));
    }

    #[test]
    fn error_codes_and_the_write_acknowledgement() {
        let block = [vec![4], data()].concat();
        let req = write_request(4, &block).unwrap();
        for (code, c) in [(0x81, Code::UnknownCommand), (0x87, Code::WrongLength), (0x83, Code::Markers), (0xE2, Code::Rejected)] {
            let mut f = Framer::new();
            f.push(&answer(4, code, &[]));
            assert_eq!(f.next(&req), Some(Reply::Error(c)));
            assert_eq!(c.byte(), code);
            // an error code with data is no answer of the service
            let mut f = Framer::new();
            f.push(&answer(4, code, &[1]));
            assert_eq!(f.next(&req), None);
        }
        // the echo of the write, then the ACK of another pack, then ours
        let mut f = Framer::new();
        f.push(&[req.clone(), answer(5, CMD_WRITE, &[]), answer(4, CMD_WRITE, &[])].concat());
        assert_eq!(f.next(&req), Some(Reply::Ack));
        // a read answer is no acknowledgement
        let mut f = Framer::new();
        f.push(&answer(4, CMD_READ, &data()));
        assert_eq!(f.next(&req), None);
    }
}
