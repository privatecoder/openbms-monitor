---
title: Wechselrichter-Anschluss
short: Auf einem RS485-Bus darf nur ein Gerät abfragen. Unter Anlagen gibt man je Anlage an, wo der Wechselrichter angeschlossen ist; die App sendet nie auf dem Bus, den er abfragt, und hört auf RS485-1/2 vor der ersten Anfrage einer Verbindung mit. Ohne Angabe sendet sie für diese Anlage nichts.
related: topic.buses, topic.connection, topic.installations
---
- **CAN-H/L der CAN-Buchse:** der übliche Aufbau. Bei mehreren Packs ist Pack 00 Master (DIP 5–8 = Zahl der Slaves) und fragt die Slaves auf RS485-1/2 ab; die App liest über das RS485-Paar der CAN-Buchse, RS485-1/2 erst nach dem Wartungsstopp des Masters. Ein einzelnes Pack ist auf beiden Bussen erreichbar.
- **RS485-Paar der CAN-Buchse:** der Wechselrichter fragt dort ab, entweder Pack 00 nach Systemwerten (Modbus, Pylontech) oder jedes Pack einzeln. Dieser Bus ist belegt, die App nutzt RS485-1/2. Fragt dort zusätzlich Pack 00 als Master seine Slaves ab, hört die App das vor der ersten Anfrage und sendet nichts.
- **RS485-1/2:** ein externer Rechner oder Wechselrichter fragt die Packs ab (RS485-Adressschema des Herstellers, DIP 5–8 überall aus). Die App nutzt das RS485-Paar der CAN-Buchse; alle Packs erreicht sie dort nur, wenn deren CAN-Buchsen-RS485 verbunden ist. Dass ein Wechselrichter so abfragt, ist an keinem Gerät belegt.
- **Kein Wechselrichter:** beide Busse frei, solange kein Seplos-Master auf RS485-1/2 abfragt.
- **Nur ansehen und Platine flashen:** ohne Anlage gibt es keine Angabe; man wählt selbst einen freien Anschluss. Die App hört vor der ersten Anfrage kurz auf diesem Anschluss mit und sendet nichts, wenn dort schon jemand sendet.
- **Mithören auf RS485-1/2:** vor der ersten Anfrage einer Verbindung hört die App gut eine Sekunde zu. Fragt dort ein Master seine Slaves ab oder sendet ein anderes Gerät, sendet sie nichts. Das fängt auch ein einzelnes Pack ab, das trotzdem Slaves eingestellt hat.
- **Wechselrichter pausiert:** für ein Update oder eine Wartung über den Bus des Wechselrichters dessen Kommunikation trennen und in der Seitenleiste den Haken setzen. Gilt nur für diese Sitzung; vor dem Wiederverbinden des Wechselrichters abschalten. Während einer laufenden Aktion wird eine Änderung erst danach wirksam.
- **Gilt für alles:** die Regel steckt an einer Stelle, durch die jede Anfrage der App geht: Verbinden, Suche, Live-Abfrage, Wartung, Steuerung und Update.
