---
title: Firmware update
short: Writes firmware to the packs on the pack bus: released original versions, on request also custom images. Visible only when maintenance functions are switched on.
related: topic.maintenance, topic.buses
---
- **Why RS485-1/2:** the BMS bootloader listens only on this bus (19200 baud). No update over the CAN socket.
- **Why the bus must be quiet:** the bootloader's frames carry no pack address. A master polling its slaves, or a second program, disturbs the update. How to make a bank's bus quiet is explained under *Maintenance on the pack bus*.
- **What the app checks right before every update:** no byte on the bus for about 2 s, then the target pack answers 0x51 and 0x47 at its address. Nothing has to be ticked; if a check fails, the pack stays unchanged.
- **Sequence:** the app restarts the pack by command. For about the first 0.7 s the bootloader answers; the app logs in, erases, writes the blocks and finishes. Then the new firmware starts, and the app reads version and values as a check.
- **Released images:** unmodified original Seplos firmware (16.06.x), recognised by its checksum. The pack's parameters are kept. The project's maintenance builds (16.06.13 MAINT 1.1 and 1.0) are recognised by their checksum too; they are not original firmware and need “Allow custom firmware”.
- **Custom firmware:** with “Allow custom firmware” the app also accepts images with an unknown checksum if format and header match the EMU1101. It does not check the content. The check afterwards works only if the new firmware answers on the pack bus at 19200 baud.
- **Two ways:** in the pack list (“Flash”, then “Flash now”) or below under “Single pack (manual)”: choose the address, check the pack and confirm with `FLASH` and the address (custom firmware: `FLASH CUSTOM`). Both ways run through the same checks.
- **During the update:** do not disconnect power or cable. The app cannot be closed then; on macOS this does not cover *Quit* in the Dock, which cannot be intercepted. So do not quit the app from the Dock.
- **If something goes wrong:** an error before erasing changes nothing; the pack restarts its old firmware by itself. An error after that can leave the pack in its bootloader: the app remembers it (see *Maintenance on the pack bus*, “pack may hang in its bootloader”). Keep the log (folder `firmware-logs` in the app data) and do not just restart or blindly repeat the update. If only the final check fails, the new firmware has already started; check the pack's values in the monitor.
