# Firmware update in the app: design

Status: draft for review, 2026-10-11.

## Goal

Make the existing `openbms-flash` updater usable from the desktop app as a guided flow, without weakening its safety checks and without turning the monitor into a writing tool by default.

Success: a user with a USB-RS485 adapter on the pack bus of a single pack can select an original vendor image, check the pack, confirm and flash, see progress, and gets a log file. The result matches a CLI flash byte for byte (same frames, same timing).

## Scope of the first version

- One pack, serial connection (USB-RS485) on RS485-1/2 at 19200 baud, quiet bus. This is the only case verified on hardware.
- Not offered yet: TCP gateway, several packs on one bus, flashing a master that polls slaves, retries or recovery after an aborted update. They are enabled once measured on the bench.
- Same image allowlist as the CLI (`known::RELEASES`, 16.06.x originals).

## User interface

**Maintenance switch.** A checkbox "Show maintenance functions" (de: "Wartungsfunktionen anzeigen") at the bottom of the sidebar, stored in `localStorage` (`maint.enabled`, default off). Only when it is on, the navigation shows "Firmware". Default installs therefore stay read only.

**View `Firmware`** (`app/src/views/Firmware.tsx`), a single page with numbered steps that unlock one after another:

1. **Connection.** Serial port (same port list as the sidebar) and pack address 0–15 (default 0). Fixed hint: pack bus RS485-1/2, 19200 baud, only this pack on the bus. If the monitor is connected, the step says that checking the pack will disconnect it.
2. **Image.** File dialog for `.ehex`. The backend validates it (parser + allowlist) and shows release, size, block count and code SHA-256. Unknown or modified images are rejected with the reason.
3. **Check pack.** Disconnects the monitor (stops polling and recording), opens the port at 19200 baud and runs `session::inspect`: quiet bus, 0x51 and 0x47 answer. Shows device name, current firmware version and CAN protocol. The port is closed again afterwards.
4. **Confirm.** Summary "pack N: version X → release Y" and the risks in plain words (no valid application from `W F E` until `W F G`; no retry of blocks; do not disconnect power or cable). The start button is enabled only after typing exactly `FLASH N`.
5. **Progress.** Stage text and a progress bar over the image's blocks (209 for 16.06.13, ≈ 110 s), elapsed time. No cancel button: before `W F E` the run lasts well under a second, after `W F E` cancelling would leave the board without an application.
6. **Result.** Success: version before and after (from the post-check 0x51), log path with "show in folder", button "back to monitor". Failure: the error text in the user's language, the stage it happened in, the warning not to restart blindly if it happened after `W F E`, and the log path.

While a flash runs, navigation and the sidebar connection controls are disabled, and closing the window is refused with a notice.

In the browser demo the view is visible but its actions are disabled with a note that it needs the desktop app.

Help: one new topic `topic.firmware` in `tools/gen_help.py` (de/en), linked by an `InfoIcon` in the view header: why the pack bus and not the CAN socket, why one pack and a quiet bus, what the bootloader window is, what to do after an error. All strings in `i18n.ts` in German and English. Examples generic.

## Backend

### `openbms-flash`

- New `session::Progress` enum: `Preflight`, `Bootloader`, `Erase`, `Block { index, total }`, `Finalize`, `AppRestart`, `PostCheck`.
- New `session::flash_with_progress(link, address, image, log, options, progress: &mut dyn FnMut(Progress))`. The existing `flash` calls it with a no-op closure, so the CLI and its tests stay unchanged. The sequence of frames and the timing do not change.
- `FlashResult` gains `after: DeviceInfo`: `wait_for_app` already queries 0x51 but discards the answer; it now decodes and returns it.

### Tauri (`app/src-tauri/src/firmware.rs`, new module; `lib.rs` only registers commands)

- `firmware_check_image(path) -> ImageInfo { release, device, bytes, blocks, sha256 }`: read, parse, identify. Nothing else is kept; the path is passed again on start.
- `firmware_inspect(port, address) -> DeviceInfo`: stops polling, drops the monitor connection (same effect as `disconnect`), opens the port at 19200 baud, runs `session::inspect` with a log, closes the port.
- `firmware_start(port, address, path, confirmation) -> ()`: checks `confirmation == "FLASH {address}"` again, re-validates the image, refuses if a flash is running, sets `AppState.flashing`, then runs the flash on its own thread and returns at once. Emits `firmware-progress` events (`Progress` serialised) and one `firmware-done` event `{ ok, error, stage, before, after, log }`.
- Log files: app data folder `firmware-logs/<UTC timestamp>_pack<N>_<release>.log`, created new (never overwritten); same format as the CLI log plus the `CONFIRMED` line. `reveal_firmware_logs(path)` like the existing reveal commands.
- `connect` and `start_polling` refuse while `flashing` is set. The window close request is prevented while `flashing` is set and a `firmware-busy` event tells the UI to show the notice.

## Error handling

All errors use `Error::text(lang)` so the UI gets them in the user's language; the language is passed with each command. Errors before `W F E` (port, quiet bus, not the pack bus, no bootloader) leave the board running its old application and say so. Errors from `W F E` on keep the CLI wording ("board may be in update mode; do not retry a block blindly") and point to the log.

## Testing

- `openbms-flash`: test with the simulator that `flash_with_progress` reports the stages in order, exactly `total` block events with consecutive indices, and produces the same frames as `flash`; `after` is filled.
- Tauri module: confirmation check and log naming as plain functions with unit tests; the rest is thin glue.
- Frontend: the step gating (which step is enabled for a given state) as a pure function with a Vitest test.
- Manual: one flash of the bench board from the app, log compared to a CLI log (same TX frames).

## Documentation

- README: firmware section describes the app view and the maintenance switch; replace "a real flash has not yet been verified" with the verified case (one pack, serial).
- CLAUDE.md: the monitor is read only unless maintenance functions are enabled; layout gains `firmware.rs` and `Firmware.tsx`.
