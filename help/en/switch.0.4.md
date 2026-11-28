---
title: Switching off blocks charging and discharging until this pack is switched on directly
short: On: “Set pack lock (addressed off)” via the RS485-1/2 bus or the CAN socket blocks charging and discharging on this pack (also current limiter and precharge), even after a restart. Only a direct “Switch on” at exactly this pack releases it; the master's switch-on broadcast and a parameter import do not. Off: switching off is a soft off that charging or the master can lift again. A lock that already exists stays even then until the pack is switched on directly. The help texts call this function the “extended off”.
related: topic.switches
---

