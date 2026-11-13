//! ASCII frames (YD/T 1363.3 style, called "MODBUS-ASCII" by Seplos).
//!
//! `~ VER ADR CID1 CID2/RTN LENGTH INFO CHKSUM \r`, every byte sent as two uppercase hex
//! characters. The firmware only accepts uppercase hex in requests and always answers in
//! uppercase.

use crate::error::{Error, Result};

/// Protocol version sent in every frame (V2.0).
pub const VERSION: u8 = 0x20;
/// CID1 of the Seplos battery BMS.
pub const CID1_BMS: u8 = 0x46;

/// Two's complement of the sum of all ASCII characters between `~` and the checksum.
pub fn checksum(ascii: &[u8]) -> u16 {
    let sum = ascii.iter().fold(0u32, |acc, b| acc + u32::from(*b));
    (0u32.wrapping_sub(sum) & 0xFFFF) as u16
}

/// LENGTH field: 4-bit length checksum (LCHKSUM) followed by the 12-bit number of
/// ASCII characters in INFO (LENID).
pub fn length_field(info_ascii_len: usize) -> Result<u16> {
    if info_ascii_len > 0x0FFF {
        return Err(Error::InfoTooLong(info_ascii_len));
    }
    let lenid = info_ascii_len as u16;
    if lenid == 0 {
        return Ok(0);
    }
    let nibble_sum = (lenid & 0xF) + ((lenid >> 4) & 0xF) + ((lenid >> 8) & 0xF);
    let lchksum = (0u16.wrapping_sub(nibble_sum)) & 0xF;
    Ok((lchksum << 12) | lenid)
}

fn push_hex_byte(out: &mut Vec<u8>, b: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(HEX[usize::from(b >> 4)]);
    out.push(HEX[usize::from(b & 0xF)]);
}

fn push_hex_u16(out: &mut Vec<u8>, v: u16) {
    push_hex_byte(out, (v >> 8) as u8);
    push_hex_byte(out, v as u8);
}

/// Build a request frame. `info` is the binary INFO payload (it is hex-encoded here).
pub fn encode_request(address: u8, cid2: u8, info: &[u8]) -> Result<Vec<u8>> {
    encode_request_version(VERSION, address, cid2, info)
}

/// Like `encode_request` with another VER byte (the vendor's update tool sends 0x26; the firmware
/// does not check it).
pub fn encode_request_version(version: u8, address: u8, cid2: u8, info: &[u8]) -> Result<Vec<u8>> {
    let mut body = Vec::with_capacity(12 + info.len() * 2);
    push_hex_byte(&mut body, version);
    push_hex_byte(&mut body, address);
    push_hex_byte(&mut body, CID1_BMS);
    push_hex_byte(&mut body, cid2);
    push_hex_u16(&mut body, length_field(info.len() * 2)?);
    for b in info {
        push_hex_byte(&mut body, *b);
    }
    let chk = checksum(&body);
    let mut frame = Vec::with_capacity(body.len() + 6);
    frame.push(b'~');
    frame.extend_from_slice(&body);
    push_hex_u16(&mut frame, chk);
    frame.push(b'\r');
    Ok(frame)
}

/// A decoded response frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub version: u8,
    pub address: u8,
    pub cid1: u8,
    /// RTN in responses (0x00 = OK). Intra-pack answers carry 0x5A here instead.
    pub rtn: u8,
    /// INFO decoded from hex to binary.
    pub info: Vec<u8>,
}

fn hex_val(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(Error::NotHex(c)),
    }
}

fn hex_byte(s: &[u8]) -> Result<u8> {
    Ok((hex_val(s[0])? << 4) | hex_val(s[1])?)
}

fn hex_u16(s: &[u8]) -> Result<u16> {
    Ok((u16::from(hex_byte(&s[0..2])?) << 8) | u16::from(hex_byte(&s[2..4])?))
}

