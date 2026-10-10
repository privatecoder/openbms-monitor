---
title: Parameters
short: The packs' settings: limits, delays, balancing and function switches. The app only reads them, it changes nothing.
related: topic.buses, topic.switches
---
- **Read from packs:** only via RS485-1/2 (19200 baud). The CAN socket does not know the command. The **master** (DIP 0 with slaves) does not answer on RS485-1/2; get its values from a BatteryMonitor export or while it is alone on the bus.
- **Open export:** XML files from BatteryMonitor (export parameters). Values count by position, not by name. If the names do not match the known order, the app warns: such an export comes from another BatteryMonitor version and must not be written back.
- **Comparison:** every source is a column. Values that differ from the majority are marked; “Only differences” hides everything equal.
- **Save:** a pack's read values can be saved as BatteryMonitor XML (folder `parameters` in the app data).
- The names are corrected: BatteryMonitor machine-translates from Chinese (“Monomer” = cell, “pressure” = voltage, “Equalization” = balancing). Hover a name to see the original BatteryMonitor name.
