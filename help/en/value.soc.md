---
title: State of charge (SOC)
short: State of charge in % counted by the BMS. It is also corrected by fixed voltage rules.
related: value.cycles
---
- The BMS integrates the current every second (coulomb counting).
- While charging the SOC holds at **98.2 %** until a cell reaches the cell high-voltage warning (P0).
- **100 %** is set when the pack voltage stays ≥ cells × P0 (3.45–3.50 V) for 30 min while charging with less than 3 A, or on over-voltage protection.
- Further jumps to 23/70/90/98 % or 10/0 % at matching voltages.
