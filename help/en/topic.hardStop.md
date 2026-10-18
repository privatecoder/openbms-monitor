---
title: Hard stop
short: A property of the firmware image: a full pack waiting for intermittent recharge opens its own charge MOSFET.
related: topic.firmwareProfile, switch.5.1
---
- **Without hard stop (original):** while a full pack with intermittent recharge (function switch 5, bit 1) waits for its SOC to fall, only the master lowers the charge limit once all packs are that far. The pack's charge MOSFET stays closed.
- **With hard stop:** the full, waiting pack opens its own charge MOSFET and takes no more charge current, even if the inverter ignores the charge limit.
- **What the display says:** “on” or “off” is reported by the image itself (identity format 3); “unknown” means the firmware does not report it (format 2). It is a property of the flashed image, independent of the maintenance state, the release bits and whether the charge MOSFET is open or closed right now.
- **Observed:** where the firmware does not report it (format 2, original firmware, slaves), the app goes by the pack's behaviour only: waiting for recharge with its charge MOSFET off reads “observed (on …)”, otherwise “unknown”. A charge MOSFET on while waiting does not disprove it (the firmware switches it back on after 10 s of its own discharge over 10 A). A flash of the pack by the app ends the observation; a flash outside the app is not seen. Version numbers do not count. Via the CAN socket the app stores a sighting only when the master reports this installation's battery serial number; with original firmware there is no per-pack serial there, so a slave board swapped at an address goes unnoticed.
