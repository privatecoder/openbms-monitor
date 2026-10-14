# BMS clock and internal history

Status: approved r2, 2026-10-14 (r1 reviewed: `.orch/reviewer-codex-6-sol-bms-clock-history-r1.md` in the knowledge base: no path from 0x4E/0x4B to flash, EEPROM data or history head).

## Why

The vendor tool BatteryMonitor reads and sets the BMS clock and reads the pack's internal history. OpenBMS Monitor has neither. The internal history (up to 500 records per pack in an I²C EEPROM, written every parameter 75/76 interval) covers times when no app was polling, so it shows problems after the fact (alarms, cell spread, temperatures). Its timestamps come from the BMS clock: a wrong clock makes them useless, so both belong together.

## Protocol (knowledge base, `Seplos-ASCII-Protokoll.md` §5.6/5.7, firmware 04.06–06.13)

- **Pack bus only (RS485-1/2, UART0).** The CAN socket bus answers 0x42/0x44/0x51 only; 0x4B/0x4D/0x4E there → RTN 04. Same rule as parameters today (`parameters` refuses on the CAN socket bus).
- **0x4D** read clock: no INFO, answer 7 bytes: year u16 BE, month, day, hour, minute, second. RTN 08 on RTC error.
- **0x4E** set clock: INFO 7 bytes as above, empty answer. Year ≤ 2000 is stored as 2022; no other plausibility check.
- **0x4B** history: INFO 2 bytes `[0]=00`, `[1]=00` oldest record, `≠00` next record. Answer 97 bytes: `01`, flag "last record" (1/0), year u16, record bytes 1…93 (layout §5.6: timestamp, system status, alarm events 1–8, 16 cell mV, 4 cell temps + ambient + power 0.1 K, current 0.01 A, pack voltage 0.01 V, remaining/full capacity 0.01 Ah, SOC 0.1 %, rated capacity, cycles, SOH, port voltage). Empty answer → no records. RTN 08 → read error. Reading only advances the firmware's read pointer; nothing is written.

## Design

**Placement changed by the user 2026-10-14:** no separate view; the clock line (1) sits near the top of the existing **Parameters** view for a chosen pack, the history (2) is a second tab **„Aus dem BMS“** next to **„App-Aufzeichnung“** in **Verlauf**, with the pack's clock deviation and a pointer to Parameters; the help topic is `topic.bmsHistory`. The text below describes the content of both parts.

One new view **„BMS-Speicher“** (sidebar, next to „Verlauf“), available while the monitor is connected on RS485-1/2 (otherwise the same hint as parameters: „nur über RS485-1/2 lesbar“). A pack picker (addresses of the connection) on top.

1. **Clock line:** BMS time, PC time, difference („BMS-Uhr geht 3 min 12 s nach“). Button **„Uhr stellen: <PC local time> (<PC time zone, e.g. MEZ/UTC+1>)“** — the label shows the exact time and zone that will be sent (the BMS stores calendar fields without a zone; a PC in another time zone would shift all later records). Sends 0x4E with the PC's local time, reads 0x4D back and shows the new difference. One click, no dialog. Note: 0x4E rewrites 16 RTC registers from register 0 (control bytes and weekday to 00); no known effect in this firmware. Optional: „Alle Packs stellen“ (one 0x4E per pack in the list).
2. **History:** button **„Historie lesen“**: 0x4B oldest, then next until the last flag or an empty answer; progress „Datensatz n“ and a cancel button. Polling of the monitor pauses meanwhile (the connection lock); afterwards it resumes.
   - Table, newest first: time, SOC, current, pack voltage, min/max cell (with cell number) and spread, max temperature, active alarms/protections as text (decoded with the existing alarm tables).
   - A hint when the clock was far off: timestamps come from the BMS clock at the time of writing.
   - **„Als CSV speichern“** (all fields). Not stored in the app's own history database (v1).
   - The read is no atomic snapshot: with a full ring (500) a record written between two answers may shift the ring start, so one old record can be skipped or one new one appear. No endless loop (index modulo count, end flag at count−1). Say so in the help, not in the UI.
3. Generic help topic `topic.bmsMemory` (DE/EN): what the records are, interval parameters 75/76, clock dependence.

## Out of scope

Clearing the history (0x45 type 5 also sets the cycle count to 0; not built), calibration, CAN protocol, serial number writing (later specs).

## Damage check (only concrete cases)

- 0x4E with a wrong time: only timestamps of later records are wrong; fixable by setting again.
- History read while another master polls the pack bus: the existing pack-bus connection already has this problem for parameters; reading fails with no answer, nothing is written.
- Long read (500 frames, ~1 min) blocks polling: shown as progress, cancellable.

- Review r1: 0x4E writes only the RTC (I²C 0xA2, handler 0x0CF0 → 0x5882); 0x4B only reads EEPROM 0xA4 and moves the RAM read index (0x0C2E → 0x5C88/0x5C6C); history writes run in the same main loop and never overlap one handler. Layouts confirmed (0x4B: 97 bytes, year BE = 2000 + record byte 0).
