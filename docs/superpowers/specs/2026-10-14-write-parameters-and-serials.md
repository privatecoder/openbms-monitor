# Writing parameters and serial numbers

Status: draft r2, 2026-10-14 (r1 reviewed: `.orch/reviewer-codex-6-sol-write-params-r1.md` in the knowledge base).

## Why

Reading the parameters of all packs and comparing them works. Changing them still needs the vendor tool BatteryMonitor, one pack at a time. For a bank this is the essential part: set one value on all packs alike, or bring a pack back in line with the others. Serial numbers: a replaced board or a pack maker's placeholder should get a proper value.

## Protocol (knowledge base, `Protokoll-Erweiterungen.md` §1.4/§1.9/§1.11, `Seplos-ASCII-Protokoll.md` §5.12/§5.13, firmware 04.06–06.13)

- **Pack bus only (RS485-1/2, UART0)**, like reading parameters, the clock and calibration.
- **0xA1** write parameter block, INFO 169 bytes with the layout of the 0x47 answer: `[0]` ignored, `[1]=0x3C` + 60 × u16 BE, `[122]=0x1B` + 27 × u8, `[150]=0x08` + 8 function switch bytes + 10 bytes device name. Wrong marker → RTN 04. Plausibility check `0x4C3C` → RTN 06 (rules below). **The function switches and the name are taken into RAM before the check**: on RTN 06 they are live but not stored. On success the block goes into the internal flash parameter block (mirrored). Then the handler compares the **learned full capacity with the P58 just sent**: if they differ by ≥ 10.00 Ah, full capacity and the coulomb maximum are set to P58 and flash block 6 is written (`0x08FC–0x09A8`). **This happens on every successful 0xA1, also when P58 is unchanged.** No CAN re-init and no reset of protection counters; the next 1-s protection run uses the new thresholds and switches at once (MOSFETs, balancing, heating, CAN limits may change).
- **Rules of `0x4C3C`** (16.06.13, raw values, equality allowed, first failure → RTN 06; review r1 §1):
  - cell: P0 ≥ P1 ≥ P3 ≥ P2 ≥ 2000, P0 ≤ 4300; P4 ≥ P5 ≥ P7 ≥ P6 ≥ 2000, P4 ≤ 4300; 3330 ≤ P8 ≤ P4; 1000 ≤ P9 ≤ P6.
  - pack: P10 ≥ P11 ≥ P13 ≥ P12 ≥ 1600, P10 ≤ 6000; P14 ≥ P15 ≥ P17 ≥ P16 ≥ 1600, P14 ≤ 6000; P14 ≤ P19 ≤ P18 ≤ 6500.
  - temperatures (0.1 K): for each group (P20–23, P24–27, P28–31, P32–35, P38–41, P42–45) a ≥ b ≥ d ≥ c ≥ 2431 and a ≤ 3531; heating P26 ≤ P36 ≤ P37 ≤ 2931; power 3231 ≤ P47 ≤ P46 ≤ P48 ≤ 3931 and 3231 ≤ P49 ≤ P48.
  - currents (signed 0.01 A): P55 ≤ P52 ≤ P53 ≤ P51 ≤ P50 ≤ P54 ≤ 30000, −30000 ≤ P56 ≤ P55.
  - 500 ≤ P57 ≤ 5000; P59 ≤ P58 ≤ 65000; 10 ≤ P61 ≤ P60 ≤ 100; 10 ≤ P63 ≤ P62 ≤ 100.
  - ranges: P64 1–18, P65 7–16, P66/67 3–180, P68 5–250, P69 10–250, P70–72 1–30, P73 1–24, P74 1–100, P75 1–250, P76 30–250, P77 1–250, 1 ≤ P79 ≤ P78 ≤ 30, P80 70–99, P81 10–150, P82 0–100, P83 1–9, P84 0–50, P85 10–16, P86 0–50.
  - Nothing checks the function switches, the chemistry or P65 against the wiring.
