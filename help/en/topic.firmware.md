---
title: Firmware update
short: Writes an original Seplos firmware to a single pack. Visible only when maintenance functions are switched on.
related: topic.buses, topic.connection
---
- **Why RS485-1/2:** the BMS bootloader listens only on this bus (19200 baud). No update over the CAN socket.
- **Why one pack on the bus:** the bootloader's frames carry no pack address. A master polling its slaves, or a second program, disturbs the update. The app first checks for about 2 s that the bus is quiet.
- **Sequence:** the app restarts the pack by command. For about the first 0.7 s the bootloader answers; the app logs in, erases, writes the blocks and finishes. Then the new firmware starts, and the app reads version and values as a check.
- **Released images only:** unmodified original Seplos firmware (16.06.x), recognised by its checksum. The pack's parameters are kept.
- **If something goes wrong:** an error before erasing changes nothing, the old firmware keeps running. An error after that can leave the pack in update mode. Keep the log (folder `firmware-logs` in the app data) and do not just restart or blindly repeat the update.
