---
title: Firmwareprofil der Verbindung
short: Nach dem Verbinden über die CAN-Buchse fragt die App den Master nach seiner Firmware und richtet sich danach ein.
related: topic.patchGates, topic.maintenance, topic.buses
---
- **Wann:** bei jedem Verbinden über die CAN-Buchse, auf genau diesem Anschluss, bevor die Abfrage beginnt (Befehl 0x64 an Adresse 0). Das Profil gilt nur für diese Verbindung; ein anderer Anschluss, ein neues Verbinden oder ein Update wird neu geprüft. RS485-1/2 kennt den Befehl nicht; dort gibt es kein Profil.
- **Patch-Firmware (ab MAINT 1.5):** Die Zeile zeigt den Build (gelistet: 16.06.13 MAINT 1.5; sonst „MAINT x.y (Basis …, Build nicht gelistet)“), den Wartungszustand und die Freigaben des Masters. Dann liest die App beim Verbinden die Seriennummern jedes Packs über die CAN-Buchse, bietet „Master anhalten“ an und richtet die Parameteransicht nach den Freigabebits ein. Was eine Patch-Firmware kann, folgt aus ihrer Patch-Version, nicht aus der Liste: Die Patch-Funktionen bleiben in späteren Versionen gleich. Ein nicht gelisteter Build wird dabei allein an seiner gemeldeten Version erkannt.
- **Unbekannte Patch-Firmware:** ein älterer oder anderer Patch-Stand. Die App nutzt dann nur Original-Funktionen und sendet keine Patch-Befehle.
- **0x64 nicht unterstützt:** typisch für Original-Firmware, beweist sie aber nicht. Alle Original-Funktionen gehen wie gewohnt.
- **Firmware nicht erkannt:** keine oder keine gültige Antwort. Mit „Erneut prüfen“ fragt die App noch einmal.
- **Slaves:** Die Kennung meldet nur der Master. Ob ein Slave Patch-Firmware hat, zeigt erst sein eigenes Antworten über die CAN-Buchse; was die Freigabebits bei ihm bedeuten, gilt erst mit Beleg (siehe „Patch-Firmware: Freigabebits“).
