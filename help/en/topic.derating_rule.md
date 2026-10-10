---
title: Charge current by temperature
short: Many datasheets limit the charge current by cell temperature (and sometimes SOC). The table gives values at points.
related: topic.cells
---
- **Between two points the lower value applies**, also between SOC columns and on shared band bounds. Example EVE LF280K Rev C: at 3 °C, 0.03 P applies (the value at 0 °C), not an intermediate value.
- **Outside the table there is no charging.**
- This is an app rule, not a manufacturer statement: only the printed points are documented, and the lower neighbour is certain not to exceed what the manufacturer allows. If a datasheet explicitly allows linear interpolation, the entry says so.
- The datasheet's temperature range is **not a charge permit**: some cells allow 0 A at its limits. What counts is where the table allows more than 0.
