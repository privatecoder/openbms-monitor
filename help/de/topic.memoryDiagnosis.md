---
title: Speicherdiagnose
short: Prüft die gespeicherten Kopien von Parametern und Ladestand eines Packs (MAINT 1.5.3/1.6.3, Befehl 64/04). Liest nur, ändert nichts.
related: topic.hardStop, topic.control
---
- **Was sie zeigt:** je für Parameter und Ladestand/Kapazität die Haupt- und die Sicherungskopie: „leer“ (nie geschrieben), „gültig“ (Aufbau und Prüfsumme stimmen) oder „ungültig“, und ob beide gleich sind. Zwei gültige, aber verschiedene Kopien sagen nicht, welche neuer ist.
- **Packsperre:** nur bei Builds mit gespeicherter Packsperre (MAINT 1.6.3): die in beiden Parameterkopien gespeicherte Sperre und ob der Laufzeitspeicher gültig ist. Bei 1.5.3 steht „nicht unterstützt“.
- **Wege:** über den RS485-1/2-Bus und die CAN-Buchse, an der eigenen Adresse jedes Packs, ohne Freigabe. Die App fragt nur auf Knopfdruck, nie im Hintergrund.
- **Nach einem Neustart:** MAINT 1.6.3 sichert vor einem geplanten Neustart den Ladestand in beide Kopien; schlägt das fehl, lehnt es den Neustart ab (RTN 08). Die Diagnose zeigt dann den Zustand der Kopien.
