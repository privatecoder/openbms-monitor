---
title: Sperre für erweitertes Aus
short: Patch-Firmware mit Packsperre (MAINT 1.6), gilt für gezieltes Aus über RS485-1/2 und die CAN-Buchse: 1 (an, wie beim Altwert FF) sperrt das erweiterte Aus, ein gezieltes Aus bleibt das bisherige Soft-Aus. 0 (aus) gibt das erweiterte Aus frei: Gezieltes Ausschalten sperrt dann Lade- und Entladepfad einschließlich Strombegrenzer und Vorladung, und die Sperre bleibt über Neustarts erhalten. Einschaltbroadcasts des Masters und Parameterimporte lösen sie nicht; zum Freigeben dieses Pack gezielt einschalten.
related: topic.switches
---

