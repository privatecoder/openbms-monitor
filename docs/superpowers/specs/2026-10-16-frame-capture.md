# Frame capture under Diagnosis

Status: r2, 2026-10-16 (r1 reviewed: `.orch/reviewer-codex-6-sol-frame-capture-r1.md` in the knowledge base).

## Why

When a pack does not answer, answers on the wrong bus, or a write is refused, the app shows one line of text. What actually went over the wire (which request, which answer, what else was on the bus, how long the silence was) is only visible with a separate sniffer or the CLI. A capture inside the app lets a user send a bug report with the exact frames and lets a developer see the bus as the app saw it.

## What is captured

Every exchange of every `openbms_transport::Connection` the app opens while the capture runs: the monitor's connection, the other-bus probe (`probe_other_bus`) and the pack-bus connections of maintenance and firmware (`open_port`, `open_pack_for`). One capture spans reconnects and several connections; each entry names its connection.

Recorded at byte level in the transport (r2, review findings 1 and 4), so nothing that passes the port is missed and the answer selection of `ascii` stays as it is:

- **TX:** every write: the request in `ascii` and `modbus_read`, and every `write_raw` (maintenance dialog `dialog::exchange`, flash preflight 0x51/0x47, the 0x45 reset and the check after the flash all use the raw path).
- **RX:** every chunk read, as it arrives, copied in the read branch of `read_until` and in `read_raw`, with its time. Partial lines, trailing lines after our answer, noise and collisions are therefore in the capture; a timeout does not drop what arrived.
- **Timeout:** an `ascii`/`modbus_read` exchange ended without its answer, with the waited time.
- **Error:** I/O error or gateway closed, with the text.

Lines are assembled only at export (per connection: RX bytes up to `\r` for ASCII; a raw chunk run between two TX for Modbus and other binary data).

**Bootloader left out.** `session::flash_with_progress` pauses the capture for its connection from `R F L` up to and including `W F G` (a `Link` method with a no-op default, `Connection` sets a flag); the capture notes „Bootloader-Verkehr ausgelassen (n Byte gesendet, m empfangen)“ there. Preflight, reset and the check after the flash are captured. Not captured: bytes dropped by `clear_input` before a request (the serial driver drops them unseen).

Each entry: wall-clock time (ms), monotonic offset from capture start (ms), connection label, direction/kind, bytes.

Connection label: endpoint and bus as the app knows them, e.g. `USB /dev/cu.usbserial-X · RS485-1/2` or `TCP 192.0.2.10:4196 · CAN-Buchse`; the transport gets it from the caller (`Connection::set_label`), default from the endpoint.

## Where it lives

A capture sink in `openbms-transport` (module `capture`): process-wide, off by default. `Connection` records only while a capture runs (one atomic check per write/read when off). Recording takes a mutex for a push of an `Arc<Entry>`; nothing is decoded, formatted or written while capturing. Recording never fails an exchange: a poisoned mutex or a full capture is ignored by the transport.

- **Short lock (review finding 2).** Export and status copy the list of `Arc`s under the lock (pointer copies) and format and write the file outside it, on a background thread. „Verwerfen“ swaps the list out under the lock and drops it outside. No file I/O or large free happens under the lock, so a 300-ms restart answer or a 600-ms maintenance dialog never waits for it.
- **Memory only.** Nothing goes to the database or disk until the user exports. Quitting the app discards it.
- **Limits (review finding 3):** 100 000 entries **or** 32 MiB of captured bytes, whichever comes first; one entry holds at most 4 KiB, the rest is counted („… 12 345 Byte abgeschnitten“). The transport's own buffers and frame selection are not limited or changed by this. On reaching a limit the capture stops by itself; the UI says so („Mitschnitt voll, angehalten“).
- The CLI does not use it (never started there).

## UI: block „Mitschnitt“ in Diagnosis

Under the existing sections, a block with the same header style (`SectionHead`):

- **„Mitschnitt starten“ / „Anhalten“** (one button). One capture at a time: starting again continues it, **„Verwerfen“** clears it.
- Status line: running/stopped, number of entries, time span, number of timeouts.
- **„Exportieren …“**: save dialog, default name `openbms-mitschnitt-YYYYMMDD-HHMMSS.txt`. Under the button a plain note (no checkbox): „Die Datei enthält Seriennummern, Messwerte und Einstellungen aller Packs, die während des Mitschnitts angesprochen wurden, auch früherer Verbindungen, sowie die benutzten Ports und Netzwerkadressen. Vor dem Weitergeben ansehen.“ (review finding 5; the export filters nothing out.)
- Works without a connection (export/clear of an earlier capture); starting without a connection is allowed (it records the next connect).

## Export format

UTF-8 text, one header, then one line per entry, tab-separated (opens in a spreadsheet as well as in an editor):

```
# OpenBMS Monitor <version> – Mitschnitt
# Start 2026-10-16 10:00:00.000 (Ortszeit, UTC+2), Ende 2026-10-16 10:05:12.345, 1234 Einträge, 3 Timeouts
# Verbindungen: USB /dev/cu.usbserial-X · RS485-1/2
Zeit	+ms	Verbindung	Richtung	Adresse	Befehl	Daten	Deutung
10:00:00.012	12	USB … · RS485-1/2	TX	00	42	~20004642E00200FD37	Telemetrie lesen (0x42)
10:00:00.141	141	USB … · RS485-1/2	RX	00	RTN 00	~2000460010…\r	Antwort Telemetrie, 74 Byte INFO
10:00:01.660	1660	USB … · RS485-1/2	Timeout			1500 ms	keine Antwort von 03
```

- **Daten:** ASCII frames as their text (they are hex already), `\r` shown as `\r`; Modbus and anything not printable as spaced hex bytes.
- **Adresse / Befehl:** from the frame header when it parses (ASCII: ADR, CID2 for requests, RTN for answers; Modbus: slave address and function).
- **Deutung:** command name from a table in the app (the commands the app knows: 0x42, 0x44, 0x47, 0x51, 0x45 with type, 0x49, 0xA0–0xA6, 0x4B, 0x5A intra-pack, 0x64 maintenance, Modbus 03/04 with start/count), the RTN meaning for non-zero RTN (e.g. „RTN 06: Wert abgelehnt“), INFO length, „nicht für uns (Adresse 05)“, „Checksumme falsch“ / „kein gültiger Frame“. No value decoding (telemetry values are in the view; the raw frame is there for tools).
- Times in local time with the UTC offset in the header.

## Out of scope

Live view of the frames in the app, filters, a pcap or other binary format, capturing the bootloader, capturing in the CLI.

## Review questions

1. Can the capture change what goes over the wire or its timing in a way that matters (flash, restart watch, maintenance halt)?
2. Anything in an export that should not be in it beyond what the note says?
