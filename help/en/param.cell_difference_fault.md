---
title: Cell difference fault from
short: Fault when the highest and lowest cell are this far apart. Blocks balancing.
related: topic.params
---
Message: event 1 bit 4 (fault), not maskable: the firmware always evaluates it. In the original firmware function switch 0, bit 4 has no influence on it; in MAINT 1.6.3 that bit is the extended off and has nothing to do with the cell difference either. Recovery: “Cell difference fault off below”.
