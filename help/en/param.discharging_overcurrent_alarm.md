---
title: Discharge over-current warning
short: Basis of the discharge current limit (DCL): this amount − 10 A per pack.
related: topic.params
---
Message: event 5 bit 2 (enable: function switch 4, bit 2). The value is negative (discharging). The 10 A deduction applies only to amounts above 20 A. The master's discharge limit is this share × dischargeable packs without a discharge-related warning; if all warn, it is 0. Protection stage: discharge over-current protection.
