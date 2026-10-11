//! Firmware updater for the Seplos BMS V2.0 (10C/10E, module EMU1101).
//!
//! Speaks to the vendor bootloader on the pack bus (RS485-1/2, UART0, 19200 baud), the same way the
//! vendor's `FirmwareUpdate.exe` does: an ASCII restart (CID2 0x45 type 6) opens a window of about
//! 0.65 s in which the bootloader answers binary frames (`::::` R/W F L/E/D/G … `\r\r\r\r`).
//!
//! Unlike the monitor, this crate **writes** to the BMS. The CLI accepts known vendor `.ehex`
//! code hashes by default; `--allow-custom-firmware` permits structurally valid images with an
//! unknown code hash. Library callers must apply their own image authorization policy.

pub mod boot;
pub mod ehex;
pub mod known;
pub mod link;
pub mod session;
pub mod sim;

use std::time::Duration;

/// Language of user-facing texts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

impl Lang {
    pub fn pick<'a>(self, de: &'a str, en: &'a str) -> &'a str {
        match self {
            Lang::De => de,
            Lang::En => en,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("transport: {0}")]
    Transport(String),
    #[error("image: {0}")]
    Image(String),
    #[error("protocol: {0}")]
    Proto(String),
    #[error("pack {0} does not answer")]
    NoAnswer(u8),
    #[error("the bus is not quiet ({0} bytes from other devices)")]
    BusBusy(usize),
    #[error("pack {0} answers 0x51 but not 0x47: not connected to RS485-1/2 (pack bus)")]
    NotPackBus(u8),
    #[error("the bootloader did not answer within {0:?}")]
    NoBootloader(Duration),
    #[error("bootloader reports packet size {0}; only 256 is supported")]
    PacketSize(u32),
    #[error("bootloader rejected the frame (R F N)")]
    Rejected,
    #[error("unexpected bootloader answer {0}")]
    Unexpected(String),
    #[error("no answer to {what} within {after:?}")]
    Timeout { what: String, after: Duration },
    #[error("address {0} is not a single pack (0..=15)")]
    Address(u8),
    #[error("cancelled")]
    Cancelled,
}

impl From<openbms_transport::Error> for Error {
    fn from(e: openbms_transport::Error) -> Self {
        Error::Transport(e.to_string())
    }
}

impl From<openbms_proto::Error> for Error {
    fn from(e: openbms_proto::Error) -> Self {
        Error::Proto(e.to_string())
    }
}

impl Error {
    /// The message in the user's language (Display is English, for logs).
    pub fn text(&self, lang: Lang) -> String {
        if lang == Lang::En {
            return self.to_string();
        }
        match self {
            Error::Io(e) => format!("Ein-/Ausgabe: {e}"),
            Error::Transport(e) => format!("Verbindung: {e}"),
            Error::Image(e) => format!("Image: {e}"),
            Error::Proto(e) => format!("Protokoll: {e}"),
            Error::NoAnswer(a) => format!("Pack {a} antwortet nicht"),
            Error::BusBusy(n) => format!("Der Bus ist nicht still ({n} Byte von anderen Geräten)"),
            Error::NotPackBus(a) => format!("Pack {a} antwortet auf 0x51, aber nicht auf 0x47: nicht an RS485-1/2 (Pack-Bus) angeschlossen"),
            Error::NoBootloader(d) => format!("Der Bootloader hat nicht innerhalb von {d:?} geantwortet"),
            Error::PacketSize(n) => format!("Der Bootloader meldet Paketgröße {n}; unterstützt ist nur 256"),
            Error::Rejected => "Der Bootloader hat den Rahmen abgelehnt (R F N)".into(),
            Error::Unexpected(s) => format!("Unerwartete Antwort des Bootloaders: {s}"),
            Error::Timeout { what, after } => format!("Keine Antwort auf {what} innerhalb von {after:?}"),
            Error::Address(a) => format!("Adresse {a} ist kein einzelnes Pack (0–15)"),
            Error::Cancelled => "Abgebrochen".into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
