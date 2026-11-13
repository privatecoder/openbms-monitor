---
title: Recharge only below
short: After 100 % charging is blocked until the SOC falls to this value. When all packs are there, the master reports a charge limit of 0. Only with function switch 5, bit 1.
related: topic.params
---
Message: event 6 bit 1 (enable: function switch 5, bit 1). If a pack requests heating, the charge limit 0 is raised to the heating minimum. Above this value the SOC additionally drifts by 0.2 A in standby.
Original firmware: the waiting pack itself does not lock its charge MOSFET, only the master lowers the charge limit. Patched firmware with hard stop opens the waiting pack's charge MOSFET (selectable with function switch 0, bits 6/7 on EMU1101-EXT Full).
