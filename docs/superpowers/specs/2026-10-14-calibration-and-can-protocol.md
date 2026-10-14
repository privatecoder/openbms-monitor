# Calibration and CAN inverter protocol

Status: approved r2, 2026-10-14 (r1 reviewed: `.orch/reviewer-codex-6-sol-calibration-r1.md` in the knowledge base).

## Why

The vendor tool BatteryMonitor calibrates the current zero point, the current gain and the voltage (0xA0) and sets the CAN inverter protocol (0xA6). OpenBMS Monitor shows the protocol (0x51) but cannot set it, and cannot calibrate. A pack with "current not calibrated" or a drifting SOC needs the vendor tool today.

## Protocol (knowledge base, `Seplos-ASCII-Protokoll.md` §5.11/§5.14, `Protokoll-Erweiterungen.md` §1.10/§1.12, firmware 04.06–06.13)

- **Pack bus only (RS485-1/2, UART0)**, like parameters and the clock. On the CAN socket bus the app does not offer either.
- **0xA6** set CAN protocol: INFO 1 byte, 1…20 accepted (8…20 act like 1), else RTN 04. Names: 1 Pylontech/Deye/Luxpower/TBB, 2 Growatt SPF/SPH, 3 Victron, 4 SMA/Sofar, 5 Ginlong, 6 Studer, 7 MUST (from 06.11; switching to or from 7 re-initialises CAN, 100 ↔ 500 kbit/s). Stored at once; from 06.11 an unchanged value is not stored. The current protocol is the text in 0x51 (bytes 12…31).
- **0xA0** calibration: INFO `[0]` sub, `[1..2]` reference u16/s16 BE; stored via `0x5174` in the calibration flash block 2 (`0x27000`, mirror `0x28000`; both CRC-checked at boot, a broken copy is repaired). No write-back check: a success answer proves the RAM value, not the stored one. New factors are computed from the old factor and the ratio reference/measured, so a new calibration with a good measurement corrects an old error (within the 3-A limit). No UART command reads the factors. Refused → RTN 06, unknown sub → RTN 04.
  - Sub 0 current zero point: accepted when both converted current channels are |x| < 1000 mA, then the momentary ADC bases become the zero point: **a current flowing during sub 0 stays as a permanent offset** (up to ~1 A). The 0x42 current reads 0 in standby, so it cannot prove that no current flows. Clears event 8 bit 4.
  - Sub 1 current gain, reference in 0.01 A (s16): accepted when |measured − reference| < 3.01 A, **no minimum current** (BMS 2 A, reference 0.01 A is accepted and divides the gain by 200; one new calibration cannot undo that because of the 3-A limit). Clears event 8 bit 3.
  - Sub 2 voltage, reference = cell voltage in mV (compared with the arithmetic mean of the cell voltages): accepted when cell spread < 20 mV, |mean cell − reference| < 30 mV and |port voltage P+/P− − reference × cells| < 2 V (port 0 V with the MOSFETs off → refused); sets all cell factors and the pack voltage factor. Clears event 8 bit 2.
- **0xA6 with 7 (MUST):** 06.04/06.05 accept and store 7 but run protocol 1; a later update to ≥ 06.11 then switches to MUST/100 kbit/s at boot and the inverter loses CAN. The 0x51 version (`10 06`) does not tell 06.05 from 06.11.

## Design

Both go into the existing **Parameters** view (maintenance functions on, RS485-1/2), next to the clock line, for the pack chosen in the clock's pack picker. The view's subtitle („Die App ändert nichts am BMS“) changes: clock, protocol and calibration do change the BMS.

1. **CAN protocol line:** „CAN-Protokoll: Pylontech/Deye/… (laut Pack)“ from 0x51, a select 1…6 and **„Setzen“** (7 MUST is not offered: the app cannot tell whether the firmware supports it, see above; a pack already on 7 shows it). One click; afterwards 0x51 is read again and shows the new text. Hint: only the master's CAN socket talks to the inverter; the inverter must be set to the same protocol; with 7 the CAN speed changes.
2. **„Kalibrieren (Experten)“**, collapsed by default. Inside, a short measuring guide and three blocks, each with the live value from the pack (telemetry 0x42 of this pack, read every 2 s while the block is open) and one button:
   - **Strom-Nullpunkt:** guide: no current may flow — open the pack's breaker or disconnect the pack cable, check with a clamp meter (the BMS shows 0 A in standby even when a little current flows, so its own value proves nothing); a current left over stays as a permanent offset. Button „Nullpunkt kalibrieren“. No check (nothing the app can read proves it).
   - **Strom:** guide: steady current of at least 10 A, clamp meter around one pack cable, charging positive. Field „gemessener Strom (A)“, BMS current beside it, difference. Button „Strom kalibrieren auf X A“, **enabled only when both the entered value and the BMS current are at least 10 A in magnitude and have the same sign** (review r1: without a minimum the firmware accepts e.g. 0.01 A against 2 A and ruins the gain beyond one-step repair).
   - **Spannung:** guide: at rest (no current), MOSFETs on, multimeter at the pack's output P+/P− (the firmware compares with that port voltage). Field „gemessene Pack-Spannung (V)“; the app sends the mean cell voltage = value ÷ cell count, rounded to mV (shown: „= 3,301 V je Zelle“). Shows BMS cell mean and spread (the firmware needs < 20 mV).
   - The firmware's own limits are shown as text („Das BMS nimmt nur Abweichungen bis 3 A an“ …); a refusal (RTN 06) is shown as „Das BMS hat die Kalibrierung abgelehnt: Abweichung zu groß oder Bedingung nicht erfüllt“ with the condition of that sub.
   - After a success the block reads telemetry again and shows the new value and whether the event-8 bit („… nicht kalibriert“) is gone. The help says that this shows the value in RAM; that it was stored shows after a restart of the pack.
3. Help topic `topic.calibration` (DE/EN): what each calibration does, the measuring guide, the firmware limits, that a wrong reference shifts all later readings of that pack (SOC, protections) until calibrated again.

## Out of scope

Serial number writing (0xA3/0xA5, next spec), restart, on/off, clearing history.

## Review r1 answers (kept for reference)

1. Does 0xA0 sub 1/2 store an absolute factor or one relative to the current factor (repeated calibration compounding)? Can the current factors be read (to show or restore them)?
2. Sub 0 with current flowing but raw < 1000 mA: a permanent offset of up to ~1 A. Is that the real behaviour? Is a check on the shown current worth it, or only the guide?
3. `0x5174` writes the internal flash parameter block: same mirrored write as 0xA1, any risk on power loss?
4. 0xA6 to an unnamed value 8…20, or switching to/from 7 on 06.04/06.05 images: any harm beyond a wrong inverter protocol?
5. Anything else that makes either function damage a pack beyond wrong readings that a new calibration fixes.
