---
title: Wechselrichter-Anschluss
short: Auf einem RS485-Bus darf nur ein Gerät abfragen. Die App sendet deshalb nie auf dem Bus, den der Wechselrichter abfragt, und hört auf RS485-1/2 vor der ersten Anfrage einer Verbindung mit. Ohne Angabe sendet sie gar nichts.
related: topic.buses, topic.connection
---
- **CAN (Pack 00 sammelt die Werte):** der übliche Aufbau. Bei mehreren Packs ist Pack 00 Master (DIP 5–8 = Zahl der Slaves) und fragt die Slaves auf RS485-1/2 ab; die App liest über die CAN-Buchse, RS485-1/2 erst nach dem Wartungsstopp des Masters. Ein einzelnes Pack ist auf beiden Bussen erreichbar.
- **RS485 der CAN-Buchse an Pack 00 (Systemwerte):** der Wechselrichter fragt Pack 00 per Modbus oder Pylontech-Systembefehlen ab. Die CAN-Buchse ist belegt. Ein einzelnes Pack ist über RS485-1/2 erreichbar; bei mehreren Packs fragt Pack 00 dort zusätzlich seine Slaves ab, dann ist kein Bus frei, solange Wechselrichter und Master laufen.
- **RS485-1/2, jedes Pack einzeln:** der Aufbau nach dem RS485-Adressschema des Herstellers (DIP 5–8 überall aus), ein externer Rechner oder Wechselrichter fragt die Packs ab. RS485-1/2 ist belegt, die App nutzt die CAN-Buchse; alle Packs erreicht sie dort nur, wenn deren CAN-Buchsen-RS485 verbunden ist. Dass ein Wechselrichter so abfragt, ist an keinem Gerät belegt.
- **RS485 der CAN-Buchse, jedes Pack einzeln:** die CAN-Buchse ist belegt, die App nutzt RS485-1/2. Auch hier ist kein konkreter Wechselrichter belegt.
- **Kein Wechselrichter:** beide Busse frei, solange kein Seplos-Master auf RS485-1/2 abfragt.
- **Mithören auf RS485-1/2:** vor der ersten Anfrage einer Verbindung hört die App gut eine Sekunde zu. Fragt dort ein Master seine Slaves ab oder sendet ein anderes Gerät, sendet sie nichts. Das fängt auch ein einzelnes Pack ab, das trotzdem Slaves eingestellt hat.
- **Wechselrichter pausiert:** für ein Update oder eine Wartung über den Bus des Wechselrichters: dessen Kommunikation trennen und den Haken setzen. Gilt nur für diese Sitzung; vor dem Wiederverbinden des Wechselrichters abschalten. Während eines Updates lässt sich die Angabe nicht ändern.
- **Gilt für alles:** die Regel steckt an einer Stelle, durch die jede Anfrage der App geht: Verbinden, Suche, Live-Abfrage, Wartung, Steuerung und Update.
