---
title: Reading pair values
short: How voltage, current and state of charge of a pair's two packs relate, and what a pattern means.
related: topic.pairs, topic.diagnosis
---
- **Principle:** the two packs are joined directly by two short bridges, so their outputs sit at practically the same voltage. What the two BMS show differs only by the drop across bridges and plugs and by the BMS measurement error: 10 mV resolution, plus a fixed offset of up to a few 10 mV per device.
- **Current follows resistance:** both packs share the pair's current. A pack with extra resistance in its path (bad plug, loose bridge) gets less, when charging and when discharging.
- **What a real resistance looks like:** the pack behind the resistance gets less current **and** shows the lower output voltage when charging, the higher one when discharging. The gap grows with the current (double current, double gap) and reverses when charging turns into discharging.
- **What a measurement offset looks like:** the gap stays the same however much current flows and in which direction, and the currents still split evenly. Or voltage and current do not fit together: the pack with the lower voltage gets **more** current when charging instead of less.
- **Magnitude:** a pack has only a few mΩ inside (cells, MOSFETs, shunt). 40 mV at 10 A would be 4 mΩ extra, roughly as much as the whole pack again; the current would then split clearly unevenly. With an even split, such a gap cannot be a resistance.
- **Uneven currents without a voltage gap:** if the outputs are level, the difference is inside the packs: usually different states of charge (the emptier one takes more when charging and gives less when discharging), otherwise a higher internal resistance (Diagnosis → inside).
- **State of charge (SOC):** the BMS counts it, it does not measure it. Between about 20 % and 90 % the LiFePO4 cell voltage is almost flat, and a difference of 2–3 points is normal there. The value can only realign when a pack is charged full.
- **Small currents:** below 20 A per pack, equalizing currents between the packs affect the split, so the app rates the split only above that. Gaps of 3–10 mV are within the resolution and mean nothing.

**What to do**
- Gap below 50 mV, currents similar → nothing.
- Gap constant, currents equal → measurement offset, nothing to do. To confirm, measure with a multimeter directly at both pack outputs: if it shows the same, one of the BMS reads off.
- Gap grows with the current, one pack gets less → check the bridge in this pack's path: for the pair's first pack (main + cable) the minus bridge, for the second (main − cable) the plus bridge. Under load a bad plug is often noticeably warm.

**Examples**
- Pair 04/05 (2026-10-10): 38–47 mV from −24 A to +35 A, constant. When charging, 05 shows the lower voltage but gets more current (10.0 A vs 9.0 A). Result: measurement offset.
- Bad bridge plug at pack 00 (2026-10-10): 170–390 mV, growing with the current. After the repair pair 00/01 read 10 mV; pack 00, the emptiest, then took the most charge current.
