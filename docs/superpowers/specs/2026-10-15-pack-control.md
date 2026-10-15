# Pack control: restart, on/off, heater, clear history, remaining capacity

Status: r2, 2026-10-15 (r1 reviewed: `.orch/reviewer-codex-6-sol-pack-control-r1.md` in the knowledge base).

## Why

The vendor tool BatteryMonitor can restart a pack, switch it on and off, force the heater, clear the pack's history and counters (0x45) and set the remaining capacity (0x49). OpenBMS Monitor can do none of it; a pack with a wrong SOC or one that hangs needs the vendor tool or a trip to the pack.

## Protocol (knowledge base: `Seplos-ASCII-Protokoll.md` §5.3/§5.5, `Protokoll-Erweiterungen.md` §1.3/§1.5, `Zustaende-und-Ausloeser.md` §14, lab notebook results 12 and 24)

- **Pack bus only (RS485-1/2, UART0)**, like parameters, clock and calibration. A master with slaves configured (DIP 5–8 ≠ 0) polls and does not process requests from the pack bus at all (measured, result 12): it needs the maintenance halt or DIP 5–8 OFF first, as for parameter writes.
- **0x45** INFO `00 <type> <param>`, answer empty with RTN 00, unknown type RTN 04:
  - **Type 6 restart** (watchdog). **No answer**: the board resets before it could send one (measured). About 3 s silent, then it answers again (measured, result 24). A RAM state is gone after it (e.g. a state set by 0x49 not yet stored, a switch made live by a refused 0xA1).
  - **Type 4 on (`10`) / off (`1F`)**: sets/clears system status bit 5 at once; the output follows at the next 1-s evaluation. „Off“ blocks **only the discharge MOSFET**; charging stays possible and charging clears „off“. The board keeps answering on the bus when off (measured). If a reason for „off“ persists (undervoltage or the empty flag without charging, `0x887C`: 60 s, then every second), the firmware sets it again within a second after „on“ (measured). „Off“ by command does not survive a restart (measured). A master with slaves passes every edge of bit 5 to the slaves as a broadcast, once, but only while it polls; a polling master does not take 0x45 from the pack bus, and a halted master (maintenance halt, DIP 5–8 OFF) has no broadcast path and `0x944C` drops the pending broadcast. **So through the app „off“ always switches exactly one pack**; a slave switched off by unicast stays off while the master is on. Bit 5 = 0 only means „not switched off“: the discharge MOSFET can still be open for undervoltage, temperature, SOC protection and other events (`0x7184–0x720A`).
  - **Type 3 heater**: `10` forces the heater on for 3600 s (counter in the 1-s cycle), any other value stops it. **Forcing skips every heater condition** (BitGroup3 bit 6, temperature request, charging/charger, `0x735C–0x736A`): a connected heating film runs for one hour even on a warm battery without a charger and draws energy. On-off bit 3 (status 0x44 switch state) shows the heater output.
  - **Type 5 clear history and counters**: EEPROM history header and record count, cumulative charged/discharged energy and **cycle count = 0**, then flash block 6 is saved (`0x5D76–0x5DAC`). Remaining/full capacity and SOC stay. The old records become unreachable (not erased one by one). Not reversible.
- **0x49** INFO `00 <index> <u16 BE>`, only indices 58 and 59, others RTN 04:
  - **Index 59 remaining capacity** (10 mAh): accepted when full capacity ≥ value, then the coulomb counter is set to it (SOC = remaining ÷ full); else RTN 06. The 1-s SOC logic goes on from there (current integration, full/empty rules can set it to full or zero); a value under P79 raises the SOC protection (event 6 bit 3) and **blocks discharging** (`0x91EE–0x91FE`, `0x71F2–0x720A`). **RAM only**: stored with the next save of flash block 6 (e.g. SOC change > 5 % in standby, new cycle). A restart or power loss before that brings back the old value.
  - Index 58 (rated → learned full capacity) is not offered: the parameter write already handles P58 and its capacity reset (spec `2026-10-14-write-parameters-and-serials.md`).

## Design

A block **„Steuern“** in the Parameters view (maintenance functions on, RS485-1/2, connection with an installation), under the clock / CAN protocol / calibration lines, for the pack chosen in the same pack picker. Each action is one button; the result line under the block says what happened, red on failure. Every command goes through the existing guards: `pack_bus_only`, `bound(installation)`, the connection lock (`with_conn`, one frame at a time, refuses during a flash) and the busy lock of the view (`lockedBut`).

**Pack reservation (review r1):** `with_conn` holds the bus for one frame only, so a restart could fall between the 0x47 and the 0xA1 of a parameter write, and clearing could cut a running 0x4B read of the same pack. New in the backend: a set of reserved pack addresses (`AppState.pack_busy`, a guard that frees on drop). Taken for the whole action by every control command, by `write_parameters` (fresh read, write, read-back) and by the BMS history read (`bms_history`, until it ends or is cancelled); a second action on the **same** pack gets the error `pack-busy` („Pack 00 ist gerade mit … beschäftigt“). Actions on different packs still interleave frame by frame.

The block first shows the pack's state, read with the existing telemetry/status call (0x42/0x44) when the block opens and after every action, with the command state and the real outputs apart: „Ein/Aus-Befehl: an / aus“ (system status bit 5), „Entladen: frei / gesperrt“ (switch state bit 0, discharge MOSFET) with the blocking alarms when it is closed while bit 5 is clear, „Laden: frei / gesperrt“ (bit 1), heater output on/off (bit 3), SOC and remaining/full capacity (Ah), cycles.

