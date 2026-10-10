---
title: Grenzen laut Datenblatt
short: Live-Werte des Packs gegen das Datenblatt seiner Zellen. Das BMS schützt nach seinen eigenen Parametern; dieser Vergleich zeigt, ob diese zum Datenblatt passen.
related: topic.cellAssign
---
- **Zellspannung:** höchste Zelle gegen den Ladeschluss (nahe ab 50 mV darunter), niedrigste gegen den Entladeschluss (nahe ab 200 mV darüber).
- **Temperatur:** kälteste und wärmste Zelle gegen den Bereich fürs Laden bzw. Entladen (nahe ab 5 K vor der Grenze). Außerhalb des Ladebereichs ist das nur ein Verstoß, solange geladen wird; sonst steht dort „Laden jetzt nicht erlaubt“.
- **Ladestrom:** erlaubt ist der Wert der Derating-Tabelle des Datenblatts bei der kältesten und der wärmsten Zelle (der niedrigere zählt), höchstens der Dauerstrom. Ohne Tabelle gilt der Dauerstrom innerhalb des Ladebereichs. „Empfohlen“ ist der Standard-Ladestrom. Nahe ab 80 %.
- **Entladestrom** gegen den Dauerstrom, mal Zellen parallel.
- Bei Lade- und Entladegrenze in der Übersicht addiert die App die laut Datenblatt erlaubten Ströme aller Packs („Datenblatt erlaubt …“). Steht dort rot, meldet der Master dem Wechselrichter mehr, als die Zellen jetzt erlauben.
