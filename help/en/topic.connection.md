---
title: Connection
short: How the app connects to the batteries: via a USB-RS485 adapter or an RS485-to-Ethernet gateway (TCP).
related: topic.buses
---
- **Network (TCP):** gateway address as `host:port`, e.g. `192.168.1.10:4196`. The baud rate is configured on the gateway (CAN socket 9600, RS485-1/2 19200).
- **USB/serial:** select the adapter's port; the baud rate follows the selected bus.
- **Order after connecting:** on the CAN socket the app first reads the master's system values (Modbus) and shows them at once. If the master reports its slave count (only with the maintenance firmware, command 0x64), the app polls exactly these packs without a search. Otherwise the **Packs** setting applies: a fixed number starts polling addresses 0 to number−1 at once, “search” scans addresses 0–15. Then all packs are polled every 2 s; names and firmware versions follow shortly after the start. While the master's maintenance mode is active it reports 0 slaves; then the app searches.
- Without a valid answer for **60 s**, the connection counts as lost (“Connection lost”). If the gateway closes the connection or it fails with an error, it counts as lost at once. The app then reconnects automatically every 30 s until packs answer again.
- Only one program may poll the bus at a time. If e.g. the Home Assistant add-on uses the same gateway, requests collide.
