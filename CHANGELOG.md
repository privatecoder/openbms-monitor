# Changelog

All notable changes of OpenBMS Monitor. Newest first; the app shows this file in its About view.

## Unreleased

- About view: version, links to the repository, releases, protocol docs and license, the data folder, and this changelog.
- BMS history archive: every history read is stored per installation and board (checked before and after the read), deduplicated, shown without a connection, exported as CSV and carried in the installation export (`bms-history.jsonl`).
- MAINT 1.5.4/1.6.4: the firmware identity reports read capabilities (format MNT05); the BMS history is read via the CAN socket where reported; remote diagnosis reads the calibration (active and both stored copies), the stored capacity and SOC against the live values, and the full-width energy counters.
- Reconnect also runs while the window is hidden, with growing pauses (1, 2, 4, 8, 16 min, then every 30 min, ten attempts).
- Release workflow: the temporary `.app.tar.gz` is removed after the upload.

## 0.1.0-beta — 2026-10-28

First pre-release, for macOS (Apple silicon) and Windows 10/11 (x64, arm64), unsigned.

- Monitoring via USB-RS485 adapter or RS485-to-Ethernet gateway on either bus: overview with the master's system values, pack detail, groups and towers, notifications, connection watchdog.
- Installations: everything stored belongs to an installation; boards identified by serial number, board swaps noticed; view only mode; merge, archive, delete; export and import as a checked ZIP file.
- Analysis: pair check, resistance diagnosis, optional recording into SQLite with history charts, health report, cell analysis, external balancer timeline.
- Maintenance functions: parameters read, compared, checked against the cell datasheet and written (single pack or several), own TOML parameter file, serial numbers, BMS clock, CAN protocol, calibration, pack control, BMS history, frame capture.
- Firmware updater (maintenance view of the app and CLI `openbms-flash`) with checked images and logs.
- Cell database with bundled LiFePO4 datasheets and own entries.
- German and English UI, light and dark theme, help behind every info icon.
