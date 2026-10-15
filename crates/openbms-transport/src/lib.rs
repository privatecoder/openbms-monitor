//! Blocking transport for the BMS buses: USB-RS485 adapters (serial) and RS485-to-Ethernet
//! gateways (raw TCP). One request at a time; the BMS answers within a few hundred ms.

use openbms_proto::{calibration, control, frame, memory, modbus, params, serial, BmsTime, Calibration, Comeback, DeviceInfo, HistoryRecord, Frame, MaintStatus, Parameters, SerialNumbers, SerialValue, Status, SystemValues, Telemetry};
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
    /// What arrived during ASCII exchanges since the last `reset_rx`; tells a silent bus from a wrong baud rate.
    rx: RxStats,
}

/// Received bytes, valid frames and intra-pack frames (master polling its slaves) of ASCII exchanges.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RxStats {
    pub bytes: usize,
    pub frames: usize,
    pub intra: usize,
}

/// Result of a reachability check: the first address that answered 0x51, and what else was heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub answered: Option<u8>,
    /// bytes arrived, but no valid frame: typical for a wrong baud rate
    pub noise: bool,
    /// a master polled its slaves (CID2 0x5A) on this bus
    pub intra: bool,
}

/// The addresses to check: the given ones first, then the rest of 0..=15.
pub fn probe_order(first: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for a in first.iter().copied().chain(0..=15) {
        if a <= 15 && !out.contains(&a) {
            out.push(a);
        }
    }
    out
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
        Ok(Self { port, timeout, gap: Duration::from_millis(100), tcp, rx: RxStats::default() })
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
                    self.rx.bytes += n;
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
                let parsed = frame::parse(&line[start..]);
                if let Ok(f) = &parsed {
                    self.rx.frames += 1;
                    if f.rtn == 0x5A && request.get(7..9) != Some(b"5A") {
                        self.rx.intra += 1;
                    }
                }
                match parsed {
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

    /// Write a parameter block (0xA1; RS485-1/2 only), built with `Values::apply` on a fresh read. RTN 06: the
    /// firmware's plausibility check failed (the switches are live but not stored); RTN 04: wrong markers.
    pub fn write_parameters(&mut self, address: u8, block: &[u8]) -> Result<()> {
        self.ascii(&params::write_request(address, block)?, address)?;
        Ok(())
    }

    /// Write a serial number string (0xA3/0xA5; RS485-1/2 only), stored at once.
    pub fn set_serial(&mut self, address: u8, which: serial::Which, text: &str) -> Result<()> {
        self.ascii(&serial::write_request(address, which, text)?, address)?;
        Ok(())
    }

    /// One serial number string (0xA2/0xA4), e.g. to read a write back. Unlike `serial_numbers` every failure is
    /// an error here.
    pub fn serial(&mut self, address: u8, which: serial::Which) -> Result<SerialValue> {
        let f = self.ascii(&which.read_request(address)?, address)?;
        Ok(serial::parse_ascii_serial(&f.info)?)
    }

    /// The BMS clock (0x4D; RS485-1/2 only).
    pub fn clock(&mut self, address: u8) -> Result<BmsTime> {
        let f = self.ascii(&memory::clock_request(address)?, address)?;
        Ok(memory::parse_clock(&f.info)?)
    }

    /// Set the BMS clock (0x4E; RS485-1/2 only). Implausible calendar fields are refused before sending.
    pub fn set_clock(&mut self, address: u8, time: BmsTime) -> Result<()> {
        self.ascii(&memory::set_clock_request(address, time)?, address)?;
        Ok(())
    }

    /// One calibration step (0xA0; RS485-1/2 only). A refusal comes back as RTN 06, an unknown sub as RTN 04
    /// (`openbms_proto::Refusal::from_error`). Success proves the new factor in RAM only.
    pub fn calibrate(&mut self, address: u8, cal: Calibration) -> Result<()> {
        self.ascii(&calibration::calibrate_request(address, cal)?, address)?;
        Ok(())
    }

    /// Set the CAN inverter protocol (0xA6; RS485-1/2 only), 1..=6. The new name shows in 0x51.
    pub fn set_can_protocol(&mut self, address: u8, protocol: u8) -> Result<()> {
        self.ascii(&calibration::set_can_protocol_request(address, protocol)?, address)?;
        Ok(())
    }

    /// Restart the pack (0x45 type 6; RS485-1/2 only). The board resets before it can answer, so a missing answer is
    /// the expected outcome and waits only 300 ms; RTN 00 counts as well. Returns when the request was sent, the start
    /// of the silence `await_comeback` measures.
    pub fn restart(&mut self, address: u8) -> Result<Instant> {
        let request = control::restart_request(address)?;
        let saved = self.timeout;
        self.timeout = Duration::from_millis(300);
        let sent = Instant::now();
        let r = self.ascii(&request, address);
        self.timeout = saved;
        match r {
            Ok(_) | Err(Error::Timeout(_)) => Ok(sent),
            Err(e) => Err(e),
        }
    }

    /// Switch the pack on or off (0x45 type 4). „Off“ blocks only the discharge MOSFET, from the next 1-s evaluation.
    pub fn on_off(&mut self, address: u8, on: bool) -> Result<()> {
        self.ascii(&control::on_off_request(address, on)?, address)?;
        Ok(())
    }

    /// Force the heater for one hour or stop it (0x45 type 3).
    pub fn heater(&mut self, address: u8, on: bool) -> Result<()> {
        self.ascii(&control::heater_request(address, on)?, address)?;
        Ok(())
    }

    /// Clear the pack's history, energy counters and cycle count (0x45 type 5). Not reversible.
    pub fn clear(&mut self, address: u8) -> Result<()> {
        self.ascii(&control::clear_request(address)?, address)?;
        Ok(())
    }

    /// Set the remaining capacity in 10 mAh (0x49 index 59). RTN 06: larger than the full capacity.
    pub fn set_remaining(&mut self, address: u8, raw: u16) -> Result<()> {
        self.ascii(&control::set_remaining_request(address, raw)?, address)?;
        Ok(())
    }

    /// Whether the pack answers 0x51 within 300 ms; any frame from it counts, also a refusal. Only a broken link is
    /// an error.
    pub fn answers(&mut self, address: u8) -> Result<bool> {
        let request = DeviceInfo::request(address)?;
        let saved = self.timeout;
        self.timeout = Duration::from_millis(300);
        let r = self.ascii(&request, address);
        self.timeout = saved;
        match r {
            Ok(_) | Err(Error::Proto(openbms_proto::Error::Rtn(_))) => Ok(true),
            Err(Error::Timeout(_) | Error::Proto(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// After `restart`: 0x51 back to back until the pack answers after a silence, or `limit` after `sent` is over.
    pub fn await_comeback(&mut self, address: u8, sent: Instant, limit: Duration) -> Result<Comeback> {
        watch_comeback(sent, limit, || self.answers(address))
    }

    /// One record of the internal history (0x4B; RS485-1/2 only): the oldest with `next = false`, else the one
    /// after the last read. `None`: no records; the flag marks the last record.
    pub fn history_record(&mut self, address: u8, next: bool) -> Result<Option<(HistoryRecord, bool)>> {
        let f = self.ascii(&memory::history_request(address, next)?, address)?;
        Ok(memory::parse_history(&f.info)?)
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

    /// One serial number string over ASCII (pack bus). RTN 04 (command unknown) means this pack does not offer
    /// it; silence, another RTN or a malformed answer leave this value without an answer. Only a broken link
    /// (I/O, closed gateway) is an error. The flag tells whether the pack stayed silent.
    fn ascii_serial(&mut self, request: openbms_proto::Result<Vec<u8>>, address: u8) -> Result<(SerialValue, bool)> {
        let answer = request.map_err(Error::from).and_then(|r| self.ascii(&r, address));
        match answer.and_then(|f| Ok(serial::parse_ascii_serial(&f.info)?)) {
            Ok(v) => Ok((v, false)),
            Err(Error::Proto(openbms_proto::Error::Rtn(0x04))) => Ok((SerialValue::Unavailable, false)),
            Err(Error::Timeout(_)) => Ok((SerialValue::NoAnswer, true)),
            Err(Error::Proto(_)) => Ok((SerialValue::NoAnswer, false)),
            Err(e) => Err(e),
        }
    }

    /// Battery and BMS serial number of one pack (0xA2, 0xA4; RS485-1/2 only). If the pack leaves 0xA2
    /// unanswered, 0xA4 is not sent: it would only wait for the timeout once more.
    pub fn serial_numbers_pack_bus(&mut self, address: u8) -> Result<SerialNumbers> {
        let (battery, silent) = self.ascii_serial(serial::battery_request(address), address)?;
        let bms = if silent { SerialValue::NoAnswer } else { self.ascii_serial(serial::bms_request(address), address)?.0 };
        Ok(SerialNumbers { battery, bms })
    }

    /// The BMS serial number alone (0xA4, RS485-1/2 only), e.g. to recognise a bank by its master.
    pub fn bms_serial(&mut self, address: u8) -> Result<SerialValue> {
        Ok(self.ascii_serial(serial::bms_request(address), address)?.0)
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
    /// Reachability: 0x51 to each address with a short timeout, stop at the first answer. Read-only.
    pub fn probe(&mut self, addresses: impl IntoIterator<Item = u8>) -> Probe {
        let saved = self.timeout;
        self.timeout = Duration::from_millis(300);
        self.rx = RxStats::default();
        // any frame from that address proves it is reachable, also a refusal or an answer this version formats differently
        let answered = addresses.into_iter().find(|a| match DeviceInfo::request(*a) {
            Ok(r) => matches!(self.ascii(&r, *a), Ok(_) | Err(Error::Proto(openbms_proto::Error::Rtn(_)))),
            Err(_) => false,
        });
        self.timeout = saved;
        Probe { answered, noise: answered.is_none() && self.rx.bytes > 0 && self.rx.frames == 0, intra: self.rx.intra > 0 }
    }

    /// The pack answers 0x47 (parameters): UART0 (RS485-1/2) knows it, UART1 (CAN socket) does not. Read-only.
    pub fn answers_parameters(&mut self, address: u8) -> bool {
        let saved = self.timeout;
        self.timeout = Duration::from_millis(600);
        let ok = self.parameters(address).is_ok();
        self.timeout = saved;
        ok
    }

    pub fn scan(&mut self, addresses: impl IntoIterator<Item = u8>) -> Vec<(u8, DeviceInfo)> {
        let saved = self.timeout;
        self.timeout = Duration::from_millis(600);
        let found = addresses.into_iter().filter_map(|a| self.device_info(a).ok().map(|d| (a, d))).collect();
        self.timeout = saved;
        found
    }
}

/// The restart watch: `answered` is one 0x51 try. A silence followed by an answer is a comeback, counted from `sent`;
/// answers throughout or silence to the end of `limit` say the restart was not seen.
pub fn watch_comeback(sent: Instant, limit: Duration, mut answered: impl FnMut() -> Result<bool>) -> Result<Comeback> {
    let mut missed = false;
    let mut any = false;
    while sent.elapsed() < limit {
        if answered()? {
            if missed {
                return Ok(Comeback::Back { after_ms: sent.elapsed().as_millis() as u64 });
            }
            any = true;
        } else {
            missed = true;
        }
    }
    Ok(if any && !missed { Comeback::NoInterruption } else { Comeback::NoAnswer })
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
        Connection { port, timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: false, rx: RxStats::default() }
    }

    /// An answer frame: the request layout with RTN in place of CID2.
    fn answer(address: u8, rtn: u8, info: &[u8]) -> Vec<u8> {
        frame::encode_request(address, rtn, info).unwrap()
    }

    #[test]
    fn serial_numbers_rtn04_is_unavailable_silence_is_no_answer() {
        let mut text = b"SYN-0001".to_vec();
        text.resize(30, b' ');
        let mut c = scripted(vec![
            (serial::battery_request(3).unwrap(), answer(3, 0, &text)),
            (serial::bms_request(3).unwrap(), answer(3, 0x04, &[])),
            (serial::battery_request(4).unwrap(), answer(4, 0, &[b' '; 30])),
            // malformed (short) battery answer: no answer for that one, 0xA4 is still asked
            (serial::battery_request(6).unwrap(), answer(6, 0, b"SYN")),
            (serial::bms_request(6).unwrap(), answer(6, 0, &text)),
            // 0xA2 silent: 0xA4 must not be sent (its answer here would show up otherwise)
            (serial::bms_request(5).unwrap(), answer(5, 0, &text)),
        ]);
        let s = c.serial_numbers(Bus::PackBus, 3).unwrap();
        assert_eq!(s.battery, SerialValue::Set { value: "SYN-0001".into(), truncated: false });
        assert_eq!(s.bms, SerialValue::Unavailable);
        let s = c.serial_numbers(Bus::PackBus, 4).unwrap();
        assert_eq!((s.battery, s.bms), (SerialValue::Empty, SerialValue::NoAnswer));
        let s = c.serial_numbers(Bus::PackBus, 5).unwrap();
        assert_eq!((s.battery, s.bms), (SerialValue::NoAnswer, SerialValue::NoAnswer));
        let s = c.serial_numbers(Bus::PackBus, 6).unwrap();
        assert_eq!(s.battery, SerialValue::NoAnswer);
        assert_eq!(s.bms, SerialValue::Set { value: "SYN-0001".into(), truncated: false });
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

    #[test]
    fn clock_and_history_records() {
        let t = BmsTime { year: 2026, month: 1, day: 2, hour: 3, minute: 4, second: 5 };
        let mut rec = vec![0x01, 0x01];
        rec.extend(2026u16.to_be_bytes());
        rec.resize(memory::HISTORY_LEN, 0);
        let mut c = scripted(vec![
            (memory::clock_request(1).unwrap(), answer(1, 0, &t.to_bytes())),
            (memory::set_clock_request(1, t).unwrap(), answer(1, 0, &[])),
            (memory::history_request(1, false).unwrap(), answer(1, 0, &[])),
            (memory::history_request(2, false).unwrap(), answer(2, 0, &rec)),
            (memory::history_request(3, false).unwrap(), answer(3, 0x08, &[])),
        ]);
        assert_eq!(c.clock(1).unwrap(), t);
        c.set_clock(1, t).unwrap();
        assert!(c.history_record(1, false).unwrap().is_none());
        let (r, last) = c.history_record(2, false).unwrap().unwrap();
        assert!(last);
        assert_eq!(r.time.year, 2026);
        assert!(matches!(c.history_record(3, false), Err(Error::Proto(openbms_proto::Error::Rtn(0x08)))));
    }

    #[test]
    fn calibration_and_can_protocol() {
        let cur = Calibration::Current { centi_amps: 2000 };
        let mut c = scripted(vec![
            (calibration::calibrate_request(1, Calibration::ZeroPoint).unwrap(), answer(1, 0, &[])),
            (calibration::calibrate_request(1, cur).unwrap(), answer(1, 0x06, &[])),
            (calibration::set_can_protocol_request(1, 3).unwrap(), answer(1, 0, &[])),
        ]);
        c.calibrate(1, Calibration::ZeroPoint).unwrap();
        let refused = match c.calibrate(1, cur) {
            Err(Error::Proto(e)) => openbms_proto::Refusal::from_error(&e),
            other => panic!("{other:?}"),
        };
        assert_eq!(refused, Some(openbms_proto::Refusal::Refused));
        c.set_can_protocol(1, 3).unwrap();
        // 7 is refused before anything is sent
        assert!(matches!(c.set_can_protocol(1, 7), Err(Error::Proto(openbms_proto::Error::Layout(_)))));
    }

    #[test]
    fn pack_control() {
        let mut c = scripted(vec![
            (control::restart_request(2).unwrap(), answer(2, 0x04, &[])),
            (control::on_off_request(1, false).unwrap(), answer(1, 0, &[])),
            (control::heater_request(1, true).unwrap(), answer(1, 0, &[])),
            (control::clear_request(1).unwrap(), answer(1, 0, &[])),
            (control::set_remaining_request(1, 30000).unwrap(), answer(1, 0x06, &[])),
            (control::set_remaining_request(1, 14000).unwrap(), answer(1, 0, &[])),
            (DeviceInfo::request(1).unwrap(), answer(1, 0, &[])),
            (DeviceInfo::request(3).unwrap(), answer(3, 0x04, &[])),
        ]);
        // silence is what a restart looks like; a refusal is still an error
        let started = Instant::now();
        let sent = c.restart(1).unwrap();
        assert!(sent >= started && sent.elapsed() < Duration::from_millis(1000));
        assert!(matches!(c.restart(2), Err(Error::Proto(openbms_proto::Error::Rtn(0x04)))));
        c.on_off(1, false).unwrap();
        c.heater(1, true).unwrap();
        c.clear(1).unwrap();
        assert!(matches!(c.set_remaining(1, 30000), Err(Error::Proto(openbms_proto::Error::Rtn(0x06)))));
        c.set_remaining(1, 14000).unwrap();
        assert!(c.answers(1).unwrap());
        assert!(c.answers(3).unwrap());
        assert!(!c.answers(4).unwrap());
    }

    #[test]
    fn comeback_needs_a_silence_then_an_answer() {
        let limit = Duration::from_secs(5);
        let mut tries = [false, false, true].into_iter();
        let r = watch_comeback(Instant::now() - Duration::from_millis(500), limit, || Ok(tries.next().unwrap())).unwrap();
        assert!(matches!(r, Comeback::Back { after_ms } if after_ms >= 500));
        // answered throughout, silent to the end, answered and then silent
        let short = Duration::from_millis(30);
        assert_eq!(watch_comeback(Instant::now(), short, || Ok(true)).unwrap(), Comeback::NoInterruption);
        assert_eq!(watch_comeback(Instant::now(), short, || Ok(false)).unwrap(), Comeback::NoAnswer);
        let mut n = 0;
        assert_eq!(watch_comeback(Instant::now(), short, || { n += 1; Ok(n < 3) }).unwrap(), Comeback::NoAnswer);
        // a broken link ends the watch
        assert!(watch_comeback(Instant::now(), limit, || Err(Error::Closed)).is_err());
    }

    #[test]
    fn probe_order_puts_the_polled_addresses_first() {
        assert_eq!(probe_order(&[]), (0..=15).collect::<Vec<u8>>());
        let o = probe_order(&[2, 0, 2, 20]);
        assert_eq!(&o[..3], &[2, 0, 1]);
        assert_eq!(o.len(), 16);
    }

    #[test]
    fn probe_stops_at_the_first_answer() {
        let mut c = scripted(vec![
            (DeviceInfo::request(11).unwrap(), answer(11, 0, &[])),
            (DeviceInfo::request(12).unwrap(), answer(12, 0, &[])),
        ]);
        let p = c.probe(probe_order(&[0]));
        assert_eq!(p, Probe { answered: Some(11), noise: false, intra: false });
        // a refusal is an answer as well
        let mut c = scripted(vec![(DeviceInfo::request(3).unwrap(), answer(3, 0x04, &[]))]);
        assert_eq!(c.probe([3]).answered, Some(3));
    }

    #[test]
    fn probe_tells_noise_and_master_polling_from_silence() {
        let mut c = scripted(vec![]);
        assert_eq!(c.probe([0]), Probe { answered: None, noise: false, intra: false });
        // a wrong baud rate: bytes, but no frame
        let mut c = scripted(vec![(DeviceInfo::request(0).unwrap(), vec![0xF0, 0x0F, 0x80, 0x00, 0xFE])]);
        assert_eq!(c.probe([0]), Probe { answered: None, noise: true, intra: false });
        // a master polling its slaves (CID2 0x5A), nobody answering the app
        let mut c = scripted(vec![(DeviceInfo::request(0).unwrap(), answer(1, 0x5A, b"\x01"))]);
        assert_eq!(c.probe([0]), Probe { answered: None, noise: false, intra: true });
    }

    #[test]
    fn parameter_and_serial_writes() {
        let mut block = vec![0x00, 0x3C];
        block.resize(122, 0x01);
        block.push(0x1B);
        block.resize(150, 0x02);
        block.push(0x08);
        block.resize(params::BLOCK_LEN, b' ');
        let mut other = block.clone();
        other[5] = 0x09;
        let mut text = b"SYN-0009".to_vec();
        text.resize(30, b' ');
        let mut c = scripted(vec![
            (params::write_request(2, &block).unwrap(), answer(2, 0, &[])),
            (params::write_request(2, &other).unwrap(), answer(2, 0x06, &[])),
            (serial::write_request(2, serial::Which::Bms, "SYN-0009").unwrap(), answer(2, 0, &[])),
            (serial::bms_request(2).unwrap(), answer(2, 0, &text)),
        ]);
        c.write_parameters(2, &block).unwrap();
        assert!(matches!(c.write_parameters(2, &other), Err(Error::Proto(openbms_proto::Error::Rtn(0x06)))));
        // a block without the markers is not sent
        assert!(matches!(c.write_parameters(2, &block[..100]), Err(Error::Proto(openbms_proto::Error::Layout(_)))));
        c.set_serial(2, serial::Which::Bms, "SYN-0009").unwrap();
        assert_eq!(c.serial(2, serial::Which::Bms).unwrap(), SerialValue::Set { value: "SYN-0009".into(), truncated: false });
        assert!(matches!(c.set_serial(2, serial::Which::Bms, "ä"), Err(Error::Proto(openbms_proto::Error::Layout(_)))));
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
