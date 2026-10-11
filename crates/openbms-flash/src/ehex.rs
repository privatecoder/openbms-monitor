//! `.ehex` = 16 header records (256 bytes, checksum deliberately `AA`) followed by plain Intel HEX.
//!
//! The vendor tool sends the data fields of **all** records in file order: header, image, the four
//! bytes of the type 05 record. We build the same stream, but only from files where that stream is
//! unambiguous (contiguous image from 0, no type 04, type 05 and EOF last).

use crate::{Error, Result};
use sha2::{Digest, Sha256};

pub const HEADER_LEN: usize = 256;
const HEADER_RECORDS: usize = 16;
/// Flash of the SWM181 (64 KiB); the image must fit well below it.
const MAX_IMAGE: usize = 64 * 1024;

/// The three text fields at the start of the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// e.g. `SWM181RCT6_64_M0`
    pub mcu: String,
    /// e.g. `EMU1101_10D_HW16`
    pub device: String,
    /// e.g. `RS485Y19200N9600`
    pub interface: String,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Image {
    pub header: Header,
    /// Application image from flash address 0.
    pub code: Vec<u8>,
    /// Data of the type 05 record (start address), big-endian as in the file.
    pub start: [u8; 4],
    /// SHA-256 of `code`, lowercase hex.
    pub code_sha256: String,
    /// SHA-256 of the file as read, lowercase hex.
    pub file_sha256: String,
    /// Initial stack pointer and reset vector from the vector table.
    pub stack_top: u32,
    pub reset_vector: u32,
}

impl Image {
    /// The byte stream the bootloader receives: header + image + start address.
    pub fn stream(&self) -> Vec<u8> {
        let mut s = self.header.raw.clone();
        s.extend_from_slice(&self.code);
        s.extend_from_slice(&self.start);
        s
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

struct Record {
    line: usize,
    kind: u8,
    addr: u16,
    data: Vec<u8>,
    checksum: u8,
}

fn hex(s: &str, line: usize) -> Result<Vec<u8>> {
    if s.len() & 1 != 0 {
        return Err(Error::Image(format!(
            "line {line}: odd number of hex digits"
        )));
    }
    s.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|text| u8::from_str_radix(text, 16).ok())
                .ok_or_else(|| Error::Image(format!("line {line}: not hex")))
        })
        .collect()
}

fn record(text: &str, line: usize) -> Result<Record> {
    let body = text
        .strip_prefix(':')
        .ok_or_else(|| Error::Image(format!("line {line}: does not start with ':'")))?;
    let b = hex(body, line)?;
    if b.len() < 5 || b.len() != usize::from(b[0]) + 5 {
        return Err(Error::Image(format!(
            "line {line}: length does not match the byte count"
        )));
    }
    let count = b[0] as usize;
    Ok(Record {
        line,
        kind: b[3],
        addr: u16::from_be_bytes([b[1], b[2]]),
        data: b[4..4 + count].to_vec(),
        checksum: b[4 + count],
    })
}

/// Parse an original-style `.ehex`. The vendor sends every record's data field, so any gap or
/// address extension would create a different stream and is rejected rather than filled in.
pub fn parse(file: &[u8]) -> Result<Image> {
    let text = std::str::from_utf8(file).map_err(|_| Error::Image("file is not ASCII".into()))?;
    let records: Vec<Record> = text
        .lines()
        .enumerate()
        .map(|(i, s)| record(s.trim_end_matches('\r'), i + 1))
        .collect::<Result<_>>()?;
    if records.len() < HEADER_RECORDS + 3 {
        return Err(Error::Image("file is too short".into()));
    }
    let mut raw = Vec::with_capacity(HEADER_LEN);
    for rec in &records[..HEADER_RECORDS] {
        if rec.kind != 0 || rec.addr != 0 || rec.data.len() != 16 || rec.checksum != 0xaa {
            return Err(Error::Image(format!(
                "line {}: invalid vendor header record",
                rec.line
            )));
        }
        raw.extend_from_slice(&rec.data);
    }
    let header = Header {
        mcu: String::from_utf8_lossy(&raw[0..16]).to_string(),
        device: String::from_utf8_lossy(&raw[16..32]).to_string(),
        interface: String::from_utf8_lossy(&raw[32..48]).to_string(),
        raw,
    };
    if header.mcu != "SWM181RCT6_64_M0"
        || header.device != "EMU1101_10D_HW16"
        || header.interface != "RS485Y19200N9600"
        || !header.raw[48..].iter().all(|b| *b == 0x55)
    {
        return Err(Error::Image(
            "image header is not for the supported EMU1101 bootloader".into(),
        ));
    }
    let mut code = Vec::new();
    let mut start = None;
    let mut eof = false;
    for rec in &records[HEADER_RECORDS..] {
        let mut sum = (rec.data.len() as u8)
            .wrapping_add((rec.addr >> 8) as u8)
            .wrapping_add(rec.addr as u8)
            .wrapping_add(rec.kind)
            .wrapping_add(rec.checksum);
        for b in &rec.data {
            sum = sum.wrapping_add(*b);
        }
        if sum != 0 {
            return Err(Error::Image(format!(
                "line {}: bad Intel HEX checksum",
                rec.line
            )));
        }
        if eof {
            return Err(Error::Image(format!("line {}: record after EOF", rec.line)));
        }
        match rec.kind {
            0 if start.is_none() => {
                if usize::from(rec.addr) != code.len() || rec.data.is_empty() {
                    return Err(Error::Image(format!(
                        "line {}: image address gap or overlap",
                        rec.line
                    )));
                }
                if code.len() + rec.data.len() > MAX_IMAGE {
                    return Err(Error::Image("image exceeds 64 KiB".into()));
                }
                code.extend_from_slice(&rec.data);
            }
            5 if start.is_none() && rec.addr == 0 && rec.data.len() == 4 => {
                start = Some([rec.data[0], rec.data[1], rec.data[2], rec.data[3]]);
            }
            1 if start.is_some() && rec.addr == 0 && rec.data.is_empty() => {
                eof = true;
            }
            _ => {
                return Err(Error::Image(format!(
                    "line {}: unsupported record type or order",
                    rec.line
                )))
            }
        }
    }
    if !eof || code.len() < 8 {
        return Err(Error::Image(
            "missing EOF or application vector table".into(),
        ));
    }
    if start != Some([0, 0, 0, 0xc1]) {
        return Err(Error::Image("unsupported start address record".into()));
    }
    let stack_top = u32::from_le_bytes(code[0..4].try_into().unwrap());
    let reset_vector = u32::from_le_bytes(code[4..8].try_into().unwrap());
    Ok(Image {
        header,
        code_sha256: sha256_hex(&code),
        file_sha256: sha256_hex(file),
        code,
        start: start.unwrap(),
        stack_top,
        reset_vector,
    })
}
