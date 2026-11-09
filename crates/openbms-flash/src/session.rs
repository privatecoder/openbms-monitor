//! Probe and flash state machine. No write is retried: whether the bootloader permits duplicate
//! writes has not been measured. Each bootloader reply is awaited before the next frame.

use crate::{boot, ehex::Image, link::Link, Error, Result};
use openbms_proto::{frame, DeviceInfo, Parameters, Telemetry};
use openbms_transport::timing;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
pub struct Options {
    pub boot_delay: Duration,
    pub min_frame_gap: Duration,
    pub ack_timeout: Duration,
    pub post_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            boot_delay: timing::BOOT_DELAY,
            min_frame_gap: Duration::ZERO,
            ack_timeout: timing::BOOT_ACK,
            post_timeout: timing::BOOT_COMEBACK,
        }
    }
}

pub struct ProbeResult {
    pub before: DeviceInfo,
    pub packet_size: u16,
    pub app_returned: bool,
}
pub struct FlashResult {
    pub before: DeviceInfo,
    /// 0x51 answer of the restarted application.
    pub after: DeviceInfo,
    /// The target's parameters and charge before the reset and once the restarted application has settled: for a
    /// comparison only, nothing depends on them.
    pub state_before: PackState,
    pub state_after: PackState,
    pub blocks: usize,
    pub app_returned: bool,
    pub wfd_reply_times: ReplyTimes,
    /// The first log write or flush error after the confirmed erase: the flash went on, the log is incomplete.
    pub log_error: Option<String>,
}

/// What a flash keeps of the target for the comparison before/after: its own 0x47 and 0x42 (on the pack bus, at its
/// address: the pack's local values). None: no valid answer, not compared.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackState {
    pub parameters: Option<Parameters>,
    pub telemetry: Option<Telemetry>,
}

/// The log of a flash: until the erase is confirmed a write or flush error ends the flash like any other (the board
/// still has its application). From then on (`lenient`) such an error no longer stops the bootloader steps, which
/// would leave the board without an application: the first one is kept for the result, the steps go on.
struct FlashLog<'a, W: Write> {
    inner: &'a mut W,
    lenient: bool,
    error: Option<std::io::Error>,
}

impl<W: Write> FlashLog<'_, W> {
    fn keep(&mut self, e: std::io::Error) -> std::io::Result<()> {
        if !self.lenient {
            return Err(e);
        }
        self.error.get_or_insert(e);
        Ok(())
    }
}

impl<W: Write> Write for FlashLog<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.inner.write(buf) {
            Err(e) => self.keep(e).map(|_| buf.len()),
            ok => ok,
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.inner.flush() {
            Err(e) => self.keep(e),
            ok => ok,
        }
    }
}

/// Stage of a running flash, reported to the caller (the app shows it as progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Preflight,
    Bootloader,
    Erase,
    Block { index: usize, total: usize },
    Finalize,
    AppRestart,
    PostCheck,
}

#[derive(Debug, Clone, Copy)]
pub struct ReplyTimes {
    pub min: Duration,
    pub median: Duration,
    pub max: Duration,
}

impl ReplyTimes {
    fn from_samples(mut samples: Vec<Duration>) -> Result<Self> {
        samples.sort_unstable();
        let count = samples.len();
        let min = *samples
            .first()
            .ok_or_else(|| Error::Proto("no W F D replies".into()))?;
        let max = *samples.last().unwrap();
        let median = if count.is_multiple_of(2) {
            (samples[count / 2 - 1] + samples[count / 2]) / 2
        } else {
            samples[count / 2]
        };
        Ok(Self { min, median, max })
    }
}

struct BootReply {
    elapsed: Duration,
}

fn log(log: &mut impl Write, label: &str, bytes: &[u8]) -> Result<()> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    writeln!(
        log,
        "{ms} {label} {}",
        bytes.iter().map(|b| format!("{b:02X}")).collect::<String>()
    )?;
    log.flush()?;
    Ok(())
}

fn note(log: &mut impl Write, message: &str) -> Result<()> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    writeln!(log, "{ms} STATE {message}")?;
    log.flush()?;
    Ok(())
}

fn send(link: &mut impl Link, log_file: &mut impl Write, bytes: &[u8]) -> Result<()> {
    log(log_file, "TX", bytes)?;
    link.write_raw(bytes)
}

fn receive(link: &mut impl Link, log_file: &mut impl Write, buf: &mut [u8]) -> Result<usize> {
    let n = link.read_raw(buf)?;
    if n > 0 {
        log(log_file, "RX", &buf[..n])?;
    }
    Ok(n)
}

