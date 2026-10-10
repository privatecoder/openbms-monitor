---
title: Pairs
short: Two packs with diagonal wiring: main + cable at the first pack, main − cable at the second, two short bridges in between.
related: topic.diagnosis
---
- Each pack has one bridge in its path: the first the minus bridge, the second the plus bridge. Both share the main cables.
- **A pair's total clearly below the other pairs** (under 85 % of the median) → check the pair's shared main cables and their connections. Not shown when the stronger pack carries a normal share; the weak pack alone then explains the low total.
- **One pack gets clearly less than its partner** (under 70 %) → check the bridge in its path, its connectors and the pack itself.
- The check only runs under load (total from 3 A).

Define pairs under Overview → Edit groups; "Pair neighbouring packs" forms 00/01, 02/03 and so on.
- **Evaluation:** on the mean current of the last 60 s, only from 20 A per pack (median of all packs). Below that, equalizing currents decide the split: packs at different states of charge equalize through their bridges, up to ~5 A were measured with the bank idle. At 10 A per pack this even made the wrong pack look weak and only once every pack has answered. A note appears below 70 % or 85 % and disappears only above 75 % or 90 %, so it does not flicker near the limit.
- **Equalizing:** if the states of charge of a pair differ by 3 points or more, the emptier pack gives less when discharging and the fuller pack takes less when charging. The footer then says the states of charge are equalizing instead of warning.
- **Outputs apart:** both packs of a pair sit on the same bridges; their output voltages differ only by the drop across bridges and plugs (plus a few 10 mV of sensor offset). From 100 mV (60 s mean) this turns red: contact resistance in a bridge or plug. Healthy pairs stay below 50 mV; the bad plug on 2026-10-10 showed 170–390 mV. Independent of the load.
- **Constant or growing:** the app keeps the gap for up to 6 hours. Once the current has varied by at least 20 A per pack, it fits a line through the values. If the gap stays within 20 mV over the whole current range, one BMS simply measures a little differently: **measurement offset, no resistance**, and not red. If it grows with the current, the resistance is shown (≈ mΩ). Example 2026-10-10, pair 04/05: 40–47 mV from −24 A to +35 A with the currents split evenly, i.e. an offset.
