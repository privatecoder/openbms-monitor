---
title: Charge and discharge limits
short: CVL/CCL/DCL/DVL the master sends to the inverter.
related: value.system
---
- **CVL** (charge voltage) = parameter 14 (pack over-voltage protection), fixed.
- **CCL** (charge current) = charge over-current warning (P50) ÷ 2 or − 10 A, times chargeable packs; 10 A in total when all packs have a charge warning; 0 when a cell exceeds OVP + 30 mV or no pack may charge.
- **DCL** (discharge current) = discharge over-current warning (P52) − 10 A, times dischargeable packs.
- **DVL** = parameter 12.