pub(crate) fn ascii_query(
    link: &mut impl Link,
    address: u8,
    cid2: u8,
    log_file: &mut impl Write,
    timeout: Duration,
) -> Result<frame::Frame> {
    let request = frame::encode_request(address, cid2, &[])?;
    ascii_exchange(link, address, &request, log_file, timeout)
}

/// One ASCII request and the first valid answer from `address` within `timeout`.
fn ascii_exchange(
    link: &mut impl Link,
    address: u8,
    request: &[u8],
    log_file: &mut impl Write,
    timeout: Duration,
) -> Result<frame::Frame> {
    link.clear_input()?;
    send(link, log_file, request)?;
    let deadline = Instant::now() + timeout;
    let mut lines = frame::Lines::default();
    let mut buf = [0u8; 512];
    while Instant::now() < deadline {
        let n = receive(link, log_file, &mut buf)?;
        lines.push(&buf[..n]);
        while let Some(line) = lines.next_line() {
            if let Ok(reply) = frame::parse(&line.text) {
                if reply.address == address && reply.rtn == 0 {
                    return Ok(reply);
                }
            }
        }
        if lines.unterminated() > 4096 {
            return Err(Error::Proto("ASCII answer too long".into()));
        }
    }
    Err(Error::Timeout {
        what: format!("ASCII 0x{:02X}", frame::parse(request).map(|f| f.rtn).unwrap_or(0)),
        after: timeout,
    })
}

/// The next bootloader frame within `timeout`. `skip_rfb`: an R F B is a late answer to an R F L probe of the entry,
/// never the answer to a write: it is dropped and the wait goes on (frames that came with it are kept).
fn read_boot(
    link: &mut impl Link,
    log_file: &mut impl Write,
    timeout: Duration,
    sent_at: Instant,
    skip_rfb: bool,
) -> Result<(boot::Frame, Duration)> {
    let deadline = Instant::now() + timeout;
    let mut pending = Vec::new();
    let mut buf = [0u8; 512];
    while Instant::now() < deadline {
        let n = link.read_raw(&mut buf)?;
        let received_at = Instant::now();
        if n > 0 {
            log(log_file, "RX", &buf[..n])?;
        }
        pending.extend_from_slice(&buf[..n]);
        if let Some(start) = pending.windows(4).position(|w| w == b"::::") {
            if start > 0 {
                pending.drain(..start);
            }
            if pending.len() >= 9 {
                let len = u16::from_be_bytes([pending[7], pending[8]]) as usize;
                if len > 1024 {
                    return Err(Error::Proto(format!(
                        "boot answer length {len} is excessive"
                    )));
                }
                if pending.len() >= len + 17 {
                    let frame = boot::decode(&pending[..len + 17])?;
                    if skip_rfb && (frame.cid1, frame.cid2, frame.cid3) == (b'R', b'F', b'B') {
                        pending.drain(..len + 17);
                        note(log_file, "late R F B (an earlier R F L probe) skipped")?;
                        continue;
                    }
                    return Ok((frame, received_at.duration_since(sent_at)));
                }
            }
        } else if pending.len() > 3 {
            // Only noise before "::::" is trimmed; a started frame stays
            // until the deadline, even across quiet reads.
            let keep = pending.split_off(pending.len() - 3);
            pending = keep;
        }
    }
    Err(Error::Timeout {
        what: "bootloader".into(),
        after: timeout,
    })
}