1. **„Neu starten“**, one confirmation: „Pack 00 startet neu und ist einige Sekunden ohne Ausgang. Versorgt dieses Pack allein den Wechselrichter oder das Gateway, fällt deren Versorgung kurz aus.“ Backend `pack_restart(address, installation)`:
   - first 0x51 to the pack; no answer → refuse „Das Pack antwortet nicht“ (a polling master never answers; the hint names maintenance halt / DIP 5–8 as the parameter write does).
   - send 0x45 type 6 with a short answer timeout (300 ms instead of 1.5 s); a missing answer is expected, RTN 00 is also accepted.
   - then 0x51 with 300 ms timeout back to back for up to 15 s; the silence is measured **from sending the 0x45**. At least one unanswered try followed by an answer → „Pack 00 nach x s Unterbrechung wieder erreichbar (Neustart)“. Answered throughout → „Neustart nicht bestätigt: das Pack hat ohne Unterbrechung geantwortet“. No answer for 15 s → „Pack 00 antwortet seit dem Neustart nicht“ (red; read again later).
   - afterwards the UI reads the pack again (status, parameters column if shown).
2. **„Einschalten“ / „Ausschalten“** (one button, label by state).
   - „Ausschalten“ asks once (dialog): „Pack 00 sperrt dann das Entladen; Laden bleibt möglich und schaltet es wieder ein. Die anderen Packs bleiben an. Versorgt dieses Pack allein den Rechner oder das Gateway, gibt es nach dem Ausschalten unter Umständen keinen Weg zurück per App.“
   - „Einschalten“ without dialog.
   - After the command the app waits 2 s and reads the status again. „On“ sent but bit 5 set again → „Das BMS hat das Pack wieder ausgeschaltet: <reason>“, reason from the active alarms (cell/pack undervoltage, SOC protection) or „ein Schutz besteht weiter“.
3. **„Heizung 1 h“ / „Heizung aus“** (one button by the forced state; the app does not know the counter, so: after „Heizung 1 h“ the button reads „Heizung aus“ until the view is left or the status shows the heater output off for a pack the app forced). Text at the button: „Heizt eine Stunde lang ohne Rücksicht auf Temperatur und Ladegerät und entnimmt dabei dem Pack Energie; nur Packs mit Heizfolie heizen.“ After the command the status is read again and the real heater output shown.
4. **„Restkapazität setzen“**: field in Ah (0.01) with the SOC it gives beside it („= 42 % von 280,00 Ah“, full capacity from 0x42). Enabled when 0 ≤ value ≤ full capacity. A value under P79 (SOC protection, from the pack's parameters when read, else „unter der SOC-Schutzgrenze“) shows: „Unter x % sperrt das BMS das Entladen.“ After success read telemetry again and show the new SOC. Text under the field: „Bleibt erst gespeichert, wenn das BMS seinen Stand sichert (z. B. nach 5 % SOC-Änderung); ein Neustart vorher stellt den alten Wert wieder her.“ RTN 06 → „Das BMS hat den Wert abgelehnt (größer als die Vollkapazität)“.
5. **„Historie und Zähler löschen …“** (at the end of the block, plain button): dialog with what is lost, read from the pack just before: „Pack 00: 123 Zyklen, 4,5 MWh geladen, 4,3 MWh entladen, bis zu 500 Einträge der internen Historie werden gelöscht. Das lässt sich nicht rückgängig machen. Die Zähler gehören zu den Zellen: Löschen passt, wenn die Platine an andere Zellen kommt oder einzeln weitergegeben wird.“ Confirm button „Löschen“ (red). After success read status/telemetry again (cycles 0). The app's own copies (recording, saved BMS history files) stay.

Help topic `topic.control` (DE/EN): what each action does, that „off“ blocks only discharging, the master broadcast, why „on“ may not hold, RAM-only 0x49, that clearing resets the cycle count for good.

Backend: `openbms-proto` `control.rs` (`restart_request`, `on_off_request`, `heater_request`, `clear_request`, `set_remaining_request`; RTN mapping as in calibration: RTN 06 refused, RTN 04 invalid), `openbms-transport` methods with the same names; Tauri commands `pack_restart`, `pack_on_off(on)`, `pack_heater(on)`, `pack_clear_history`, `pack_set_remaining(raw)` in a new `control.rs` beside `bms_memory.rs`, using `pack_bus_only`, `bound`, `with_conn`, `refusable`. Preview mode simulates all of them.

## Out of scope

The frame log (own step after this). 0x49 index 58. Broadcast (`FF`) commands: always one pack.

## Review r1 questions (answered in the review, kept for reference)

1. Restart detection: is „silent for some of the first 2 s, then answering“ a sound test, or can a pack that did not restart also miss a 0x51 there (e.g. while it answers a poll of the app)?
2. Master in maintenance halt or with DIP 5–8 OFF (effective slaves 0): does an „off“ sent then reach the slaves at all, now or after „Master freigeben“ (`0x944C` clears the broadcast flags)? If never, the master text in the off dialog is wrong and the bank-wide off cannot happen through the app.
3. Heater type 3: does forcing bypass BitGroup3 bit 6 and the „charging / charger present“ condition of the heater output (`0x7350`)? If not, the button does nothing on most packs and the hint must say so.
4. 0x49 index 59 while charging or discharging: any race with the coulomb counter or the 100 %/0 % SOC rules that makes the set value jump at once?
5. Type 5 while a history read (0x4B) of the app runs on another pack: harmless? Anything else type 5 resets that the dialog should name?
