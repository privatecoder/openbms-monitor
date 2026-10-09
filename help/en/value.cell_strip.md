---
title: Cell deviation
short: One tick per cell: how far its voltage is from the median of all cells of all packs (shown in the column header). A flat line means well balanced.
related: value.cell_voltages, value.cell_delta
---
- The median is the middle cell voltage: half of all cells are above it, half below. Unlike the average it is not pulled by a single outlier cell.
- Up (violet) = cell above the median, down (blue) = below.
- Full height equals ±20 mV; larger deviations hit the edge.
- Deviations below 4 mV stay grey.
- A coloured mark at the bottom shows the cell is being balanced.

This way a pack or a single cell that is out of line stands out immediately, even when all absolute values look normal.
