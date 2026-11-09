# OpenBMS Monitor

Cross-platform monitor for the **Seplos BMS V2.0** (boards 10C/10E, module EMU1101, SH Energy ETECH firmware), as a replacement for the vendor's Windows-only BatteryMonitor. German and English UI, light and dark theme, help for every value behind an info icon.

Status: **pre-release (0.3.5-beta)**. Monitoring of multi-pack banks is in daily use. Release builds for macOS (Apple silicon) and Windows 10/11 (x64 and arm64) are on the releases page; they are not signed, so macOS and Windows warn on first start. The monitor is read only; with maintenance functions on, the Parameters view writes a pack's parameters, the serial numbers, the BMS clock, the CAN protocol and the calibration on a click and controls a pack (restart, on/off, heater, remaining capacity, clearing history and counters), and firmware updates (CLI `openbms-flash`, or the app's maintenance view) write only after explicit confirmation. Protocol knowledge: [seplos-emu1101-docs](https://github.com/privatecoder/seplos-emu1101-docs).

## Features

**Installations**
- Everything the app knows about a bank belongs to an **installation**, chosen in the sidebar before connecting: recording, the boards identified per address (by the BMS serial number, with board swaps noticed), cell types, groups and pairs, diagnosis, parameter states and firmware logs. A gateway linked to exactly one installation chooses it; a USB port never changes the choice. **View only** connects without storing or writing anything. **Right bank:** after connecting, each pack's BMS serial is compared with the installation; boards it does not know yet are recorded only after one confirmation in the sidebar, a board of another installation shows a red line (disconnect, connect with that installation, or take it over) and nothing is stored under the chosen installation meanwhile. Every write reads the target's serial first and refuses an unconfirmed board or a silent pack; packs without a serial are named with a hint to set one.
- **Installations view:** list with rename, merge (adds, never overwrites; what was not taken is kept in a log), archive and delete (only without open work, with the list of what goes); per installation a pack table with the current and previous board, cell type, last parameter state and last firmware update. Data of earlier versions (kept by endpoint) is moved once into installations of their own, marked “taken over”, with a one-click merge where the endpoint belongs to one installation.
- **Export / import:** an installation as one ZIP file (boards, cell types, groups and pairs, diagnosis, parameter states, firmware logs; the recording with its messages on request), checked on import (known entries only, size limits, SHA-256 per file) and imported additively: into the same installation, as a new one, or merged by the master's serial number. Repeating an import changes nothing; an interrupted one is resumed.

**Monitoring**
- Connects via a USB-RS485 adapter or an RS485-to-Ethernet gateway (TCP), on either bus of the packs (see below). Finds packs at addresses 0–15 and polls them continuously. The desktop app sends nothing on the bus configured as the inverter's (see [Inverter connection](#inverter-connection)).
- **Updates:** the app looks for a newer release on GitHub (at start and every 12 hours, or by hand in the About view) and, on a click, downloads the build for this platform, checks its signature, installs it and restarts; all data are kept.
- Connection watchdog: without data for 60 s the connection counts as lost and is retried after 1, 2, 4, 8 and 16 min, then every 30 min, ten times in all (about 3 h); this also runs while the window is hidden.
- **Overview:** the master's system values (total current, SOC, charge and discharge limits with the per-cell voltage limits, bank-wide cell voltage and temperature extremes with their pack) and all packs with state, SOC, messages, balancing and cycles. Packs can be arranged in groups and towers.
- **Pack detail:** cell voltages around that pack's median cell, temperatures, current, capacity, energy, active messages.
- **Notifications** (system notifications, per installation): a protection or fault alarm starts, a pack stops answering, the rest cell spread stays above a threshold, or the connection is lost for the set minutes. Each fires once until its cause cleared, rate-limited per pack and trigger.

**Analysis**
- **Pair check** for packs wired in pairs: compares the current split and the output voltages inside each pair (60 s mean, with hysteresis, judged from 20 A per pack), explains equalizing currents by state of charge and flags contact resistance from 100 mV output difference.
- **Resistance diagnosis** (maintenance functions): zero at rest, then inner and outer resistance per pack under load against the busbar or the pair's load switch.
- **Recording:** optional (off by default), into an SQLite history database while connected: all packs or a selection, the master's system values and every message with start and end. Every poll is kept 30 days, then merged into minute values kept forever (both configurable).
- **History:** the database as synchronized time charts of the chosen installation (also without a connection) and span: per pack current, current share, SOC, voltage, cell extremes, cell difference and temperatures; the master's current with charge/discharge limits, SOC, voltage and cell extremes; all cells of one pack. Drag to zoom, resolution follows the zoom. Messages in the span. Export as CSV or JSON Lines, import of earlier JSON Lines recordings.
- **Health report:** from the recording, over 7, 30 or 90 days or everything: per pack cell spread, cycles and temperature, per pair the current share.
- **Cell analysis** (pack detail, from the recording): per cell the deviation from the median cell at rest, at the top, at the bottom and under load, the resistance from load steps, and a classification that says what the data allow. External active balancers can be defined per installation; their activity is computed from the recorded cells with their settings and shown as a timeline, and their working rows are left out of the analysis.
- **BMS history** (maintenance functions): reads a pack's internal event history with CSV export, via RS485-1/2 from every pack and via the CAN socket from packs whose firmware identity reports it (MAINT 1.5.4/1.6.4), there while the master keeps polling; the app's polling pauses meanwhile.
- **Parameters** (maintenance functions): reads all parameters and function switches of the packs (RS485-1/2; on the CAN socket from packs with patched firmware, MAINT 1.5.3/1.5.4 or 1.6.3/1.6.4, stored and written after checking the board; the release bits of function switch 0 are named only where the pack's patch version is proven) or opens parameter files, and compares them side by side with the differences marked. Corrected names and help for every parameter and switch bit. **Parameter file:** the app saves its own TOML file (format `openbms-parameters`), with every value under its readable key and the help as comments; import is strict (unknown, missing, duplicate or invalid entries are refused). BatteryMonitor XML exports are refused with an explanation, since their names and order are inconsistent and partly wrong: save a parameter file from the app instead. Files carry over between original and MAINT firmware both ways. A read pack column can be edited and written, to one pack or, with *Also to other packs*, one after another to several (each target gets the confirmed changes applied to its own values, never a copied foreign block): take over the values of another pack or a file, a change list (taking over from a file, another column or a previous state changes only bit 1 of function switch 0; the other bits stay as the target pack has them), the firmware's own plausibility rules checked before sending, a warning when the write resets the learned full capacity, read-back with values not taken marked, and the previous state of the last write kept per installation, pack and board. Without a connection the view shows the last stored state of every pack of the chosen installation (compare, datasheet check and saving a parameter file work offline). For a chosen pack the view writes the serial numbers, sets the BMS clock and the CAN inverter protocol (1–6) and calibrates current zero point, current and voltage (collapsed expert section with live values). „Control“ restarts a pack and watches it come back, switches it off and on (with original firmware and MAINT 1.5.3 off blocks discharging only; MAINT 1.6.3 with its pack lock enabled stores a lock of charging and discharging that survives restarts and only a targeted on clears; on the master 0, off and on act on the whole bank, and on at the master does not unlock individually locked slaves), forces the heater for an hour, sets the remaining capacity with the SOC it gives, and clears the pack's history and counters after a confirmation that names what is lost; it shows the off command apart from the real MOSFET and heater outputs. It also writes the device name, chooses the hard stop of MAINT 1.6.3 and reads the storage diagnosis of patched packs; with MAINT 1.5.4/1.6.4 a remote diagnosis reads the calibration (active and both stored copies side by side, differences marked), the stored capacity and SOC against the live values and the full-width energy counters, also via the CAN socket while the master polls (the stored state also shows in the restart dialog and, read on the flash target, before a flash; information only); the Diagnosis view records the bus frames with a text export. Every write and history read holds its pack until it ends, so two actions on one pack never interleave.
- **Parameters against the datasheet:** alarm and protection limits (cell and pack voltage, charge and discharge temperature, currents, rated capacity) checked against the assigned cell type, violations marked red.
- **Cell database and cell types:** bundled datasheet data of common LiFePO4 cells plus own entries. Packs get a cell type (all, by group or single, with cells in parallel); each pack then shows its live values against the datasheet (cut-off voltages, charge and discharge temperature, continuous and temperature-derated charge current), and the overview shows the charge current the cells allow next to the master's charge limit.

## The two buses

| Bus | Baud | What the app gets |
|---|---|---|
| RS485 pins of the **CAN socket** | 9600 | telemetry and messages of every pack, system values of the master (Modbus). Recommended for monitoring. |
| **RS485-1/2** | 19200 | the full command set (parameters, history). A master (DIP 0 with slaves) does not answer here. |

## Inverter connection

A bus can have only one master. Each installation records how the inverter is connected (Installations view): CAN-H/L of the CAN socket, the RS485 pair of the CAN socket, RS485-1/2, or no inverter. The app does not detect the inverter; it follows this setting, enforced in one place for every request the desktop app sends (the CLI does not apply it):

- The bus set as the inverter's stays closed to the app. For maintenance, cut the inverter's communication and set „paused“ in the sidebar for this session.
- On RS485-1/2 the app first listens briefly (1.2 s) and sends nothing if another device is talking (a polling master, a computer, a gateway). This is a snapshot: a device with longer pauses can go unheard.
- An installation without this setting sends nothing; the sidebar links to the setting. **View only** (no installation) only listens briefly on the chosen port before its first request.

## Layout

| Path | Content |
|---|---|
| `crates/openbms-proto` | protocol library: ASCII frames (0x42 telemetry, 0x44 status, 0x47 parameters, 0x51 device info, 0x5A intra-pack), Modbus RTU (system values 0x1000), tests with captured frames |
| `crates/openbms-transport` | serial (USB-RS485) and TCP (gateway) transport |
| `crates/openbms-cli` | read-only command line tool `openbms` (`ports`, `scan`, `telemetry`, `status`, `info`, `serial`, `params`, `system`) |
| `crates/openbms-flash` | standalone firmware probe, image dry-run and guarded flasher for EMU1101 on the 19200 baud pack bus |
| `app/` | desktop app: Tauri 2 + React 19 + TypeScript + Tailwind 4 |
| `data/` | cell database and its JSON schema |
| `help/` | help content (de/en), generated by `tools/gen_help.py` |
| `CHANGELOG.md` | changes per version, newest first; also shown in the app's About view and used as the release notes |
| `tools/` | generators for the help and the parameter file comments (`gen_help.py`, `param_help.py`) |
| `.github/workflows/` | release build: a pushed tag `v*` builds macOS arm64 (DMG) and Windows x64/arm64 (NSIS installer) and publishes them as a pre-release; a manual run keeps them as workflow artifacts |

## Build

```sh
cargo test                      # protocol and app backend tests
cargo run -p openbms-cli -- --tcp 192.168.1.10:4196 --bus can scan
cd app && npm install
npm test                        # UI logic tests
npm run dev                     # browser preview with a recorded 12-pack system
npm run tauri dev               # desktop app
```

## Firmware updater

**In the app:** switch on the maintenance functions with the wrench button at the bottom of the sidebar; the *Firmware* view then lists the packs on the pack bus with a *Flash* button each (see Maintenance below), or flashes a single pack by address with pack check and typed confirmation (`FLASH <address>`). It accepts the allowlisted original images; with *Allow custom firmware* switched on it also accepts an unknown code hash after the same format and header checks, confirmed with `FLASH CUSTOM <address>`. It uses RS485-1/2 (19200 baud) on a quiet bus, through a USB adapter or a network gateway (flashing through a gateway has not been tested on hardware), and writes a log to the app data folder (`firmware-logs`), named with the installation and listed at its pack in the Installations view. While it runs, the monitor is disconnected and closing the window or quitting via the menu or Cmd+Q is refused. On macOS, *Quit* from the Dock cannot be intercepted: do not quit the app from the Dock during an update.

`openbms-flash` uses the **RS485-1/2 pack bus at 19200 baud** (`--serial` or `--tcp` for a gateway), not the CAN-socket RS485 pair. By default it accepts the six validated original EMU1101 `.ehex` releases in its built-in allowlist (16.06.x). `--allow-custom-firmware` permits an unknown code hash after the same strict `.ehex` format and device-header checks. The app's Firmware view (maintenance functions) offers the same guarded flow, including custom firmware behind its own switch and confirmation phrase. The app's flow has flashed original and MAINT images on real packs; flashing through a network gateway has not been tested on hardware yet.

```sh
cargo run -p openbms-flash -- dry-run --image vendor.ehex
cargo run -p openbms-flash -- dry-run --image custom.ehex --allow-custom-firmware
cargo run -p openbms-flash -- --serial /dev/cu.usbserial-XXXX probe --address 0 --log probe.log
cargo run -p openbms-flash -- --serial /dev/cu.usbserial-XXXX flash --address 0 --image vendor.ehex --log flash.log
cargo run -p openbms-flash -- --serial /dev/cu.usbserial-XXXX flash --address 0 --image custom.ehex --allow-custom-firmware --log custom-flash.log
# If measured behavior on another board requires a pause after each reply:
cargo run -p openbms-flash -- --serial /dev/cu.usbserial-XXXX --frame-gap-ms 100 flash --address 0 --image vendor.ehex --log flash-with-gap.log
```

`dry-run` does not open a port. `probe` reads the device, sends an application reset and `R F L`, then waits for the application to return; it sends no flash write. `flash` validates and precomputes every block before opening the port, checks the target and pack bus, then requires the exact interactive phrase `FLASH <address>` for a known image or `FLASH CUSTOM <address>` for an unknown code hash before sending anything to the bootloader. The log file must be new and records TX/RX bytes and state markers. No timed-out write block is retried. After `W F E`, an error may leave the board in update mode; preserve the log and do not blindly restart the command.

The flasher waits for each bootloader reply, then sends the next frame without a fixed pause by default. The 0 ms timing completed a full 16.06.13 lab flash in 33.4 seconds on one board using an independently checked test script. `--frame-gap-ms` accepts 0–1000 ms after each complete reply; increase it if measurements on another board require a pause. The 50 ms reset-to-`R F L` delay and 10 s reply timeout remain fixed. Each bootloader reply time is logged, and a successful flash prints the minimum, median and maximum `W F D` response times. The default allowlist excludes the available 16.04.06 release because its application uses 9600 baud on the pack bus. The override can send it, but the current 19200 baud post-check cannot verify that application. Custom firmware likewise needs compatible 19200 baud replies to pass the post-check. Use a quiet, exclusive pack bus with one target; a master polling slaves on that bus can interfere with the unaddressed binary bootloader frames.

App data (own cells, `history.sqlite` with the recordings, `maintenance.sqlite` with installations and what they own, saved parameters, firmware logs) lives in the platform's app data folder under `io.github.privatecoder.openbms-monitor`.

Targets: macOS (Apple silicon), Windows 10/11 (x64 and arm64). Developed and tested on macOS; on Windows x64 the installer and monitoring through a network gateway have been tried, USB adapters not yet; Windows arm64 is built but untested. The Windows installer installs for all users under Program Files.

Flashing restarts the board. Original firmware and MAINT 1.5.3 keep the state of charge only at their own save points, so a restart can bring back an older SOC; a full charge does not guarantee a current saved value, so compare SOC and capacities before and after the flash. MAINT 1.6.3 saves the SOC before a planned restart.

## Maintenance (multi-pack banks)

A master with configured slaves polls the pack bus constantly, so no pack of the bank can be flashed while it runs. The *Pack bus* panel in the firmware view is one screen: a live status line (pack bus quiet or polled, and with the CAN socket set up what the master reports), then a way to make the bus quiet — **Halt master** for a patched master firmware (a command over the CAN socket sets a marker that survives restarts) or **DIP 5-8 OFF** on the master, which works with any firmware — and on a quiet bus the list of packs that answer, each with its version and a *Flash* button. Afterwards *Release master* (or DIP 5-8 back) gives the bus back. Right before every flash the app checks a quiet bus and the target's answer; it also checks the master's BMS serial number against the installation, remembers a pack that may hang in its bootloader after an interrupted flash (and flashes no other pack, on any port, until that board is recognised again by its stored serial or the entry is cleared by hand), and shows a set marker as a banner until it reads off. The inverter is not controlled.

Patched firmware: besides the original releases the app supports the patch levels MAINT 1.5.3 and 1.6.3, and for now the first MAINT 1.6 build<!-- TEMP(maint-1.6) --> (recognised by the patch version the master reports over the CAN socket; listed builds are named and accepted as flash images, an unlisted build with a supported patch version is configured the same way). Right after connecting via the CAN socket the app reads this **firmware profile** on the connected port and sets itself up by it: maintenance halt and release, serial numbers and parameters of every pack over the CAN socket, and the two release bits in function switch 0 (writing via the CAN socket, maintenance start), which it names only where the pack's patch version is proven. Without a supported patch the app uses the original functions only.

Maintenance flashing belongs to the installation chosen in the sidebar; the separate *Flash a board* tab flashes a single board on a freely chosen RS485-1/2 port without an installation. The installation's maintenance setup is the CAN socket and the RS485-1/2 port, each a USB adapter or a network gateway. Flashing through a gateway is possible but not yet tested on hardware: keep a USB adapter at hand to recover a pack left in its bootloader.

## License

MIT, see [LICENSE](LICENSE). Not affiliated with Seplos or SH Energy.
