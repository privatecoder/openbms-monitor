---
title: Cell heating on
short: Heating switches on when the coldest cell reaches this value. Only with a heating pad and function switch 3, bit 6.
related: topic.params
---
Request: event 4 bit 6 (enable: function switch 3, bit 6). The heating output switches only while charging or with a detected charger. While heating is requested, the master reports at least packs × 10 A as charge limit, even if it would otherwise be 0.
