---
title: Remote diagnosis
short: Reads a pack's calibration, stored state and energy counters (commands 64/05 and 64/06). Reads only, changes nothing.
related: topic.memoryDiagnosis, topic.bmsHistory, topic.control, topic.access
---
- **When offered:** only when the pack reports the read capability in its firmware identity (64/03). The version number alone is not enough for the app. The app asks only on a click, never in the background; it needs neither an installation nor a release.
- **Read calibration**
  - UART0 · RS485-1/2: EMU1101-EXT from Light 1.5.4 with read capability calibration pages (bit 1 of the firmware identity); note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with read capability calibration pages (bit 1 of the firmware identity).
- **Read stored state and energy counters**
  - UART0 · RS485-1/2: EMU1101-EXT from Light 1.5.4 with read capability stored state or energy counters (bit 2 or 3 of the firmware identity); note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with read capability stored state or energy counters (bit 2 or 3 of the firmware identity).
- **Calibration:** the values the firmware calculates with now (active), beside the stored primary and backup copy: one gain per cell, two gains for the pack voltage, gain and zero point for both current channels. Differing values are marked. Gains and zero points are raw values; a zero point is not a current. The firmware sets the active gain of current B itself at start; the stored copies may differ from it. A copy of the wrong layout is not shown as values, even when the firmware accepts its checksum. A difference between packs or a single current comparison does not yet prove a measuring fault: whether gain or zero point is off shows only a comparison at different currents.
- **Stored state:** remaining and full capacity, cycles and energy counters as they are in memory, beside the live state of charge. From them follows the stored state of charge of the copy the BMS would take at start, and the deviation from the live value. After a restart, a flash or a power loss the BMS takes over this stored state. If the chosen copy is invalid, the app gives no number and does not take the other one instead.
- **No forecast:** the BMS saves again on its own, parameter changes act too, and builds that save before a planned restart (EMU1101-EXT Full) save again before it; the app then says so. The display is a snapshot.
- **Energy counters:** charged and discharged energy at full width. Ordinary telemetry gives only a substitute value from 6500 kWh on; here is the real counter.
- **Where else:** as one line in the restart dialog (Control) and in the firmware view before flashing, there on a click on the flash target itself. Nowhere is it a condition.
