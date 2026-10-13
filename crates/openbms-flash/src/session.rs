//! Probe and flash state machine. No write is retried: whether the bootloader permits duplicate
//! writes has not been measured. Each bootloader reply is awaited before the next frame.

use crate::{boot, ehex::Image, link::Link, Error, Result};
use openbms_proto::{frame, DeviceInfo};
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
            boot_delay: Duration::from_millis(50),
            min_frame_gap: Duration::ZERO,
            ack_timeout: Duration::from_secs(10),
            post_timeout: Duration::from_secs(20),
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
    pub blocks: usize,
    pub app_returned: bool,
    pub wfd_reply_times: ReplyTimes,
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
    frame: boot::Frame,
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
    link.clear_input()?;
    let request = frame::encode_request(address, cid2, &[])?;
    send(link, log_file, &request)?;
    let deadline = Instant::now() + timeout;
    let mut pending = Vec::new();
    let mut buf = [0u8; 512];
    while Instant::now() < deadline {
        let n = receive(link, log_file, &mut buf)?;
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\r') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            if let Some(start) = line.iter().position(|b| *b == b'~') {
                if let Ok(reply) = frame::parse(&line[start..]) {
                    if reply.address == address && reply.rtn == 0 {
                        return Ok(reply);
                    }
                }
            }
        }
        if pending.len() > 4096 {
            return Err(Error::Proto("ASCII answer too long".into()));
        }
    }
    Err(Error::Timeout {
        what: format!("ASCII 0x{cid2:02X}"),
        after: timeout,
    })
}

fn read_boot(
    link: &mut impl Link,
    log_file: &mut impl Write,
    timeout: Duration,
    sent_at: Instant,
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
                    return Ok((frame, received_at.duration_since(sent_at)));
                }
            }
        } else if pending.len() > 3 {
            let keep = pending.split_off(pending.len() - 3);
            pending = keep;
        }
    }
    Err(Error::Timeout {
        what: "bootloader".into(),
        after: timeout,
    })
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
    let answer = read_boot(link, log_file, options.ack_timeout, started);
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
        (b'R' | b'W', b'F', b'B') => Ok(BootReply {
            frame: answer,
            elapsed,
        }),
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
) -> Result<DeviceInfo> {
    if address > 15 {
        return Err(Error::Address(address));
    }
    quiet(link, log_file)?;
    let ascii_timeout = options.ack_timeout.min(Duration::from_millis(600));
    let answer =
        ascii_query(link, address, 0x51, log_file, ascii_timeout).map_err(|e| match e {
            Error::Timeout { .. } => Error::NoAnswer(address),
            other => other,
        })?;
    let before = DeviceInfo::parse(&answer.info)?;
    ascii_query(link, address, 0x47, log_file, ascii_timeout).map_err(|e| match e {
        Error::Timeout { .. } => Error::NotPackBus(address),
        other => other,
    })?;
    Ok(before)
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
    let answer = boot_exchange(link, rfl, log_file, options).map_err(|e| match e {
        Error::Timeout { .. } => Error::NoBootloader(options.ack_timeout),
        other => other,
    })?;
    let answer = answer.frame;
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
        match ascii_query(link, address, 0x51, log_file, Duration::from_millis(300)) {
            Ok(reply) => return Ok(DeviceInfo::parse(&reply.info)?),
            Err(Error::Timeout { .. }) => link.wait(Duration::from_millis(100)),
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
    preflight(link, address, log_file, options)
}

/// Read-only bootloader probe. The reset does not get an ACK and no flash command is sent.
pub fn probe(
    link: &mut impl Link,
    address: u8,
    log_file: &mut impl Write,
    options: Options,
) -> Result<ProbeResult> {
    let before = preflight(link, address, log_file, options)?;
    let rfl = boot::encode(b'R', b'L', 0, &[])?;
    let packet_size = enter_boot(link, address, log_file, options, &rfl)?;
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
    let frames = dry_run(image)?;
    let total = frames.len() - 3;
    progress(Progress::Preflight);
    let before = preflight(link, address, log_file, options)?;
    progress(Progress::Bootloader);
    enter_boot(link, address, log_file, options, &frames[0])?;
    note(
        log_file,
        "sending W F E; board is in update mode until application restart",
    )?;
    progress(Progress::Erase);
    boot_exchange(link, &frames[1], log_file, options).map_err(|e| uncertain("W F E", e))?;
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
    progress(Progress::AppRestart);
    let after = wait_for_app(link, address, log_file, options)
        .map_err(|e| uncertain("application restart", e))?;
    progress(Progress::PostCheck);
    let ascii_timeout = options.ack_timeout.min(Duration::from_millis(600));
    ascii_query(link, address, 0x42, log_file, ascii_timeout)
        .map_err(|e| uncertain("0x42 post-check", e))?;
    ascii_query(link, address, 0x44, log_file, ascii_timeout)
        .map_err(|e| uncertain("0x44 post-check", e))?;
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
        blocks: total,
        app_returned: true,
        wfd_reply_times,
    })
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
}