/// R F L probes after the one reset until the bootloader answers, within `ack_timeout` in all: a next probe only after
/// `RFL_PROBE` without any byte of a bootloader answer begun (a begun one is collected, never answered twice). A MAINT
/// 1.6.3 saves the SOC before it resets, so the first probe may come too early. An ASCII answer to the reset with an RTN
/// other than 00 (1.6.3: RTN 08, the SOC could not be saved, no restart) ends it: nothing is flashed, nothing repeated.
fn boot_entry(link: &mut impl Link, address: u8, reset: &[u8], rfl: &[u8], log_file: &mut impl Write, options: Options) -> Result<boot::Frame> {
    let deadline = Instant::now() + options.ack_timeout;
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];
    let mut probes = 0u32;
    loop {
        probes += 1;
        log(log_file, "TX", rfl)?;
        let sent = Instant::now();
        link.write_raw(rfl)?;
        let next = sent + timing::RFL_PROBE;
        loop {
            let n = link.read_raw(&mut buf)?;
            if n > 0 {
                log(log_file, "RX", &buf[..n])?;
                pending.extend_from_slice(&buf[..n]);
            }
            // an ASCII answer to the reset before any bootloader frame: RTN 00 changes nothing, another one refuses the
            // restart (an echo of the reset itself is skipped)
            while let Some(end) = pending.iter().position(|b| *b == b'\r') {
                if pending.windows(4).position(|w| w == b"::::").is_some_and(|s| s < end) {
                    break;
                }
                let line: Vec<u8> = pending.drain(..=end).collect();
                let Some(start) = line.iter().position(|b| *b == b'~') else { continue };
                if line[start..] == *reset {
                    continue;
                }
                if let Ok(reply) = frame::parse(&line[start..]) {
                    if reply.address == address && reply.rtn != 0 {
                        return Err(Error::ResetRefused(reply.rtn));
                    }
                }
            }
            if let Some(start) = pending.windows(4).position(|w| w == b"::::") {
                pending.drain(..start);
                if pending.len() >= 9 {
                    let len = u16::from_be_bytes([pending[7], pending[8]]) as usize;
                    if len > 1024 {
                        return Err(Error::Proto(format!("boot answer length {len} is excessive")));
                    }
                    if pending.len() >= len + 17 {
                        let answer = boot::decode(&pending[..len + 17])?;
                        note(log_file, &format!("TIMING command=RFL response={}{}{} response_ms={:.1}", answer.cid1 as char, answer.cid2 as char, answer.cid3 as char, sent.elapsed().as_secs_f64() * 1000.0))?;
                        if !options.min_frame_gap.is_zero() {
                            link.wait(options.min_frame_gap);
                        }
                        // a later answer to an earlier probe must not be taken for the answer to W F E
                        if probes > 1 {
                            link.wait(2 * timing::RFL_PROBE);
                            link.clear_input()?;
                            note(log_file, &format!("boot entry after {probes} R F L probes; later answers dropped"))?;
                        }
                        return match (answer.cid1, answer.cid2, answer.cid3) {
                            (b'R', b'F', b'B') => Ok(answer),
                            (b'R', b'F', b'N') => Err(Error::Rejected),
                            _ => Err(Error::Unexpected(format!("{}{}{}", answer.cid1 as char, answer.cid2 as char, answer.cid3 as char))),
                        };
                    }
                }
            }
            if pending.len() > 4096 {
                return Err(Error::Proto("boot entry answer too long".into()));
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::NoBootloader(options.ack_timeout));
            }
            // a begun bootloader answer (or the first colons of one) is collected before another probe
            let begun = pending.windows(4).any(|w| w == b"::::") || pending.ends_with(b":");
            if now >= next && !begun {
                break;
            }
        }
    }
}

fn boot_exchange(
    link: &mut impl Link,
    bytes: &[u8],
    log_file: &mut impl Write,
    options: Options,
) -> Result<BootReply> {
    link.clear_input()?;
    log(log_file, "TX", bytes)?;
    let started = Instant::now();
    link.write_raw(bytes)?;
    boot_answer(link, bytes, log_file, options, started)
}

/// The bootloader's answer to `bytes`, sent at `started`: read, timed, checked.
fn boot_answer(
    link: &mut impl Link,
    bytes: &[u8],
    log_file: &mut impl Write,
    options: Options,
    started: Instant,
) -> Result<BootReply> {
    // every frame answered here is a write (W F E, W F D, W F G): only an empty W F B at address 0 acknowledges it
    let answer = read_boot(link, log_file, options.ack_timeout, started, true);
    if let Ok((reply, elapsed)) = &answer {
        let command = std::str::from_utf8(&bytes[4..7]).unwrap_or("???");
        let response = format!(
            "{}{}{}",
            reply.cid1 as char, reply.cid2 as char, reply.cid3 as char
        );
        note(
            log_file,
            &format!(
                "TIMING command={command} response={response} response_ms={:.1}",
                elapsed.as_secs_f64() * 1000.0
            ),
        )?;
    }
    // The measured bootloader ACK means it is ready for the next frame.
    if !options.min_frame_gap.is_zero() {
        link.wait(options.min_frame_gap);
    }
    let (answer, elapsed) = answer?;
    match (answer.cid1, answer.cid2, answer.cid3) {
        (b'W', b'F', b'B') if answer.address == 0 && answer.data.is_empty() => Ok(BootReply { elapsed }),
        (b'R', b'F', b'N') => Err(Error::Rejected),
        _ => Err(Error::Unexpected(format!(
            "{}{}{}",
            answer.cid1 as char, answer.cid2 as char, answer.cid3 as char
        ))),
    }
}

