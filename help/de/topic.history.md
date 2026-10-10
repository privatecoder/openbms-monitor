---
title: Verlauf
short: Zeigt die aufgezeichneten Werte als Diagramme über die Zeit, mit Export und Import.
related: topic.recording, value.share
---
- **Anlage und Zeitraum:** Die Daten sind nach Anlage getrennt (Gateway-Adresse bzw. Port). Zeiträume bis jetzt (1 h bis 30 Tage) aktualisieren sich alle 30 s; „Alles“ zeigt die ganze Aufzeichnung.
- **Diagramme je Pack:** Strom, Stromanteil, SOC, Pack-Spannung, höchste und niedrigste Zelle, Zelldifferenz, wärmste und kälteste Zelle. Jeder Pack hat überall dieselbe Farbe; über die Pack-Knöpfe blendest du einzelne aus.
- **Master** (wenn aufgezeichnet): Gesamtstrom mit Lade- und Entladegrenze, SOC, Spannung mit Ladespannung, Zellextreme der Bank.
- **Zellen eines Packs:** alle Zellspannungen eines gewählten Packs, z. B. um Balancing oder eine schwache Zelle zu sehen.
- **Vergrößern:** im Diagramm einen Bereich ziehen; alle Diagramme folgen und zeigen feinere Werte, bis zu jeder Abfrage. Der Mauszeiger zeigt in allen Diagrammen denselben Zeitpunkt.
- **Auflösung:** höchstens etwa 1500 Punkte je Diagramm. Jeder Punkt ist der Mittelwert seines Zeitabschnitts; bei Zellextremen, Zelldifferenz und Temperaturen der Extremwert, damit Spitzen nicht verschwinden. Kurz fehlende Werte werden überbrückt, längere Ausfälle bleiben als Lücke sichtbar.
- **Meldungen:** Unter den Diagrammen stehen die Meldungen im Zeitraum mit Beginn und Ende. Ein Klick auf die Zeit vergrößert die Diagramme auf diesen Moment.
- **Exportieren:** der gezeigte Zeitraum und die eingeblendeten Packs, als CSV (eine Zeile je Pack und Zeitpunkt mit allen Zellen; die Masterwerte in einer zweiten Datei) oder als JSON Lines (das frühere Aufzeichnungsformat, zum Weitergeben und Wiedereinlesen). Zeiten in UTC.
- **Importieren:** JSON-Lines-Aufzeichnungen früherer Versionen oder Exporte, für die gezeigte Anlage. Bereits vorhandene Zeitpunkte werden nicht überschrieben, doppeltes Importieren schadet also nicht.
