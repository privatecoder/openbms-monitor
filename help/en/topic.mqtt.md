---
title: Home Assistant (MQTT)
short: Instead of polling over RS485 itself, the app reads the measurements the Home Assistant add-on “Seplos BMS” polls anyway, over MQTT. Read-only.
related: topic.connection, topic.pairs
---
- **Switch on in the add-on:** in the add-on's app (Overview) switch on the **OpenBMS Monitor feed**. While it is on, the add-on publishes every answer raw with its read time, polls faster (round pause 0, bus quiet 200 ms) and reads the packs of a parallel set one right after the other. Optionally it switches itself off after 1, 4 or 24 hours.
- **Connect:** choose “Home Assistant” in the connection and enter broker, port (usually 1883), user, password and the add-on's MQTT topic (default `seplos`); TLS as needed. The password is kept in the app data, readable by you only.
- **What works:** overview, pack views, pair check, balancing line (with the parameters the add-on reads), recording, health report and notifications, as over RS485. The recording keeps the add-on's read times.
- **What does not:** anything that talks to a BMS: reading and writing parameters, calibration, control commands, BMS history, firmware, diagnosis reads, search. For those stop the add-on and connect directly. Stored parameter states stay viewable; the app stores none from the feed, as it does not show which board sits at an address.
- **Status:** below the connection it says when the feed is off, the add-on is offline, no measurements came yet, or the Home Assistant host's clock differs by more than 5 s. The pair check runs on the add-on's clock and stays right; only the times in the recording need a correctly set clock.
- **Pair check:** as over RS485, only with readings taken at most 3 s apart. With the feed on, the add-on reads a set's packs one after the other; a round takes longer than the app's own, so 60 s sometimes hold fewer pairs.
- **Archive:** the add-on stores nothing; the app records only while it receives. A restart of the add-on starts a new session.
