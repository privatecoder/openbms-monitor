---
title: Patched firmware: release bits
short: Two bits in function switch 0 allow writing via the CAN socket (bit 0) and the master's maintenance start (bit 2).
related: topic.firmwareProfile, topic.switches, topic.paramWrite
---
- **Bit 0, writing via the CAN socket:** 1 allows it, 0 locks it; the pack then takes nothing via the CAN socket (reading goes on). When it is off the app does not even send and shows “Writing locked” in the column. If you switch it off via the CAN socket yourself, the app explains the way back first: unlock only via RS485-1/2 (in a bank with the master halted, or a single pack) or on the display.
- **Bit 2, maintenance start:** 1 allows “Halt master” via the CAN socket, 0 refuses it; “Release master” always works. It cannot be changed via the CAN socket (the pack would refuse the whole write); the app shows it locked there. Changeable via RS485-1/2 or on the display.
- **Both locked on a master that polls:** then the CAN socket no longer helps. For RS485-1/2 release the bus by hand (DIP 5–8 on site) or use the display.
- **When the app interprets the bits:** only with a proof for exactly this pack: for the master its firmware identity on this connection; for a slave an entry for its board (BMS serial number), which the app creates after its own flash with a patched image or which you set with “Confirm patched firmware”. Every new flash of this pack by the app removes the proof. A flash outside the app cannot be seen: then remove the proof with “take back”.
- **Without proof:** the bits read “meaning only with proven patched firmware”, and a write via the CAN socket keeps them as the pack has them. If a pack still refuses with E3, writing was locked there: nothing taken, read the state again.
- **Updating to patched firmware:** the flash takes the existing bits over unchanged. Older values with bit 0 or 2 at 0 lock afterwards; the firmware view shows the state beforehand (freshly read, else the last stored one with its date, unchecked).
