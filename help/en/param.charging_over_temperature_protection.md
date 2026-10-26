---
title: Charging: over-temperature protection
short: Protection at a cell sensor: charging is blocked until the recovery value is reached.
related: topic.params
---
Message: event 3 bit 1 (enable: function switch 2, bit 1). The protection is reported only in the charging direction; from 16.06.11 the charge lock tests the raw protection bit regardless of the current direction, so it also blocks while discharging.
