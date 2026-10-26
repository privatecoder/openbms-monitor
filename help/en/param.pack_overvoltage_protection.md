---
title: Pack over-voltage protection
short: The master reports this value to the inverter as charge voltage (CVL). As a protection it opens the charge MOSFET, provided function switch 1, bit 5 is on.
related: topic.params
---
As CVL the value always applies, even with the protection masked. As a protection: event 2 bit 5; the charge MOSFET opens and the SOC jumps to 100 %. With bit 5 off, the protection never trips and does not set the SOC to 100 % either.
