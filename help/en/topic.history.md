---
title: History
short: Shows the recorded values as charts over time, with export and import.
related: topic.recording, value.share
---
- **Installation and span:** data is kept per installation (gateway address or port). Spans up to now (1 h to 30 days) refresh every 30 s; “All” shows the whole recording.
- **Charts per pack:** current, current share, SOC, pack voltage, highest and lowest cell, cell difference, warmest and coldest cell. Each pack keeps its colour everywhere; the pack buttons hide single packs.
- **Master** (if recorded): total current with charge and discharge limit, SOC, voltage with charge voltage, the bank's cell extremes.
- **Cells of one pack:** all cell voltages of a chosen pack, e.g. to see balancing or a weak cell.
- **Zoom:** drag across a chart; all charts follow and show finer values, down to every poll. The cursor shows the same moment in every chart.
- **Resolution:** at most about 1500 points per chart. Each point is the mean of its time slice; for cell extremes, cell difference and temperatures the extreme, so peaks do not disappear. Briefly missing values are bridged, longer outages stay visible as a gap.
- **Messages:** below the charts are the messages in the span with start and end. Clicking the time zooms the charts to that moment.
- **Export:** the span shown and the packs shown, as CSV (one row per pack and time with all cells; the master's values in a second file) or as JSON Lines (the earlier recording format, to pass on and import again). Times in UTC.
- **Import:** JSON Lines recordings of earlier versions or exports, for the installation shown. Times already present are not overwritten, so importing twice does no harm.
