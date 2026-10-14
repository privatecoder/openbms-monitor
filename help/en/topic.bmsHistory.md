---
title: History from the BMS
short: The records each pack keeps itself, and the BMS clock their timestamps come from.
related: topic.history, topic.params
---
- **What is stored:** each pack writes a record to its own memory (EEPROM) with the time, its state (charging, discharging, standby), all alarms and protections, the 16 cell voltages, the temperatures, current, voltage, capacities, SOC and cycles. There is room for **up to 500 records**; after that the newest overwrites the oldest.
- **When:** at once when the state or a message changes (then even several records one second apart), otherwise at the interval: parameter **75** while working, parameter **76** in standby (minutes each). A record caused by a change still shows the messages from **before** the change; the new state is in the next record. So the history also covers times when no app was connected, e.g. to understand a protection event afterwards.
- **Reading:** under History → “From the BMS”, only via **RS485-1/2** (the CAN socket does not know the command; the master does not answer on RS485-1/2). The app asks for the oldest record and then one after the other up to the last; with 500 records this takes about one to two minutes. Polling of the packs pauses meanwhile; “Cancel” stops after the current record. The table shows the newest first, “Save as CSV” writes all fields. Nothing is stored in the app's database.
- **No snapshot:** if the memory is full and the pack writes a new record during the read, the start of the ring moves. One old record may then be missing or a new one appear.
- **Clock:** the timestamps come from the BMS clock at the time of writing. If it is wrong, all times are shifted accordingly. The deviation is shown above the table; set the clock under **Parameters** (maintenance functions), for one pack or all.
- **Time zone:** this **PC's local time** is sent; the button shows exactly the time and zone that arrive. The BMS stores only date and time, no zone. A PC in another time zone shifts all later records. After a daylight saving change the BMS clock is an hour off until you set it again.
- **Clearing** the history is not offered: the command for it also resets the cycle count to 0.