pub(crate) fn quiet(link: &mut impl Link, log_file: &mut impl Write) -> Result<()> {
    let mut buf = [0u8; 512];
    let mut count = 0;
    // Observe roughly two seconds to catch an active master polling the pack bus.
    for _ in 0..20 {
        count += receive(link, log_file, &mut buf)?;
    }
    if count > 0 {
        Err(Error::BusBusy(count))
    } else {
        Ok(())
    }
}

fn preflight(
    link: &mut impl Link,
    address: u8,
    log_file: &mut impl Write,
    options: Options,
) -> Result<(DeviceInfo, Option<Parameters>)> {
    if !openbms_proto::address::is_dip(address) {
        return Err(Error::Address(address));
    }
    quiet(link, log_file)?;
    let ascii_timeout = options.ack_timeout.min(timing::FLASH_ASCII);
    let answer =
        ascii_query(link, address, 0x51, log_file, ascii_timeout).map_err(|e| match e {
            Error::Timeout { .. } => Error::NoAnswer(address),
            other => other,
        })?;
    let before = DeviceInfo::parse(&answer.info)?;
    let params = ascii_query(link, address, 0x47, log_file, ascii_timeout).map_err(|e| match e {
        Error::Timeout { .. } => Error::NotPackBus(address),
        other => other,
    })?;
    Ok((before, Parameters::parse(&params.info).ok()))
}

