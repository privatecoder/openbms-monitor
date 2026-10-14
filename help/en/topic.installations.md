---
title: Installations
short: Everything the app knows about a bank belongs to an installation. The Installations view lists them with their packs.
related: topic.connection, topic.history, topic.params, topic.maintenance
---
- **What belongs to it:** the recording, the boards identified per address, the cell type per pack, groups and pairs, the diagnosis, the parameter states and the update logs. User cell models stay shared by all installations.
- **Choose:** in the sidebar before connecting (see Connection). Without a connection the chosen installation decides what History and Parameters show; the overview always shows the bus actually connected.
- **Boards:** every BMS serial number read (String 2) is recorded at its address. If another one is read there, the app says “Another board at address 07?” and keeps the old one as “previous board”; its parameter states no longer count as the current state. Empty, placeholder or no answer: “unclear”, nothing is claimed. When the app writes String 2 itself, it stays the same board. The cell type belongs to the address, since the cells stay when the board is swapped.
- **Taken over:** earlier versions kept data by endpoint (gateway address or port). Each such endpoint becomes once an installation of its own with its name, marked “taken over”, even if the endpoint already belongs to an installation: the same adapter may have served two banks, and mixed data cannot be separated afterwards. If the endpoint belongs to exactly one installation, the card offers “Merge with installation X”.
- **Merge:** adds, never overwrites. What the target installation has set (a pack's cell type, groups, pairs, diagnosis) stays; the other values are kept in the log of the move. Two recording rows of the same pack at the same time: the target's stays, the other is counted. The merged installation is gone afterwards.
- **Archive:** keeps everything; the installation is just no longer offered and never chosen by itself. “Bring back” undoes it.
- **Delete:** only without open work (no marker on, no pack in its bootloader) and not while the app is connected with it. Before, the app lists what goes (recording, boards, parameter states, cell types, groups, diagnosis). Update logs stay as files and are then listed under “Logs without installation”, like all logs from before installations; the app does not assign them by guessing.
- **Pack table:** per address the current board (and the previous one after a swap), the cell type, the last parameter state (a click opens it under Parameters) and the last update with image, result and log.
