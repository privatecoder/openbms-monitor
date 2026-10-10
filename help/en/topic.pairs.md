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
