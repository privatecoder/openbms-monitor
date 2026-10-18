---
title: Hard-Stop
short: Eine Eigenschaft des Firmware-Images: Ein voller Pack, der auf die intermittierende Nachladung wartet, öffnet selbst seinen Lade-MOSFET.
related: topic.firmwareProfile, switch.5.1
---
- **Ohne Hard-Stop (Original):** Wartet ein voller Pack mit intermittierender Nachladung (Funktionsschalter 5, Bit 1) darauf, dass sein SOC fällt, senkt nur der Master die Ladegrenze, sobald alle Packs so weit sind. Der Lade-MOSFET des Packs bleibt zu.
- **Mit Hard-Stop:** Der volle, wartende Pack öffnet seinen eigenen Lade-MOSFET und nimmt keinen Ladestrom mehr an, auch wenn der Wechselrichter die Ladegrenze nicht beachtet.
- **Was die Anzeige sagt:** „an“ oder „aus“ meldet das Image selbst (Kennung im Format 3); „unbekannt“ heißt, die Firmware meldet es nicht (Format 2). Es ist eine Eigenschaft des eingespielten Images, unabhängig vom Wartungszustand, von den Freigabebits und davon, ob der Lade-MOSFET gerade offen oder geschlossen ist.
- **Beobachtet:** Meldet die Firmware es nicht (Format 2, Original-Firmware, Slaves), schließt die App nur aus dem Verhalten des Packs: Wartet er auf die Nachladung und ist sein Lade-MOSFET dabei aus, heißt es „beobachtet (am …)“, sonst „unbekannt“. Ein eingeschalteter Lade-MOSFET während des Wartens widerlegt das nicht (die Firmware schaltet ihn nach 10 s eigener Entladung über 10 A wieder ein). Ein Flash des Packs durch die App beendet die Beobachtung; einen Flash außerhalb der App sieht die App nicht. Versionsnummern zählen nicht. Über die CAN-Buchse speichert die App eine Sichtung nur, wenn der Master die Batterie-Seriennummer dieser Anlage meldet; mit Original-Firmware gibt es dort keine Seriennummer je Pack, eine an einer Adresse getauschte Slave-Platine bemerkt die App dann nicht.