- **0xA3 / 0xA5** write String 1 (battery serial) / String 2 (BMS serial), 30 bytes, stored at once in flash block 4 (`0x29000`, mirror `0x2A000`). Up to 06.05 exactly 30 bytes are read; from 06.11 the length comes from LENGTH (max 30) and the rest is filled with spaces. No password (the vendor tool's lock is UI only).
- The app's bank check before flashing compares the master's String 2 (0xA4 at address 0) with the value stored for the installation.

## Design

### 1. Parameters: edit and write

In the **Parameters** view, a read pack column gets a pencil button „Bearbeiten“ (only on RS485-1/2, maintenance functions on, values read in this session). Then:

- The column's cells become inputs (display units as shown, e.g. V with 3 decimals; switches as checkboxes). One column is edited at a time.
- **„Werte übernehmen aus …“**: a select with the other columns (packs and opened BatteryMonitor files). It fills the edited column with all values and switches of that source; the user can still change single cells. This is how a bank is brought in line: read all, edit one pack, take over from the reference, write; next pack.
- Changed cells are marked. Under the table header (sticky) a bar: „N Änderungen an Pack 03“, the list „P12 Pack-Unterspannungsalarm 44,80 → 46,40 V“ (max ~10 lines, then „und M weitere“), datasheet findings for the new values (same check as today, in the same colours), and the buttons **„Auf Pack 03 schreiben“** and „Verwerfen“.
- Local checks before the button is enabled, **only against concrete harm**:
  - each value in its raw range (u16 / u8, signed for currents) and representable in the raw unit (rounding shown);
  - `cells_in_series` (P65) unchanged unless the user unlocks it in the bar (a wrong count makes all cell protections wrong);
  - **the firmware's rules above, checked in the app**: a broken rule disables the button and names it („P1 Zellspannung-Hoch-Erholung muss ≤ P0 sein“). A block the app lets through therefore should not be refused; this matters because a refused block leaves the switches live (see below).
  - **learned full capacity** (0x42 of this pack) against the P58 to be written: if they differ by ≥ 10 Ah, the bar says „Beim Schreiben setzt das BMS die gelernte Vollkapazität (265,0 Ah) auf die Nennkapazität (280,0 Ah); der SOC springt.“ with a one-click „Nennkapazität auf 265,00 Ah setzen“ that avoids it. Shown for every write, not only when P58 changes.
- **Writing** (one click):
  1. read 0x47 again (fresh base); if it differs from the values the edit started from in any cell the user did not touch, stop: „Pack 03 hat sich seit dem Lesen geändert“, show the column fresh (no write).
  2. build the block from the fresh read with the user's changes, keep `[0]`, markers and device name as read; send 0xA1.
  3. RTN 00 → read 0x47 again; the column shows the read-back values; any cell that differs from what was sent is marked red („nicht übernommen“). RTN 06 (rules differ in this firmware version) → „Vom BMS abgelehnt: Werte nicht plausibel. Die Schalter dieses Blocks sind trotzdem schon aktiv, aber nicht gespeichert.“ with the switches that differ from the stored state, and a button „Gespeicherte Schalter wiederherstellen“ that writes the fresh block from step 1 (with the capacity line above if it applies). No automatic resend: it can reset the full capacity. A pack restart also restores the stored switches.
- **Snapshots** (deferred item, now part of this): every 0x47 read and every write is stored in the local database per installation and pack address: time, kind (read / written / refused), the 169-byte block (hex). A pack column shows „zuletzt geschrieben: 14.10.2026 18:40“; „Vorherigen Stand“ in the column menu fills the edit with the block from before the last write of this pack (undo by writing it, through the same bar, checks and change list). Stored additively (`CREATE TABLE IF NOT EXISTS`), not cleared by clearing the recording.

### 2. Serial numbers

A line „Seriennummern“ under the CAN protocol, for the chosen pack (same picker as clock, CAN, calibration): both strings as read (0xA2/0xA4), each with a text field and „Schreiben“.

- Input: printable ASCII 0x20–0x7E, at most 30 characters; sent padded with spaces to 30 bytes (works on all versions). Read back at once and shown.
- **String 2 at address 0 (master)**: if the installation's stored master serial equals the old value, the app updates it to the new value in the same step (after a successful read-back), so the bank check before flashing still matches. The line says this. Device write and database update are not atomic: after a crash in between, the bank check reports a mismatch and the existing „adopt master serial“ step resolves it.
- No lock like the vendor tool's 888888.

## Out of scope

Device name editing (written as read), writing several packs in one click (one pack per click keeps refusals and read-backs per pack), restart, on/off, clearing history.

## Review r1 (answered)

Rules of `0x4C3C`, the capacity reset on every 0xA1, switches live after RTN 06 (no automatic resend), serial write safe with 30 bytes padded, block 4 mirrored. See the review file.
