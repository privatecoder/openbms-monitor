---
title: Hard stop at recharge pause
short: Effective only while bit 7 is off and recharge waiting is enabled (function switch 5, bit 1). On: while the pack waits for the recharge threshold after a full charge, it blocks charging itself (charge MOSFET off). Off: the recharge pause alone does not block charging at this pack; the master takes it into account in the bank's charge limit. Set only via the “Hard stop” line, never via “Edit” or a file.
related: topic.switches
---

