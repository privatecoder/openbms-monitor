---
title: Current limiting duration
short: How long the pack charges through the hardware current limiter instead of the charge MOSFET after a charge over-current warning.
related: topic.params
---
Only with function switch 6, bit 7 (event 7 bit 7). Limiting starts when the charge over-current warning lasts half the charge over-current protection delay, and also ends after 5 s without charging. With function switch 6, bit 6 this passive limiting has no effect.
