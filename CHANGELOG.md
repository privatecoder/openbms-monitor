# Changelog

All notable changes of OpenBMS Monitor. Newest first; the app shows this file in its About view.

## Unreleased

- Firmware update, pack bus panel: several packs (or all) can be flashed one after another with the chosen image and one typed confirmation (`FLASH <n> PACKS`, `FLASH CUSTOM <n> PACKS` for custom images) that lists every picked pack with its current version and is bound to exactly those packs. The slaves go first by address, the master last; each pack gets every check of a single flash right before it and the same before/after comparison, and the next pack starts only after the previous one answers cleanly with the new image. Any refusal, flash error, missing or differing comparison or pending bootloader record stops the series; “Cancel” stops before the next pack, never during a flash. A summary lists each pack's outcome with its version before and after and the packs not reached. A single flash is unchanged; it and the bootloader recovery wait while a series is open, and “End open series” ends one left open by a closed view.
- Parameters, writing to several packs: a pack whose write did not land or could not be checked (no answer, unclear result, read-back failed or differing, pack changed since its read) is retried up to a chosen number of times (0–5, 2 by default, picked in the confirmation), each retry on a fresh read of that pack with only what still differs written; after the last attempt the pack is skipped and the run goes on. An unclear result no longer stops the run; refusals by the pack or by the CAN socket's checks (open update, write lock, unsupported build, another board) are not retried, and a cancel takes effect before a retry writes. The result shows each pack's attempts and a line with the counts.
- Help and hints: the pages on maintenance, firmware update, firmware profile, release bits and calibration, the function switch 0 texts and the maintenance, marker and firmware hints now take their ports and firmware needs from the app's access table too. Each release bit lists the actions it gates from the table (bit 3 now also names the pack lock over the CAN socket); the calibration help no longer says the factors cannot be read where the firmware offers reading them.
- Help and hints: which port and firmware an action needs now comes from the app's access table itself. The help pages on access paths, buses, parameters, writing, BMS history, control and the diagnoses list every way of an action (port, firmware or read capability, release bits, notes, whether the app uses it), and the bus hints in Parameters, clock, history, control and diagnosis are built from the same data; the master's Modbus serial (String 1, 22 characters) stays apart from the 30-character answers, and the identity query is named a detection query. The bus overview no longer shows a table that did not render.
- Help and hints: the 22-character note when filling the battery serial applies only to the master's Modbus answer without patched firmware; the access matrix explains “unclear” as a missing proof (firmware identity, flags, read capabilities or switches), not only unread switches; the balancing help no longer claims a lock reason readable only on RS485-1/2.

## 0.4.0-beta — 2026-11-13

- The patched firmware is now called EMU1101-EXT: Light (1.5.x) and Full (1.6.x). The app reads its identity with the signature `EXT` (format 05, 21 bytes) and supports Light from 1.5.4 and Full from 1.6.4, plus for now the first MAINT 1.6; the original firmware releases stay supported. The four releases Light 1.5.4/1.5.5 and Full 1.6.4/1.6.5 are listed by checksum; builds before them (MAINT 1.5.3, 1.6.3 and older) count as unknown patch firmware. Names, help and texts use the new wording throughout. A stored proof that names an older MAINT build no longer counts; the pack's live identity proves it again.
- A late device-info answer whose name starts with `EXT` or `MNT` is no longer taken for the firmware identity.
- A malformed answer with an odd number of data digits (ASCII) or an odd byte count (Modbus) is an error instead of stopping the poll.
- Access matrix: a cell with several ways names the firmware need of the way its state comes from (e.g. the master's Modbus serial number from OEM 16.06.11 next to the binary one of EMU1101-EXT).
- Flash result: for an EMU1101-EXT image the version the pack reports afterwards is compared with the image's OEM base; a different one is named.
- Help and hints: the CAN socket's functions with EMU1101-EXT (clock, on/off, restart, serial numbers, diagnostics), the restart with Full's SOC save and its RTN 08 refusal, the flash sequence, the bootloader recovery for a pack that hangs in its bootloader, and the function switch 0 bits are described as they are now.
- Release workflow: `latest.json` is written only with a signature of its own for each of the three platforms.

## 0.3.9-beta — 2026-11-10

- 0.3.8-beta could keep "Connect" locked as "loading" on its first start: taking over the pack selection remembered on this computer emptied the installation settings and nothing read them again (until another installation was chosen). Settings are read again at once now, and a read started before is not taken.
- A connect refused before it opens the port (installation busy or gone) leaves a running connection's bus rule as it is.
- The sidebar's packs line shows the running connection's selection only while connected; an installation deleted or merged away is no longer asked for its settings again and again.

## 0.3.8-beta — 2026-11-10

- Installations → Ports: "Packs polled over RS485" (search, a count from address 00, or one address) belongs to the installation and is exported with it; the selection last used on this computer is taken over once. The sidebar shows it as one line ("12 packs · addresses 00–11"); the picker stays for "View only". A change during a connection applies from the next one. A set count or address now wins over the slave count the master reports, which counts only when searching.
- "Inverter connected via" can be changed while connected over Home Assistant; it applies from the next direct RS485 connection. A connect always takes the stored value, and a running RS485 connection takes no late change of another installation.
- When a field or button is offered or locked is decided in one place for the whole app; a locked button names its reason on hover. The bank check's "Disconnect" and "Connect with B" wait for a take-over running, and its answer counts only for the connection it was asked on.

## 0.3.7-beta — 2026-11-10

- Firmware → Flash a board: "Bootloader recovery" for a board without a working firmware whose bootloader already waits (it answers no search). "Check bootloader" sends one R F L without a restart and refuses unless exactly one bootloader answers; "Recover" writes the chosen image and then looks for the application at every address. A recovery keeps its own pending record, so no normal flash starts while a board may still hang in its bootloader.
- Home Assistant (MQTT) per installation: the broker belongs to the installation (exported with it, never the password). The password stays in this computer's app data, bound to its broker, so it is never sent to another one. Choosing an installation fills in its broker and preselects the way last used; a first broker is saved after a connect that worked, another one is offered. Installations → Ports shows, changes or removes it.

## 0.3.6-beta — 2026-11-09

- Home Assistant (MQTT): the add-on's retained feed description (every pack's identity and parameters) is larger than the MQTT client's 10 KB default; the connection broke again and again and the app said "add-on offline". Messages up to 1 MB are taken now. A broker error is shown with its own text, a refused subscription (the MQTT user's rights) is named, and after a reconnect the app waits for new measurements before it says the feed is live.

