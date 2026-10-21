//! Blocking transport for the BMS buses: USB-RS485 adapters (serial) and RS485-to-Ethernet
//! gateways (raw TCP). One request at a time; the BMS answers within a few hundred ms.

use openbms_proto::{calibration, control, frame, memory, modbus, params, serial, uart1, BmsTime, Calibration, Comeback, DeviceInfo, HistoryRecord, Frame, Parameters, SerialNumbers, SerialValue, Status, SystemValues, Telemetry};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub mod capture;
pub mod timing;

use timing::Class;

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
    /// A binary answer started but never completed, with a valid-looking frame inside it: whether the pack answered
    /// is open (a write treats it like a lost answer).
    #[error("answer incomplete")]
    Incomplete,
    /// The bus never got quiet before the request (`timing::QUIET_LIMIT`): nothing was sent.
    #[error("bus busy for {0:?}: nothing sent")]
    BusBusy(Duration),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Time to establish a TCP connection. Without it a gateway that is not reachable yet (Wi-Fi still coming
/// up after sleep) blocks a connection attempt for the operating system's timeout, over a minute on macOS.
pub const CONNECT_TIMEOUT: Duration = timing::CONNECT;

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
    /// Drop what was received so far. Bytes read in user space to drop them (TCP) go to `dropped` when given.
    fn clear_input(&mut self, dropped: Option<&mut Vec<u8>>) -> std::io::Result<()>;
}

impl Port for Box<dyn serialport::SerialPort> {
    // what waits in the driver is read first when the caller wants it (the bus quiet counts it as traffic), then dropped
    fn clear_input(&mut self, dropped: Option<&mut Vec<u8>>) -> std::io::Result<()> {
        if let Some(d) = dropped {
            let waiting = self.bytes_to_read().unwrap_or(0) as usize;
            if waiting > 0 {
                let mut buf = vec![0u8; waiting];
                if let Ok(n) = self.read(&mut buf) {
                    d.extend_from_slice(&buf[..n]);
                }
            }
        }
        self.clear(serialport::ClearBuffer::Input).map_err(std::io::Error::other)
    }
}

impl Port for TcpStream {
    fn clear_input(&mut self, mut dropped: Option<&mut Vec<u8>>) -> std::io::Result<()> {
        self.set_nonblocking(true)?;
        let mut buf = [0u8; 256];
        loop {
            match self.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Some(d) = dropped.as_deref_mut() {
                        d.extend_from_slice(&buf[..n]);
                    }
                }
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
    /// The answer time of a normal ASCII request before learning (`timing::ASCII_ANSWER`)
    pub timeout: Duration,
    /// Pause after each exchange; the BMS needs a short gap between requests.
    pub gap: Duration,
    /// A TCP read of 0 bytes means the gateway closed the connection; on serial ports it does not.
    tcp: bool,
    /// What arrived during ASCII exchanges since the last `reset_rx`; tells a silent bus from a wrong baud rate.
    rx: RxStats,
    /// Where this connection's bytes go while a capture runs (`capture::GLOBAL`).
    tap: capture::Tap,
    /// How long valid answers took on this connection; learns over TCP only (`timing`).
    latency: timing::Latency,
    /// Addresses that gave a valid answer on this connection and did not stay silent through a retry since: only they
    /// get the one longer retry of a read (a pack that never answered is not asked twice).
    answered: std::collections::HashSet<u8>,
    /// The quiet the bus needs after the last byte heard before we send (`set_bus`; `timing::UART1_QUIET` on the CAN
    /// socket, none on RS485-1/2), and when that last byte came.
    quiet: Duration,
    heard: Option<Instant>,
}

/// A failed ein/aus (`Connection::on_off_phased`): `sent` whether the request had left completely before `error`.
#[derive(Debug)]
pub struct SwitchFailure {
    pub sent: bool,
    pub error: Error,
}

impl SwitchFailure {
    /// The pack may have switched: the request left and no definite answer (an RTN) came back. Silence, a closed
    /// gateway, an I/O error or a broken answer after sending leave the result open.
    pub fn unclear(&self) -> bool {
        self.sent && !matches!(self.error, Error::Proto(openbms_proto::Error::Rtn(_)))
    }
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
    /// the bus never got quiet before a request (`Error::BusBusy`): at least one address was not asked at all
    pub busy: bool,
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
        let timeout = timing::ASCII_ANSWER;
        let tcp = matches!(endpoint, Endpoint::Tcp { .. });
        let port: Box<dyn Port> = match endpoint {
            Endpoint::Serial { path, baud } => Box::new(serialport::new(path, *baud).timeout(timing::READ_CHUNK).open()?),
            Endpoint::Tcp { addr } => {
                let s = connect_tcp(addr)?;
                s.set_read_timeout(Some(timing::READ_CHUNK))?;
                // a dead link (e.g. after the computer slept) must not block a write either
                s.set_write_timeout(Some(timing::TCP_WRITE))?;
                s.set_nodelay(true)?;
                Box::new(s)
            }
        };
        let label = match endpoint {
            Endpoint::Serial { path, baud } => format!("USB {path} · {baud} Bd"),
            Endpoint::Tcp { addr } => format!("TCP {addr}"),
        };
        Ok(Self { port, timeout, gap: timing::GAP, tcp, rx: RxStats::default(), tap: capture::Tap::new(&capture::GLOBAL, label), latency: timing::Latency::new(tcp), answered: Default::default(), quiet: Duration::ZERO, heard: None })
    }

    /// The answer times learnt on this connection (empty and inactive on USB).
    pub fn latency(&self) -> &timing::Latency {
        &self.latency
    }

    /// How long to wait for an answer of `class` whose fixed time is `floor`, on this connection (`Latency::budget`).
    pub fn reply_budget(&self, class: Class, floor: Duration) -> Duration {
        self.latency.budget(class, floor)
    }

    /// A valid answer of `address` that took `took`: a sample, and the address may get a retry from now on.
    fn answered_in(&mut self, class: Class, address: u8, took: Duration) {
        self.latency.record(class, took);
        self.answered.insert(address);
    }

    /// Whether a read of `address` that timed out gets its one longer retry: over TCP, for a pack that answered
    /// before. A retry that stays silent too takes the pack off until it answers again.
    fn retries(&self, address: u8) -> bool {
        self.latency.adaptive() && self.answered.contains(&address)
    }

