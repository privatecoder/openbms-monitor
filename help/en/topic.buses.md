---
title: The two RS485 buses
short: The packs have two separate RS485 buses. Which one is connected decides what the app can read and change.
related: topic.connection, value.system
---
| Bus | Baud | What works |
|---|---|---|
| **CAN socket (RS485)** (wires pins 1/2/3 or 6/7/8) | 9600 | values and messages of **all** packs at their DIP address, system values of the master (Modbus). With the original firmware no control commands; with EMU1101-EXT (Light 1.5.4 / Full 1.6.4) on the pack itself also parameters, reading serial numbers, clock, on/off and restart, and read-only diagnostics (BMS history, calibration, stored state, memory). |
| **RS485-1/2 bus** | 19200 | full command set: read and write parameters, control commands, history. The **master** (DIP 0 with slaves) **never** answers here because it polls its slaves itself. |

For monitoring several packs the CAN socket bus is ideal. Writing serial numbers, the CAN protocol, calibration, heater, remaining capacity and clearing the history work only via RS485-1/2, with the original firmware every change does.

**Serial numbers:** every BMS stores two: the **battery SN** (“Battery_SN” in BatteryMonitor) and the board's **BMS SN** (“BMS_SN”). On the RS485-1/2 bus the app reads both from every pack that answers there. On the CAN socket only the master shows the first 22 characters of its battery SN (firmware 16.06.11 and later); the BMS SN and the slaves' numbers are read there only from packs with EMU1101-EXT (each at its address). “Not set (placeholder)” means the factory entry `EMU110110S-XXXX-…` is still stored and nobody has assigned a number. The app reads the numbers once after connecting and changes nothing by itself; writing them is a separate action under Parameters (maintenance functions, RS485-1/2 bus only).
