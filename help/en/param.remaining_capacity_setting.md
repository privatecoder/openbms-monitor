---
title: Remaining capacity (set value)
short: The last stored set value, not the current charge. Only writing it as a single parameter sets the counter to this value.
related: topic.params
---
Given in Ah, not percent. A single write sets the running counter to the written value, only if it does not exceed the full capacity. Writing the whole parameter block only stores the field. If both stored capacity records are invalid at start, the remaining capacity starts from this value.
