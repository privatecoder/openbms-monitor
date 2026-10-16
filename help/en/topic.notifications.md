---
title: Notifications
short: Reports important events of the chosen installation as system notifications while the app runs and is connected. Off by default.
related: topic.recording, topic.connection, topic.alarms, topic.healthReport
---
- **When:** (1) a protection or fault alarm becomes active on a pack, e.g. “Pack 03: cell overvoltage protection”, again only after it cleared; (2) a pack that answered misses three polls in a row; (3) a pack's cell spread at rest (below 2 A) stays above the threshold for 5 minutes, again only after it fell 10 mV below the threshold; (4) no data for the set minutes (connection lost). Warnings and info messages trigger nothing.
- **How often:** at most one notification per pack and trigger in 10 minutes. Several packs at once share one notification (“cell overvoltage protection on packs 03, 04”). A click brings the app to the front.
- **Checked data only:** on RS485-1/2 only a pack whose board the installation knows counts; new, moved or foreign packs report nothing until you confirm them. The CAN socket has no per-pack check: notifications run there only if the master's battery serial matches the one stored for this installation. Otherwise the setting shows “Off for this connection: bank not checked (CAN socket)”. If none is stored yet, connect once over RS485-1/2 with the installation: the app then stores the serial numbers of the master's board (address 0). A pack without a serial number in its BMS reports nothing. “View only” reports nothing.
- **Threshold:** 50 mV at rest is a good start. A spread that stays higher points to a need for balancing or a weak cell; the health report under History shows it per day.
- **Permission:** the first time, the system asks whether the app may show notifications. If nothing appears, allow the app's notifications in the system settings; “Send test notification” checks that.
- **Limits:** nothing arrives while the app is closed. No push to phone, e-mail or MQTT.
