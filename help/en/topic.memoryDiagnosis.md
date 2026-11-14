---
title: Memory diagnosis
short: Checks a pack's stored copies of parameters and charge (EMU1101-EXT, command 64/04). Reads only, changes nothing.
related: topic.hardStop, topic.control, topic.access
---
- **What it shows:** for parameters and for charge/capacity each the primary and the backup copy: “empty” (never written), “valid” (layout and checksum fit) or “invalid”, and whether both are equal. Two valid but different copies do not tell which is newer.
- **Pack lock:** only with builds that store a pack lock (EMU1101-EXT Full): the lock stored in both parameter copies and whether the runtime cache is valid. With Light it reads “not supported”.
- **Ways:** at every pack's own address, without a release (see below). The app asks only on a click, never in the background.
- **Read the memory diagnosis**
  - UART0 · RS485-1/2: EMU1101-EXT from Light 1.5.4 with memory diagnosis (image flag bit 5); note: reliable only without a polling master.
  - UART1 · CAN socket: EMU1101-EXT from Light 1.5.4 with memory diagnosis (image flag bit 5).
- **After a restart:** EMU1101-EXT Full saves the charge into both copies before a planned restart; if that fails, it refuses the restart (RTN 08). The diagnosis then shows the state of the copies.