## 0.3.5-beta — 2026-11-09

- Home Assistant (MQTT): a third connection type reads the measurements of the Seplos BMS add-on (its "OpenBMS Monitor feed", switched on in the add-on) from the MQTT broker instead of polling the RS485 bus a second time. Overview, pack views, pair check, balancing line, recording, health report and notifications work as before; the recording keeps the add-on's read times, and the pair window runs on the add-on's clock. Read-only: everything that talks to a BMS is refused for this source.

## 0.3.4-beta — 2026-11-04

- Pairs: the two packs of a pair are polled right after each other, and the live check pairs only answers read at most 3 s apart (as the health report), timed from the telemetry read itself; before, answers up to a whole polling round apart could be paired when one was missing. The answers are collected outside the overview, so opening a pack and coming back no longer shows "no simultaneous samples" for a minute, and one pack without data no longer stops the check of all pairs. The recording stores the time of the telemetry read.

## 0.3.3-beta — 2026-11-04

- Pack view, balancing line: with the pack's parameters read once, the reason it does not balance uses its own values (start voltage, start spread, function switches 6) instead of the usual ones, and says when they are reached and it still does not balance. An external balancer assigned to the pack gets its own line: working, off, or between its thresholds, computed from its settings.

## 0.3.2-beta — 2026-11-03

- Cell analysis and balancing timeline: the time axis hides a date whose label would run into the start or end label (seen at 7 and 30 days on a narrow window). The history charts leave more room per date at spans over two days and show the date alone at midnight, so the labels no longer run into each other.
- About view, Data & storage: shows what the data folder holds (recording, installation data with the BMS archive, firmware logs, parameter files, exports, migration backups) and the free space; clears the recording of one installation or all, deletes the migration backups, or empties the data folder at the next start (installations, the BMS archive and the packs' safety states stay). Before a delete the data can be backed up as installation ZIPs; if a backup fails, nothing is deleted. A backup can be restored again after its recording was cleared. The Recording sheet links here.
- About view: a failed update check shows only its cause below "The check failed", no longer "The update could not be completed".

## 0.3.1-beta — 2026-11-03

- Overview: in a narrow window the pack boxes keep the width of their columns while the table scrolls sideways, instead of cutting the rows off.
- Cell database: the datasheet values sit in one column when the detail column is narrow, instead of running out of their boxes.
- macOS: the app bundle is signed ad hoc as a whole, so macOS no longer calls a downloaded copy "damaged" but offers Open Anyway in Privacy & Security.

## 0.3.0-beta — 2026-11-03

- Updater: the app looks for a newer release on GitHub (at start and every 12 hours, can be switched off, or by hand in the About view), shows its notes and on a click downloads the matching build, checks its signature, installs it and restarts. Waits while a firmware update, a pack or maintenance action, a BMS history read, a parameter write or an installation import, merge or delete runs. The first version with the updater has to be installed by hand once.

## 0.2.0-beta — 2026-11-02

- About view: version, links to the repository, releases, protocol docs and license, the data folder, and this changelog.
- BMS history archive: every history read is stored per installation and board (checked before and after the read), deduplicated, shown without a connection, exported as CSV and carried in the installation export (`bms-history.jsonl`).
- EMU1101-EXT Light 1.5.4 / Full 1.6.4: the firmware identity reports read capabilities (format 05); the BMS history is read via the CAN socket where reported; remote diagnosis reads the calibration (active and both stored copies), the stored capacity and SOC against the live values, and the full-width energy counters.
- Reconnect also runs while the window is hidden, with growing pauses (1, 2, 4, 8, 16 min, then every 30 min, ten attempts).
- Release workflow: release notes come from this changelog; the temporary `.app.tar.gz` is removed after the upload.

## 0.1.0-beta — 2026-10-28

First pre-release, for macOS (Apple silicon) and Windows 10/11 (x64, arm64), unsigned.

- Monitoring via USB-RS485 adapter or RS485-to-Ethernet gateway on either bus: overview with the master's system values, pack detail, groups and towers, notifications, connection watchdog.
- Installations: everything stored belongs to an installation; boards identified by serial number, board swaps noticed; view only mode; merge, archive, delete; export and import as a checked ZIP file.
- Analysis: pair check, resistance diagnosis, optional recording into SQLite with history charts, health report, cell analysis, external balancer timeline.
- Maintenance functions: parameters read, compared, checked against the cell datasheet and written (single pack or several), own TOML parameter file, serial numbers, BMS clock, CAN protocol, calibration, pack control, BMS history, frame capture.
- Firmware updater (maintenance view of the app and CLI `openbms-flash`) with checked images and logs.
- Cell database with bundled LiFePO4 datasheets and own entries.
- German and English UI, light and dark theme, help behind every info icon.
