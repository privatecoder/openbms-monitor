---
title: Inverter connection
short: Only one device may poll an RS485 bus. So the app never sends on the bus the inverter polls, and on RS485-1/2 it listens before a connection's first request. Without this setting it sends nothing at all.
related: topic.buses, topic.connection
---
- **CAN (pack 00 collects the values):** the usual setup. With several packs pack 00 is master (DIP 5–8 = number of slaves) and polls the slaves on RS485-1/2; the app reads via the CAN socket, RS485-1/2 only after the master's maintenance stop. A single pack is reachable on both buses.
- **CAN socket RS485 at pack 00 (system values):** the inverter polls pack 00 with Modbus or Pylontech system commands. The CAN socket is taken. A single pack is reachable over RS485-1/2; with several packs pack 00 also polls its slaves there, so no bus is free while inverter and master run.
- **RS485-1/2, every pack itself:** the setup of the maker's RS485 address scheme (DIP 5–8 off everywhere), an external computer or inverter polls the packs. RS485-1/2 is taken, the app uses the CAN socket; it reaches every pack there only if their CAN socket RS485 is connected. No device has shown an inverter polling this way.
- **CAN socket RS485, every pack itself:** the CAN socket is taken, the app uses RS485-1/2. No specific inverter is shown here either.
- **No inverter:** both buses free as long as no Seplos master polls on RS485-1/2.
- **Listening on RS485-1/2:** before a connection's first request the app listens for a little more than a second. If a master polls its slaves there or another device sends, it sends nothing. This also catches a single pack that has slaves set anyway.
- **Inverter paused:** for an update or maintenance over the inverter's bus: cut its communication and tick the box. For this session only; switch it off before the inverter is connected again. The setting cannot change during an update.
- **Applies to everything:** the rule sits in one place every request of the app passes: connect, search, live polling, maintenance, control and update.
