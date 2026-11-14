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

On both buses the BMS reports only which cells it balances, never why it does not. The pack view explains identifiable causes from the readings and the pack's read parameters (thresholds parameters 8 and 62, switches), read over RS485-1/2 or, with EMU1101-EXT, also over the CAN socket; without read parameters it names the usual values. When every checked condition is met, the actual blocking reason stays unknown. With an external balancer on the pack (installations, devices), its own line says whether it works by computation.
