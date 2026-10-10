---
title: Parameters vs. datasheet
short: Checks the BMS alarm and protection limits against the cells' datasheet.
related: topic.params, topic.cellAssign, value.datasheet
---
- **Which datasheet:** by default the cell type most packs have under Overview → Cell types. You can pick another cell model and number of cells in parallel, e.g. for an export from another installation.
- **Checked** are alarm and protection values only, not recovery values (those lie inside on purpose):
  - cell voltage high/overvoltage ≤ charge cut-off, low/undervoltage ≥ discharge cut-off
  - pack voltage likewise, times cells in series (parameter 65)
  - charge and discharge temperature inside the datasheet range; with a derating table its outer charge range counts
  - charge and discharge current (warning and protection) ≤ continuous current × cells in parallel; the BMS's negative discharge current counts by magnitude
  - rated capacity (parameter 58) ≈ cell capacity × parallel (±2 %); a difference is only a note
- **Red** marks a violation, the **Datasheet** column shows the limit. A value exactly on the limit counts as kept.
- A violation does not mean the BMS protects wrongly: many vendors set e.g. the charge under-temperature protection below 0 °C so the BMS tolerates small currents. The datasheet, however, allows charging only from its lower limit on.
