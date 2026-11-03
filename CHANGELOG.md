# Changelog

All notable changes of OpenBMS Monitor. Newest first; the app shows this file in its About view.

## Unreleased

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
- MAINT 1.5.4/1.6.4: the firmware identity reports read capabilities (format MNT05); the BMS history is read via the CAN socket where reported; remote diagnosis reads the calibration (active and both stored copies), the stored capacity and SOC against the live values, and the full-width energy counters.
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
