---
title: Speicherdiagnose
short: Prüft die gespeicherten Kopien von Parametern und Ladestand eines Packs (EMU1101-EXT, Befehl 64/04). Liest nur, ändert nichts.
related: topic.hardStop, topic.control, topic.access
---
- **Was sie zeigt:** je für Parameter und Ladestand/Kapazität die Haupt- und die Sicherungskopie: „leer“ (nie geschrieben), „gültig“ (Aufbau und Prüfsumme stimmen) oder „ungültig“, und ob beide gleich sind. Zwei gültige, aber verschiedene Kopien sagen nicht, welche neuer ist.
- **Packsperre:** nur bei Builds mit gespeicherter Packsperre (EMU1101-EXT Full): die in beiden Parameterkopien gespeicherte Sperre und ob der Laufzeitspeicher gültig ist. Bei Light steht „nicht unterstützt“.
- **Wege:** an der eigenen Adresse jedes Packs, ohne Freigabe (siehe unten). Die App fragt nur auf Knopfdruck, nie im Hintergrund.
- **Speicherdiagnose lesen**
  - UART0 · RS485-1/2: EMU1101-EXT ab Light 1.5.4 mit Speicherdiagnose (Imageflag Bit 5); Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4 mit Speicherdiagnose (Imageflag Bit 5).
- **Nach einem Neustart:** EMU1101-EXT Full sichert vor einem geplanten Neustart den Ladestand in beide Kopien; schlägt das fehl, lehnt es den Neustart ab (RTN 08). Die Diagnose zeigt dann den Zustand der Kopien.
