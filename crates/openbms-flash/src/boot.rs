//! Binary frames used by the EMU1101 bootloader.

use crate::{Error, Result};

const START: &[u8; 4] = b"::::";
const END: &[u8; 4] = b"\r\r\r\r";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub cid1: u8,
    pub cid2: u8,
    pub cid3: u8,
    pub address: u16,
    pub data: Vec<u8>,
}

fn checksum(body: &[u8]) -> u16 {
    body.iter()
        .fold(0u16, |s, b| s.wrapping_add(u16::from(*b)))
        .wrapping_neg()
}

/// Build a bootloader request. Reject sums for which the vendor's modulo-65535 transmitter
/// differs from the ordinary modulo-65536 receiver; its behavior there has not been measured.
pub fn encode(cid1: u8, cid3: u8, address: u16, data: &[u8]) -> Result<Vec<u8>> {
    let len = u16::try_from(data.len())
        .map_err(|_| Error::Proto("boot frame data exceeds u16".into()))?;
    let mut body = Vec::with_capacity(7 + data.len());
    body.extend_from_slice(&[cid1, b'F', cid3]);
    body.extend_from_slice(&len.to_be_bytes());
    body.extend_from_slice(&address.to_be_bytes());
    body.extend_from_slice(data);
    let raw_sum: u32 = body.iter().map(|b| u32::from(*b)).sum();
    if raw_sum >= 65_535 {
        return Err(Error::Proto(format!(
            "boot frame checksum sum {raw_sum} is unverified"
        )));
    }
    let mut out = Vec::with_capacity(body.len() + 10);
    out.extend_from_slice(START);
    out.extend_from_slice(&body);
    out.extend_from_slice(&checksum(&body).to_be_bytes());
    out.extend_from_slice(END);
    Ok(out)
}

/// Decode exactly one complete frame, with no extra bytes.
pub fn decode(raw: &[u8]) -> Result<Frame> {
    if raw.len() < 17 || !raw.starts_with(START) || !raw.ends_with(END) {
        return Err(Error::Proto("invalid boot frame markers or length".into()));
    }
    let len = u16::from_be_bytes([raw[7], raw[8]]) as usize;
    if raw.len() != len + 17 {
        return Err(Error::Proto(format!(
            "boot frame length: declared {len}, received {}",
            raw.len()
        )));
    }
    if !matches!(raw[4], b'R' | b'W') || raw[5] != b'F' {
        return Err(Error::Proto("unexpected boot frame command".into()));
    }
    let got = u16::from_be_bytes([raw[11 + len], raw[12 + len]]);
    let expected = checksum(&raw[4..11 + len]);
    if got != expected {
        return Err(Error::Proto(format!(
            "boot frame checksum: received {got:04X}, expected {expected:04X}"
        )));
    }
    Ok(Frame {
        cid1: raw[4],
        cid2: raw[5],
        cid3: raw[6],
        address: u16::from_be_bytes([raw[9], raw[10]]),
        data: raw[11..11 + len].to_vec(),
    })
}
