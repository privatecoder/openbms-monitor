---
title: Aufzeichnung
short: Speichert die abgefragten Werte in der Verlaufs-Datenbank der App, solange sie verbunden ist. Standardmäßig aus.
related: topic.history, topic.connection
---
- **Aufzeichnen, solange verbunden:** Einmal eingeschaltet, bleibt die Einstellung über Neustarts erhalten. Die App schreibt dann bei jeder Verbindung mit, ohne dass du etwas starten musst.
- **Was:** alle Packs oder eine Auswahl, dazu auf der CAN-Buchse die Systemwerte des Masters. Meldungen werden immer mit Beginn und Ende gespeichert.
- **Wie oft:** jede Abfrage (für Fehlersuche, etwa 11 MB pro Tag bei 12 Packs), einmal pro Minute oder alle 5 Minuten.
- **Aufbewahren:** Jede Abfrage bleibt zunächst erhalten (Standard 30 Tage). Danach fasst die App sie zu einem Wert je Minute zusammen: Mittelwert für Strom, SOC und Spannung, der Extremwert für höchste und niedrigste Zelle und die Temperaturen, die niedrigere Grenze bei Lade- und Entladegrenze. Minutenwerte bleiben für immer oder werden nach der gewählten Zeit gelöscht.
- **Wo:** eine SQLite-Datei `history.sqlite` im Datenordner der App. Ansehen, exportieren und ältere Aufzeichnungen (.jsonl) importieren unter **Verlauf**.
