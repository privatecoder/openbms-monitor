---
title: Nachladen erst wieder unter
short: Nach 100 % ist Laden gesperrt, bis der SOC auf diesen Wert fällt. Sind alle Packs so weit, meldet der Master Ladegrenze 0. Nur mit Funktionsschalter 5, Bit 1.
related: topic.params
---
Meldung: Event 6 Bit 1 (Freigabe Funktionsschalter 5, Bit 1). Fordert ein Pack Heizung an, hebt das die Ladegrenze 0 auf den Heizungs-Mindeststrom. Über diesem Wert driftet der SOC im Standby zusätzlich um 0,2 A.
Original-Firmware: Der wartende Pack selbst sperrt seinen Lade-MOSFET nicht, nur der Master senkt die Ladegrenze. Patch-Firmware mit Hard-Stop öffnet den Lade-MOSFET des wartenden Packs (bei EMU1101-EXT Full per Funktionsschalter 0, Bit 6/7 wählbar).
