---
title: Verbindung
short: So verbindet sich die App mit den Batterien: per USB-RS485-Adapter oder per RS485-zu-Ethernet-Gateway (TCP).
related: topic.buses
---
- **Anlage:** Vor dem Verbinden wählt man oben die Anlage. Alles, was die App speichert (Aufzeichnung, Seriennummern, Parameterstände, Zelltypen, Gruppen), gehört dann zu ihr. Ein USB-Port ändert die Wahl nie, denn derselbe Adapter wandert zwischen Batterien. Ein Gateway, das genau einer aktiven Anlage gehört, wählt diese von selbst; gehört es mehreren, bleibt die gewählte, wenn sie dazugehört, sonst bittet die App um die Wahl. Ein neuer Anschluss wird beim Verbinden der gewählten Anlage zugeordnet (eine Zeile, kein Dialog). Solange die App verbunden ist, bleibt die Wahl fest; auch das automatische Neuverbinden nutzt sie.
- **Nur ansehen:** für einen kurzen Blick auf eine fremde Bank. Nichts wird gespeichert (keine Aufzeichnung, keine Seriennummern, keine Parameterstände), und nichts wird in die Packs geschrieben.
- **Netzwerk (TCP):** Adresse des Gateways im Format `Host:Port`, z. B. `192.168.1.10:4196`. Die Baudrate stellst du am Gateway ein (CAN-Buchse 9600, RS485-1/2 19200).
- **USB/Seriell:** Port des Adapters wählen; die Baudrate ergibt sich aus dem gewählten Bus.
- **Reihenfolge nach dem Verbinden:** An der CAN-Buchse liest die App zuerst die Systemwerte des Masters (Modbus) und zeigt sie sofort an. Meldet der Master seine Slave-Zahl (nur mit der Wartungs-Firmware, Befehl 0x64), fragt die App genau diese Packs ab, ohne Suche. Sonst gilt die Einstellung **Packs**: eine feste Zahl startet die Abfrage der Adressen 0 bis Zahl−1 sofort, „suchen“ fragt die Adressen 0–15 ab. Danach werden alle Packs alle 2 s abgefragt; Name und Firmware-Version folgen kurz nach dem Start. Ist der Wartungsmodus des Masters aktiv, meldet er 0 Slaves; dann sucht die App.
- Kommt **60 s** lang keine gültige Antwort, gilt die Verbindung als getrennt („Verbindung verloren“). Schließt das Gateway die Verbindung oder bricht sie mit einem Fehler ab, gilt sie sofort als getrennt. Die App verbindet dann alle 30 s automatisch neu, bis wieder Packs antworten.
- Nur ein Programm darf gleichzeitig auf dem Bus fragen. Läuft z. B. das Home-Assistant-Add-on am selben Gateway, kollidieren die Anfragen.
