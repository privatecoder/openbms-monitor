---
title: Function switches
short: Eight bytes (BitGroup0–7) with eight switches each.
related: topic.params
---
- **BitGroup1–5** enable the messages: a bit at 0 switches the message **and its reaction** off (no MOSFET switching, no effect on charge limit or SOC).
- **BitGroup0** is not a mask; only bit 1 (temperature sensor check) has an effect.
- **BitGroup6/7** are functions: balancing, charge activation, current limiting, shutdown, charge limit.
- The app greys out bits without a function.
