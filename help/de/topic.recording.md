---
title: Aufzeichnung
short: Schreibt die abgefragten Werte in eine Datei, ähnlich dem Debug-Log in Home Assistant.
related: topic.connection
---
- **Was:** alle Packs oder eine Auswahl nach Gruppen bzw. einzelnen Packs; dazu wahlweise die Systemwerte des Masters (Modbus, nur über die CAN-Buchse).
- **Wie oft:** jede Abfrage (Fehlersuche, z. B. Stromaufteilung in Paaren, Ausgleichsströme) oder 1× pro Minute bzw. alle 5 Minuten (Langzeitbeobachtung). Die App schätzt die Dateigröße vorab.
- **Datei:** im Ordner `recordings` der App-Daten, Name mit Startzeit (UTC). Format JSON Lines: eine Zeile je Antwort, `{"t": Unix-ms, "pack": {…}}` bzw. `"system"`; die erste Zeile beschreibt die Einstellungen. Fehler (keine Antwort) werden mitgeschrieben.
- Die Aufzeichnung läuft weiter, wenn die Verbindung abbricht und die App neu verbindet, und endet mit „Beenden“ oder beim Schließen der App.
- Zum Ändern der Auswahl die Aufzeichnung beenden und neu starten (neue Datei).
