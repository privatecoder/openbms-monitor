---
title: Energy
short: Charged and discharged energy since the last reset, counted by the BMS.
related: value.cycles
---
The BMS sums current × voltage every second while the pack charges or discharges (0.1 kWh steps). The counters survive restarts but are only saved on certain occasions (e.g. SOC change > 5 % in standby). Beyond 6,500 kWh the firmware reports a fixed 650.0 kWh.
