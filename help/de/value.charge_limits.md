---
title: Lade- und Entladegrenzen
short: CVL/CCL/DCL/DVL, die der Master an den Wechselrichter sendet.
related: value.system
---
- **CVL** (Ladespannung) = Parameter 14 (Pack-Überspannungsschutz), fest.
- **CCL** (Ladestrom) = Lade-Überstrom-Warnung (P50) ÷ 2 bzw. − 10 A, mal ladefähige Packs; 10 A gesamt, wenn alle Packs eine Lade-Warnung haben; 0 bei Zelle über OVP + 30 mV oder wenn kein Pack laden darf.
- **DCL** (Entladestrom) = Entlade-Überstrom-Warnung (P52) − 10 A, mal entladefähige Packs.
- **DVL** = Parameter 12.
