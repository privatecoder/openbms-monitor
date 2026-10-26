---
title: History while active every
short: Interval of entries in the history memory (max. 500) while the pack charges or discharges.
related: topic.params
---
The history is kept in EEPROM and can be read with 0x4B. In addition the BMS writes an entry at once on every change of system status or messages; the counter then starts over.
