---
title: Parameter
short: Die Einstellungen der Packs: Grenzwerte, Verzögerungen, Balancing und Funktionsschalter. Die App liest sie nur, sie ändert nichts.
related: topic.buses, topic.switches
---
- **Aus den Packs lesen:** nur über RS485-1/2 (19200 Baud). Die CAN-Buchse kennt den Befehl nicht. Der **Master** (DIP 0 mit Slaves) antwortet auf RS485-1/2 nicht; seine Werte bekommst du über einen BatteryMonitor-Export oder wenn er kurz allein am Bus hängt.
- **Export öffnen:** XML-Dateien aus BatteryMonitor (Parameter exportieren). Die Werte zählen nach Position, nicht nach Namen. Passen die Namen nicht zur bekannten Reihenfolge, warnt die App: Ein solcher Export stammt aus einer anderen BatteryMonitor-Version und darf nicht zurückgespielt werden.
- **Vergleich:** Jede Quelle ist eine Spalte. Werte, die von der Mehrheit abweichen, sind markiert; „Nur Abweichungen“ blendet alles Gleiche aus.
- **Speichern:** Die gelesenen Werte eines Packs lassen sich als BatteryMonitor-XML sichern (Ordner `parameters` der App-Daten).
- Die Bezeichnungen sind korrigiert: BatteryMonitor übersetzt maschinell aus dem Chinesischen („Monomer“ = Zelle, „pressure“ = Spannung, „Equalization“ = Balancing). Den Originalnamen aus BatteryMonitor zeigt die App, wenn du mit der Maus auf einen Namen zeigst.
