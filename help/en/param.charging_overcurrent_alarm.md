---
title: Charge over-current warning
short: Basis of the charge current limit (CCL): half of this value per pack, or this value − 10 A with function switch 7, bit 7. Can activate the hardware current limiter.
related: topic.params
---
Message: event 5 bit 0 (enable: function switch 4, bit 0). The master's charge limit is this share × chargeable packs without a charge-related warning. With function switch 6, bit 7 the warning switches to the hardware current limiter after half the charge over-current protection delay. Protection stage: charge over-current protection.
