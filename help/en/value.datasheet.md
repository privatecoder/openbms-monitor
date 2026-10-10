---
title: Limits per datasheet
short: The pack's live values against its cells' datasheet. The BMS protects by its own parameters; this comparison shows whether they fit the datasheet.
related: topic.cellAssign
---
- **Cell voltage:** highest cell against the charge cut-off (close from 50 mV below), lowest against the discharge cut-off (close from 200 mV above).
- **Temperature:** coldest and warmest cell against the range for charging or discharging (close from 5 K before the limit). Outside the charge range this is a violation only while charging; otherwise it reads “charging not allowed now”.
- **Charge current:** allowed is the datasheet's derating table at the coldest and the warmest cell (the lower counts), at most the continuous current. Without a table, the continuous current applies inside the charge range. “Recommended” is the standard charge current. Close from 80 %.
- **Discharge current** against the continuous current, times cells in parallel.
- At the charge and discharge limit on the overview the app adds up the currents the datasheet allows for all packs (“datasheet allows …”). If it is red, the master reports more to the inverter than the cells allow now.
