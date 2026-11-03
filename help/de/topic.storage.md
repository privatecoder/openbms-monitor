---
title: Daten & Speicher
short: Zeigt auf der Seite „Über“, was im Datenordner der App liegt, und gibt Platz von Hand frei.
related: topic.recording, topic.installations
---
- **Im Alltag** hält die Aufbewahrung der Aufzeichnung den Umfang klein: Jede Abfrage bleibt die gewählten Tage, danach Minutenwerte. Von Hand freigeben ist die Ausnahme.
- **Aufzeichnung leeren** (eine Anlage oder alle): löscht die aufgezeichneten Werte und Ereignisse. Anlagen, Platinen, Parameterstände und das BMS-Archiv bleiben.
- **Sicherungskopien löschen:** Die App legt vor jeder Umstellung ihrer Datenbanken eine Kopie an. Ohne sie gibt es keinen Rückweg auf den Stand davor.
- **Datenordner leeren:** löscht Aufzeichnung, Parameterstände, Parameterdateien, Firmware-Protokolle, Exporte, Sicherungskopien und Reste von Importen; die App startet dafür neu und leert beim Start, bevor sie etwas öffnet. Es bleiben die Anlagen mit Platinen, Plätzen, Aufbau und Benachrichtigungen, das BMS-Archiv (ältere Einträge kann das BMS nicht mehr liefern), die Sicherheitszustände der Packs und Importe, die sich noch fortsetzen lassen. Scheitert ein Schritt, löscht die App danach nichts weiter und bietet „Rest erneut versuchen“ oder „Verwerfen“. Solange ein Import offen ist, löscht sie gar nichts.
- **Sicherung vorher:** „Als ZIP sichern“ legt je Anlage mit Daten eine ZIP-Datei in einen Ordner deiner Wahl außerhalb des Datenordners, wie „Anlage exportieren“ mit Aufzeichnung; vorhandene Dateien bleiben unangetastet, „Anlage importieren“ holt die Daten zurück. Von der Sicherung bis zum Löschen pausiert die Aufzeichnung, damit nichts Ungesichertes mitgelöscht wird. Scheitert eine Sicherung, wird nichts gelöscht. Parameterdateien, Parameterstände und Exporte sind nicht in der ZIP: vorher aus dem Ordner kopieren, was du behalten willst.
- **Platz:** Gelöschte Werte geben ihren Platz erst frei, wenn die Datei verkleinert ist; dafür braucht das Laufwerk vorübergehend freien Platz. Gelingt das nicht, steht es dort, und neue Werte nutzen den Platz in der Datei.
