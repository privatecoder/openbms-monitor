---
title: Die zwei RS485-Busse
short: Die Packs haben zwei getrennte RS485-Busse. Welcher Bus angeschlossen ist, bestimmt, was die App lesen und ändern kann.
related: topic.connection, value.system
---
| Bus | Baudrate | Was geht |
|---|---|---|
| **CAN-Buchse** (RS485-Adern Pin 1/2/3 bzw. 6/7/8) | 9600 | Werte und Meldungen **aller** Packs unter ihrer DIP-Adresse, Systemwerte des Masters (Modbus). Keine Parameter, keine Steuerbefehle. |
| **RS485-1/2** | 19200 | voller Befehlssatz: Parameter lesen und schreiben, Steuerbefehle, Historie. Der **Master** (DIP 0 mit Slaves) antwortet hier **nie**, weil er selbst seine Slaves abfragt. |

Für die Überwachung mehrerer Packs ist der Bus der CAN-Buchse ideal; zum Ändern von Einstellungen braucht man RS485-1/2.
