---
title: Firmware-Update
short: Spielt eine Original-Firmware von Seplos auf ein einzelnes Pack. Nur sichtbar, wenn die Wartungsfunktionen eingeschaltet sind.
related: topic.buses, topic.connection
---
- **Warum RS485-1/2:** Der Bootloader des BMS hört nur auf diesem Bus (19200 Baud). Über die CAN-Buchse geht kein Update.
- **Warum nur ein Pack am Bus:** Die Rahmen des Bootloaders tragen keine Pack-Adresse. Ein Master, der seine Slaves abfragt, oder ein zweites Programm stört das Update. Die App prüft vorher etwa 2 s lang, ob der Bus still ist.
- **Ablauf:** Die App startet das Pack per Befehl neu. In den ersten etwa 0,7 s antwortet der Bootloader; die App meldet sich darin an, löscht, schreibt die Blöcke und schließt ab. Danach startet die neue Firmware, und die App liest Version und Werte zur Kontrolle.
- **Nur freigegebene Images:** unveränderte Original-Firmware von Seplos (16.06.x), erkannt an der Prüfsumme. Die Parameter des Packs bleiben erhalten.
- **Wenn etwas schiefgeht:** Ein Fehler vor dem Löschen ändert nichts, die alte Firmware läuft weiter. Ein Fehler danach kann das Pack im Update-Modus lassen. Dann das Log sichern (Ordner `firmware-logs` der App-Daten) und nicht einfach neu starten oder das Update blind wiederholen.
