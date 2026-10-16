//! Frame capture under Diagnosis: start, stop, clear, status and the text export of `openbms_transport::capture`.
//! The export decodes frame headers only (address, command, RTN, lengths), never values.

use chrono::{Local, Offset, TimeZone};
use openbms_flash::Lang;
use openbms_proto::{frame, modbus};
use openbms_transport::capture::{self, Full, Kind, Line, Snapshot, GLOBAL};
use openbms_transport::{Bus, Endpoint};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::Write as _;

/// The capture label of a connection: endpoint and bus as the UI names them.
pub(crate) fn label(endpoint: &Endpoint, bus: Bus) -> String {
    let bus = match bus {
        Bus::PackBus => "RS485-1/2",
        Bus::CanSocketBus => "CAN-Buchse",
    };
    match endpoint {
        Endpoint::Serial { path, .. } => format!("USB {path} · {bus}"),
        Endpoint::Tcp { addr } => format!("TCP {addr} · {bus}"),
    }
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct CaptureStatus {
    running: bool,
    entries: usize,
    bytes: usize,
    timeouts: usize,
    span_ms: u64,
    /// "entries" or "bytes": the capture stopped itself at that limit
    full: Option<&'static str>,
}

fn full_key(f: Full) -> &'static str {
    match f {
        Full::Entries => "entries",
        Full::Bytes => "bytes",
    }
}

impl From<capture::Status> for CaptureStatus {
    fn from(s: capture::Status) -> Self {
        CaptureStatus { running: s.running, entries: s.entries, bytes: s.bytes, timeouts: s.timeouts, span_ms: s.span_ms, full: s.full.map(full_key) }
    }
}

#[tauri::command]
pub fn capture_start() -> CaptureStatus {
    GLOBAL.start().into()
}

#[tauri::command]
pub fn capture_stop() -> CaptureStatus {
    GLOBAL.stop().into()
}

#[tauri::command]
pub fn capture_clear() -> CaptureStatus {
    GLOBAL.clear().into()
}

#[tauri::command]
pub fn capture_status() -> CaptureStatus {
    GLOBAL.status().into()
}

/// Write the capture as it is now to `path` (from the save dialog). Only the snapshot (pointer copies) is taken under
/// the capture's lock; formatting and writing run on a background thread. Returns the number of entries.
#[tauri::command]
pub async fn capture_export(path: String, lang: String) -> Result<usize, String> {
    let p = std::path::PathBuf::from(&path);
    if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("txt")) != Some(true) {
        return Err(format!("not a .txt file: {path}"));
    }
    let snap = GLOBAL.snapshot();
    let l = if lang.starts_with("de") { Lang::De } else { Lang::En };
    tauri::async_runtime::spawn_blocking(move || {
        let text = export_text(&snap, env!("CARGO_PKG_VERSION"), l, &Local);
        std::fs::write(&p, text).map_err(|e| format!("{path}: {e}"))?;
        Ok(snap.entries.len())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The last request sent on a connection, to read its answers against.
#[derive(Clone, Copy)]
enum Req {
    Ascii { address: u8, cid2: u8, info0: Option<u8>, info1: Option<u8> },
    Modbus,
    Other,
}

struct Row {
    address: String,
    command: String,
    data: String,
    meaning: String,
}

/// „12 345“ (DE) or „12,345“ (EN).
fn grouped(n: usize, l: Lang) -> String {
    let digits = n.to_string();
    let sep = l.pick(" ", ",");
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push_str(sep);
        }
        out.push(c);
    }
    out
}

/// ASCII frames as their text with `\r` shown as `\r`; anything not printable as spaced hex bytes.
fn data_text(bytes: &[u8]) -> String {
    if bytes.iter().all(|b| (0x20..0x7F).contains(b) || *b == b'\r' || *b == b'\n') {
        bytes.iter().map(|b| match b {
            b'\r' => "\\r".to_string(),
            b'\n' => "\\n".to_string(),
            _ => (*b as char).to_string(),
        }).collect()
    } else {
        bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
    }
}

