---
title: Recording
short: Writes the polled values to a file, similar to the debug log in Home Assistant.
related: topic.connection
---
- **What:** all packs or a selection by group or single pack; optionally the master's system values (Modbus, CAN socket only).
- **How often:** every poll (troubleshooting, e.g. current split in pairs, equalizing currents) or once a minute / every 5 minutes (long-term observation). The app estimates the file size up front.
- **File:** in the `recordings` folder of the app data, named with the start time (UTC). Format JSON Lines: one line per answer, `{"t": Unix ms, "pack": {…}}` or `"system"`; the first line describes the settings. Errors (no answer) are recorded too.
- The recording keeps running when the connection drops and the app reconnects, and ends with “Stop” or when the app closes.
- To change the selection, stop and start again (new file).
