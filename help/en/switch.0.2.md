---
title: Allow maintenance start via the CAN socket
short: On: the master (address 0) allows “Start maintenance” via the CAN socket when automatic addressing is off. Only a successful start stops polling the slaves and frees the RS485-1/2 bus, e.g. for flashing. Off: the start is blocked. This bit has no effect on ending maintenance. Change only via the RS485-1/2 bus or through this pack's enabled display port.
related: topic.switches
---

