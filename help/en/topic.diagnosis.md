---
title: Resistance diagnosis
short: Compares each pack's resistance inside and on the way to the busbar, to find bad contacts or cables.
related: topic.pairs
---
- **Inside:** every BMS reports the sum of its cells and the voltage at its output. Under current the difference is the voltage drop inside the pack (MOSFETs, shunt, internal bars); divided by the current it gives the resistance. Calculated continuously and smoothed, from 3 A.
- **Outside:** difference between the pack output and a reference point where you measure with a multimeter and enter the value: the **busbar** (one value for all) or the **pair's load switch, input on the pack side** (one value per pair; covers only the stretch from the packs to the switch, you measure the switch itself separately). The app then takes a snapshot of all packs.
- **Zero reading:** at rest every measurement shows a small fixed error. The app remembers it and subtracts it later, per reference point. **Leave the load switches on** and put the inverter on standby: with open switches the busbar would not be at the packs' potential. Without the zero reading the values are not usable.
- **Rating:** a pack above 1.5 × the median of all packs and at least 1 mΩ above it is conspicuous.
- **Limits:** voltages have 10 mV resolution, so read them only with enough current and only in comparison. Where exactly the BMS measures its output is not visible in the firmware; part of the inner path may end up in "outside". For pairs, "outside" also includes the shared main cables that carry both packs' current.
