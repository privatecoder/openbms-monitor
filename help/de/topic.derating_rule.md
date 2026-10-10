---
title: Ladestrom nach Temperatur
short: Viele Datenblätter begrenzen den Ladestrom je nach Zelltemperatur (und teils SOC). Die Tabelle gibt Werte an Stützpunkten an.
related: topic.cells
---
- **Zwischen zwei Stützpunkten gilt der kleinere Wert**, ebenso zwischen SOC-Spalten und an gemeinsamen Grenzen von Bändern. Beispiel EVE LF280K Rev C: bei 3 °C gilt 0,03 P (der Wert bei 0 °C), nicht ein Zwischenwert.
- **Außerhalb der Tabelle wird nicht geladen.**
- Das ist eine Festlegung der App, keine Herstellerangabe: Belegt sind nur die gedruckten Punkte, und der kleinere Nachbarwert liegt sicher nicht über dem, was der Hersteller erlaubt. Erlaubt ein Datenblatt ausdrücklich lineares Interpolieren, wird das im Eintrag vermerkt.
- Der Temperaturbereich im Datenblatt ist **keine Ladefreigabe**: Manche Zellen erlauben an dessen Grenzen 0 A. Maßgeblich ist, wo die Tabelle mehr als 0 zulässt.
