---
title: Widerstands-Diagnose
short: Vergleicht den Widerstand jedes Packs im Inneren und auf dem Weg zur Sammelschiene, um schlechte Kontakte oder Kabel zu finden.
related: topic.pairs
---
- **Im Pack:** Jedes BMS meldet die Summe seiner Zellen und die Spannung an seinem Ausgang. Unter Strom ist die Differenz der Spannungsfall im Pack (u. a. MOSFETs, Shunt, interne Schienen); geteilt durch den Strom ergibt das den Widerstand. Wird laufend berechnet und geglättet, ab 3 A.
- **Außen:** Differenz zwischen Ausgang des Packs und einem Bezugspunkt, an dem du mit dem Multimeter misst und den Wert einträgst: **Sammelschiene** (ein Wert für alle) oder **Lastschalter des Paares, Eingang auf der Pack-Seite** (ein Wert je Paar; erfasst nur das Stück von den Packs bis zum Schalter, den Schalter selbst misst du getrennt). Die App macht dann eine Momentaufnahme aller Packs.
- **Nullabgleich:** In Ruhe zeigt jede Messung einen kleinen festen Fehler. Den merkt sich die App und zieht ihn später ab; je Bezugspunkt getrennt. Dafür **Lastschalter an lassen** und den Wechselrichter auf Standby schalten: Bei offenen Schaltern läge die Sammelschiene nicht auf dem Potenzial der Packs. Ohne Abgleich sind die Werte nicht verwertbar.
- **Bewertung:** auffällig ist ein Pack über dem 1,5-Fachen des Medians aller Packs und mindestens 1 mΩ darüber.
- **Grenzen:** Die Spannungen haben 10 mV Auflösung, deshalb nur bei genug Strom und nur im Vergleich deuten. Wo genau das BMS den Ausgang abgreift, zeigt die Firmware nicht; je nachdem steckt ein Teil des inneren Pfads im Wert „außen“. Bei Paaren enthält „außen“ auch die gemeinsamen Hauptkabel, die den Strom beider Packs führen.

Ausführliche Anleitung mit Stromzange und Messprotokoll: Messanleitung in der Projekt-Dokumentation.
