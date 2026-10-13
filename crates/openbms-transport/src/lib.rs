//! Blocking transport for the BMS buses: USB-RS485 adapters (serial) and RS485-to-Ethernet
//! gateways (raw TCP). One request at a time; the BMS answers within a few hundred ms.

use openbms_proto::{frame, modbus, serial, DeviceInfo, Frame, MaintStatus, Parameters, SerialNumbers, SerialValue, Status, SystemValues, Telemetry};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
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
    #[error("connection closed by the gateway")]
    Closed,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Time to establish a TCP connection. Without it a gateway that is not reachable yet (Wi-Fi still coming
/// up after sleep) blocks a connection attempt for the operating system's timeout, over a minute on macOS.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Connect to the first address of `addr` that answers within CONNECT_TIMEOUT, name resolution included.
fn connect_tcp(addr: &str) -> Result<TcpStream> {
    connect_tcp_within(
        addr,
        CONNECT_TIMEOUT,
        |name| name.to_socket_addrs().map(|a| a.collect()),
        TcpStream::connect_timeout,
    )
}

/// Resolution runs on a helper thread, so a hanging name server cannot stretch the budget; every address
/// gets only what is left of it.
fn connect_tcp_within(
    addr: &str,
    budget: Duration,
    resolve: impl FnOnce(String) -> std::io::Result<Vec<std::net::SocketAddr>> + Send + 'static,
    mut connect: impl FnMut(&std::net::SocketAddr, Duration) -> std::io::Result<TcpStream>,
) -> Result<TcpStream> {
    let deadline = Instant::now() + budget;
    let (tx, rx) = std::sync::mpsc::channel();
    let name = addr.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(resolve(name));
    });
    let timed_out =
        || std::io::Error::new(std::io::ErrorKind::TimedOut, format!("{addr}: no connection within {budget:?}"));
    let addrs = rx.recv_timeout(budget).map_err(|_| timed_out())??;
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("{addr}: no address"));
    for a in addrs {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(timed_out().into());
        }
        match connect(&a, left) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last.into())
}

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
    /// A TCP read of 0 bytes means the gateway closed the connection; on serial ports it does not.
    tcp: bool,
}

impl Connection {
    pub fn open(endpoint: &Endpoint) -> Result<Self> {
        let timeout = Duration::from_millis(1500);
        let tcp = matches!(endpoint, Endpoint::Tcp { .. });
        let port: Box<dyn Port> = match endpoint {
            Endpoint::Serial { path, baud } => Box::new(serialport::new(path, *baud).timeout(Duration::from_millis(100)).open()?),
            Endpoint::Tcp { addr } => {
                let s = connect_tcp(addr)?;
                s.set_read_timeout(Some(Duration::from_millis(100)))?;
                // a dead link (e.g. after the computer slept) must not block a write either
                s.set_write_timeout(Some(Duration::from_millis(1500)))?;
                s.set_nodelay(true)?;
                Box::new(s)
            }
        };
        Ok(Self { port, timeout, gap: Duration::from_millis(100), tcp })
    }

