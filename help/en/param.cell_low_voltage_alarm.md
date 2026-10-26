---
title: Cell low-voltage warning
short: Warning when a cell reaches this value. It also corrects the SOC (see below).
related: topic.params
---
Message: event 2 bit 2 (enable: function switch 1, bit 2). The SOC jumps to (SOC warning + SOC protection) / 2 when a cell reaches this value and the SOC is still at or above the SOC warning. As long as all cells are above it, the SOC does not count below SOC protection + 1 % while discharging (checked in whole percent).
Also the empty mark for capacity learning. As a discharge-related warning it removes the pack from the discharge limit (DCL) sum; if all packs warn, the master reports DCL = 0. Protection stage: cell under-voltage protection.
