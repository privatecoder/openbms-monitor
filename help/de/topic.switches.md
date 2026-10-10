---
title: Funktionsschalter
short: Acht Bytes (BitGroup0–7) mit je acht Schaltern.
related: topic.params
---
- **BitGroup1–5** geben die Meldungen frei: Ein Bit auf 0 schaltet die Meldung **und ihre Reaktion** ab (kein MOSFET-Abschalten, keine Folge für Ladegrenze oder SOC).
- **BitGroup0** ist keine Maske; nur Bit 1 (Prüfung der Temperaturfühler) wirkt.
- **BitGroup6/7** sind Funktionen: Balancing, Ladeaktivierung, Strombegrenzung, Selbstabschaltung, Ladegrenze.
- Bits ohne Funktion zeigt die App ausgegraut.
