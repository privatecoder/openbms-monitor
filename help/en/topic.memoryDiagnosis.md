---
title: Memory diagnosis
short: Checks a pack's stored copies of parameters and charge (EMU1101-EXT, command 64/04). Reads only, changes nothing.
related: topic.hardStop, topic.control
---
- **What it shows:** for parameters and for charge/capacity each the primary and the backup copy: “empty” (never written), “valid” (layout and checksum fit) or “invalid”, and whether both are equal. Two valid but different copies do not tell which is newer.
- **Pack lock:** only with builds that store a pack lock (EMU1101-EXT Full): the lock stored in both parameter copies and whether the runtime cache is valid. With Light it reads “not supported”.
- **Ways:** over the RS485-1/2 bus and the CAN socket, at every pack's own address, without a release. The app asks only on a click, never in the background.
- **After a restart:** EMU1101-EXT Full saves the charge into both copies before a planned restart; if that fails, it refuses the restart (RTN 08). The diagnosis then shows the state of the copies.
