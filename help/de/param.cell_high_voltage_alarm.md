---
title: Zell-Hochspannungs-Warnung
short: Warnung, wenn eine Zelle diesen Wert erreicht. Haben alle Packs eine Lade-Warnung, senkt der Master die Ladegrenze auf 10 A gesamt. Außerdem Grundlage der 100-%-Regel des SOC (Zellen × diesem Wert, begrenzt auf 3,45–3,50 V je Zelle).
related: topic.params
---
Meldung: Event 2 Bit 0 (Freigabe Funktionsschalter 1, Bit 0). Für die Ladegrenze fällt jeder warnende Pack aus der Summe; erst wenn keiner übrig ist, gilt 10 A gesamt.
SOC: Oberhalb 98,2 % zählt der SOC beim Laden nur weiter, wenn eine Zelle diesen Wert erreicht. 100 % setzt die Firmware, wenn die Pack-Spannung 30 min lang mindestens Zellen × diesem Wert beträgt. Außerdem die Voll-Marke für das Lernen der Kapazität. Hysterese: Rückkehrwert „Zell-Hochspannungs-Warnung aus“.
