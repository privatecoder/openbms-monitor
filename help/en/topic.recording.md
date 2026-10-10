---
title: Recording
short: Stores the polled values in the app's history database while it is connected. Off by default.
related: topic.history, topic.connection
---
- **Record while connected:** once switched on, the setting survives restarts. The app then records with every connection without you starting anything.
- **What:** all packs or a selection, plus the master's system values on the CAN socket. Messages are always stored with start and end.
- **How often:** every poll (for troubleshooting, about 11 MB per day with 12 packs), once a minute or every 5 minutes.
- **Keep:** every poll is kept at first (default 30 days). After that the app merges it into one value per minute: the mean for current, SOC and voltage, the extreme for highest and lowest cell and the temperatures, the lower limit for charge and discharge limit. Minute values are kept forever or deleted after the chosen time.
- **Where:** an SQLite file `history.sqlite` in the app's data folder. View, export and import earlier recordings (.jsonl) under **History**.
