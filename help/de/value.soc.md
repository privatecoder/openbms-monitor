---
title: Ladestand (SOC)
short: Vom BMS gezählter Ladestand in %. Er wird zusätzlich über feste Spannungsregeln korrigiert.
related: value.cycles
---
- Das BMS zählt jede Sekunde den Strom (Coulomb-Zählung).
- Beim Laden bleibt der SOC bei **98,2 %** stehen, bis eine Zelle die Zell-Hochspannungswarnung (P0) erreicht.
- **100 %** setzt das BMS, wenn die Pack-Spannung 30 min lang ≥ Zellenzahl × P0 (3,45–3,50 V) liegt und mit weniger als 3 A geladen wird, oder bei Überspannungsschutz.
- Weitere Sprünge auf 23/70/90/98 % bzw. 10/0 % bei passenden Spannungen.
