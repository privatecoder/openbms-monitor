---
title: Hard-Stop
short: Eine Eigenschaft des Firmware-Images: Ein voller Pack, der auf die intermittierende Nachladung wartet, öffnet selbst seinen Lade-MOSFET.
related: topic.firmwareProfile, switch.5.1
---
- **Ohne Hard-Stop (Original):** Wartet ein voller Pack mit intermittierender Nachladung (Funktionsschalter 5, Bit 1) darauf, dass sein SOC fällt, senkt nur der Master die Ladegrenze, sobald alle Packs so weit sind. Der Lade-MOSFET des Packs bleibt zu.
- **Mit Hard-Stop:** Der volle, wartende Pack öffnet seinen eigenen Lade-MOSFET und nimmt keinen Ladestrom mehr an, auch wenn der Wechselrichter die Ladegrenze nicht beachtet.
- **Was die Anzeige sagt:** „an“ oder „aus“ meldet das Image selbst (Kennung im Format 3); „unbekannt“ heißt, die Firmware meldet es nicht (Format 2). Es ist eine Eigenschaft des eingespielten Images, unabhängig vom Wartungszustand, von den Freigabebits und davon, ob der Lade-MOSFET gerade offen oder geschlossen ist.
- **Beobachtet:** Meldet die Firmware es nicht (Format 2, Original-Firmware, Slaves), schließt die App nur aus dem Verhalten des Packs: Wartet er auf die Nachladung und ist sein Lade-MOSFET dabei aus, heißt es „beobachtet (am …)“; wartet er mit eingeschaltetem Lade-MOSFET, „nicht beobachtet (am …)“; sonst „unbekannt“. Versionsnummern oder ein Flash zählen dafür nicht.
