---
title: Pack high-voltage warning
short: Warning from this pack voltage. If all packs have a charge warning, the master lowers the charge limit to 10 A in total.
related: topic.params
---
Message: event 2 bit 4 (enable: function switch 1, bit 4). For the charge limit every warning pack drops out of the sum. No influence on the SOC: the SOC rules use cells × cell high-voltage warning, not this value.