/// The pack's own 0x42, for the comparison only: no answer or another layout is None, never a failure.
fn read_telemetry(link: &mut impl Link, address: u8, log_file: &mut impl Write, timeout: Duration) -> Result<Option<Telemetry>> {
    let request = Telemetry::request(address)?;
    match ascii_exchange(link, address, &request, log_file, timeout) {
        Ok(reply) => Ok(Telemetry::parse(&reply.info).ok()),
        Err(Error::Timeout { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Parameters and charge once the restarted application has settled (`timing::FLASH_SETTLE` after its first answer:
/// before that the stored SOC may not be loaded yet). Only a link error ends it; a missing answer is None.
fn settled_state(
    link: &mut impl Link,
    address: u8,
    log_file: &mut impl Write,
    timeout: Duration,
    back: Instant,
) -> Result<PackState> {
    link.wait(timing::FLASH_SETTLE.saturating_sub(back.elapsed()));
    let parameters = match ascii_query(link, address, 0x47, log_file, timeout) {
        Ok(reply) => Parameters::parse(&reply.info).ok(),
        Err(Error::Timeout { .. }) => None,
        Err(e) => return Err(e),
    };
    let telemetry = read_telemetry(link, address, log_file, timeout)?;
    Ok(PackState { parameters, telemetry })
}

fn enter_boot(
    link: &mut impl Link,
    address: u8,
    log_file: &mut impl Write,
    options: Options,
    rfl: &[u8],
) -> Result<u16> {
    link.clear_input()?;
    let reset = frame::encode_request_version(0x26, address, 0x45, &[1, 6, 0])?;
    send(link, log_file, &reset)?;
    link.wait(options.boot_delay);
    // from R F L on the frame capture leaves the bootloader out; the caller takes the link in again
    link.pause_capture(true);
    let answer = boot_entry(link, address, &reset, rfl, log_file, options)?;
    if answer.cid1 != b'R' || answer.address != 0 || answer.data.len() != 2 {
        return Err(Error::Unexpected(
            "R F L did not return a two-byte packet size".into(),
        ));
    }
    let packet_size = u16::from_be_bytes([answer.data[0], answer.data[1]]);
    if packet_size != 256 {
        return Err(Error::PacketSize(u32::from(packet_size)));
    }
    Ok(packet_size)
}

fn wait_for_app(
    link: &mut impl Link,
    address: u8,
    log_file: &mut impl Write,
    options: Options,
) -> Result<DeviceInfo> {
    let deadline = Instant::now() + options.post_timeout;
    while Instant::now() < deadline {
        match ascii_query(link, address, 0x51, log_file, timing::FLASH_REACH_TRY) {
            Ok(reply) => return Ok(DeviceInfo::parse(&reply.info)?),
            Err(Error::Timeout { .. }) => link.wait(timing::FLASH_REACH_PAUSE),
            Err(other) => return Err(other),
        }
    }
    Err(Error::Timeout {
        what: "application restart".into(),
        after: options.post_timeout,
    })
}

/// Inspect the target and confirm that command 0x47 works on this bus, without resetting it.
pub fn inspect(
    link: &mut impl Link,
    address: u8,
    log_file: &mut impl Write,
    options: Options,
) -> Result<DeviceInfo> {
    preflight(link, address, log_file, options).map(|(info, _)| info)
}

/// Read-only bootloader probe. The reset does not get an ACK and no flash command is sent.
pub fn probe(
    link: &mut impl Link,
    address: u8,
    log_file: &mut impl Write,
    options: Options,
) -> Result<ProbeResult> {
    let (before, _) = preflight(link, address, log_file, options)?;
    let rfl = boot::encode(b'R', b'L', 0, &[])?;
    let entered = enter_boot(link, address, log_file, options, &rfl);
    link.pause_capture(false);
    let packet_size = entered?;
    note(log_file, "probe complete; waiting for application")?;
    wait_for_app(link, address, log_file, options)?;
    Ok(ProbeResult {
        before,
        packet_size,
        app_returned: true,
    })
}

/// Generate exactly the vendor sequence after the ASCII reset. Data blocks are always 256 bytes,
/// padded with zero, and use an address increment of 16 per block.
pub fn dry_run(image: &Image) -> Result<Vec<Vec<u8>>> {
    let stream = image.stream();
    let mut frames = vec![
        boot::encode(b'R', b'L', 0, &[])?,
        boot::encode(b'W', b'E', 0, &[])?,
    ];
    for (index, chunk) in stream.chunks(256).enumerate() {
        let addr = index
            .checked_mul(16)
            .and_then(|n| u16::try_from(n).ok())
            .ok_or_else(|| Error::Image("too many blocks for 16-bit address".into()))?;
        let mut data = [0u8; 256];
        data[..chunk.len()].copy_from_slice(chunk);
        frames.push(boot::encode(b'W', b'D', addr, &data)?);
    }
    frames.push(boot::encode(b'W', b'G', 0, &[])?);
    Ok(frames)
}

fn uncertain(stage: &str, error: Error) -> Error {
    Error::Unexpected(format!(
        "{stage}: {error}; the board may be in update mode; do not retry a block blindly"
    ))
}

/// The bootloader part of a flash: R F L, W F E, the data blocks, W F G. Returns the W F D reply times. From the
/// moment W F E is sent, log errors no longer stop it (`FlashLog`), its ACK's RX and timing lines included.
fn boot_phase<W: Write>(
    link: &mut impl Link,
    address: u8,
    frames: &[Vec<u8>],
    log_file: &mut FlashLog<'_, W>,
    options: Options,
    progress: &mut dyn FnMut(Progress),
) -> Result<Vec<Duration>> {
    progress(Progress::Bootloader);
    enter_boot(link, address, log_file, options, &frames[0])?;
    program(link, frames, log_file, options, progress)
}

/// W F E, the data blocks and W F G to a bootloader that has answered R F L: shared by the flash and the recovery. From
/// the moment W F E is sent, log errors no longer stop it (`FlashLog`). Nothing is repeated.
fn program<W: Write>(
    link: &mut impl Link,
    frames: &[Vec<u8>],
    log_file: &mut FlashLog<'_, W>,
    options: Options,
    progress: &mut dyn FnMut(Progress),
) -> Result<Vec<Duration>> {
    let total = frames.len() - 3;
    note(
        log_file,
        "sending W F E; board is in update mode until application restart",
    )?;
    progress(Progress::Erase);
    // W F E as `boot_exchange`, but tolerant of log errors once the frame goes out
    link.clear_input().map_err(|e| uncertain("W F E", e))?;
    log(log_file, "TX", &frames[1])?;
    log_file.lenient = true;
    let started = Instant::now();
    link.write_raw(&frames[1]).map_err(|e| uncertain("W F E", e))?;
    boot_answer(link, &frames[1], log_file, options, started).map_err(|e| uncertain("W F E", e))?;
    let mut wfd_times = Vec::with_capacity(total);
    for (index, frame) in frames[2..frames.len() - 1].iter().enumerate() {
        progress(Progress::Block { index, total });
        let reply = boot_exchange(link, frame, log_file, options)
            .map_err(|e| uncertain(&format!("W F D block {index}"), e))?;
        wfd_times.push(reply.elapsed);
    }
    progress(Progress::Finalize);
    boot_exchange(link, frames.last().unwrap(), log_file, options)
        .map_err(|e| uncertain("W F G", e))?;
    Ok(wfd_times)
}

/// Flash after the caller has validated the vendor image and obtained explicit operator consent.
/// Every frame is precomputed before reset, including the unverified-checksum guard.
pub fn flash(
    link: &mut impl Link,
    address: u8,
    image: &Image,
    log_file: &mut impl Write,
    options: Options,
) -> Result<FlashResult> {
    flash_with_progress(link, address, image, log_file, options, &mut |_| {})
}

/// Like [`flash`], reporting each stage before it starts. Frames and timing are identical.
pub fn flash_with_progress(
    link: &mut impl Link,
    address: u8,
    image: &Image,
    log_file: &mut impl Write,
    options: Options,
    progress: &mut dyn FnMut(Progress),
) -> Result<FlashResult> {
    let mut log = FlashLog { inner: log_file, lenient: false, error: None };
    let r = flash_steps(link, address, image, &mut log, options, progress);
    let log_error = log.error.map(|e| e.to_string());
    match (r, log_error) {
        (Ok(r), log_error) => Ok(FlashResult { log_error, ..r }),
        (Err(Error::Unexpected(m)), Some(le)) => Err(Error::Unexpected(format!("{m}; log write failed: {le}"))),
        (Err(e), _) => Err(e),
    }
}

fn flash_steps<W: Write>(
    link: &mut impl Link,
    address: u8,
    image: &Image,
    log_file: &mut FlashLog<'_, W>,
    options: Options,
    progress: &mut dyn FnMut(Progress),
) -> Result<FlashResult> {
    let frames = dry_run(image)?;
    let total = frames.len() - 3;
    progress(Progress::Preflight);
    let (before, parameters) = preflight(link, address, log_file, options)?;
    let ascii_timeout = options.ack_timeout.min(timing::FLASH_ASCII);
    let state_before = PackState { parameters, telemetry: read_telemetry(link, address, log_file, ascii_timeout)? };
    // R F L up to and including W F G stay out of a frame capture, also when one of them fails
    let wfd_times = boot_phase(link, address, &frames, log_file, options, progress);
    link.pause_capture(false);
    let wfd_times = wfd_times?;
    progress(Progress::AppRestart);
    let after = wait_for_app(link, address, log_file, options)
        .map_err(|e| uncertain("application restart", e))?;
    let back = Instant::now();
    progress(Progress::PostCheck);
    ascii_query(link, address, 0x42, log_file, ascii_timeout)
        .map_err(|e| uncertain("0x42 post-check", e))?;
    ascii_query(link, address, 0x44, log_file, ascii_timeout)
        .map_err(|e| uncertain("0x44 post-check", e))?;
    // the flash is done here: a lost link while reading the state leaves the comparison out, not the result
    let state_after = settled_state(link, address, log_file, ascii_timeout, back).unwrap_or_default();
    let wfd_reply_times = ReplyTimes::from_samples(wfd_times)?;
    note(
        log_file,
        &format!(
            "WFD response_ms min={:.1} median={:.1} max={:.1}",
            wfd_reply_times.min.as_secs_f64() * 1000.0,
            wfd_reply_times.median.as_secs_f64() * 1000.0,
            wfd_reply_times.max.as_secs_f64() * 1000.0,
        ),
    )?;
    note(
        log_file,
        "flash complete; application answered 0x51, 0x42 and 0x44",
    )?;
    Ok(FlashResult {
        before,
        after,
        state_before,
        state_after,
        blocks: total,
        app_returned: true,
        wfd_reply_times,
        log_error: None,
    })
}

/// The result of a recovery: no "before" exists (the board had no application), the application is looked for at every
/// DIP address afterwards.
pub struct RecoveryResult {
    pub packet_size: u16,
    pub blocks: usize,
    /// the address and 0x51 of the restarted application; None: none answered within `post_timeout` (a master polling
    /// its slaves on this bus does not answer, for example): the transfer is confirmed, the application unchecked
    pub found: Option<(u8, DeviceInfo)>,
    pub wfd_reply_times: ReplyTimes,
    pub log_error: Option<String>,
}

/// Whether a bootloader already answers on this link, without any reset or write: one R F L. Only a single complete
/// R F B at bootloader address 0 with the 256-byte packet size counts; anything after it (a second bootloader, other
/// traffic) refuses, because the bootloader has no pack address and every bootloader on the line would be erased.
pub fn probe_bootloader(link: &mut impl Link, log_file: &mut impl Write, options: Options) -> Result<u16> {
    quiet(link, log_file)?;
    let rfl = boot::encode(b'R', b'L', 0, &[])?;
    link.clear_input()?;
    log(log_file, "TX", &rfl)?;
    let sent = Instant::now();
    link.write_raw(&rfl)?;
    // everything received is kept: a gateway may pack two answers (or an answer and other bytes) into one read
    let timeout = options.ack_timeout.min(timing::FLASH_ASCII);
    let mut got: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];
    let frame_len = loop {
        let n = link.read_raw(&mut buf)?;
        if n > 0 {
            log(log_file, "RX", &buf[..n])?;
            got.extend_from_slice(&buf[..n]);
        }
        if let Some(start) = got.windows(4).position(|w| w == b"::::") {
            if start > 0 {
                return Err(Error::Unexpected(format!("{start} bytes before the bootloader answer: more than one board on the line?")));
            }
            if got.len() >= 9 {
                let len = u16::from_be_bytes([got[7], got[8]]) as usize;
                if len > 1024 {
                    return Err(Error::Proto(format!("boot answer length {len} is excessive")));
                }
                if got.len() >= len + 17 {
                    break len + 17;
                }
            }
        } else if got.len() > 4 && !got.ends_with(b":") {
            return Err(Error::Unexpected("bytes without a bootloader answer: more than one board or traffic on the line?".into()));
        }
        if got.len() > 4096 {
            return Err(Error::Proto("bootloader answer too long".into()));
        }
        if sent.elapsed() >= timeout {
            return Err(Error::NoBootloader(timeout));
        }
    };
    let answer = boot::decode(&got[..frame_len])?;
    if (answer.cid1, answer.cid2, answer.cid3) != (b'R', b'F', b'B') || answer.address != 0 || answer.data.len() != 2 {
        return Err(Error::Unexpected(format!("{}{}{} is no bootloader answer", answer.cid1 as char, answer.cid2 as char, answer.cid3 as char)));
    }
    let packet_size = u16::from_be_bytes([answer.data[0], answer.data[1]]);
    if packet_size != 256 {
        return Err(Error::PacketSize(u32::from(packet_size)));
    }
    // a second answer or any other byte, in the same read or after it: more than one board (or traffic) on the line
    let mut extra = got.len() - frame_len;
    for _ in 0..3 {
        extra += receive(link, log_file, &mut buf)?;
    }
    if extra > 0 {
        return Err(Error::Unexpected(format!("{extra} more bytes after the bootloader answer: more than one board on the line?")));
    }
    note(log_file, "bootloader answers R F B with 256-byte packets")?;
    Ok(packet_size)
}

/// The application after a recovery: 0x51 at every DIP address in turn until one answers, within `post_timeout`.
fn find_app(link: &mut impl Link, log_file: &mut impl Write, options: Options) -> Result<Option<(u8, DeviceInfo)>> {
    let deadline = Instant::now() + options.post_timeout;
    while Instant::now() < deadline {
        for address in openbms_proto::address::DIP {
            match ascii_query(link, address, 0x51, log_file, timing::FLASH_REACH_TRY) {
                Ok(reply) => return Ok(Some((address, DeviceInfo::parse(&reply.info)?))),
                Err(Error::Timeout { .. }) => {}
                Err(other) => return Err(other),
            }
        }
        link.wait(timing::FLASH_REACH_PAUSE);
    }
    Ok(None)
}

/// Recover a board whose bootloader already answers (no application to reset): R F L once more on this link, then the
/// same W F E, data blocks and W F G as a flash. Never a reset (0x45), never a 0x51/0x47 precondition, no retries. The
/// caller has confirmed that exactly one board is on the line.
pub fn recover_with_progress(
    link: &mut impl Link,
    image: &Image,
    log_file: &mut impl Write,
    options: Options,
    progress: &mut dyn FnMut(Progress),
) -> Result<RecoveryResult> {
    let mut log = FlashLog { inner: log_file, lenient: false, error: None };
    let r = recover_steps(link, image, &mut log, options, progress);
    let log_error = log.error.map(|e| e.to_string());
    match (r, log_error) {
        (Ok(r), log_error) => Ok(RecoveryResult { log_error, ..r }),
        (Err(Error::Unexpected(m)), Some(le)) => Err(Error::Unexpected(format!("{m}; log write failed: {le}"))),
        (Err(e), _) => Err(e),
    }
}

fn recover_steps<W: Write>(
    link: &mut impl Link,
    image: &Image,
    log_file: &mut FlashLog<'_, W>,
    options: Options,
    progress: &mut dyn FnMut(Progress),
) -> Result<RecoveryResult> {
    let frames = dry_run(image)?;
    let blocks = frames.len() - 3;
    progress(Progress::Bootloader);
    link.pause_capture(true);
    let programmed = probe_bootloader(link, log_file, options).and_then(|packet_size| {
        program(link, &frames, log_file, options, progress).map(|times| (packet_size, times))
    });
    link.pause_capture(false);
    let (packet_size, times) = programmed?;
    progress(Progress::AppRestart);
    // the transfer is done: an application that does not show is no failure of it
    let found = find_app(link, log_file, options).unwrap_or(None);
    let wfd_reply_times = ReplyTimes::from_samples(times)?;
    match &found {
        Some((address, _)) => note(log_file, &format!("recovery complete; application answered 0x51 at address {address}"))?,
        None => note(log_file, "recovery transfer complete; no application answered 0x51 (unchecked)")?,
    }
    Ok(RecoveryResult { packet_size, blocks, found, wfd_reply_times, log_error: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_time_summary_uses_middle_sample_or_average_of_middle_pair() {
        let ms = Duration::from_millis;
        let odd = ReplyTimes::from_samples(vec![ms(233), ms(153), ms(156)]).unwrap();
        assert_eq!((odd.min, odd.median, odd.max), (ms(153), ms(156), ms(233)));
        let even = ReplyTimes::from_samples(vec![ms(233), ms(153), ms(230), ms(156)]).unwrap();
        assert_eq!(
            (even.min, even.median, even.max),
            (ms(153), ms(193), ms(233))
        );
    }

    /// Hands out scripted chunks; an empty chunk stands for a quiet 100-ms read.
    struct Chunks(std::collections::VecDeque<Vec<u8>>);

    impl Link for Chunks {
        fn clear_input(&mut self) -> Result<()> {
            Ok(())
        }
        fn write_raw(&mut self, _: &[u8]) -> Result<()> {
            Ok(())
        }
        fn read_raw(&mut self, out: &mut [u8]) -> Result<usize> {
            let chunk = self.0.pop_front().unwrap_or_default();
            out[..chunk.len()].copy_from_slice(&chunk);
            Ok(chunk.len())
        }
        fn wait(&mut self, _: Duration) {}
    }

    fn read_chunks(chunks: Vec<Vec<u8>>) -> Result<boot::Frame> {
        let mut link = Chunks(chunks.into());
        let started = Instant::now();
        read_boot(
            &mut link,
            &mut std::io::sink(),
            Duration::from_millis(200),
            started,
            false,
        )
        .map(|(frame, _)| frame)
    }

    #[test]
    fn a_late_rfb_is_never_taken_for_the_answer_to_a_write() {
        let rfb = boot::encode(b'R', b'B', 0, &[1, 0]).unwrap();
        let wfb = boot::encode(b'W', b'B', 0, &[]).unwrap();
        // both in one read: the W F B behind the late R F B is kept
        let mut link = Chunks(vec![[rfb.clone(), wfb.clone()].concat()].into());
        let (frame, _) = read_boot(&mut link, &mut std::io::sink(), Duration::from_millis(200), Instant::now(), true).unwrap();
        assert_eq!((frame.cid1, frame.cid3), (b'W', b'B'));
        // an R F B alone is no acknowledgement: the wait ends without one
        let mut link = Chunks(vec![rfb].into());
        assert!(read_boot(&mut link, &mut std::io::sink(), Duration::from_millis(100), Instant::now(), true).is_err());
    }

    #[test]
    fn boot_reply_survives_quiet_reads_between_its_fragments() {
        let reply = boot::encode(b'R', b'B', 0, &[1, 0]).unwrap();
        let mut head = b"noise".to_vec();
        head.extend_from_slice(&reply[..10]);
        let frame = read_chunks(vec![head, Vec::new(), Vec::new(), reply[10..].to_vec()]).unwrap();
        assert_eq!((frame.cid1, frame.cid3), (b'R', b'B'));
    }

    #[test]
    fn boot_header_split_inside_the_colons_is_kept_across_quiet_reads() {
        let reply = boot::encode(b'W', b'B', 0, &[]).unwrap();
        let mut head = b"xyz".to_vec();
        head.extend_from_slice(&reply[..3]);
        let frame = read_chunks(vec![head, Vec::new(), reply[3..].to_vec()]).unwrap();
        assert_eq!((frame.cid1, frame.cid3), (b'W', b'B'));
    }
}
