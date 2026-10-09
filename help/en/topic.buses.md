---
title: The two RS485 buses
short: The packs have two separate RS485 buses. Which one is connected decides what the app can read and change.
related: topic.connection, value.system
---
| Bus | Baud | What works |
|---|---|---|
| **CAN socket** (RS485 wires pins 1/2/3 or 6/7/8) | 9600 | values and messages of **all** packs at their DIP address, system values of the master (Modbus). No parameters, no control commands. |
| **RS485-1/2** | 19200 | full command set: read and write parameters, control commands, history. The **master** (DIP 0 with slaves) **never** answers here because it polls its slaves itself. |

For monitoring several packs the CAN socket bus is ideal; changing settings requires RS485-1/2.
