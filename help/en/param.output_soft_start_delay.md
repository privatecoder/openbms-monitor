---
title: Output pre-charge
short: When switching on, the BMS first charges through a pre-charge path, at most this long. Then it checks for a short circuit and switches the discharge MOSFET on.
related: topic.params
---
Pre-charge ends earlier when the output voltage stops rising. If the output is then below half the target, the BMS reports a short circuit (event 5 bit 5, only with function switch 4, bit 5).
