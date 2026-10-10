---
title: Parameter gegen Datenblatt
short: Prüft die Alarm- und Schutzgrenzen des BMS gegen das Datenblatt der Zellen.
related: topic.params, topic.cellAssign, value.datasheet
---
- **Welches Datenblatt:** standardmäßig der Zelltyp, den die meisten Packs unter Übersicht → Zelltypen haben. Du kannst ein anderes Zellmodell und die Zahl paralleler Zellen wählen, etwa für einen Export einer anderen Anlage.
- **Geprüft werden** nur Alarm- und Schutzwerte, keine Wiederherstellungswerte (die liegen absichtlich innen):
  - Zellspannung hoch/Überspannung ≤ Ladeschluss, niedrig/Unterspannung ≥ Entladeschluss
  - Packspannung genauso, mal Zellen in Serie (Parameter 65)
  - Temperatur beim Laden und Entladen innerhalb des Datenblattbereichs; bei einer Derating-Tabelle zählt deren äußerer Ladebereich
  - Lade- und Entladestrom (Warnung und Schutz) ≤ Dauerstrom × Zellen parallel; der negative Entladestrom des BMS zählt nach Betrag
  - Nennkapazität (Parameter 58) ≈ Zellkapazität × parallel (±2 %); eine Abweichung ist nur ein Hinweis
- **Rot** markiert einen Verstoß, die Spalte **Datenblatt** zeigt die Grenze. Ein Wert genau auf der Grenze gilt als eingehalten.
- Ein Verstoß heißt nicht, dass das BMS falsch schützt: Viele Hersteller stellen z. B. den Lade-Untertemperaturschutz unter 0 °C, weil das BMS bei kleinen Strömen tolerant sein soll. Das Datenblatt erlaubt Laden aber erst ab seiner Untergrenze.
