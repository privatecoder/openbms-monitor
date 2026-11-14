---
title: Parameter
short: Die Einstellungen der Packs: Grenzwerte, Verzögerungen, Balancing und Funktionsschalter. Die App liest und vergleicht sie und schreibt sie auf Knopfdruck, auf ein Pack oder dieselbe Änderung auf mehrere; dazu Seriennummern, BMS-Uhr, CAN-Protokoll und Kalibrierung. Nur sichtbar, wenn die Wartungsfunktionen eingeschaltet sind.
related: topic.buses, topic.access, topic.switches, topic.paramWrite, topic.calibration, topic.control
---
- **Aus den Packs lesen:** über die Wege unten. An der CAN-Buchse liest jedes Pack an seiner Adresse und mit seiner eigenen Firmware, ohne dass der Master anhält; die Spalte trägt dann „CAN“. Liest die App dort auch seine Seriennummern (Wege unten), prüft sie die Platine wie auf dem RS485-1/2-Bus, speichert den Stand und schreibt über jeden Weg, den die Liste fürs Schreiben nennt. Ohne eindeutige BMS-Seriennummer bleibt es beim Lesen, und nichts wird gespeichert. Antwortet ein Pack dort nicht, hat es den Patch nicht oder ist nicht erreichbar; dann RS485-1/2 verwenden. Die Freigabebits in Funktionsschalter 0 erklärt „Patch-Firmware: Freigabebits“. Ein **Master**, der seine Slaves abfragt (DIP 0 mit Slaves), antwortet auf dem RS485-1/2-Bus nicht. Er wird erreichbar, sobald er nicht mehr abfragt: mit Patch-Firmware über „Master anhalten“ im Wartungsbereich (danach „Master freigeben“), sonst mit DIP 5–8 auf OFF oder kurz allein am Bus; oder seine Werte kommen aus einer Parameterdatei der App.
- **Parameter lesen**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4.
  - UART2 · Display/Bluetooth: Originalfirmware genügt; Group7.Bit3 (Displayport aktiv) = 1; nur die Platine am Displayport; die App nutzt diesen Weg nicht.
- **Parameter und Schalter schreiben**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4; Group0.Bit0 (UART1-Schreibfreigabe) = 1.
  - UART2 · Display/Bluetooth: Originalfirmware genügt; Group7.Bit3 (Displayport aktiv) = 1; nur die Platine am Displayport; die App nutzt diesen Weg nicht.
- **Seriennummern lesen**
  - UART0 · RS485-1/2: Originalfirmware genügt; liefert Batterie- und BMS-Seriennummer, je bis 30 Zeichen; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4; liefert Batterie- und BMS-Seriennummer, je bis 30 Zeichen.
  - UART1 · CAN-Buchse: Originalfirmware ab 16.06.11; nur am Master, Adresse 0; liefert nur die Batterie-Seriennummer (String 1), die ersten 22 Zeichen.
- **Parameterdatei öffnen:** TOML-Dateien, die diese App gespeichert hat („Als Parameterdatei speichern“). Jeder Wert steht darin unter seinem technischen Schlüssel; Name, Einheit und die Erklärung aus dieser Hilfe stehen als Kommentar daneben und werden beim Öffnen ignoriert. Die App übernimmt eine Datei nur, wenn alle 87 Parameter und die acht Schalter-Bytes vorhanden und gültig sind (Dezimalpunkt, Schalter 0–255). Exporte aus BatteryMonitor und Dateien früherer App-Versionen (XML) lehnt sie ab: dann die Packs lesen und eine neue Parameterdatei speichern. Wirken Werte verschoben, warnt die App und bietet die Datei nicht zum Zurückspielen an.
- **Funktionsschalter 0 beim Übernehmen:** Aus einer Datei, einer anderen Spalte oder einem vorherigen Stand kommt nur Bit 1 (das einzige der Original-Firmware). Die EMU1101-EXT-Bits (Freigaben, erweitertes Aus, Packsperre, Hard-Stop) bleiben, wie das Zielpack sie hat, egal welche Firmware die Quelle hatte. So passt eine Datei auf Packs mit Original-Firmware und mit EMU1101-EXT; die Freigaben stellt man in der Tabelle gezielt ein.
- **Vergleich:** Jede Quelle ist eine Spalte. Werte, die von der Mehrheit abweichen, sind markiert; „Nur Abweichungen“ blendet alles Gleiche aus.
- **Speichern:** Die gelesenen Werte eines Packs lassen sich als Parameterdatei (TOML) sichern (Ordner `parameters` der App-Daten).
- **Letzter Stand (auch ohne Verbindung):** zeigt je Pack den zuletzt gespeicherten Stand der gewählten Anlage als Spalte, mit Datum und ob gelesen oder geschrieben. Gemeint ist der Stand der jetzigen Platine an dieser Adresse; gibt es von ihr keinen, zeigt die Spalte den neuesten und sagt „vorherige Platine“ oder „Platine ungeklärt“. Vergleich, Datenblatt-Prüfung und Speichern als Parameterdatei gehen damit; Schreiben braucht eine Verbindung mit dieser Anlage. Mit „Nur ansehen“ lässt sich lesen, aber nicht schreiben.
- Die Bezeichnungen sind korrigiert: BatteryMonitor übersetzt maschinell aus dem Chinesischen („Monomer“ = Zelle, „pressure“ = Spannung, „Equalization“ = Balancing). Den Originalnamen aus BatteryMonitor zeigt die App, wenn du mit der Maus auf einen Namen zeigst.