/// The command name of an ASCII request (the commands the app sends, the master's intra-pack poll).
fn command_name(cid2: u8, info0: Option<u8>, info1: Option<u8>, l: Lang) -> String {
    let p = |de: &str, en: &str| l.pick(de, en).to_string();
    let name = match cid2 {
        0x42 => p("Telemetrie lesen", "read telemetry"),
        0x44 => p("Alarme und Status lesen", "read alarms and status"),
        0x45 => {
            let kind = match info1 {
                Some(3) => l.pick("Heizung", "heater"),
                Some(4) => l.pick("Ein/Aus", "on/off"),
                Some(5) => l.pick("Verlauf löschen", "clear history"),
                Some(6) => l.pick("Neustart", "restart"),
                _ => "",
            };
            let t = info1.map(|t| format!(" {} {t}", l.pick("Typ", "type"))).unwrap_or_default();
            let base = l.pick("Pack-Steuerung", "pack control");
            return if kind.is_empty() { format!("{base} (0x45{t})") } else { format!("{base}: {kind} (0x45{t})") };
        }
        0x47 => p("Parameter lesen", "read parameters"),
        0x49 if info1 == Some(59) => p("Restkapazität setzen", "set remaining capacity"),
        0x49 => p("Wert setzen", "set value"),
        0x4B => p("Verlauf lesen", "read history record"),
        0x4D => p("Uhr lesen", "read clock"),
        0x4E => p("Uhr stellen", "set clock"),
        0x51 => p("Geräteinfo lesen", "read device info"),
        0x5A => p("Intra-Pack", "intra-pack"),
        0x64 => match info0 {
            Some(0x02) => p("Wartungsmodus lesen", "read maintenance state"),
            Some(0x03) => p("Wartungsinfo lesen", "read maintenance info"),
            Some(0x00) => p("Wartungsmodus aus", "maintenance mode off"),
            Some(0x01) => p("Wartungsmodus ein", "maintenance mode on"),
            _ => p("Wartungsmodus", "maintenance mode"),
        },
        0xA0 => p("Kalibrieren", "calibrate"),
        0xA1 => p("Parameter schreiben", "write parameters"),
        0xA2 => p("Batterie-Seriennummer lesen", "read battery serial number"),
        0xA3 => p("Batterie-Seriennummer schreiben", "write battery serial number"),
        0xA4 => p("BMS-Seriennummer lesen", "read BMS serial number"),
        0xA5 => p("BMS-Seriennummer schreiben", "write BMS serial number"),
        0xA6 => p("CAN-Protokoll setzen", "set CAN protocol"),
        _ => return format!("{} 0x{cid2:02X}", l.pick("Befehl", "command")),
    };
    format!("{name} (0x{cid2:02X})")
}

fn rtn_meaning(rtn: u8, l: Lang) -> String {
    let m = match rtn {
        0x02 => l.pick("Checksumme laut BMS falsch", "checksum wrong according to the BMS"),
        0x03 => l.pick("Länge falsch", "length wrong"),
        0x04 => l.pick("Befehl, Typ oder Index unbekannt", "command, type or index unknown"),
        0x05 => l.pick("Rahmenfehler", "frame error"),
        0x06 => l.pick("Wert abgelehnt", "value refused"),
        0x08 => l.pick("Speicher- oder Uhrfehler", "memory or clock error"),
        _ => l.pick("unbekannter Code", "unknown code"),
    };
    format!("RTN {rtn:02X}: {m}")
}

fn frame_error(e: &openbms_proto::Error, l: Lang) -> String {
    match e {
        openbms_proto::Error::Checksum { .. } => l.pick("Checksumme falsch", "checksum wrong").into(),
        _ => l.pick("kein gültiger Frame", "not a valid frame").into(),
    }
}

fn modbus_crc_ok(b: &[u8]) -> bool {
    b.len() >= 4 && modbus::crc16(&b[..b.len() - 2]).to_le_bytes() == b[b.len() - 2..]
}

fn decode_tx(bytes: &[u8], l: Lang) -> (Row, Req) {
    let mut row = Row { address: String::new(), command: String::new(), data: data_text(bytes), meaning: String::new() };
    if bytes.first() == Some(&b'~') {
        match frame::parse(bytes) {
            Ok(f) => {
                let (info0, info1) = (f.info.first().copied(), f.info.get(1).copied());
                row.address = format!("{:02X}", f.address);
                row.command = format!("{:02X}", f.rtn);
                row.meaning = command_name(f.rtn, info0, info1, l);
                return (row, Req::Ascii { address: f.address, cid2: f.rtn, info0, info1 });
            }
            Err(e) => row.meaning = frame_error(&e, l),
        }
        return (row, Req::Other);
    }
    if bytes.len() == 8 && matches!(bytes[1], 0x03 | 0x04) && modbus_crc_ok(bytes) {
        let start = u16::from_be_bytes([bytes[2], bytes[3]]);
        let count = u16::from_be_bytes([bytes[4], bytes[5]]);
        row.address = format!("{:02X}", bytes[0]);
        row.command = format!("{:02X}", bytes[1]);
        row.meaning = match l {
            Lang::De => format!("Modbus {:02X} lesen: Start 0x{start:04X}, {count} Register", bytes[1]),
            Lang::En => format!("Modbus {:02X} read: start 0x{start:04X}, {count} registers", bytes[1]),
        };
        return (row, Req::Modbus);
    }
    row.meaning = l.pick("unbekanntes Format", "unknown framing").into();
    (row, Req::Other)
}

