---
title: The two RS485 buses
short: The packs have two separate RS485 buses. Which one is connected decides what the app can read and change.
related: topic.connection, value.system, topic.access
---
- **CAN socket (RS485)** (wires pins 1/2/3 or 6/7/8), 9600 baud: values and messages of **all** packs at their DIP address and the system values of the master (Modbus). Every pack answers there with its own firmware; with the original firmware there are no control commands there.
- **RS485-1/2 bus**, 19200 baud: the original firmware's full command set. The **master** (DIP 0 with slaves) **never** answers here while it polls its slaves itself.

For monitoring several packs the CAN socket bus is ideal. Where the buses differ (every way under “Access paths”):

- **Read parameters**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4.
  - UART2 · display/Bluetooth: the original firmware is enough; Group7.bit3 (display port on) = 1; only the board at the display port; the app does not use this way.
- **Write parameters and switches**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4; Group0.bit0 (UART1 write release) = 1.
  - UART2 · display/Bluetooth: the original firmware is enough; Group7.bit3 (display port on) = 1; only the board at the display port; the app does not use this way.
- **Read internal history**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with read capability history (bit 0 of the firmware identity).
- **Read calibration**
  - UART0 · RS485-1/2: EMU1101-EXT from Light 1.5.4 with read capability calibration pages (bit 1 of the firmware identity); note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with read capability calibration pages (bit 1 of the firmware identity).
- **Read stored state and energy counters**
  - UART0 · RS485-1/2: EMU1101-EXT from Light 1.5.4 with read capability stored state or energy counters (bit 2 or 3 of the firmware identity); note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with read capability stored state or energy counters (bit 2 or 3 of the firmware identity).
- **Read the memory diagnosis**
  - UART0 · RS485-1/2: EMU1101-EXT from Light 1.5.4 with memory diagnosis (image flag bit 5); note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with memory diagnosis (image flag bit 5).
- **Set clock**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with power/RTC (image flag bit 1); release in Group0.bit3 (power/RTC), value per build (see function switch 0).
- **Restart**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with power/RTC (image flag bit 1); release in Group0.bit3 (power/RTC), value per build (see function switch 0).
- **On/off**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with power/RTC (image flag bit 1); release in Group0.bit3 (power/RTC), value per build (see function switch 0).
- **Write serial numbers**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
- **Set CAN protocol**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
  - UART2 · display/Bluetooth: the original firmware is enough; Group7.bit3 (display port on) = 1; only the board at the display port; the app does not use this way.
- **Calibrate**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
- **Force heater**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
- **Set remaining charge**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.
- **Clear history and counters**
  - UART0 · RS485-1/2: the original firmware is enough; note: reliable only without a polling master.

**Serial numbers:** every BMS stores two: the **battery SN** (“Battery_SN” in BatteryMonitor) and the board's **BMS SN** (“BMS_SN”). Which way returns which:

- **Read serial numbers**
  - UART0 · RS485-1/2: the original firmware is enough; returns battery and BMS serial number, up to 30 characters each; note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4; returns battery and BMS serial number, up to 30 characters each.
  - UART1 · CAN socket: original firmware from 16.06.11; only at the master, address 0; returns only the battery serial number (String 1), its first 22 characters.

“Not set (placeholder)” means the factory entry `EMU110110S-XXXX-…` is still stored and nobody has assigned a number. The app reads the numbers once after connecting and changes nothing by itself; writing them is a separate action under Parameters (maintenance functions).
