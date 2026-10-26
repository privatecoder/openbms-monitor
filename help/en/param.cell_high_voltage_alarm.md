---
title: Cell high-voltage warning
short: Warning when a cell reaches this value. If all packs have a charge warning, the master lowers the charge limit to 10 A in total. Also the basis of the SOC's 100 % rule (cells × this value, limited to 3.45–3.50 V per cell).
related: topic.params
---
Message: event 2 bit 0 (enable: function switch 1, bit 0). For the charge limit every warning pack drops out of the sum; only when none is left does 10 A in total apply.
SOC: above 98.2 % the SOC only keeps counting up while charging if a cell reaches this value. The firmware sets 100 % when the pack voltage stays at or above cells × this value for 30 min. Also the full mark for capacity learning. Hysteresis: recovery value “Cell high-voltage warning off”.