/// Parse a complete frame (`~` … `\r`, the trailing `\r` is optional).
pub fn parse(raw: &[u8]) -> Result<Frame> {
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    if raw.first() != Some(&b'~') {
        return Err(Error::NoStart);
    }
    let body = &raw[1..];
    if body.len() < 16 {
        return Err(Error::TooShort(raw.len()));
    }
    let (content, chk) = body.split_at(body.len() - 4);
    let expected = hex_u16(chk)?;
    let actual = checksum(content);
    if expected != actual {
        return Err(Error::Checksum { expected, actual });
    }
    let length = hex_u16(&content[8..12])?;
    let lenid = usize::from(length & 0x0FFF);
    if length_field(lenid)? != length {
        return Err(Error::LengthChecksum(length));
    }
    let info_ascii = &content[12..];
    if info_ascii.len() != lenid {
        return Err(Error::LengthMismatch { declared: lenid, actual: info_ascii.len() });
    }
    // a valid LENGTH may still declare an odd number of hex digits: never a byte string
    if lenid % 2 != 0 {
        return Err(Error::Layout("INFO has an odd number of hex digits"));
    }
    let info = info_ascii
        .chunks(2)
        .map(hex_byte)
        .collect::<Result<Vec<u8>>>()?;
    Ok(Frame {
        version: hex_byte(&content[0..2])?,
        address: hex_byte(&content[2..4])?,
        cid1: hex_byte(&content[4..6])?,
        rtn: hex_byte(&content[6..8])?,
        info,
    })
}

/// One received line (`Lines`): the bytes from its `~` up to and including the CR (the whole line if it has no `~`),
/// and how many bytes the line took in all, garbage before the `~` included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: Vec<u8>,
    pub len: usize,
}

/// ASCII frames as they come in, line by line across reads. One splitter for every receiver (the transport's answers,
/// the maintenance dialog, the flasher's ASCII queries, passive listening); what a receiver accepts stays its own.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Lines {
    pending: Vec<u8>,
}

impl Lines {
    pub fn push(&mut self, data: &[u8]) {
        self.pending.extend_from_slice(data);
    }

    /// The next complete line, None while no CR came.
    pub fn next_line(&mut self) -> Option<Line> {
        let end = self.pending.iter().position(|b| *b == b'\r')?;
        let line: Vec<u8> = self.pending.drain(..=end).collect();
        let start = line.iter().position(|b| *b == b'~').unwrap_or(0);
        Some(Line { len: line.len(), text: line[start..].to_vec() })
    }

    /// Bytes waiting without a CR.
    pub fn unterminated(&self) -> usize {
        self.pending.len()
    }

    /// Drop what waits without a CR (garbage past a receiver's limit).
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_odd_number_of_info_digits_is_an_error_not_a_panic() {
        let len = length_field(3).unwrap();
        let body = format!("20004600{len:04X}ABC");
        let raw = format!("~{body}{:04X}\r", checksum(body.as_bytes()));
        assert!(matches!(parse(raw.as_bytes()), Err(Error::Layout(_))));
    }

    #[test]
    fn lines_split_across_reads_from_their_tilde() {
        let mut l = Lines::default();
        l.push(b"xx~2001465A00");
        assert_eq!(l.next_line(), None);
        l.push(b"00FD9D\rnoise\r~20");
        assert_eq!(l.next_line(), Some(Line { text: b"~2001465A0000FD9D\r".to_vec(), len: 20 }));
        assert_eq!(l.next_line().unwrap().text, b"noise\r".to_vec());
        assert_eq!((l.next_line(), l.unterminated()), (None, 3));
    }

    #[test]
    fn length_field_examples() {
        // Example from the Seplos documentation: LENID 18 -> 0xD012.
        assert_eq!(length_field(18).unwrap(), 0xD012);
        assert_eq!(length_field(0).unwrap(), 0);
        // Nibble sum 16 must give LCHKSUM 0, not 16.
        assert_eq!(length_field(0x088).unwrap(), 0x0088);
    }

    #[test]
    fn checksum_example() {
        // The vendor document claims 0x038E; the real character sum is 0x038F.
        assert_eq!(checksum(b"1203400456ABCEFE"), 0xFC71);
    }

    #[test]
    fn encode_telemetry_request() {
        let f = encode_request(0x01, 0x42, &[0x01]).unwrap();
        assert_eq!(f, b"~20014642E00201FD35\r".to_vec());
    }

    #[test]
    fn roundtrip() {
        let f = encode_request(0x03, 0x47, &[0x03]).unwrap();
        let p = parse(&f).unwrap();
        assert_eq!((p.address, p.rtn, p.info.as_slice()), (0x03, 0x47, &[0x03][..]));
    }

    #[test]
    fn rejects_lowercase_and_bad_checksum() {
        assert!(parse(b"~20014642e00201FD35\r").is_err());
        assert!(parse(b"~20014642E00201FD36\r").is_err());
    }
}