    /// The name this connection has in a capture, e.g. endpoint and bus as the caller knows them.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.tap.label = label.into().into();
    }

    /// Leave this connection's traffic out of the capture (the bootloader) or take it in again; on resuming the
    /// capture notes how many bytes were left out.
    pub fn pause_capture(&mut self, paused: bool) {
        self.tap.pause(paused);
    }

    /// Note how an exchange ended without its answer: a timeout or a broken link (I/O, gateway closed). Other
    /// failures (RTN, a malformed answer) are visible in the received bytes.
    fn note_failure<T>(&mut self, r: &Result<T>, address: u8) {
        match r {
            Err(Error::Timeout(d)) => self.tap.event(capture::Kind::Timeout { waited_ms: d.as_millis() as u64, address }),
            Err(e @ (Error::Io(_) | Error::Serial(_) | Error::Closed | Error::Incomplete)) => self.tap.event(capture::Kind::Error(e.to_string())),
            _ => {}
        }
    }

    /// Drop what was received before a request; on TCP a late answer read to drop it shows up in the capture.
    fn drop_input(&mut self) -> std::io::Result<()> {
        let mut dropped = Vec::new();
        let r = self.port.clear_input(Some(&mut dropped));
        // bytes waiting unread were bus traffic too (a late answer): the quiet before our next request counts from now
        if !dropped.is_empty() {
            self.heard = Some(Instant::now());
        }
        if self.tap.sink.is_running() {
            self.tap.dropped(&dropped);
        }
        r
    }

    /// The bus this connection talks on: its quiet before a request (`timing::UART1_QUIET` on the CAN socket).
    pub fn set_bus(&mut self, bus: Bus) {
        self.quiet = if bus == Bus::CanSocketBus { timing::UART1_QUIET } else { Duration::ZERO };
    }

    /// Bytes arrived: the bus was busy until now.
    fn heard_now(&mut self) {
        self.heard = Some(Instant::now());
    }

    /// `write_all` that shows up in the capture; first the bus's quiet after the last byte heard (`set_bus`), for every
    /// kind of request alike.
    fn send(&mut self, data: &[u8]) -> Result<()> {
        self.await_quiet()?;
        self.tap.tx(data);
        Ok(self.port.write_all(data)?)
    }

    /// Wait for a whole quiet period without a byte (`set_bus`): bytes that come in meanwhile (the rest of a late answer)
    /// are dropped and start it again, so the packs' receivers have closed every frame before ours begins.
    fn await_quiet(&mut self) -> Result<()> {
        if self.quiet.is_zero() {
            return Ok(());
        }
        let start = Instant::now();
        loop {
            self.drop_input()?;
            let Some(t) = self.heard else { return Ok(()) };
            let left = (t + self.quiet).saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            // never quiet: the request does not go out (callers take it as not sent, not as a missing answer)
            if start.elapsed() + left > timing::QUIET_LIMIT {
                return Err(Error::BusBusy(timing::QUIET_LIMIT));
            }
            // in steps, so bytes during the wait are seen and the quiet starts again after them
            std::thread::sleep(left.min(Duration::from_millis(20)));
        }
    }

    /// Read until `done` or the deadline of the whole exchange; lines that are not ours must not extend it.
    fn read_until<F: Fn(&[u8]) -> bool>(&mut self, deadline: Instant, waited: Duration, done: F) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        while Instant::now() < deadline {
            match self.port.read(&mut buf) {
                Ok(0) if self.tcp => return Err(Error::Closed),
                Ok(0) => {}
                Ok(n) => {
                    self.heard_now();
                    self.tap.rx(&buf[..n]);
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
        Err(Error::Timeout(waited))
    }

    /// Send an ASCII request and return the answer for `address`. Interleaved intra-pack
    /// frames (CID2 0x5A, master polling on the pack bus) and an exact echo of the request are skipped.
    pub fn ascii(&mut self, request: &[u8], address: u8) -> Result<Frame> {
        self.ascii_as(request, address, |_| true)
    }

    /// `ascii` for an answer whose layout can be checked: an answer carries no CID2, so a late answer to another
    /// request (0x42 arriving while 0x44 waits) looks like ours. An RTN 00 frame that `fits` refuses is skipped and the
    /// wait goes on within the same deadline; if none fits by then, the last one is returned (its parse error shows).
    fn ascii_as(&mut self, request: &[u8], address: u8, fits: impl Fn(&[u8]) -> bool) -> Result<Frame> {
        let limit = self.reply_budget(Class::Ascii, self.timeout);
        self.ascii_within(request, address, &fits, limit)
    }

    /// `ascii_as` for a read: a pack that answered before is listened to longer for the same request (`extended`), never
    /// asked twice (a second, indistinguishable request would leave its answer on the line for the next command).
    fn ascii_read(&mut self, request: &[u8], address: u8, fits: impl Fn(&[u8]) -> bool) -> Result<Frame> {
        let limit = self.read_budget(Class::Ascii, self.timeout, address);
        let r = self.ascii_within(request, address, &fits, limit);
        self.silent_through(&r, address);
        r
    }

    /// The wait of a read of `address`: the longer one (`retry_budget`) for a pack that answered before over TCP.
    fn read_budget(&self, class: Class, floor: Duration, address: u8) -> Duration {
        if self.retries(address) { self.latency.retry_budget(class, floor) } else { self.reply_budget(class, floor) }
    }

    /// A read that stayed silent through its whole wait takes the pack off the longer wait until it answers again.
    fn silent_through<T>(&mut self, r: &Result<T>, address: u8) {
        if matches!(r, Err(Error::Timeout(_))) {
            self.answered.remove(&address);
        }
    }

    /// One ASCII exchange that waits `limit` for the answer.
    fn ascii_within(&mut self, request: &[u8], address: u8, fits: &dyn Fn(&[u8]) -> bool, limit: Duration) -> Result<Frame> {
        let r = self.ascii_exchange(request, address, fits, limit);
        self.note_failure(&r, address);
        r
    }

    fn ascii_exchange(&mut self, request: &[u8], address: u8, fits: &dyn Fn(&[u8]) -> bool, limit: Duration) -> Result<Frame> {
        self.drop_input()?;
        self.send(request)?;
        self.ascii_answer(request, address, fits, limit)
    }

    /// The answer part of `ascii_exchange`, once the request left completely. A valid answer of the address (also a
    /// refusal) is a sample of the answer time; the pause after it is not.
    fn ascii_answer(&mut self, request: &[u8], address: u8, fits: &dyn Fn(&[u8]) -> bool, limit: Duration) -> Result<Frame> {
        let started = Instant::now();
        let deadline = started + limit;
        let mut pending = Vec::new();
        // an answer of ours whose layout did not fit: returned only if no fitting one comes
        let mut misfit: Option<Frame> = None;
        let mut fitted = false;
        let result = loop {
            if Instant::now() > deadline {
                break misfit.take().ok_or(Error::Timeout(limit));
            }
            if let Some(end) = pending.iter().position(|b| *b == b'\r') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                let start = line.iter().position(|b| *b == b'~').unwrap_or(0);
                // an adapter or gateway that echoes what it sent: the request itself is no answer
                if line[start..] == *request {
                    // not counted as received data: an echo alone is no garbled answer either
                    self.rx.bytes = self.rx.bytes.saturating_sub(line.len());
                    continue;
                }
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
                        if f.rtn == 0x00 && !fits(&f.info) {
                            misfit = Some(f);
                            continue;
                        }
                        fitted = true;
                        break Ok(f);
                    }
                    _ => continue,
                }
            }
            match self.read_until(deadline, limit, |b| b.contains(&b'\r')) {
                Ok(chunk) => pending.extend(chunk),
                Err(Error::Timeout(_)) if misfit.is_some() => break Ok(misfit.take().unwrap()),
                Err(e) => break Err(e),
            }
        };
        // only an answer that fits is a sample: a misfit returned at the deadline came earlier and answered another request
        if result.is_ok() && fitted {
            self.answered_in(Class::Ascii, address, started.elapsed());
        }
        std::thread::sleep(self.gap);
        let f = result?;
        if f.rtn != 0x00 && f.rtn != 0x5A {
            return Err(openbms_proto::Error::Rtn(f.rtn).into());
        }
        Ok(f)
    }

    pub fn telemetry(&mut self, address: u8) -> Result<Telemetry> {
        let f = self.ascii_read(&Telemetry::request(address)?, address, |i| Telemetry::parse(i).is_ok())?;
        Ok(Telemetry::parse(&f.info)?)
    }

    pub fn status(&mut self, address: u8) -> Result<Status> {
        let f = self.ascii_read(&Status::request(address)?, address, |i| Status::parse(i).is_ok())?;
        Ok(Status::parse(&f.info)?)
    }

    pub fn device_info(&mut self, address: u8) -> Result<DeviceInfo> {
        let f = self.ascii_read(&DeviceInfo::request(address)?, address, |i| DeviceInfo::parse(i).is_ok())?;
        Ok(DeviceInfo::parse(&f.info)?)
    }

    pub fn parameters(&mut self, address: u8) -> Result<Parameters> {
        let f = self.ascii_read(&Parameters::request(address)?, address, |i| Parameters::parse(i).is_ok())?;
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
        Ok(self.serial_canonical(address, which)?.0)
    }

    /// `serial`, and whether its raw bytes were canonical (`serial::canonical`).
    pub fn serial_canonical(&mut self, address: u8, which: serial::Which) -> Result<(SerialValue, bool)> {
        let f = self.ascii(&which.read_request(address)?, address)?;
        Ok(serial::parse_ascii_serial_canonical(&f.info)?)
    }

    /// The BMS clock (0x4D; RS485-1/2, and the CAN socket of a pack with the power and clock service).
    pub fn clock(&mut self, address: u8) -> Result<BmsTime> {
        let f = self.ascii(&memory::clock_request(address)?, address)?;
        Ok(memory::parse_clock(&f.info)?)
    }

    /// Set the BMS clock (0x4E; RS485-1/2, and the CAN socket of a pack with the power and clock service). Implausible calendar fields are refused before sending.
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

    /// Restart the pack (0x45 type 6; RS485-1/2, and the CAN socket of a pack with the power and clock service). The board resets before it can answer, so a missing answer is
    /// the expected outcome and waits only 300 ms; RTN 00 counts as well. Returns when the request was sent, the start
    /// of the silence `await_comeback` measures.
    pub fn restart(&mut self, address: u8) -> Result<Instant> {
        let request = control::restart_request(address)?;
        let sent = Instant::now();
        let r = self.ascii_within(&request, address, &|_| true, timing::RESTART_ANSWER);
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

    /// `on_off` that tells whether a failure came before the request had left completely (`sent: false`: not switched)
    /// or after (`sent: true`: a missing, broken or refused answer). Never repeated here.
    pub fn on_off_phased(&mut self, address: u8, on: bool) -> std::result::Result<(), SwitchFailure> {
        let request = control::on_off_request(address, on).map_err(|e| SwitchFailure { sent: false, error: e.into() })?;
        let before = self.drop_input().map_err(Error::from).and_then(|_| self.send(&request));
        if before.is_err() {
            self.note_failure(&before, address);
            return before.map_err(|error| SwitchFailure { sent: false, error });
        }
        let limit = self.reply_budget(Class::Ascii, self.timeout);
        let r = self.ascii_answer(&request, address, &|_| true, limit).map(|_| ());
        self.note_failure(&r, address);
        r.map_err(|error| SwitchFailure { sent: true, error })
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

    /// The learned full capacity, `raw` in 10 mAh (0x49 index 58); RTN 06 when the remaining capacity exceeds it.
    pub fn set_learned_full(&mut self, address: u8, raw: u16) -> Result<()> {
        self.ascii(&control::set_learned_full_request(address, raw)?, address)?;
        Ok(())
    }

    /// Whether the pack answers 0x51 within the normal timeout; any frame from it counts, also a refusal. Only a broken
    /// link is an error.
    pub fn answers(&mut self, address: u8) -> Result<bool> {
        let t = self.reply_budget(Class::Ascii, self.timeout);
        self.answers_within(address, t)
    }

    /// `answers` with its own timeout for the whole answer (a gateway may deliver it late).
    pub fn answers_within(&mut self, address: u8, timeout: Duration) -> Result<bool> {
        let request = DeviceInfo::request(address)?;
        let r = self.ascii_within(&request, address, &|_| true, timeout);
        match r {
            Ok(_) | Err(Error::Proto(openbms_proto::Error::Rtn(_))) => Ok(true),
            Err(Error::Timeout(_) | Error::Proto(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// One record of the internal history (0x4B; RS485-1/2 only): the oldest with `next = false`, else the one
    /// after the last read. `None`: no records; the flag marks the last record.
    pub fn history_record(&mut self, address: u8, next: bool) -> Result<Option<(HistoryRecord, bool)>> {
        let f = self.ascii(&memory::history_request(address, next)?, address)?;
        Ok(memory::parse_history(&f.info)?)
    }

    /// Modbus FC03/04 read (master only, CAN socket bus).
    pub fn modbus_read(&mut self, function: u8, start: u16, count: u16) -> Result<Vec<u16>> {
        let request = modbus::read_request(function, start, count);
        let raw = self.drop_input().map_err(Error::from).and_then(|_| self.send(&request)).and_then(|_| {
            let want = modbus::response_len(count);
            let limit = self.reply_budget(Class::Modbus, timing::MODBUS_ANSWER);
            let started = Instant::now();
            self.read_until(started + limit, limit, |b| b.len() >= want || (b.len() >= 5 && b[1] & 0x80 != 0)).map(|b| (b, started.elapsed()))
        });
        self.note_failure(&raw, request[0]);
        std::thread::sleep(self.gap);
        let (raw, took) = raw?;
        let parsed = modbus::parse_read_response(function, &raw);
        // a valid answer, also an exception: a sample (a broken one is not)
        if matches!(parsed, Ok(_) | Err(openbms_proto::Error::ModbusException(_))) {
            self.answered_in(Class::Modbus, request[0], took);
        }
        Ok(parsed?)
    }

    pub fn system_values(&mut self) -> Result<SystemValues> {
        let regs = self.modbus_read(0x03, SystemValues::START, SystemValues::COUNT)?;
        Ok(SystemValues::from_registers(&regs)?)
    }

    /// One serial number string over ASCII (pack bus). RTN 04 (command unknown) means this pack does not offer
    /// it; silence, another RTN or a malformed answer leave this value without an answer. Only a broken link
    /// (I/O, closed gateway) is an error. The flag tells whether the pack stayed silent.
    fn ascii_serial(&mut self, request: openbms_proto::Result<Vec<u8>>, address: u8) -> Result<(SerialValue, bool)> {
        self.ascii_serial_canonical(request, address).map(|(v, silent, _)| (v, silent))
    }

    /// `ascii_serial`, and whether the raw bytes were canonical (`serial::canonical`).
    fn ascii_serial_canonical(&mut self, request: openbms_proto::Result<Vec<u8>>, address: u8) -> Result<(SerialValue, bool, bool)> {
        let answer = request.map_err(Error::from).and_then(|r| self.ascii(&r, address));
        match answer.and_then(|f| Ok(serial::parse_ascii_serial_canonical(&f.info)?)) {
            Ok((v, canonical)) => Ok((v, false, canonical)),
            Err(Error::Proto(openbms_proto::Error::Rtn(0x04))) => Ok((SerialValue::Unavailable, false, false)),
            Err(Error::Timeout(_)) => Ok((SerialValue::NoAnswer, true, false)),
            Err(Error::Proto(_)) => Ok((SerialValue::NoAnswer, false, false)),
            Err(e) => Err(e),
        }
    }

    /// Battery and BMS serial number of one pack (0xA2, 0xA4; RS485-1/2 only). If the pack leaves 0xA2
    /// unanswered, 0xA4 is not sent: it would only wait for the timeout once more.
    pub fn serial_numbers_pack_bus(&mut self, address: u8) -> Result<SerialNumbers> {
        Ok(self.serial_numbers_pack_bus_canonical(address)?.0)
    }

    /// `serial_numbers_pack_bus`, and whether the raw BMS serial was canonical (`serial::canonical`).
    pub fn serial_numbers_pack_bus_canonical(&mut self, address: u8) -> Result<(SerialNumbers, bool)> {
        let (battery, silent) = self.ascii_serial(serial::battery_request(address), address)?;
        let (bms, canonical) = if silent { (SerialValue::NoAnswer, false) } else {
            let (v, _, c) = self.ascii_serial_canonical(serial::bms_request(address), address)?;
            (v, c)
        };
        Ok((SerialNumbers { battery, bms }, canonical))
    }

    /// The BMS serial number alone (0xA4, RS485-1/2 only), e.g. to recognise a bank by its master.
    pub fn bms_serial(&mut self, address: u8) -> Result<SerialValue> {
        Ok(self.ascii_serial(serial::bms_request(address), address)?.0)
    }

    /// `bms_serial`, and whether its raw bytes were canonical (`serial::canonical`).
    pub fn bms_serial_canonical(&mut self, address: u8) -> Result<(SerialValue, bool)> {
        self.ascii_serial_canonical(serial::bms_request(address), address).map(|(v, _, c)| (v, c))
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
        let limit = self.latency.probe_budget(Class::Ascii, timing::REACH_TRY);
        self.rx = RxStats::default();
        // an answer from that address proves it is reachable, also an answer this version formats differently or a
        // refusal with a BMS RTN code (01–09); an echo of the request is skipped, any other RTN proves nothing
        let mut busy = false;
        let answered = addresses.into_iter().find(|a| match DeviceInfo::request(*a) {
            Ok(r) => match self.ascii_within(&r, *a, &|_| true, limit) {
                Ok(_) | Err(Error::Proto(openbms_proto::Error::Rtn(0x01..=0x09))) => true,
                // not sent: says nothing about this address
                Err(Error::BusBusy(_)) => {
                    busy = true;
                    false
                }
                Err(_) => false,
            },
            Err(_) => false,
        });
        Probe { answered, noise: answered.is_none() && self.rx.bytes > 0 && self.rx.frames == 0, intra: self.rx.intra > 0, busy }
    }

    /// The pack answers 0x47 (parameters): UART0 (RS485-1/2) knows it, UART1 (CAN socket) does not. Read-only.
    pub fn answers_parameters(&mut self, address: u8) -> bool {
        // silence is the finding here: no retry
        let limit = self.latency.probe_budget(Class::Ascii, timing::CAPABILITY_TRY);
        let Ok(request) = Parameters::request(address) else { return false };
        self.ascii_within(&request, address, &|i| Parameters::parse(i).is_ok(), limit).is_ok_and(|f| Parameters::parse(&f.info).is_ok())
    }

    /// One exchange with the binary parameter service of a patched pack (`uart1.rs`), only on the CAN socket bus.
    /// One deadline for the whole answer: an echo of the request, frames of other packs and broken bytes are skipped
    /// without sending again and without extending it. A timeout says only that no answer came, not which firmware runs.
    pub fn binary_exchange(&mut self, bus: Bus, request: &[u8], timeout: Duration) -> Result<uart1::Reply> {
        let limit = self.reply_budget(Class::Binary, timeout);
        self.binary_exchange_within(bus, request, limit)
    }

    /// `binary_exchange` that waits exactly `timeout`; a valid answer is a sample of the answer time.
    fn binary_exchange_within(&mut self, bus: Bus, request: &[u8], timeout: Duration) -> Result<uart1::Reply> {
        if bus != Bus::CanSocketBus {
            return Err(openbms_proto::Error::Layout("binary parameter frames exist only on the CAN socket bus").into());
        }
        let address = request.get(1).copied().unwrap_or(0);
        let mut started = Instant::now();
        let r = self.drop_input().map_err(Error::from).and_then(|_| self.send(request)).and_then(|_| {
            started = Instant::now();
            let deadline = started + timeout;
            let mut framer = uart1::Framer::new();
            let mut buf = [0u8; 256];
            // once at the end (deadline or the gateway closing): a started frame that never completed must not hide an
            // answer behind it, nor make a frame inside it an answer
            let end = |framer: &mut uart1::Framer, otherwise: Error| match framer.finish(request) {
                uart1::Finish::Reply(r) => Ok(r),
                uart1::Finish::Truncated => Err(Error::Incomplete),
                uart1::Finish::Nothing => Err(otherwise),
            };
            while Instant::now() < deadline {
                match self.port.read(&mut buf) {
                    Ok(0) if self.tcp => return end(&mut framer, Error::Closed),
                    Ok(0) => {}
                    Ok(n) => {
                        self.heard_now();
                        self.tap.rx(&buf[..n]);
                        framer.push(&buf[..n]);
                        if let Some(reply) = framer.next(request) {
                            return Ok(reply);
                        }
                    }
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            end(&mut framer, Error::Timeout(timeout))
        });
        if r.is_ok() {
            self.answered_in(Class::Binary, address, started.elapsed());
        }
        self.note_failure(&r, address);
        std::thread::sleep(self.gap);
        r
    }

    /// All parameters of one pack over the binary service (CAN socket bus, patched pack). An error code comes back as
    /// `Proto(Uart1(code))`.
    pub fn uart1_parameters(&mut self, bus: Bus, address: u8) -> Result<Parameters> {
        let request = uart1::read_request(address)?;
        // a pack that answered before is listened to longer, never asked twice (`ascii_read`)
        let limit = self.read_budget(Class::Binary, UART1_TIMEOUT, address);
        let r = self.binary_exchange_within(bus, &request, limit);
        self.silent_through(&r, address);
        match r? {
            uart1::Reply::Params(data) => Ok(uart1::parameters(address, &data)?),
            uart1::Reply::Error(code) => Err(openbms_proto::Error::Uart1(code.byte()).into()),
            uart1::Reply::Ack | uart1::Reply::Serial(_) => Err(openbms_proto::Error::Layout("UART1: no parameters in the answer to a read").into()),
        }
    }

    /// Write a block built by `Values::apply` over the binary service (its first 158 data bytes); Ok once acknowledged.
    /// E2 (the firmware's check failed: nothing taken) and the other codes come back as `Proto(Uart1(code))`.
    pub fn uart1_write(&mut self, bus: Bus, address: u8, block: &[u8]) -> Result<()> {
        match self.binary_exchange(bus, &uart1::write_request(address, block)?, UART1_TIMEOUT)? {
            uart1::Reply::Ack => Ok(()),
            uart1::Reply::Error(code) => Err(openbms_proto::Error::Uart1(code.byte()).into()),
            uart1::Reply::Params(_) | uart1::Reply::Serial(_) => Err(openbms_proto::Error::Layout("UART1: no acknowledgement in the answer to a write").into()),
        }
    }

    /// String 1 (`uart1::CMD_STRING1`) or String 2 (`uart1::CMD_STRING2`) of one pack, the 30 raw bytes.
    /// An image without it answers 0x81 (`Proto(Uart1(0x81))`: not offered); a timeout says nothing about the firmware.
    pub fn uart1_serial(&mut self, bus: Bus, address: u8, cmd: u8) -> Result<Vec<u8>> {
        match self.binary_exchange(bus, &uart1::serial_request(address, cmd)?, UART1_SERIAL_TIMEOUT)? {
            uart1::Reply::Serial(raw) => Ok(raw),
            uart1::Reply::Error(code) => Err(openbms_proto::Error::Uart1(code.byte()).into()),
            uart1::Reply::Ack | uart1::Reply::Params(_) => Err(openbms_proto::Error::Layout("UART1: no serial number in the answer").into()),
        }
    }

    /// The packs among `addresses` that answer 0x51. A bus that never got quiet before a request is an error
    /// (`Error::BusBusy`): an address not asked is no missing pack, so no partial list comes back.
    pub fn scan(&mut self, addresses: impl IntoIterator<Item = u8>) -> Result<Vec<(u8, DeviceInfo)>> {
        let limit = self.latency.probe_budget(Class::Ascii, timing::CAPABILITY_TRY);
        let mut found = Vec::new();
        for a in addresses {
            let Ok(request) = DeviceInfo::request(a) else { continue };
            match self.ascii_within(&request, a, &|i| DeviceInfo::parse(i).is_ok(), limit) {
                Ok(f) => found.extend(DeviceInfo::parse(&f.info).ok().map(|d| (a, d))),
                Err(e @ Error::BusBusy(_)) => return Err(e),
                Err(_) => {}
            }
        }
        Ok(found)
    }
}

/// The whole answer of the binary parameter service (`timing::BINARY_PARAMS`).
pub const UART1_TIMEOUT: Duration = timing::BINARY_PARAMS;
/// A serial number answer (`timing::BINARY_SERIAL`).
pub const UART1_SERIAL_TIMEOUT: Duration = timing::BINARY_SERIAL;

pub use timing::{SILENT_FOR, SILENT_TRIES};

/// The restart watch: `answered` is one 0x51 try. An interruption (`SILENT_TRIES` misses in a row or `SILENT_FOR` of
/// silence) followed by an answer is a comeback, counted from `sent`. A single late answer (a gateway delay) is no
/// interruption. Silent at the end of `limit` (an interruption that did not end): no answer; else not seen.
pub fn watch_comeback<E>(sent: Instant, limit: Duration, mut answered: impl FnMut() -> std::result::Result<bool, E>) -> std::result::Result<Comeback, E> {
    // the start of the current run of unanswered tries, and their number
    let mut silent: Option<(Instant, u32)> = None;
    while sent.elapsed() < limit {
        let started = Instant::now();
        if answered()? {
            if let Some((since, misses)) = silent {
                if misses >= SILENT_TRIES || started.duration_since(since) >= SILENT_FOR {
                    return Ok(Comeback::Back { after_ms: sent.elapsed().as_millis() as u64 });
                }
            }
            silent = None;
        } else {
            let (since, misses) = silent.unwrap_or((started, 0));
            silent = Some((since, misses + 1));
        }
    }
    // a single late miss on the last try is no interruption either
    let interrupted = silent.is_some_and(|(since, misses)| misses >= SILENT_TRIES || since.elapsed() >= SILENT_FOR);
    Ok(if interrupted { Comeback::NoAnswer } else { Comeback::NoInterruption })
}

/// Raw byte access for tools that speak another framing on the same bus (the bootloader).
impl Connection {
    /// Drop everything received so far.
    pub fn clear_input(&mut self) -> Result<()> {
        Ok(self.drop_input()?)
    }

    /// Write and wait until the bytes have left the computer (serial: transmitted; TCP: handed to the socket).
    pub fn write_raw(&mut self, data: &[u8]) -> Result<()> {
        let r = self.send(data).and_then(|_| Ok(self.port.flush()?));
        self.note_failure(&r, 0);
        r
    }

    /// One read: waits up to the port's read timeout (100 ms) and returns what arrived; 0 = nothing yet.
    pub fn read_raw(&mut self, buf: &mut [u8]) -> Result<usize> {
        let r = match self.port.read(buf) {
            Ok(0) if self.tcp => Err(Error::Closed),
            Ok(n) => Ok(n),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => Ok(0),
            Err(e) => Err(e.into()),
        };
        match &r {
            Ok(n) if *n > 0 => {
                self.heard_now();
                self.tap.rx(&buf[..*n])
            }
            _ => self.note_failure(&r, 0),
        }
        r
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
        /// writes (counted from 0) that get no answer, like a reply lost at a gateway
        silent: Vec<usize>,
        writes: Arc<Mutex<usize>>,
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
            let n = {
                let mut w = self.writes.lock().unwrap();
                *w += 1;
                *w - 1
            };
            if let Some((_, a)) = self.script.iter().find(|(r, _)| r.as_slice() == data).filter(|_| !self.silent.contains(&n)) {
                self.out.extend_from_slice(a);
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // behaves like a TCP gateway: what is dropped was read
    impl Port for Scripted {
        fn clear_input(&mut self, dropped: Option<&mut Vec<u8>>) -> std::io::Result<()> {
            if let Some(d) = dropped {
                d.extend_from_slice(&self.out);
            }
            self.out.clear();
            Ok(())
        }
    }

    /// A master polling its slave: an intra-pack line every `period` (reads wait 10 ms each), never our answer.
    struct Polling {
        start: Instant,
        period: Duration,
        sent: u32,
    }

    impl Read for Polling {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(10));
            if self.start.elapsed() < self.period * (self.sent + 1) {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            self.sent += 1;
            let line = b"~2001465A0000FD9D\r";
            buf[..line.len()].copy_from_slice(line);
            Ok(line.len())
        }
    }

    impl Write for Polling {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Port for Polling {
        fn clear_input(&mut self, _: Option<&mut Vec<u8>>) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn foreign_lines_do_not_extend_the_timeout() {
        let port = Box::new(Polling { start: Instant::now(), period: Duration::from_millis(300), sent: 0 });
        let sink = Box::leak(Box::new(capture::Capture::new()));
        let mut c = Connection { port, timeout: Duration::from_millis(400), gap: Duration::ZERO, tcp: false, rx: RxStats::default(), tap: capture::Tap::new(sink, "test".into()) , latency: timing::Latency::new(false), answered: Default::default(), quiet: Duration::ZERO, heard: None };
        let t = Instant::now();
        let r = c.ascii(&Telemetry::request(0).unwrap(), 0);
        assert!(matches!(r, Err(Error::Timeout(_))));
        // a line every ~300 ms: before the fix it restarted the wait, ending only after the next line (≥ 600 ms)
        assert!(t.elapsed() < Duration::from_millis(520), "{:?}", t.elapsed());
        assert!(c.rx.intra >= 1);
    }

    fn scripted(script: Vec<(Vec<u8>, Vec<u8>)>) -> Connection {
        scripted_into(script, Box::leak(Box::new(capture::Capture::new())))
    }

    fn scripted_into(script: Vec<(Vec<u8>, Vec<u8>)>, sink: &'static capture::Capture) -> Connection {
        let port = Box::new(Scripted { script, out: Vec::new(), silent: Vec::new(), writes: Default::default() });
        Connection { port, timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: false, rx: RxStats::default(), tap: capture::Tap::new(sink, "test".into()) , latency: timing::Latency::new(false), answered: Default::default(), quiet: Duration::ZERO, heard: None }
    }

    #[test]
    fn the_can_socket_keeps_the_bus_quiet_after_an_answer_before_the_next_request() {
        let ask = Telemetry::request(1).unwrap();
        for (bus, at_least, below) in [(Bus::CanSocketBus, timing::UART1_QUIET, Duration::from_secs(5)), (Bus::PackBus, Duration::ZERO, Duration::from_millis(150))] {
            let mut c = scripted(vec![(ask.clone(), TELE1.to_vec())]);
            c.set_bus(bus);
            c.telemetry(1).unwrap();
            let t = Instant::now();
            // the second request waits for the quiet after the first answer heard (every kind of request: one `send`)
            c.telemetry(1).unwrap();
            assert!(t.elapsed() >= at_least.saturating_sub(Duration::from_millis(5)), "{bus:?} {:?}", t.elapsed());
            assert!(t.elapsed() < below, "{bus:?} {:?}", t.elapsed());
        }
    }

    /// A port that receives a byte at the given offsets after creation (as a late, fragmented answer), records when
    /// each write left, answers nothing.
    struct Trickle {
        start: Instant,
        at: Vec<Duration>,
        writes: Arc<Mutex<Vec<Duration>>>,
    }

    impl Trickle {
        fn due(&mut self) -> usize {
            let now = self.start.elapsed();
            let n = self.at.iter().take_while(|t| **t <= now).count();
            self.at.drain(..n);
            n
        }
    }

    impl Read for Trickle {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(5));
            Err(std::io::ErrorKind::TimedOut.into())
        }
    }

    impl Write for Trickle {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.writes.lock().unwrap().push(self.start.elapsed());
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Port for Trickle {
        fn clear_input(&mut self, dropped: Option<&mut Vec<u8>>) -> std::io::Result<()> {
            let n = self.due();
            if let Some(d) = dropped {
                d.extend(std::iter::repeat_n(b'x', n));
            }
            Ok(())
        }
    }

    #[test]
    fn bytes_during_the_quiet_start_it_again() {
        // the last answer byte heard at 0, the rest of a late answer at 120 ms: the request leaves only a whole quiet
        // after that, never 200 ms after the first
        let writes = Arc::new(Mutex::new(vec![]));
        let port = Box::new(Trickle { start: Instant::now(), at: vec![Duration::from_millis(120)], writes: writes.clone() });
        let mut c = Connection { port, timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: true, rx: RxStats::default(), tap: capture::Tap::new(Box::leak(Box::new(capture::Capture::new())), "test".into()), latency: timing::Latency::new(false), answered: Default::default(), quiet: Duration::ZERO, heard: Some(Instant::now()) };
        c.set_bus(Bus::CanSocketBus);
        c.write_raw(b"~x\r").unwrap();
        let sent = writes.lock().unwrap()[0];
        assert!(sent >= Duration::from_millis(120) + timing::UART1_QUIET - Duration::from_millis(5), "{sent:?}");
    }

    #[test]
    fn a_bus_that_never_gets_quiet_sends_nothing() {
        // a byte every 150 ms for 12 s: no 200-ms quiet within the limit, so no request is sent at all
        let writes = Arc::new(Mutex::new(vec![]));
        let at = (1..=80).map(|i| Duration::from_millis(150 * i)).collect();
        let port = Box::new(Trickle { start: Instant::now(), at, writes: writes.clone() });
        let mut c = Connection { port, timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: true, rx: RxStats::default(), tap: capture::Tap::new(Box::leak(Box::new(capture::Capture::new())), "test".into()), latency: timing::Latency::new(false), answered: Default::default(), quiet: Duration::ZERO, heard: Some(Instant::now()) };
        c.set_bus(Bus::CanSocketBus);
        let t = Instant::now();
        assert!(matches!(c.write_raw(b"~x\r"), Err(Error::BusBusy(_))));
        assert!(t.elapsed() <= timing::QUIET_LIMIT + Duration::from_millis(100), "{:?}", t.elapsed());
        assert!(writes.lock().unwrap().is_empty());
        // a restart that could not be sent is an error, never „sent at“
        assert!(matches!(c.restart(1), Err(Error::BusBusy(_))));
        // a search on such a bus is an error, never „pack not there“; the reachability check says the bus was busy
        assert!(matches!(c.scan([0, 1]), Err(Error::BusBusy(_))));
        let p = c.probe([0]);
        assert!(p.busy && p.answered.is_none(), "{p:?}");
    }

    /// An answer frame: the request layout with RTN in place of CID2.
    fn answer(address: u8, rtn: u8, info: &[u8]) -> Vec<u8> {
        frame::encode_request(address, rtn, info).unwrap()
    }

    const TELE1: &[u8] = b"~2001460010960001100D050D050D060D050D050D070D060D060D060D060D060D060D060D060D070D07060B270B200B210B270B580B30000014D6569A0A6D6003176D60000703E814D9FFF3FF770359027FDDB1\r";
    const STATUS1: &[u8] = b"~20014600806200011000000000000000000000000000000000060000000000000000140000000000000300000200000000000000000002EB2F\r";

    /// A gateway whose answers come late: write n gets its scripted answer after `delays[n]` ms (missing: at once,
    /// `u64::MAX`: never). Counts the writes.
    struct Late {
        script: Vec<(Vec<u8>, Vec<u8>)>,
        delays: Vec<u64>,
        queue: Vec<(Instant, Vec<u8>)>,
        writes: Arc<Mutex<usize>>,
    }

    impl Read for Late {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.queue.iter().position(|(at, _)| *at <= Instant::now()) {
                Some(i) => {
                    let (_, a) = self.queue.remove(i);
                    buf[..a.len()].copy_from_slice(&a);
                    Ok(a.len())
                }
                None => {
                    std::thread::sleep(Duration::from_millis(2));
                    Err(std::io::ErrorKind::TimedOut.into())
                }
            }
        }
    }

    impl Write for Late {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            let n = {
                let mut w = self.writes.lock().unwrap();
                *w += 1;
                *w - 1
            };
            let delay = self.delays.get(n).copied().unwrap_or(0);
            if let Some((_, a)) = self.script.iter().find(|(r, _)| r.as_slice() == data).filter(|_| delay != u64::MAX) {
                self.queue.push((Instant::now() + Duration::from_millis(delay), a.clone()));
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // like a gateway: only what already arrived is dropped
    impl Port for Late {
        fn clear_input(&mut self, _: Option<&mut Vec<u8>>) -> std::io::Result<()> {
            let now = Instant::now();
            self.queue.retain(|(at, _)| *at > now);
            Ok(())
        }
    }

    /// Over a gateway (learning on), floor 50 ms; the second value counts the writes.
    fn late(script: Vec<(Vec<u8>, Vec<u8>)>, delays: Vec<u64>, adaptive: bool) -> (Connection, Arc<Mutex<usize>>) {
        let writes = Arc::new(Mutex::new(0));
        let port = Box::new(Late { script, delays, queue: Vec::new(), writes: writes.clone() });
        let c = Connection { port, timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: false, rx: RxStats::default(), tap: capture::Tap::new(Box::leak(Box::new(capture::Capture::new())), "test".into()), latency: timing::Latency::new(adaptive), answered: Default::default(), quiet: Duration::ZERO, heard: None };
        (c, writes)
    }

    const NEVER: u64 = u64::MAX;

    #[test]
    fn a_pack_that_answered_before_is_listened_to_longer_for_the_same_request() {
        let (mut c, writes) = late(vec![(Status::request(1).unwrap(), STATUS1.to_vec())], vec![0, 80], true);
        assert!(c.status(1).is_ok());
        // 80 ms is past the 50 ms floor but within the longer wait (100 ms): one request, its own answer
        assert!(c.status(1).is_ok());
        assert_eq!(*writes.lock().unwrap(), 2);
        let ascii = c.latency.view(timing::normal_floor)[0];
        assert_eq!(ascii.samples, 2);
        // measured from its own request
        assert!(ascii.max_ms.unwrap() >= 80, "{ascii:?}");
    }

    #[test]
    fn a_pack_that_never_answered_and_one_silent_through_the_longer_wait_get_the_floor() {
        // never answered on this connection: the floor
        let (mut c, _) = late(vec![(Status::request(1).unwrap(), STATUS1.to_vec())], vec![80], true);
        assert!(matches!(c.status(1), Err(Error::Timeout(d)) if d == Duration::from_millis(50)));
        // answered, then silent through the longer wait: taken off, the next read waits the floor
        let (mut c, writes) = late(vec![(Status::request(1).unwrap(), STATUS1.to_vec())], vec![0, NEVER, NEVER], true);
        assert!(c.status(1).is_ok());
        assert!(matches!(c.status(1), Err(Error::Timeout(d)) if d == Duration::from_millis(100)));
        assert!(matches!(c.status(1), Err(Error::Timeout(d)) if d == Duration::from_millis(50)));
        assert_eq!(*writes.lock().unwrap(), 3);
        // it answers again: the longer wait is back
        assert!(c.status(1).is_ok());
        assert!(c.retries(1));
    }

    #[test]
    fn a_local_link_keeps_the_floor_and_a_late_read_answer_is_no_switch_acknowledgement() {
        let (mut c, _) = late(vec![(Status::request(1).unwrap(), STATUS1.to_vec())], vec![0, 80], false);
        assert!(c.status(1).is_ok());
        assert!(matches!(c.status(1), Err(Error::Timeout(d)) if d == Duration::from_millis(50)));
        // ein/aus right after a read over a learning link: no second read request exists whose answer could stand in
        // for the missing acknowledgement
        let off = control::on_off_request(1, false).unwrap();
        let (mut c, writes) = late(vec![(Parameters::request(1).unwrap(), b"~20014600".to_vec()), (off, Vec::new())], vec![0, NEVER], true);
        let _ = c.parameters(1);
        assert!(c.on_off_phased(1, false).is_err_and(|f| f.unclear()));
        assert_eq!(*writes.lock().unwrap(), 2);
    }

    #[test]
    fn a_misfit_answer_is_no_sample_and_earns_no_longer_wait() {
        // the 0x42 answer while 0x44 waits: returned at the deadline for its layout error, never measured
        let (mut c, _) = late(vec![(Status::request(1).unwrap(), TELE1.to_vec())], vec![], true);
        for _ in 0..8 {
            assert!(matches!(c.status(1), Err(Error::Proto(openbms_proto::Error::Layout(_)))));
        }
        assert_eq!(c.latency.view(timing::normal_floor)[0].samples, 0);
        assert!(!c.retries(1));
    }

    #[test]
    fn a_late_0x42_answer_is_skipped_while_0x44_waits_for_its_own() {
        // the 0x42 answer of pack 1 arrives first, then the real 0x44 answer: the status is the real one
        let both = [TELE1, STATUS1].concat();
        let mut c = scripted(vec![(Status::request(1).unwrap(), both)]);
        let s = c.status(1).unwrap();
        assert_eq!(s.system_status, 0x02);
        assert!(s.alarms.is_empty());
        // only the 0x42 answer: no status made up from it, its layout error shows (not a timeout)
        let mut c = scripted(vec![(Status::request(1).unwrap(), TELE1.to_vec())]);
        assert!(matches!(c.status(1), Err(Error::Proto(openbms_proto::Error::Layout(_)))));
        // and the other way round for 0x42
        let mut c = scripted(vec![(Telemetry::request(1).unwrap(), [STATUS1, TELE1].concat())]);
        assert_eq!(c.telemetry(1).unwrap().pack_voltage, 53.34);
    }

    #[test]
    fn an_echo_of_the_request_is_no_answer() {
        let req = DeviceInfo::request(0).unwrap();
        // echo, then the pack's answer (a refusal here): reachable, the answer counts, not the echo
        let mut c = scripted(vec![(req.clone(), [req.clone(), answer(0, 0x04, &[])].concat())]);
        assert!(matches!(c.ascii(&req, 0), Err(Error::Proto(openbms_proto::Error::Rtn(0x04)))));
        assert_eq!(c.probe([0]).answered, Some(0));
        // echo only: nobody answers
        let mut c = scripted(vec![(req.clone(), req.clone())]);
        assert!(matches!(c.ascii(&req, 0), Err(Error::Timeout(_))));
        let p = c.probe([0]);
        assert_eq!((p.answered, p.noise), (None, false));
        // an RTN that is no BMS code proves nothing
        let mut c = scripted(vec![(req.clone(), answer(0, 0x51, &[]))]);
        assert_eq!(c.probe([0]).answered, None);
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
        assert!(!c.answers_within(4, Duration::from_millis(20)).unwrap());
    }

    #[test]
    fn comeback_needs_a_silence_then_an_answer() {
        let limit = Duration::from_secs(5);
        let mut tries = [false, false, true].into_iter();
        let r = watch_comeback(Instant::now() - Duration::from_millis(500), limit, || Ok::<_, Error>(tries.next().unwrap())).unwrap();
        assert!(matches!(r, Comeback::Back { after_ms } if after_ms >= 500));
        // one miss alone is a late answer, not a restart
        let short = Duration::from_millis(40);
        let mut n = 0;
        assert_eq!(watch_comeback(Instant::now(), short, || { n += 1; Ok::<_, Error>(n != 2) }).unwrap(), Comeback::NoInterruption);
        // one miss that lasted a second counts
        let mut n = 0;
        let r = watch_comeback(Instant::now(), limit, || {
            n += 1;
            if n == 1 { std::thread::sleep(SILENT_FOR); }
            Ok::<_, Error>(n != 1)
        });
        assert!(matches!(r.unwrap(), Comeback::Back { .. }));
        // answered throughout, silent to the end, answered and then silent
        assert_eq!(watch_comeback(Instant::now(), short, || Ok::<_, Error>(true)).unwrap(), Comeback::NoInterruption);
        assert_eq!(watch_comeback(Instant::now(), short, || Ok::<_, Error>(false)).unwrap(), Comeback::NoAnswer);
        let mut n = 0;
        assert_eq!(watch_comeback(Instant::now(), short, || { n += 1; Ok::<_, Error>(n < 3) }).unwrap(), Comeback::NoAnswer);
        // answered throughout except one late miss on the very last try
        let mut n = 0;
        let r = watch_comeback(Instant::now(), Duration::from_millis(100), || {
            n += 1;
            std::thread::sleep(Duration::from_millis(30));
            Ok::<_, Error>(n < 4)
        });
        assert_eq!(r.unwrap(), Comeback::NoInterruption);
        // a broken link ends the watch with its error
        assert!(watch_comeback(Instant::now(), limit, || Err::<bool, _>("closed")).is_err());
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
        assert_eq!(p, Probe { answered: Some(11), noise: false, intra: false, busy: false });
        // a refusal is an answer as well
        let mut c = scripted(vec![(DeviceInfo::request(3).unwrap(), answer(3, 0x04, &[]))]);
        assert_eq!(c.probe([3]).answered, Some(3));
    }

    #[test]
    fn probe_tells_noise_and_master_polling_from_silence() {
        let mut c = scripted(vec![]);
        assert_eq!(c.probe([0]), Probe { answered: None, noise: false, intra: false, busy: false });
        // a wrong baud rate: bytes, but no frame
        let mut c = scripted(vec![(DeviceInfo::request(0).unwrap(), vec![0xF0, 0x0F, 0x80, 0x00, 0xFE])]);
        assert_eq!(c.probe([0]), Probe { answered: None, noise: true, intra: false, busy: false });
        // a master polling its slaves (CID2 0x5A), nobody answering the app
        let mut c = scripted(vec![(DeviceInfo::request(0).unwrap(), answer(1, 0x5A, b"\x01"))]);
        assert_eq!(c.probe([0]), Probe { answered: None, noise: false, intra: true, busy: false });
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

    #[test]
    fn a_timeout_keeps_what_arrived_in_the_capture() {
        let sink: &'static capture::Capture = Box::leak(Box::new(capture::Capture::new()));
        sink.start();
        // an answer from another address and a partial line: not ours, the exchange times out
        let mut stray = answer(5, 0, &[]);
        stray.extend_from_slice(b"~2003");
        let request = DeviceInfo::request(3).unwrap();
        let mut c = scripted_into(vec![(request.clone(), stray.clone())], sink);
        c.set_label("USB /dev/test · RS485-1/2");
        assert!(matches!(c.device_info(3), Err(Error::Timeout(_))));
        let es = sink.snapshot().entries;
        assert_eq!(es[0].kind, capture::Kind::Tx);
        assert_eq!(es[0].bytes, request);
        let rx: Vec<u8> = es.iter().filter(|e| e.kind == capture::Kind::Rx).flat_map(|e| e.bytes.clone()).collect();
        assert_eq!(rx, stray);
        assert_eq!(es.last().unwrap().kind, capture::Kind::Timeout { waited_ms: 50, address: 3 });
        assert!(es.iter().all(|e| &*e.label == "USB /dev/test · RS485-1/2"));
        assert_eq!(sink.status().timeouts, 1);
    }

    #[test]
    fn a_late_answer_dropped_before_the_next_request_is_captured_as_dropped() {
        let sink: &'static capture::Capture = Box::leak(Box::new(capture::Capture::new()));
        sink.start();
        let first = DeviceInfo::request(1).unwrap();
        let late = answer(1, 0, &[]);
        // the answer to the first request is still in the input when the next request goes out
        let mut c = scripted_into(vec![(first.clone(), late.clone())], sink);
        c.write_raw(&first).unwrap();
        let before = sink.status().entries;
        assert!(matches!(c.device_info(2), Err(Error::Timeout(_))));
        let es = sink.snapshot().entries;
        let kinds: Vec<capture::Kind> = es[before..].iter().map(|e| e.kind.clone()).collect();
        assert_eq!(kinds, vec![capture::Kind::Dropped, capture::Kind::Tx, capture::Kind::Timeout { waited_ms: 50, address: 2 }]);
        assert_eq!(es[before].bytes, late);
        // not recorded while the capture is off
        sink.stop();
        c.write_raw(&first).unwrap();
        let n = sink.status().entries;
        let _ = c.device_info(2);
        assert_eq!(sink.status().entries, n);
    }

    #[test]
    fn modbus_and_raw_traffic_are_captured() {
        let sink: &'static capture::Capture = Box::leak(Box::new(capture::Capture::new()));
        let mut ex = vec![0x01, 0x84, 0x02];
        let crc = modbus::crc16(&ex);
        ex.extend(crc.to_le_bytes());
        let request = modbus::read_request(0x04, 0, 21);
        let mut c = scripted_into(vec![(request.clone(), ex.clone()), (b"raw".to_vec(), b"ok".to_vec())], sink);
        // nothing is recorded before the capture starts
        c.master_battery_serial().unwrap();
        assert_eq!(sink.status().entries, 0);
        sink.start();
        c.master_battery_serial().unwrap();
        c.write_raw(b"raw").unwrap();
        let mut buf = [0u8; 8];
        assert_eq!(c.read_raw(&mut buf).unwrap(), 2);
        let es: Vec<(capture::Kind, Vec<u8>)> = sink.snapshot().entries.iter().map(|e| (e.kind.clone(), e.bytes.clone())).collect();
        assert_eq!(es, vec![
            (capture::Kind::Tx, request),
            (capture::Kind::Rx, ex),
            (capture::Kind::Tx, b"raw".to_vec()),
            (capture::Kind::Rx, b"ok".to_vec()),
        ]);
    }

    #[test]
    fn a_paused_connection_notes_the_left_out_bytes() {
        let sink: &'static capture::Capture = Box::leak(Box::new(capture::Capture::new()));
        sink.start();
        let mut c = scripted_into(vec![(b"::::boot".to_vec(), b"::::ack".to_vec())], sink);
        c.pause_capture(true);
        c.write_raw(b"::::boot").unwrap();
        let mut buf = [0u8; 16];
        assert_eq!(c.read_raw(&mut buf).unwrap(), 7);
        c.pause_capture(false);
        let es = sink.snapshot().entries;
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].kind, capture::Kind::Skipped { sent: 8, received: 7 });
    }

    /// A binary answer of the parameter service (CRC-16/XMODEM, like the requests).
    fn binary(address: u8, cmd: u8, data: &[u8]) -> Vec<u8> {
        let mut body = vec![address, cmd, data.len() as u8];
        body.extend_from_slice(data);
        let crc = uart1::crc16_xmodem(&body);
        [vec![0x55], body, crc.to_be_bytes().to_vec(), vec![0xAA]].concat()
    }

    fn uart1_data() -> Vec<u8> {
        let mut d = vec![0x3C];
        d.resize(121, 0x02);
        d.push(0x1B);
        d.resize(149, 0x03);
        d.push(0x08);
        d.resize(uart1::READ_LEN, b' ');
        d
    }

    #[test]
    fn binary_parameters_read_and_write_on_the_can_socket_only() {
        let sink: &'static capture::Capture = Box::leak(Box::new(capture::Capture::new()));
        sink.start();
        let read = uart1::read_request(11).unwrap();
        let block = [vec![11], uart1_data()].concat();
        let mut changed = block.clone();
        changed[5] = 0x07;
        let write = uart1::write_request(11, &block).unwrap();
        let refused = uart1::write_request(11, &changed).unwrap();
        // the gateway echoes the read, a frame of another pack comes in between
        let answer = [read.clone(), binary(12, 0x47, &uart1_data()), binary(11, 0x47, &uart1_data())].concat();
        let mut c = scripted_into(vec![(read.clone(), answer), (write, binary(11, 0xA1, &[])), (refused, binary(11, 0xE2, &[]))], sink);
        let p = c.uart1_parameters(Bus::CanSocketBus, 11).unwrap();
        assert_eq!((p.address, p.block.clone()), (11, block.clone()));
        c.uart1_write(Bus::CanSocketBus, 11, &block).unwrap();
        assert!(matches!(c.uart1_write(Bus::CanSocketBus, 11, &changed), Err(Error::Proto(openbms_proto::Error::Uart1(0xE2)))));
        // sent once each: the echo did not make it send again
        let tx = sink.snapshot().entries.iter().filter(|e| e.kind == capture::Kind::Tx).count();
        assert_eq!(tx, 3);
        // never on RS485-1/2: nothing is sent there
        assert!(matches!(c.uart1_parameters(Bus::PackBus, 11), Err(Error::Proto(openbms_proto::Error::Layout(_)))));
        assert_eq!(sink.snapshot().entries.iter().filter(|e| e.kind == capture::Kind::Tx).count(), 3);
    }

    /// A gateway that sends `out` and then closes the connection.
    struct Closing {
        out: Vec<u8>,
    }

    impl Read for Closing {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.out.len());
            buf[..n].copy_from_slice(&self.out[..n]);
            self.out.drain(..n);
            Ok(n)
        }
    }

    impl Write for Closing {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Port for Closing {
        fn clear_input(&mut self, _: Option<&mut Vec<u8>>) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn closing(out: Vec<u8>) -> Connection {
        Connection { port: Box::new(Closing { out }), timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: true, rx: RxStats::default(), tap: capture::Tap::new(Box::leak(Box::new(capture::Capture::new())), "test".into()) , latency: timing::Latency::new(false), answered: Default::default(), quiet: Duration::ZERO, heard: None }
    }

    /// A port whose write fails: the request never left.
    struct Unwritable;

    impl Read for Unwritable {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    impl Write for Unwritable {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Port for Unwritable {
        fn clear_input(&mut self, _: Option<&mut Vec<u8>>) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn an_on_off_tells_a_failure_after_sending_from_one_before() {
        // the gateway closes after the request left (the pack it hangs on switched off): sent, result open
        let f = closing(vec![]).on_off_phased(1, false).unwrap_err();
        assert!(f.sent && matches!(f.error, Error::Closed) && f.unclear());
        // silence after sending: open as well
        let request = control::on_off_request(1, false).unwrap();
        let f = scripted(vec![(request.clone(), vec![])]).on_off_phased(1, false).unwrap_err();
        assert!(matches!(f.error, Error::Timeout(_)) && f.unclear());
        // RTN 08 (store or verify fault of the pack cutoff, the documented answer at address 1): definite, not open
        let f = scripted(vec![(request.clone(), b"~200146080000FDAB\r".to_vec())]).on_off_phased(1, false).unwrap_err();
        assert!(f.sent && matches!(f.error, Error::Proto(openbms_proto::Error::Rtn(0x08))) && !f.unclear());
        // the request could not be written: nothing left, not switched
        let mut c = Connection { port: Box::new(Unwritable), timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: true, rx: RxStats::default(), tap: capture::Tap::new(Box::leak(Box::new(capture::Capture::new())), "test".into()) , latency: timing::Latency::new(false), answered: Default::default(), quiet: Duration::ZERO, heard: None };
        let f = c.on_off_phased(1, true).unwrap_err();
        assert!(!f.sent && !f.unclear() && matches!(f.error, Error::Io(_)));
    }

    #[test]
    fn a_closing_gateway_still_gets_the_last_scan() {
        let read = uart1::read_request(0).unwrap();
        // the cut-off 47/A8 prefix, a complete E2, then EOF: the E2 counts
        let mut c = closing([vec![0x55, 0x00, 0x47, 0xA8], binary(0, 0xE2, &[])].concat());
        assert_eq!(c.binary_exchange(Bus::CanSocketBus, &read, Duration::from_secs(3)).unwrap(), uart1::Reply::Error(uart1::Code::Rejected));
        // a 0x47 answer cut off after a frame inside its switch bytes, then EOF: incomplete, no refusal
        let mut d = uart1_data();
        d[150..157].copy_from_slice(&binary(0, 0xE2, &[]));
        let ans = binary(0, 0x47, &d);
        let mut c = closing(ans[..161].to_vec());
        assert!(matches!(c.binary_exchange(Bus::CanSocketBus, &read, Duration::from_secs(3)), Err(Error::Incomplete)));
        // nothing at all: closed
        let mut c = closing(vec![]);
        assert!(matches!(c.binary_exchange(Bus::CanSocketBus, &read, Duration::from_secs(3)), Err(Error::Closed)));
    }

    #[test]
    fn at_the_deadline_a_cut_off_prefix_hides_no_answer_behind_it() {
        let read = uart1::read_request(0).unwrap();
        let mut c = scripted(vec![(read.clone(), [vec![0x55, 0x00, 0x47, 0xA8], binary(0, 0xE2, &[])].concat())]);
        let r = c.binary_exchange(Bus::CanSocketBus, &read, Duration::from_millis(100));
        assert_eq!(r.unwrap(), uart1::Reply::Error(uart1::Code::Rejected));
    }

    #[test]
    fn serial_numbers_of_maint_1_3_and_the_answer_of_1_2() {
        let mut raw = b"SYN-0011".to_vec();
        raw.resize(uart1::SERIAL_LEN, b' ');
        let a4 = uart1::serial_request(11, uart1::CMD_STRING2).unwrap();
        let a2 = uart1::serial_request(2, uart1::CMD_STRING1).unwrap();
        let mut c = scripted(vec![(a4.clone(), [a4.clone(), binary(11, 0xA4, &raw)].concat()), (a2, binary(2, 0x81, &[]))]);
        assert_eq!(c.uart1_serial(Bus::CanSocketBus, 11, uart1::CMD_STRING2).unwrap(), raw);
        assert!(matches!(c.uart1_serial(Bus::CanSocketBus, 2, uart1::CMD_STRING1), Err(Error::Proto(openbms_proto::Error::Uart1(0x81)))));
        assert!(matches!(c.uart1_serial(Bus::PackBus, 11, uart1::CMD_STRING2), Err(Error::Proto(openbms_proto::Error::Layout(_)))));
    }

    #[test]
    fn a_binary_echo_alone_is_a_timeout_within_one_deadline() {
        let read = uart1::read_request(1).unwrap();
        let mut c = scripted(vec![(read.clone(), read.clone())]);
        let t = Instant::now();
        let r = c.binary_exchange(Bus::CanSocketBus, &read, Duration::from_millis(150));
        assert!(matches!(r, Err(Error::Timeout(d)) if d == Duration::from_millis(150)));
        assert!(t.elapsed() < Duration::from_millis(600), "{:?}", t.elapsed());
        // foreign lines do not extend it either
        let mut p = Connection { port: Box::new(Polling { start: Instant::now(), period: Duration::from_millis(50), sent: 0 }), timeout: Duration::from_millis(50), gap: Duration::ZERO, tcp: false, rx: RxStats::default(), tap: capture::Tap::new(Box::leak(Box::new(capture::Capture::new())), "test".into()) , latency: timing::Latency::new(false), answered: Default::default(), quiet: Duration::ZERO, heard: None };
        let t = Instant::now();
        assert!(matches!(p.binary_exchange(Bus::CanSocketBus, &read, Duration::from_millis(200)), Err(Error::Timeout(_))));
        assert!(t.elapsed() < Duration::from_millis(400), "{:?}", t.elapsed());
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
