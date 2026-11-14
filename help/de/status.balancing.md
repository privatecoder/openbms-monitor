---
title: Balancing
short: Passives Balancing: Eine hohe Zelle wird über einen Widerstand leicht entladen. Die App zeigt, welche Zellen das BMS gerade balanciert.
related: value.cell_delta
---
Das BMS balanciert nur, wenn alles zutrifft:
- Balancing ist eingeschaltet (Funktionsschalter BitGroup6 Bit 0).
- Der Pack lädt; mit BitGroup6 Bit 1 auch im Leerlauf, aber **nie beim Entladen**.
- Die Spreizung (höchste minus niedrigste Zelle) liegt über **Parameter 62** (oft 30 mV).
- Eine Zelle liegt mindestens bei der Startspannung **Parameter 8** (oft 3,40 V) und mindestens **Parameter 63** (oft 20 mV) über der niedrigsten Zelle. Ausgewählt wird von der höchsten Zelle abwärts.
- Keine Sperre: Zeitlimit im Leerlauf (Parameter 64), Umgebungstemperatur-Warnung, Zelldifferenz-Fehler.

Zwei balancierte Zellen müssen mindestens drei Positionen auseinander liegen (Grenze des Balancing-Chips). LiFePO4 erreicht 3,40 V nur am Ende einer Ladung; bei gut gepaarten Zellen ist Balancing deshalb selten zu sehen.

Das BMS meldet auf beiden Bussen nur, welche Zellen es balanciert, keinen Grund, warum nicht. Die Pack-Ansicht erklärt erkennbare Ursachen aus den Messwerten und den gelesenen Parametern des Packs (Schwellen Parameter 8 und 62, Schalter), über RS485-1/2 oder mit EMU1101-EXT auch über die CAN-Buchse gelesen; ohne gelesene Parameter nennt sie die üblichen Werte. Sind alle geprüften Bedingungen erfüllt, bleibt der tatsächliche Sperrgrund unbekannt. Hängt ein externer Balancer am Pack (Installationen, Geräte), zeigt eine eigene Zeile, ob er laut Berechnung arbeitet.
