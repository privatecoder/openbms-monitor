---
title: External balancers
short: Enter active balancers on the packs; the app computes from the cell voltages when they work and shows it next to the BMS balancing.
related: topic.installations, status.balancing, topic.history
---
- **What for:** An active balancer wired to a pack for good (e.g. NEEY, Heltec, JK) moves charge between the cells whenever its own conditions hold, independent of the BMS's passive balancing. Without knowing it, its work reads as cell behaviour.
- **Entering:** In Installations on the installation's card, block “Devices”: name, start from spread (highest minus lowest cell, mV), stop below spread (empty: as start), highest cell from (empty: any), when (always, while charging above +1 A, not while discharging from −1 A) and balance current (shown only). One definition can be assigned to many packs, each pack has at most one.
- **Computed, not measured:** The app does not read the balancer. Each recorded row counts as active when “when” and the cell voltage fit and the spread reaches the start; it stays active until the spread falls below the stop (within one connection; a gap of more than three polls starts over). Minute values of older days count only where their extremes are certain, otherwise “unknown”.
- **In time:** Every change of the values or the assignment applies from its time on; the recording before is still computed with the values of the time. Removing ends the assignments now.
- **Timeline in the pack details:** below the cell analysis and over the same span (24 h, 7, 30 or 90 days) two lanes: “External balancer (computed)” and “BMS balancing (reported)” from the BMS's balancing bits, with the number of cells balanced at once. Recordings from before this version have no bits (“no data”); minute values only say “balanced at least once” and the share.
