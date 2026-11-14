---
title: Guide: bring a bank to new firmware
short: Step by step from the port to the checked update: which case fits the bank, in which order to flash and what to do when something stops.
related: topic.firmware, topic.maintenance, topic.patchGates, topic.wiring
---
The guide gives the order, the decision and the expected result; conditions and background are in the linked topics. Flashing goes via the RS485-1/2 bus.

1. At the bottom of the sidebar switch on the maintenance functions (wrench icon) and open the Firmware view. Expected: the tabs “RS485-1/2 bus” and “Flash a board”; for the bank the “RS485-1/2 bus” tab applies. → [Firmware update](#topic.firmware)
2. In the sidebar choose the bank's installation (not “View only”). Under Installations → Ports it needs an endpoint for the RS485-1/2 bus; “Halt master” (case C below) works via the CAN socket, so for that it needs this port too. Expected: the tab shows both ports on top and the status line below them. → [Installations](#topic.installations)
3. Check that the installation says where the inverter is connected; without it the app sends nothing. If the inverter polls the bus the update needs, cut its communication and tick “Inverter communication is cut or paused” in the sidebar. → [Inverter connection](#topic.wiring)
4. Make the bus quiet. Do only the case that fits the bank: → [Maintenance on the RS485-1/2 bus](#topic.maintenance)
   - **A – single pack**
     1. Nothing to do: a pack without slaves does not poll. Expected: the status line shows “Bus quiet”.
   - **B – bank with original firmware on the master**
     1. Set DIP 5–8 of the master OFF and note the old positions. Expected: the status line turns green by itself (“Bus quiet”).
   - **C – bank with patched firmware on the master (EMU1101-EXT from Light 1.5.4 / Full 1.6.4)**
     1. Press “Halt master”. Expected: the master reports 0 slaves, the status line shows “Bus quiet”.
     2. If the master refuses, or the app does not offer “Halt master”, it names the reason; then make the bus quiet with DIP 5–8 as in case B and go on as in case B afterwards too. → [Patched firmware: release bits](#topic.patchGates)
5. Above the pack list choose the image once (“Choose .ehex file”). An image that is no released original firmware (EMU1101-EXT too) needs “Allow custom firmware”; with patched firmware the app first shows what the release bits mean afterwards. → [Firmware update](#topic.firmware)
6. Only in case C, decide right after choosing the image: if the new image for the master has no patch functions (original or custom firmware without maintenance functions), C′ applies at the end: neither pick nor flash the master now, only the slaves; it follows after “Release master”. The app shows this at the master's row and keeps “Check & flash” locked there. → [Maintenance on the RS485-1/2 bus](#topic.maintenance)
7. Wait for the pack list: every pack that answers is listed with its version and “Check & flash”; if one is missing, “Search again”. If the app reports another bank on the port, settle that first. → [Maintenance on the RS485-1/2 bus](#topic.maintenance)
8. Flash. Only one of the two ways: → [Maintenance on the RS485-1/2 bus](#topic.maintenance)
   - **One pack**
     1. In its row “Check & flash”: the app checks bus, installation and target and then writes the firmware right away, without a further dialog. Do not disconnect power or cable during the update.
   - **Several packs as a series**
     1. Tick the packs (or “All”; with C′ untick the master again), then “Flash selected”, type the confirmation shown and “Start series”: `FLASH <n> PACKS` with released original firmware, `FLASH CUSTOM <n> PACKS` with any other image, EMU1101-EXT too. Expected: the slaves by address first, the master last; every pack is checked before and compared after.
9. Read the result: a single update shows “Update complete” and the before/after comparison; a series at its end “Flashed · stopped · not flashed” and one line per pack. Expected: no difference. If something stops, go on with the last step. → [Firmware update](#topic.firmware)
10. Give the bus back to the master, only in the case from above (with C either C or C′): → [Maintenance on the RS485-1/2 bus](#topic.maintenance)
    - **A**
      1. Nothing to do.
    - **B**
      1. Set DIP 5–8 of the master back. Expected: the status line shows “Master polls N slaves” again.
    - **C – the master keeps patch functions (or was not flashed)**
      1. Under “Done” press “Release master”; if the app kept no slave count, it asks for it. Expected: “Marker ended”; whether the master polls again is shown by the status line.
    - **C′ – instead of C: the new master image has no patch functions**
      1. Set DIP 5–8 of the master OFF (note the old positions).
      2. Under “Done” tick “DIP 5–8 of the master are OFF” and press “Release master” while the patched firmware still runs. Expected: “Marker ended. The master works without slaves”, bus quiet; only then “Check & flash” at the master becomes free.
      3. Flash the master as above.
      4. Set DIP 5–8 of the master back. Expected: “Master polls N slaves”.
11. Only if something stops, the case that fits:
    - **Series stopped**
      1. Read the stopped pack's line: it names the reason; packs not reached read “not flashed”. Repeat nothing blindly. → [Maintenance on the RS485-1/2 bus](#topic.maintenance)
      2. Find the cause, then flash the remaining packs on their own or in a new series, and give the bus back as above afterwards.
    - **Series open but no longer running**
      1. For example after its view was closed: “End open series”. Only as a last resort restart the app, and only while no update runs. → [Maintenance on the RS485-1/2 bus](#topic.maintenance)
    - **Pack may hang in its bootloader**
      1. On top it says “Pack X on …: update interrupted …”. Keep the log (folder `firmware-logs`), repeat nothing blindly; while the note stands the app flashes no other pack. → [Firmware update](#topic.firmware)
      2. If the pack's application answers again (“Search again”), flash this pack first on its own.
      3. If it does not: connect only this one board to the port, in the “Flash a board” tab under “Bootloader recovery” press “Check bootloader”, then “Recover”. The bootloader knows no address, so really only this board.
      4. If the pack is disconnected or repaired, close the note with “Yes, pack X is disconnected or repaired”. → [Maintenance on the RS485-1/2 bus](#topic.maintenance)
