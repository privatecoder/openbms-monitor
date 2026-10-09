use crate::error::{Error, Result};

/// Sequential big-endian reader over a decoded INFO payload.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn u8(&mut self) -> Result<u8> {
        let b = *self.data.get(self.pos).ok_or(Error::Layout("INFO ends early"))?;
        self.pos += 1;
        Ok(b)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok((u16::from(self.u8()?) << 8) | u16::from(self.u8()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.data.get(self.pos..self.pos + n).ok_or(Error::Layout("INFO ends early"))?;
        self.pos += n;
        Ok(s)
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
}

/// 0.1 K -> °C
pub(crate) fn kelvin10_to_c(v: u16) -> f64 {
    (f64::from(v) - 2731.0) / 10.0
}