fn decode_rx(bytes: &[u8], req: Option<Req>, l: Lang) -> Row {
    let mut row = Row { address: String::new(), command: String::new(), data: data_text(bytes), meaning: String::new() };
    if matches!(req, Some(Req::Modbus)) && bytes.first() != Some(&b'~') {
        if bytes.len() < 5 {
            row.meaning = l.pick("Modbus-Antwort unvollständig", "Modbus answer incomplete").into();
        } else if !modbus_crc_ok(bytes) {
            row.meaning = l.pick("Modbus-CRC falsch", "Modbus CRC wrong").into();
        } else {
            row.address = format!("{:02X}", bytes[0]);
            row.command = format!("{:02X}", bytes[1]);
            row.meaning = if bytes[1] & 0x80 != 0 {
                format!("{} {:02X}", l.pick("Modbus-Ausnahme", "Modbus exception"), bytes[2])
            } else {
                match l {
                    Lang::De => format!("Modbus-Antwort, {} Byte Daten", bytes[2]),
                    Lang::En => format!("Modbus answer, {} bytes of data", bytes[2]),
                }
            };
        }
        return row;
    }
    let Some(start) = bytes.iter().position(|b| *b == b'~') else {
        row.meaning = l.pick("kein gültiger Frame", "not a valid frame").into();
        return row;
    };
    let f = match frame::parse(&bytes[start..]) {
        Ok(f) => f,
        Err(e) => {
            row.meaning = frame_error(&e, l);
            return row;
        }
    };
    row.address = format!("{:02X}", f.address);
    let info = match l {
        Lang::De => format!("{} Byte INFO", f.info.len()),
        Lang::En => format!("{} bytes INFO", f.info.len()),
    };
    let mut parts: Vec<String> = Vec::new();
    if f.rtn == 0x5A {
        row.command = "5A".into();
        parts.push(if f.info.is_empty() {
            format!("{} {:02X}", l.pick("Intra-Pack: Master fragt Pack", "intra-pack: master polls pack"), f.address)
        } else {
            format!("{} {:02X}, {info}", l.pick("Intra-Pack: Antwort von Pack", "intra-pack: answer of pack"), f.address)
        });
    } else {
        row.command = format!("RTN {:02X}", f.rtn);
        match req {
            Some(Req::Ascii { address, .. }) if address != f.address => {
                parts.push(format!("{} ({} {:02X})", l.pick("nicht für uns", "not for us"), l.pick("Adresse", "address"), f.address));
            }
            Some(Req::Ascii { cid2, info0, info1, .. }) => {
                let to = format!("{} {}", l.pick("Antwort auf", "answer to"), command_name(cid2, info0, info1, l));
                if f.rtn == 0 {
                    parts.push(format!("{to}, {info}"));
                } else {
                    parts.push(format!("{}; {to}", rtn_meaning(f.rtn, l)));
                }
            }
            _ if f.rtn == 0 => parts.push(format!("{}, {info}", l.pick("Antwort", "answer"))),
            _ => parts.push(rtn_meaning(f.rtn, l)),
        }
    }
    if start > 0 {
        parts.push(match l {
            Lang::De => format!("{start} Byte davor"),
            Lang::En => format!("{start} bytes before it"),
        });
    }
    row.meaning = parts.join("; ");
    row
}

