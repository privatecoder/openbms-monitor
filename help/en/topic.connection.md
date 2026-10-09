---
title: Connection
short: How the app connects to the batteries: via a USB-RS485 adapter or an RS485-to-Ethernet gateway (TCP).
related: topic.buses
---
- **Network (TCP):** gateway address as `host:port`, e.g. `192.168.1.10:4196`. The baud rate is configured on the gateway (CAN socket 9600, RS485-1/2 19200).
- **USB/serial:** select the adapter's port; the baud rate follows the selected bus.
- After connecting, the app scans addresses 0–15 and then polls all packs found every 2 s.
- Only one program may poll the bus at a time. If e.g. the Home Assistant add-on uses the same gateway, requests collide.
