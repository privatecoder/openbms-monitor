---
title: Hard stop
short: A property of the firmware image: a full pack waiting for intermittent recharge opens its own charge MOSFET.
related: topic.firmwareProfile, switch.5.1
---
- **Without hard stop (original):** while a full pack with intermittent recharge (function switch 5, bit 1) waits for its SOC to fall, only the master lowers the charge limit once all packs are that far. The pack's charge MOSFET stays closed.
- **With hard stop:** the full, waiting pack opens its own charge MOSFET and takes no more charge current, even if the inverter ignores the charge limit.
- **What the display says:** “on” or “off” is reported by the image itself (identity format 3); “unknown” means the firmware does not report it (format 2). It is a property of the flashed image, independent of the maintenance state, the release bits and whether the charge MOSFET is open or closed right now.
- **Observed:** where the firmware does not report it (format 2, original firmware, slaves), the app goes by the pack's behaviour only: waiting for recharge with its charge MOSFET off reads “observed (on …)”; waiting with the charge MOSFET on, “not observed (on …)”; otherwise “unknown”. Version numbers or a flash do not count.
