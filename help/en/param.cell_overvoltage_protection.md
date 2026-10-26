---
title: Cell over-voltage protection
short: When a cell reaches this value, the charge MOSFET opens and the SOC jumps to 100 %. With a cell 30 mV above it, the master reports a charge limit of 0.
related: topic.params
---
Message: event 2 bit 1 (enable: function switch 1, bit 1). The protection sets the SOC to 100 % only while charging. Recovery: “Cell over-voltage protection off”. If pack over-voltage protection is masked, this is the only overcharge protection.
