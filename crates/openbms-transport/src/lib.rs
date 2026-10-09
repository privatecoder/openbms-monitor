//! Blocking transport for the BMS buses: USB-RS485 adapters (serial) and RS485-to-Ethernet
//! gateways (raw TCP). One request at a time; the BMS answers within a few hundred ms.

use openbms_proto::{frame, modbus, DeviceInfo, Frame, Parameters, Status, SystemValues, Telemetry};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("serial port: {0}")]
    Serial(#[from] serialport::Error),
    #[error("protocol: {0}")]
    Proto(#[from] openbms_proto::Error),
    #[error("no answer within {0:?}")]
    Timeout(Duration),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Which RS485 bus the adapter is connected to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// RS485-1/2 sockets (19200 baud): full command set; a master pack does not answer here.
    PackBus,
    /// RS485 pins of the CAN socket (9600 baud): 0x42/0x44/0x51 of every pack, Modbus only master.
    CanSocketBus,
}

impl Bus {
    pub fn baud(self) -> u32 {
        match self {
            Bus::PackBus => 19200,
            Bus::CanSocketBus => 9600,
        }
    }
}

pub enum Endpoint {
    Serial { path: String, baud: u32 },
    Tcp { addr: String },
}

trait Port: Read + Write + Send {
    fn clear_input(&mut self) -> std::io::Result<()>;
}

impl Port for Box<dyn serialport::SerialPort> {
    fn clear_input(&mut self) -> std::io::Result<()> {
        self.clear(serialport::ClearBuffer::Input).map_err(std::io::Error::other)
    }
}

impl Port for TcpStream {
    fn clear_input(&mut self) -> std::io::Result<()> {
        self.set_nonblocking(true)?;
        let mut buf = [0u8; 256];
        loop {
            match self.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    self.set_nonblocking(false)?;
                    return Err(e);
                }
            }
        }
        self.set_nonblocking(false)
    }
}

pub struct Connection {
    port: Box<dyn Port>,
    pub timeout: Duration,
    /// Pause after each exchange; the BMS needs a short gap between requests.
    pub gap: Duration,
}

impl Connection {
    pub fn open(endpoint: &Endpoint) -> Result<Self> {
        let timeout = Duration::from_millis(1500);
        let port: Box<dyn Port> = match endpoint {
            Endpoint::Serial { path, baud } => Box::new(serialport::new(path, *baud).timeout(Duration::from_millis(100)).open()?),
            Endpoint::Tcp { addr } => {
                let s = TcpStream::connect(addr)?;
                s.set_read_timeout(Some(Duration::from_millis(100)))?;
                s.set_nodelay(true)?;
                Box::new(s)
            }
        };
        Ok(Self { port, timeout, gap: Duration::from_millis(100) })
    }

    fn read_until<F: Fn(&[u8]) -> bool>(&mut self, done: F) -> Result<Vec<u8>> {
        let start = Instant::now();
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        while start.elapsed() < self.timeout {
            match self.port.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    if done(&out) {
                        return Ok(out);
                    }
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Timeout(self.timeout))
    }

    /// Send an ASCII request and return the answer for `address`. Interleaved intra-pack
    /// frames (CID2 0x5A, master polling on the pack bus) are skipped.
    pub fn ascii(&mut self, request: &[u8], address: u8) -> Result<Frame> {
        self.port.clear_input()?;
        self.port.write_all(request)?;
        let deadline = Instant::now() + self.timeout;
        let mut pending = Vec::new();
        let result = loop {
            if Instant::now() > deadline {
                break Err(Error::Timeout(self.timeout));
            }
            if let Some(end) = pending.iter().position(|b| *b == b'\r') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                let start = line.iter().position(|b| *b == b'~').unwrap_or(0);
                match frame::parse(&line[start..]) {
                    Ok(f) if f.address == address && !(f.rtn == 0x5A && f.info.is_empty()) => {
                        if f.rtn == 0x5A && request.get(7..9) != Some(b"5A") {
                            continue; // intra-pack answer of a slave to the master, not ours
                        }
                        break Ok(f);
                    }
                    _ => continue,
                }
            }
            match self.read_until(|b| b.contains(&b'\r')) {
                Ok(chunk) => pending.extend(chunk),
                Err(e) => break Err(e),
            }
        };
        std::thread::sleep(self.gap);
        let f = result?;
        if f.rtn != 0x00 && f.rtn != 0x5A {
            return Err(openbms_proto::Error::Rtn(f.rtn).into());
        }
        Ok(f)
    }

    pub fn telemetry(&mut self, address: u8) -> Result<Telemetry> {
        let f = self.ascii(&Telemetry::request(address)?, address)?;
        Ok(Telemetry::parse(&f.info)?)
    }

    pub fn status(&mut self, address: u8) -> Result<Status> {
        let f = self.ascii(&Status::request(address)?, address)?;
        Ok(Status::parse(&f.info)?)
    }

    pub fn device_info(&mut self, address: u8) -> Result<DeviceInfo> {
        let f = self.ascii(&DeviceInfo::request(address)?, address)?;
        Ok(DeviceInfo::parse(&f.info)?)
    }

    pub fn parameters(&mut self, address: u8) -> Result<Parameters> {
        let f = self.ascii(&Parameters::request(address)?, address)?;
        Ok(Parameters::parse(&f.info)?)
    }

    /// Modbus FC03/04 read (master only, CAN socket bus).
    pub fn modbus_read(&mut self, function: u8, start: u16, count: u16) -> Result<Vec<u16>> {
        self.port.clear_input()?;
        self.port.write_all(&modbus::read_request(function, start, count))?;
        let want = modbus::response_len(count);
        let raw = self.read_until(|b| b.len() >= want || (b.len() >= 5 && b[1] & 0x80 != 0));
        std::thread::sleep(self.gap);
        Ok(modbus::parse_read_response(function, &raw?)?)
    }

    pub fn system_values(&mut self) -> Result<SystemValues> {
        let regs = self.modbus_read(0x03, SystemValues::START, SystemValues::COUNT)?;
        Ok(SystemValues::from_registers(&regs)?)
    }

    /// Find packs by asking every address 0..=15 for its device info.
    pub fn scan(&mut self, addresses: impl IntoIterator<Item = u8>) -> Vec<(u8, DeviceInfo)> {
        let saved = self.timeout;
        self.timeout = Duration::from_millis(600);
        let found = addresses.into_iter().filter_map(|a| self.device_info(a).ok().map(|d| (a, d))).collect();
        self.timeout = saved;
        found
    }
}

pub fn list_serial_ports() -> Vec<String> {
    serialport::available_ports().map(|v| v.into_iter().map(|p| p.port_name).collect()).unwrap_or_default()
}
