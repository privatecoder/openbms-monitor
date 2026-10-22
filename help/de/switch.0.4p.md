---
title: Erweitertes Aus
short: Patch-Firmware mit Packsperre und positiven Bits 3/4 (MAINT 1.6.1, Imageflag Bit 3), gilt für gezieltes Aus über den RS485-1/2-Bus und die CAN-Buchse: 1 (an, wie beim Altwert FF) gibt das erweiterte Aus frei: Gezieltes Ausschalten sperrt dann Lade- und Entladepfad einschließlich Strombegrenzer und Vorladung, und die Sperre bleibt über Neustarts erhalten. 0 (aus): ein gezieltes Aus bleibt das bisherige Soft-Aus. Einschaltbroadcasts des Masters und Parameterimporte lösen die Sperre nicht; zum Freigeben dieses Pack gezielt einschalten.
related: topic.switches
---

