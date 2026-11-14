---
title: Guide: write parameters to several packs
short: Make a change once and carry it to further packs: read, edit one pack, pick packs, confirm and read the result list.
related: topic.paramWrite, topic.params, topic.paramCheck
---
What is written is the change, not the whole column: every pack keeps its other values and switches. Writing parameters goes via the RS485-1/2 bus or the CAN socket; what it needs is in the linked topics.

1. Connect with an installation, not with “View only”: that reads but does not write. → [Connection](#topic.connection)
2. In Parameters press “Read from packs”. Expected: one column per pack. Only a pack read on this connection can be a target. → [Parameters](#topic.params)
3. Edit one pack: the pencil in its column header (“Edit”), then change the values. Expected: changed cells are coloured, the bar above lists the changes; if a value breaks a rule, the bar names it and writing stays off. → [Writing parameters and serial numbers](#topic.paramWrite)
4. In the bar press “Also to other packs …” and tick the targets, or “all read”. Packs not read are greyed out (“read first”). → [Writing parameters and serial numbers](#topic.paramWrite)
5. “Write to pack X and n more …”. The confirmation lists the changes and the packs for which the write resets the learned full capacity to P58 (the SOC jumps). Choose “Retries per pack” (0–5, preset 2), then “Write n packs”. → [Rated capacity](#param.rated_capacity)
6. Watch: the edited pack first, then the others by address, each with a fresh read, the write and the read-back; a new attempt builds on a freshly read state. “Cancel” acts before the next pack or attempt. → [Writing parameters and serial numbers](#topic.paramWrite)
7. Only if the series halts because a pack refused the block and its switches became active doing so: first “Restore stored switches”, then “Go on with the other packs” or “Cancel”. → [Writing parameters and serial numbers](#topic.paramWrite)
8. Read the result list: on top the counts (written, skipped after N attempts, refused, skipped …), below one line per pack with the outcome, the reason and the number of attempts. Expected: every target “written and read back”. Depending on the line: → [Writing parameters and serial numbers](#topic.paramWrite)
   - **“skipped after N attempts” or “result unclear”**
     1. Settle the reason named, read the packs again and write the change to this pack once more.
   - **“refused”**
     1. The pack itself refused; sending again changes nothing. Check this pack's values and firmware. → [Parameters vs. datasheet](#topic.paramCheck)
   - **“skipped: …” without an attempt**
     1. The pack already had the values, breaks a rule with them or has a pending refusal; the reason is in the line.
9. Finally press “Read from packs” again and switch on “Only differences”. Expected: the changed values are the same in every target. → [Parameters](#topic.params)
