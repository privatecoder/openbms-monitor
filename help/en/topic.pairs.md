---
title: Pairs
short: Two packs with diagonal wiring: main + cable at the first pack, main − cable at the second, two short bridges in between.
related: topic.pairRead, topic.diagnosis
---
- Each pack has one bridge in its path: the first the minus bridge, the second the plus bridge. Both share the main cables.
- **A pair's total clearly below the other pairs** (under 85 % of the median) → check the pair's shared main cables and their connections. Not shown when the stronger pack carries a normal share; the weak pack alone then explains the low total.
- **One pack gets clearly less than its partner** (under 70 %) → check the bridge in its path, its connectors and the pack itself.
- The check only runs under load (total from 3 A).
- **Evaluation:** on the mean current of the last 60 s, from simultaneous answers only: each answer of one pack with the nearest of the other within 3 s, at least 5 such pairs (otherwise “no simultaneous samples”, no rating; so a load step between the two answers does not read as an uneven split). Only from 20 A per pack (median of all packs, with an even count the mean of the two middle ones). Below that, equalizing currents decide the split: packs at different states of charge equalize through their bridges, with the bank idle this can be several amps. At 10 A per pack this even made the wrong pack look weak. Rating also starts only once every pack has answered. A note appears below 70 % or 85 % and disappears only above 75 % or 90 %, so it does not flicker near the limit.
- **Equalizing:** if the states of charge of a pair differ by 3 points or more, the emptier pack gives less when discharging and the fuller pack takes less when charging. The footer then says “likely because the states of charge are equalizing” instead of warning: a likely explanation, not proof; an extra resistance in the path is not ruled out by it.
- **Outputs apart:** both packs of a pair sit on the same bridges; their output voltages differ only by the drop across bridges and plugs (plus a few 10 mV of sensor offset). From 100 mV (60 s mean) this turns red: contact resistance in a bridge or plug. Healthy pairs usually stay below 50 mV; a bad plug can produce several 100 mV. Independent of the load.
- **Constant or growing:** the app keeps the gap for up to 6 hours. Once the current has varied by at least 20 A per pack, it fits a line through the values. If the gap stays within 20 mV over the whole current range, one BMS simply measures a little differently: **constant offset**, and not red. A small resistance stays below the limit of measurement: the line says above how many mΩ none is measurable (20 mV divided by the current span, 1.0 mΩ at 20 A). If it grows with the current, the resistance is shown (≈ mΩ). Example: 40–47 mV from −24 A to +35 A with the currents split evenly, i.e. an offset.

Define pairs under Overview → Edit groups; "Pair neighbouring packs" forms 00/01, 02/03 and so on.
