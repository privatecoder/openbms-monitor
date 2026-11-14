---
title: Parameters
short: The packs' settings: limits, delays, balancing and function switches. The app reads and compares them and writes them on a click, to one pack or the same change to several; also serial numbers, the BMS clock, the CAN protocol and the calibration. Visible only when maintenance functions are switched on.
related: topic.buses, topic.access, topic.switches, topic.paramWrite, topic.calibration, topic.control, guide.paramMulti
---
- **Read from packs:** over the ways below. On the CAN socket every pack reports at its address and by its own firmware, without halting the master; its column then says “CAN”. If the app reads its serial numbers there too (ways below), it checks the board as on the RS485-1/2 bus, stores the state and writes over every way the list names for writing. Without a unique BMS serial number it only reads, and nothing is stored. If a pack does not answer there, it lacks the patch or is not reachable; then use the RS485-1/2 bus. The release bits in function switch 0 are explained in “Patched firmware: release bits”. A **master** that polls its slaves (DIP 0 with slaves) does not answer on the RS485-1/2 bus. It becomes reachable once it stops polling: with patched firmware via “Halt master” in the maintenance area (then “Release master”), otherwise with DIP 5–8 set OFF or briefly alone on the bus; or take its values from a parameter file of the app.
- **Read parameters**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4.
  - UART2 · display/Bluetooth: the original firmware is enough; Group7.bit3 (display port on) = 1; only the board at the display port; the app does not use this way.
- **Write parameters and switches**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4; Group0.bit0 (UART1 write release) = 1.
  - UART2 · display/Bluetooth: the original firmware is enough; Group7.bit3 (display port on) = 1; only the board at the display port; the app does not use this way.
- **Read serial numbers**
  - UART0 · RS485-1/2: the original firmware is enough; returns battery and BMS serial number, up to 30 characters each; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4; returns battery and BMS serial number, up to 30 characters each.
  - UART1 · CAN socket: original firmware from 16.06.11; only at the master, address 0; returns only the battery serial number (String 1), its first 22 characters.
- **Open a parameter file:** TOML files this app saved (“Save as parameter file”). Each value stands under its technical key; name, unit and the explanation from this help sit next to it as comments and are ignored when opening. The app takes a file only if all 87 parameters and the eight switch bytes are present and valid (decimal point, switches 0–255). Exports from BatteryMonitor and files of earlier app versions (XML) are refused: then read the packs and save a new parameter file. If values look shifted, the app warns and does not offer the file for writing back.
- **Function switch group 0 when taking over:** from a file, another column or an earlier state only bit 1 comes over (the original firmware's only one). The EMU1101-EXT bits (releases, extended off, pack lock, hard stop) stay as the target pack has them, whatever firmware the source had. So a file fits packs with the original firmware and with EMU1101-EXT; set the releases in the table directly.
- **Comparison:** every source is a column. Values that differ from the majority are marked; “Only differences” hides everything equal.
- **Save:** a pack's read values can be saved as a parameter file (TOML) (folder `parameters` in the app data).
- **Last state (also without a connection):** shows per pack the last stored state of the chosen installation as a column, with its date and whether it was read or written. It is the state of the current board at that address; if there is none of it, the column shows the newest and says “previous board” or “board unclear”. Comparison, datasheet check and saving as a parameter file work with it; writing needs a connection with that installation. With “View only” you can read but not write.
- The names are corrected: BatteryMonitor machine-translates from Chinese (“Monomer” = cell, “pressure” = voltage, “Equalization” = balancing). Hover a name to see the original BatteryMonitor name.
