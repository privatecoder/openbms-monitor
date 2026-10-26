---
title: Inverter connection
short: Only one device may poll an RS485 bus. Under Installations you say per installation where the inverter is connected; the app never sends on the bus it polls, and on RS485-1/2 it listens before a connection's first request. Without the setting it sends nothing for that installation.
related: topic.buses, topic.connection, topic.installations
---
- **CAN-H/L of the CAN socket:** the usual setup. With several packs pack 00 is master (DIP 5–8 = number of slaves) and polls the slaves on RS485-1/2; the app reads via the CAN socket's RS485 pair, RS485-1/2 only after the master's maintenance stop. A single pack is reachable on both buses.
- **RS485 pair of the CAN socket:** the inverter polls there, either pack 00 for system values (Modbus, Pylontech) or every pack itself. That bus is taken, the app uses RS485-1/2. If pack 00 also polls its slaves there as master, the app hears it before its first request and sends nothing.
- **RS485-1/2:** an external computer or inverter polls the packs (the maker's RS485 address scheme, DIP 5–8 off everywhere). The app uses the CAN socket's RS485 pair; it reaches every pack there only if their CAN socket RS485 is connected. No device has shown an inverter polling this way.
- **No inverter:** both buses free as long as no Seplos master polls on RS485-1/2.
- **View only and flashing a board:** without an installation there is no setting; you choose a free port yourself. The app listens briefly on that port before its first request and sends nothing if something already sends there.
- **Listening on RS485-1/2:** before a connection's first request the app listens for a little more than a second. If a master polls its slaves there or another device sends, it sends nothing. This also catches a single pack that has slaves set anyway.
- **Inverter paused:** for an update or maintenance over the inverter's bus, cut its communication and tick the box in the sidebar. For this session only; switch it off before the inverter is connected again. A change during a running action takes effect after it.
- **Applies to everything:** the rule sits in one place every request of the app passes: connect, search, live polling, maintenance, control and update.