/// One line per entry; tabs and line breaks in free text would break the columns.
fn clean(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

/// The export file: a header, then one tab-separated line per TX, assembled RX line or event, in local time.
pub(crate) fn export_text<Tz: TimeZone>(snap: &Snapshot, version: &str, l: Lang, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let local = |ms: i64| tz.timestamp_millis_opt(ms).single();
    let stamp = |ms: i64, fmt: &str| local(ms).map(|t| t.format(fmt).to_string()).unwrap_or_default();
    let mut out = String::new();
    let _ = writeln!(out, "# OpenBMS Monitor {version} – {}", l.pick("Mitschnitt", "frame capture"));
    let n = snap.entries.len();
    let timeouts = snap.timeouts;
    match (snap.entries.first(), snap.entries.last()) {
        (Some(a), Some(b)) => {
            let offset = local(a.wall_ms).map(|t| {
                let secs = t.offset().fix().local_minus_utc();
                let (h, m) = (secs.abs() / 3600, secs.abs() % 3600 / 60);
                format!("UTC{}{h}{}", if secs < 0 { "−" } else { "+" }, if m > 0 { format!(":{m:02}") } else { String::new() })
            }).unwrap_or_default();
            let full = "%Y-%m-%d %H:%M:%S%.3f";
            let _ = match l {
                Lang::De => writeln!(out, "# Start {} (Ortszeit, {offset}), Ende {}, {} {}, {timeouts} {}",
                    stamp(a.wall_ms, full), stamp(b.wall_ms, full), grouped(n, l), if n == 1 { "Eintrag" } else { "Einträge" }, if timeouts == 1 { "Timeout" } else { "Timeouts" }),
                Lang::En => writeln!(out, "# Start {} (local time, {offset}), end {}, {} {}, {timeouts} {}",
                    stamp(a.wall_ms, full), stamp(b.wall_ms, full), grouped(n, l), if n == 1 { "entry" } else { "entries" }, if timeouts == 1 { "timeout" } else { "timeouts" }),
            };
        }
        _ => {
            let _ = writeln!(out, "# {}", l.pick("keine Einträge", "no entries"));
        }
    }
    let mut labels: Vec<&str> = Vec::new();
    for e in &snap.entries {
        if !labels.contains(&&*e.label) {
            labels.push(&e.label);
        }
    }
    let _ = writeln!(out, "# {}: {}", l.pick("Verbindungen", "Connections"), clean(&labels.join("; ")));
    if let Some(f) = snap.full {
        let limit = match f {
            Full::Entries => format!("{} {}", grouped(capture::MAX_ENTRIES, l), l.pick("Einträge", "entries")),
            Full::Bytes => format!("{} MiB", capture::MAX_BYTES / (1024 * 1024)),
        };
        let _ = writeln!(out, "# {} ({limit})", l.pick("Mitschnitt voll, angehalten", "capture full, stopped"));
    }
    let _ = writeln!(out, "{}", l.pick("Zeit\t+ms\tVerbindung\tRichtung\tAdresse\tBefehl\tDaten\tDeutung", "Time\t+ms\tConnection\tDirection\tAddress\tCommand\tData\tMeaning"));
    let mut last: HashMap<u64, Req> = HashMap::new();
    for line in capture::lines(&snap.entries) {
        let (dir, row) = row_of(&line, &mut last, l);
        let mut data = row.data;
        if line.truncated > 0 {
            let cut = match l {
                Lang::De => format!("… {} Byte abgeschnitten", grouped(line.truncated, l)),
                Lang::En => format!("… {} bytes cut off", grouped(line.truncated, l)),
            };
            data = if data.is_empty() { cut } else { format!("{data} {cut}") };
        }
        let _ = writeln!(out, "{}\t{}\t{}\t{dir}\t{}\t{}\t{}\t{}",
            stamp(line.wall_ms, "%H:%M:%S%.3f"), line.offset_ms, clean(&line.label), row.address, row.command, data, clean(&row.meaning));
    }
    out
}

fn row_of(line: &Line, last: &mut HashMap<u64, Req>, l: Lang) -> (&'static str, Row) {
    let empty = |data: String, meaning: String| Row { address: String::new(), command: String::new(), data, meaning };
    match &line.kind {
        Kind::Tx => {
            let (row, req) = decode_tx(&line.bytes, l);
            last.insert(line.conn, req);
            ("TX", row)
        }
        Kind::Rx => ("RX", decode_rx(&line.bytes, last.get(&line.conn).copied(), l)),
        Kind::Dropped => {
            // read against the request before it: usually its late answer
            let mut row = decode_rx(&line.bytes, last.get(&line.conn).copied(), l);
            let why = l.pick("vor der Anfrage verworfen (kam zu spät)", "dropped before the request (came late)");
            row.meaning = if row.meaning.is_empty() { why.into() } else { format!("{why}; {}", row.meaning) };
            (l.pick("RX verworfen", "RX dropped"), row)
        }
        Kind::Timeout { waited_ms, address } => {
            let mut row = empty(format!("{waited_ms} ms"), format!("{} {address:02X}", l.pick("keine Antwort von", "no answer from")));
            row.address = format!("{address:02X}");
            ("Timeout", row)
        }
        Kind::Error(text) => (l.pick("Fehler", "Error"), empty(clean(text), String::new())),
        Kind::Skipped { sent, received } => {
            let meaning = match l {
                Lang::De => format!("Bootloader-Verkehr ausgelassen ({} Byte gesendet, {} empfangen)", grouped(*sent, l), grouped(*received, l)),
                Lang::En => format!("bootloader traffic left out ({} bytes sent, {} received)", grouped(*sent, l), grouped(*received, l)),
            };
            (l.pick("Hinweis", "Note"), empty(String::new(), meaning))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use openbms_transport::capture::Entry;
    use std::sync::Arc;

    // 2026-01-02 10:00:00.000 UTC
    const T0: i64 = 1_767_348_000_000;

    fn entry(conn: u64, kind: Kind, bytes: &[u8], ms: u64) -> Arc<Entry> {
        let label = if conn == 1 { "USB /dev/cu.test · RS485-1/2" } else { "TCP 192.0.2.10:4196 · CAN-Buchse" };
        Arc::new(Entry { wall_ms: T0 + ms as i64, offset_ms: ms, conn, label: label.into(), kind, bytes: bytes.to_vec(), truncated: 0 })
    }

    fn answer(address: u8, rtn: u8, info: &[u8]) -> Vec<u8> {
        frame::encode_request(address, rtn, info).unwrap()
    }

    fn export(entries: Vec<Arc<Entry>>, timeouts: usize, l: Lang) -> Vec<String> {
        let snap = Snapshot { entries, timeouts, full: None };
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        export_text(&snap, "0.1.0", l, &tz).lines().map(String::from).collect()
    }

    fn cols(line: &str) -> Vec<&str> {
        line.split('\t').collect()
    }

    #[test]
    fn labels_name_endpoint_and_bus() {
        let ep = Endpoint::Tcp { addr: "192.0.2.10:4196".into() };
        assert_eq!(label(&ep, Bus::CanSocketBus), "TCP 192.0.2.10:4196 · CAN-Buchse");
        let ep = Endpoint::Serial { path: "/dev/cu.test".into(), baud: 19200 };
        assert_eq!(label(&ep, Bus::PackBus), "USB /dev/cu.test · RS485-1/2");
    }

    #[test]
    fn header_and_ascii_exchange() {
        let mut stray = answer(5, 0, &[]);
        stray.extend(answer(3, 0, &[0x01, 0x02]));
        let lines = export(vec![
            entry(1, Kind::Tx, &openbms_proto::Telemetry::request(3).unwrap(), 12),
            entry(1, Kind::Rx, &stray[..10], 140),
            entry(1, Kind::Rx, &stray[10..], 141),
            entry(1, Kind::Tx, &openbms_proto::control::set_remaining_request(3, 30000).unwrap(), 400),
            entry(1, Kind::Rx, &answer(3, 0x06, &[]), 450),
            entry(1, Kind::Tx, &openbms_proto::DeviceInfo::request(4).unwrap(), 1000),
            entry(1, Kind::Timeout { waited_ms: 1500, address: 4 }, &[], 2500),
            entry(1, Kind::Dropped, &answer(4, 0, &[]), 2600),
        ], 1, Lang::De);
        assert_eq!(lines[0], "# OpenBMS Monitor 0.1.0 – Mitschnitt");
        assert_eq!(lines[1], "# Start 2026-01-02 12:00:00.012 (Ortszeit, UTC+2), Ende 2026-01-02 12:00:02.600, 8 Einträge, 1 Timeout");
        assert_eq!(lines[2], "# Verbindungen: USB /dev/cu.test · RS485-1/2");
        assert_eq!(cols(&lines[3])[0], "Zeit");
        let tx = cols(&lines[4]);
        assert_eq!(tx, vec!["12:00:00.012", "12", "USB /dev/cu.test · RS485-1/2", "TX", "03", "42", "~20034642E00203FD31\\r", "Telemetrie lesen (0x42)"]);
        let other = cols(&lines[5]);
        assert_eq!((other[3], other[4], other[5], other[7]), ("RX", "05", "RTN 00", "nicht für uns (Adresse 05)"));
        assert_eq!(cols(&lines[6])[7], "Antwort auf Telemetrie lesen (0x42), 2 Byte INFO");
        assert_eq!(cols(&lines[7])[7], "Restkapazität setzen (0x49)");
        assert_eq!(cols(&lines[8])[7], "RTN 06: Wert abgelehnt; Antwort auf Restkapazität setzen (0x49)");
        let to = cols(&lines[10]);
        assert_eq!((to[3], to[4], to[6], to[7]), ("Timeout", "04", "1500 ms", "keine Antwort von 04"));
        let dropped = cols(&lines[11]);
        assert_eq!((dropped[3], dropped[4], dropped[7]), ("RX verworfen", "04", "vor der Anfrage verworfen (kam zu spät); Antwort auf Geräteinfo lesen (0x51), 0 Byte INFO"));
    }

    #[test]
    fn modbus_intra_pack_and_broken_frames() {
        let req = modbus::read_request(0x03, 0x1000, 21);
        let mut resp = vec![0x01, 0x03, 2, 0x0D, 0x0A];
        let crc = modbus::crc16(&resp);
        resp.extend(crc.to_le_bytes());
        let mut bad = answer(0, 0, &[0x01]);
        let n = bad.len();
        bad[n - 2] = if bad[n - 2] == b'0' { b'1' } else { b'0' };
        let lines = export(vec![
            entry(2, Kind::Tx, &req, 0),
            entry(2, Kind::Rx, &resp[..4], 30),
            entry(2, Kind::Rx, &resp[4..], 31),
            entry(1, Kind::Rx, &answer(2, 0x5A, &[]), 40),
            entry(1, Kind::Rx, &answer(2, 0x5A, &[0; 32]), 60),
            entry(1, Kind::Rx, &bad, 70),
            entry(1, Kind::Rx, b"\xF0\x0F\r", 80),
            entry(1, Kind::Tx, &openbms_proto::control::restart_request(1).unwrap(), 90),
            entry(1, Kind::Skipped { sent: 12_345, received: 678 }, &[], 95),
            entry(1, Kind::Error("I/O: broken pipe".into()), &[], 99),
        ], 0, Lang::En);
        assert_eq!(lines[1], "# Start 2026-01-02 12:00:00.000 (local time, UTC+2), end 2026-01-02 12:00:00.099, 10 entries, 0 timeouts");
        assert_eq!(lines[2], "# Connections: TCP 192.0.2.10:4196 · CAN-Buchse; USB /dev/cu.test · RS485-1/2");
        let tx = cols(&lines[4]);
        assert_eq!((tx[4], tx[5], tx[6], tx[7]), ("01", "03", "01 03 10 00 00 15 80 C5", "Modbus 03 read: start 0x1000, 21 registers"));
        let rx = cols(&lines[5]);
        assert_eq!((rx[1], rx[7]), ("30", "Modbus answer, 2 bytes of data"));
        assert_eq!(cols(&lines[6])[7], "intra-pack: master polls pack 02");
        assert_eq!(cols(&lines[7])[7], "intra-pack: answer of pack 02, 32 bytes INFO");
        assert_eq!(cols(&lines[8])[7], "checksum wrong");
        assert_eq!(cols(&lines[9])[6], "F0 0F 0D");
        assert_eq!(cols(&lines[9])[7], "not a valid frame");
        assert_eq!(cols(&lines[10])[7], "pack control: restart (0x45 type 6)");
        assert_eq!(cols(&lines[11])[7], "bootloader traffic left out (12,345 bytes sent, 678 received)");
        assert_eq!((cols(&lines[12])[3], cols(&lines[12])[6]), ("Error", "I/O: broken pipe"));
        assert!(lines.iter().skip(3).all(|l| cols(l).len() == 8));
    }

    #[test]
    fn truncation_full_capture_and_empty_export() {
        let mut e = (*entry(1, Kind::Tx, b"~2000", 0)).clone();
        e.truncated = 12_345;
        let snap = Snapshot { entries: vec![Arc::new(e)], timeouts: 0, full: Some(Full::Entries) };
        let text = export_text(&snap, "0.1.0", Lang::De, &FixedOffset::east_opt(0).unwrap());
        assert!(text.contains("# Mitschnitt voll, angehalten (100 000 Einträge)"), "{text}");
        assert!(text.contains("~2000 … 12 345 Byte abgeschnitten\t"), "{text}");
        assert!(text.contains("UTC+0"), "{text}");
        let empty = export_text(&Snapshot::default(), "0.1.0", Lang::De, &FixedOffset::east_opt(0).unwrap());
        assert!(empty.contains("# keine Einträge"));
    }
}