    fn read_until<F: Fn(&[u8]) -> bool>(&mut self, done: F) -> Result<Vec<u8>> {
        let start = Instant::now();
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        while start.elapsed() < self.timeout {
            match self.port.read(&mut buf) {
                Ok(0) if self.tcp => return Err(Error::Closed),
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

    /// Maintenance state of a patched master (CID2 0x64, CAN socket bus). Original firmware answers RTN 04,
    /// which comes back as an error; the short timeout keeps a missing master from delaying the connect.
    pub fn maint_status(&mut self) -> Result<MaintStatus> {
        let saved = self.timeout;
        self.timeout = Duration::from_millis(600);
        let f = MaintStatus::request().map_err(Error::from).and_then(|r| self.ascii(&r, 0));
        self.timeout = saved;
        Ok(MaintStatus::parse(&f?.info)?)
    }

    pub fn system_values(&mut self) -> Result<SystemValues> {
        let regs = self.modbus_read(0x03, SystemValues::START, SystemValues::COUNT)?;
        Ok(SystemValues::from_registers(&regs)?)
    }

    /// One serial number string over ASCII (pack bus). RTN 04 (command unknown) means this pack does not offer it.
    fn ascii_serial(&mut self, request: Result<Vec<u8>>, address: u8) -> Result<SerialValue> {
        match request.and_then(|r| self.ascii(&r, address)) {
            Ok(f) => Ok(serial::parse_ascii_serial(&f.info)?),
            Err(Error::Proto(openbms_proto::Error::Rtn(0x04))) => Ok(SerialValue::Unavailable),
            Err(e) => Err(e),
        }
    }

    /// Battery and BMS serial number of one pack (0xA2, 0xA4; RS485-1/2 only). A command the pack rejects or
    /// leaves unanswered counts as unavailable; only when neither answers is the pack itself unreachable.
    pub fn serial_numbers_pack_bus(&mut self, address: u8) -> Result<SerialNumbers> {
        let battery = self.ascii_serial(serial::battery_request(address).map_err(Error::from), address);
        let bms = self.ascii_serial(serial::bms_request(address).map_err(Error::from), address);
        match (battery, bms) {
            (Err(e @ Error::Timeout(_)), Err(Error::Timeout(_))) => Err(e),
            (b, m) => Ok(SerialNumbers { battery: unless_timeout(b)?, bms: unless_timeout(m)? }),
        }
    }

    /// First 22 characters of the master's battery serial number (Modbus FC04, CAN socket bus). Firmware before
    /// 16.06.11 answers with an exception.
    pub fn master_battery_serial(&mut self) -> Result<SerialValue> {
        match self.modbus_read(0x04, serial::MODBUS_START, serial::MODBUS_COUNT) {
            Ok(regs) => Ok(serial::battery_from_input_regs(&regs)?),
            Err(Error::Proto(openbms_proto::Error::ModbusException(_))) => Ok(SerialValue::Unavailable),
            Err(e) => Err(e),
        }
    }

    /// Serial numbers as far as `bus` exposes them. On the CAN socket bus only the master (address 0) shows its
    /// battery serial number; other addresses are not asked at all.
    pub fn serial_numbers(&mut self, bus: Bus, address: u8) -> Result<SerialNumbers> {
        match bus {
            Bus::PackBus => self.serial_numbers_pack_bus(address),
            Bus::CanSocketBus if address == 0 => {
                Ok(SerialNumbers { battery: self.master_battery_serial()?, bms: SerialValue::Unavailable })
            }
            Bus::CanSocketBus => Ok(SerialNumbers::UNAVAILABLE),
        }
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

/// Raw byte access for tools that speak another framing on the same bus (the bootloader).
impl Connection {
    /// Drop everything received so far.
    pub fn clear_input(&mut self) -> Result<()> {
        Ok(self.port.clear_input()?)
    }

    /// Write and wait until the bytes have left the computer (serial: transmitted; TCP: handed to the socket).
    pub fn write_raw(&mut self, data: &[u8]) -> Result<()> {
        self.port.write_all(data)?;
        Ok(self.port.flush()?)
    }

    /// One read: waits up to the port's read timeout (100 ms) and returns what arrived; 0 = nothing yet.
    pub fn read_raw(&mut self, buf: &mut [u8]) -> Result<usize> {
        match self.port.read(buf) {
            Ok(0) if self.tcp => Err(Error::Closed),
            Ok(n) => Ok(n),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => Ok(0),
            Err(e) => Err(e.into()),
        }
    }
}

/// A single unanswered serial number command: the pack answers otherwise, it just does not offer this one.
fn unless_timeout(r: Result<SerialValue>) -> Result<SerialValue> {
    match r {
        Err(Error::Timeout(_)) => Ok(SerialValue::Unavailable),
        other => other,
    }
}

pub fn list_serial_ports() -> Vec<String> {
    serialport::available_ports().map(|v| v.into_iter().map(|p| p.port_name).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    /// Answers each request from a script (request bytes -> answer bytes); unknown requests get no answer.
    struct Scripted {
        script: Vec<(Vec<u8>, Vec<u8>)>,
        out: Vec<u8>,
    }

    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.out.is_empty() {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            let n = buf.len().min(self.out.len());
            buf[..n].copy_from_slice(&self.out[..n]);
            self.out.drain(..n);
            Ok(n)
        }
    }

    impl Write for Scripted {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if let Some((_, a)) = self.script.iter().find(|(r, _)| r.as_slice() == data) {
                self.out.extend_from_slice(a);
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Port for Scripted {
        fn clear_input(&mut self) -> std::io::Result<()> {
            self.out.clear();
            Ok(())
        }
    }

    fn scripted(script: Vec<(Vec<u8>, Vec<u8>)>) -> Connection {
        let port = Box::new(Scripted { script, out: Vec::new() });
        Connection { port, timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: false }
    }

    /// An answer frame: the request layout with RTN in place of CID2.
    fn answer(address: u8, rtn: u8, info: &[u8]) -> Vec<u8> {
        frame::encode_request(address, rtn, info).unwrap()
    }

    #[test]
    fn serial_numbers_rtn04_and_silence_count_as_unavailable() {
        let mut text = b"SYN-0001".to_vec();
        text.resize(30, b' ');
        let mut c = scripted(vec![
            (serial::battery_request(3).unwrap(), answer(3, 0, &text)),
            (serial::bms_request(3).unwrap(), answer(3, 0x04, &[])),
            (serial::battery_request(4).unwrap(), answer(4, 0, &[b' '; 30])),
        ]);
        let s = c.serial_numbers(Bus::PackBus, 3).unwrap();
        assert_eq!(s.battery, SerialValue::Set { value: "SYN-0001".into(), truncated: false });
        assert_eq!(s.bms, SerialValue::Unavailable);
        let s = c.serial_numbers(Bus::PackBus, 4).unwrap();
        assert_eq!((s.battery, s.bms), (SerialValue::Empty, SerialValue::Unavailable));
        // no answer to either command: the pack is not there
        assert!(matches!(c.serial_numbers(Bus::PackBus, 5), Err(Error::Timeout(_))));
    }

    #[test]
    fn can_socket_bus_reads_only_the_master_battery_serial() {
        let mut regs = b"SYN-0002".to_vec();
        regs.resize(42, b' ');
        let mut resp = vec![0x01, 0x04, 42];
        resp.extend(&regs);
        let crc = modbus::crc16(&resp);
        resp.extend(crc.to_le_bytes());
        let mut c = scripted(vec![(modbus::read_request(0x04, 0, 21), resp)]);
        let s = c.serial_numbers(Bus::CanSocketBus, 0).unwrap();
        assert_eq!(s.battery, SerialValue::Set { value: "SYN-0002".into(), truncated: false });
        assert_eq!(s.bms, SerialValue::Unavailable);
        // slaves are not asked at all (the script has nothing for them, so a request would time out)
        assert_eq!(c.serial_numbers(Bus::CanSocketBus, 2).unwrap(), SerialNumbers::UNAVAILABLE);
    }

    #[test]
    fn modbus_exception_counts_as_unavailable() {
        let mut ex = vec![0x01, 0x84, 0x02];
        let crc = modbus::crc16(&ex);
        ex.extend(crc.to_le_bytes());
        let mut c = scripted(vec![(modbus::read_request(0x04, 0, 21), ex)]);
        assert_eq!(c.master_battery_serial().unwrap(), SerialValue::Unavailable);
    }

    fn two() -> Vec<SocketAddr> {
        vec!["192.0.2.1:9".parse().unwrap(), "192.0.2.2:9".parse().unwrap()]
    }

    #[test]
    fn a_hanging_name_server_does_not_stretch_the_budget() {
        let started = Instant::now();
        let r = connect_tcp_within(
            "slow.example:9",
            Duration::from_millis(300),
            |_| {
                std::thread::sleep(Duration::from_secs(3));
                Ok(two())
            },
            |_, _| unreachable!("no address resolved in time"),
        );
        assert!(r.is_err());
        assert!(started.elapsed() < Duration::from_millis(1000));
    }

    #[test]
    fn all_addresses_share_one_budget() {
        let given = Arc::new(Mutex::new(Vec::new()));
        let g = given.clone();
        let started = Instant::now();
        let r = connect_tcp_within("gw:9", Duration::from_millis(400), |_| Ok(two()), move |_, left| {
            g.lock().unwrap().push(left);
            std::thread::sleep(left.min(Duration::from_millis(250)));
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "test"))
        });
        assert!(r.is_err());
        let given = given.lock().unwrap();
        assert_eq!(given.len(), 2);
        assert!(given[1] < Duration::from_millis(200), "second address gets only what is left: {:?}", given[1]);
        assert!(started.elapsed() < Duration::from_millis(700));
    }
}
