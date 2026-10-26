---
title: Zellheizung ein
short: Die Heizung schaltet ein, wenn die kälteste Zelle diesen Wert erreicht. Nur mit Heizfolie und Funktionsschalter 3, Bit 6.
related: topic.params
---
Anforderung: Event 4 Bit 6 (Freigabe Funktionsschalter 3, Bit 6). Der Heizausgang schaltet nur beim Laden bzw. mit erkanntem Ladegerät. Solange geheizt werden soll, meldet der Master mindestens Packs × 10 A als Ladegrenze, auch wenn sie sonst 0 wäre.
