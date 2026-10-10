---
title: Balancing
short: Passive balancing: a high cell is slightly discharged through a resistor. The app shows which cells the BMS is balancing right now.
related: value.cell_delta
---
The BMS balances only if all of this holds:
- Balancing is switched on (function switch BitGroup6 bit 0).
- The pack is charging; with BitGroup6 bit 1 also while idle, but **never while discharging**.
- The spread (highest minus lowest cell) is above **parameter 62** (often 30 mV).
- A cell is at least at the start voltage **parameter 8** (often 3.40 V) and at least **parameter 63** (often 20 mV) above the lowest cell. Cells are selected from the highest downwards.
- No lock: idle time limit (parameter 64), ambient temperature warning, cell difference fault.

Two balanced cells must be at least three positions apart (limit of the balancing chip). LiFePO4 only reaches 3.40 V at the end of a charge, so with well matched cells balancing is rarely seen.

On the CAN socket bus only the balanced cells are readable. The thresholds (parameters 8, 62, 63) and the exact lock reason are read by the app on the RS485-1/2 bus.
