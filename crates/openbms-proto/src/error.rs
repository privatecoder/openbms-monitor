use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("frame does not start with '~'")]
    NoStart,
    #[error("frame too short ({0} bytes)")]
    TooShort(usize),
    #[error("not an uppercase hex character: 0x{0:02X}")]
    NotHex(u8),
    #[error("checksum mismatch: frame says 0x{expected:04X}, calculated 0x{actual:04X}")]
    Checksum { expected: u16, actual: u16 },
    #[error("invalid LENGTH checksum in 0x{0:04X}")]
    LengthChecksum(u16),
    #[error("LENGTH declares {declared} INFO characters, frame has {actual}")]
    LengthMismatch { declared: usize, actual: usize },
    #[error("INFO too long ({0} characters, max 4095)")]
    InfoTooLong(usize),
    #[error("unexpected INFO layout: {0}")]
    Layout(&'static str),
    #[error("BMS returned RTN 0x{0:02X}")]
    Rtn(u8),
    #[error("Modbus CRC mismatch")]
    ModbusCrc,
    #[error("Modbus exception 0x{0:02X}")]
    ModbusException(u8),
}

pub type Result<T> = std::result::Result<T, Error>;
