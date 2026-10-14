---
title: Firmware-Update
short: Spielt Firmware auf die Packs am Pack-Bus: freigegebene Original-Versionen, auf Wunsch auch eigene. Nur sichtbar, wenn die Wartungsfunktionen eingeschaltet sind.
related: topic.maintenance, topic.buses
---
- **Warum RS485-1/2:** Der Bootloader des BMS hört nur auf diesem Bus (19200 Baud). Über die CAN-Buchse geht kein Update.
- **Warum der Bus ruhig sein muss:** Die Rahmen des Bootloaders tragen keine Pack-Adresse. Ein Master, der seine Slaves abfragt, oder ein zweites Programm stört das Update. Wie man den Bus einer Bank ruhig bekommt, steht unter *Wartung am Pack-Bus*.
- **Was die App direkt vor jedem Update prüft:** etwa 2 s lang kein Byte am Bus, dann antwortet das Ziel-Pack an seiner Adresse auf 0x51 und 0x47. Nichts davon muss man abhaken; schlägt eine Prüfung fehl, bleibt das Pack unverändert.
- **Ablauf:** Die App startet das Pack per Befehl neu. In den ersten etwa 0,7 s antwortet der Bootloader; die App meldet sich darin an, löscht, schreibt die Blöcke und schließt ab. Danach startet die neue Firmware, und die App liest Version und Werte zur Kontrolle.
- **Freigegebene Images:** unveränderte Original-Firmware von Seplos (16.06.x), erkannt an der Prüfsumme. Die Parameter des Packs bleiben erhalten. Die Wartungs-Builds dieses Projekts (16.06.13 MAINT 1.1 und 1.0) erkennt die App ebenfalls an der Prüfsumme; sie sind keine Original-Firmware und gehen nur mit „Eigene Firmware zulassen“.
- **Eigene Firmware:** Mit „Eigene Firmware zulassen“ nimmt die App auch Images mit unbekannter Prüfsumme an, wenn Format und Kopfdaten zum EMU1101 passen. Den Inhalt prüft sie nicht. Die Kontrolle danach klappt nur, wenn die neue Firmware am Pack-Bus mit 19200 Baud antwortet.
- **Zwei Wege, zwei Tabs:** Im Tab „Pack-Bus“ in der Pack-Liste („Flashen“, dann „Jetzt flashen“) oder im Tab „Einzelnes Pack“: Adresse wählen, Pack prüfen und mit `FLASH` und der Adresse bestätigen (eigene Firmware: `FLASH CUSTOM`). Beide Wege laufen durch dieselben Prüfungen; die gewählte Datei gilt in beiden Tabs.
- **Während des Updates:** Strom und Kabel nicht trennen. Die App lässt sich dann nicht schließen; unter macOS gilt das nicht für *Beenden* im Dock, das lässt sich nicht abfangen. Die App also nicht über das Dock beenden.
- **Wenn etwas schiefgeht:** Ein Fehler vor dem Löschen ändert nichts; das Pack startet seine alte Firmware von selbst wieder. Ein Fehler danach kann das Pack im Bootloader lassen: Die App merkt sich das (siehe *Wartung am Pack-Bus*, „Pack hängt eventuell im Bootloader“). Dann das Log sichern (Ordner `firmware-logs` der App-Daten) und nicht einfach neu starten oder das Update blind wiederholen. Scheitert erst die Kontrolle danach, ist die neue Firmware bereits gestartet; dann die Werte des Packs im Monitor prüfen.
