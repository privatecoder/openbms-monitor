---
title: Patch-Firmware: Freigabebits
short: Zwei Bits in Funktionsschalter 0 erlauben das Schreiben über die CAN-Buchse (Bit 0) und den Wartungsstart des Masters (Bit 2).
related: topic.firmwareProfile, topic.switches, topic.paramWrite
---
- **Bit 0, Schreiben über die CAN-Buchse:** 1 erlaubt es, 0 sperrt es; das Pack nimmt dann über die CAN-Buchse nichts an (Lesen geht weiter). Steht es aus, sendet die App gar nicht erst und zeigt „Schreiben gesperrt“ in der Spalte. Schaltest du es über die CAN-Buchse selbst aus, erklärt die App vorher den Rückweg: entsperren nur über RS485-1/2 (in einer Bank mit angehaltenem Master oder im Einzelbetrieb) oder am Display.
- **Bit 2, Wartungsstart:** 1 erlaubt „Master anhalten“ über die CAN-Buchse, 0 verweigert es; „Master freigeben“ geht immer. Über die CAN-Buchse lässt es sich nicht ändern (das Pack würde das ganze Schreiben ablehnen); die App zeigt es dort gesperrt. Über RS485-1/2 oder am Display änderbar.
- **Beides gesperrt am Master, der abfragt:** Dann hilft die CAN-Buchse nicht mehr. Für RS485-1/2 den Bus von Hand freigeben (DIP 5–8 vor Ort) oder das Display nutzen.
- **Wann die App die Bits deutet:** nur mit Beleg für genau dieses Pack: beim Master die Firmwarekennung auf dieser Verbindung; bei einem Slave ein Eintrag für seine Platine (BMS-Seriennummer), den die App nach einem eigenen Flash mit einem Patch-Image anlegt oder den du mit „Patch-Firmware bestätigen“ setzt. Jeder neue Flash dieses Packs durch die App nimmt den Beleg weg. Ein Flash außerhalb der App ist nicht zu sehen: dann den Beleg mit „zurücknehmen“ entfernen.
- **Ohne Beleg:** Die Bits heißen „Bedeutung erst mit belegter Patch-Firmware“, und beim Schreiben über die CAN-Buchse bleiben sie so, wie sie im Pack stehen. Lehnt ein Pack trotzdem mit E3 ab, war das Schreiben dort gesperrt: nichts übernommen, Stand neu lesen.
- **Beim Update auf Patch-Firmware:** Das Flashen übernimmt die vorhandenen Bits unverändert. Ältere Werte mit Bit 0 oder 2 auf 0 sperren danach; die Firmware-Ansicht zeigt den Stand vorher (frisch gelesen, sonst den letzten gespeicherten mit Datum, ungeprüft).
