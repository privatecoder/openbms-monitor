---
title: Firmware update
short: Writes firmware to a single pack: released original versions, on request also custom images. Visible only when maintenance functions are switched on.
related: topic.buses, topic.connection
---
- **Why RS485-1/2:** the BMS bootloader listens only on this bus (19200 baud). No update over the CAN socket.
- **Why one pack on the bus:** the bootloader's frames carry no pack address. A master polling its slaves, or a second program, disturbs the update. The app first checks for about 2 s that the bus is quiet.
- **Sequence:** the app restarts the pack by command. For about the first 0.7 s the bootloader answers; the app logs in, erases, writes the blocks and finishes. Then the new firmware starts, and the app reads version and values as a check.
- **Released images:** unmodified original Seplos firmware (16.06.x), recognised by its checksum. The pack's parameters are kept.
- **Custom firmware:** with “Allow custom firmware” the app also accepts images with an unknown checksum if format and header match the EMU1101. It does not check the content. Confirm with `FLASH CUSTOM` and the address. The check afterwards works only if the new firmware answers on the pack bus at 19200 baud.
- **During the update:** do not disconnect power or cable. The app cannot be closed then; on macOS this does not cover *Quit* in the Dock, which cannot be intercepted. So do not quit the app from the Dock.
- **Several packs:** with several packs, prepare maintenance in the *Pack bus* panel first.
- **If something goes wrong:** an error before erasing changes nothing; the pack restarts its old firmware by itself. An error after that can leave the pack in update mode. Keep the log (folder `firmware-logs` in the app data) and do not just restart or blindly repeat the update. If only the final check fails, the new firmware has already started; check the pack's values in the monitor.
