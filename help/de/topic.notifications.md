---
title: Mitteilungen
short: Meldet wichtige Ereignisse der gewählten Anlage als Mitteilung des Systems, solange die App läuft und verbunden ist. Standardmäßig aus.
related: topic.recording, topic.connection, topic.alarms, topic.healthReport
---
- **Wann:** (1) ein Schutz- oder Fehleralarm wird an einem Pack aktiv, z. B. „Pack 03: Zell-Überspannungsschutz“, erst wieder, nachdem er weg war; (2) ein Pack, das geantwortet hat, antwortet drei Abfragen hintereinander nicht; (3) die Zellspreizung eines Packs in Ruhe (unter 2 A) liegt 5 Minuten über der Schwelle, erneut erst, wenn sie 10 mV unter die Schwelle gefallen ist; (4) keine Daten für die eingestellten Minuten (Verbindung verloren). Warnungen und Hinweise lösen nichts aus.
- **Wie oft:** je Pack und Auslöser höchstens eine Mitteilung in 10 Minuten. Mehrere Packs gleichzeitig kommen in eine Mitteilung („Zell-Überspannungsschutz an Pack 03, 04“). Ein Klick holt die App nach vorn.
- **Nur geprüfte Daten:** Am RS485-1/2 zählt nur ein Pack, dessen Platine die Anlage kennt; neue, umgesteckte oder fremde Packs melden nichts, bis du sie bestätigst. An der CAN-Buchse gibt es keine Prüfung je Pack: Mitteilungen laufen dort nur, wenn die Batterie-Seriennummer des Masters der gespeicherten dieser Anlage entspricht. Sonst zeigt die Einstellung „Für diese Verbindung aus: Bank nicht geprüft (CAN-Buchse)“. „Nur ansehen“ meldet nichts.
- **Schwelle:** 50 mV in Ruhe sind ein guter Anfang. Eine dauerhaft höhere Spreizung spricht für Balancing-Bedarf oder eine schwache Zelle; der Gesundheitsbericht im Verlauf zeigt den Verlauf je Tag.
- **Erlaubnis:** Beim ersten Mal fragt das System, ob die App Mitteilungen zeigen darf. Erscheint nichts, die Mitteilungen der App in den Systemeinstellungen erlauben; „Test-Mitteilung senden“ prüft das.
- **Grenzen:** Ist die App geschlossen, kommt nichts. Kein Versand an Telefon, E-Mail oder MQTT.
