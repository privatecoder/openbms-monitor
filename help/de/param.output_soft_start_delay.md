---
title: Vorladung des Ausgangs
short: Beim Einschalten lädt das BMS zuerst über einen Vorladepfad, höchstens so lange. Danach prüft es auf Kurzschluss und schaltet den Entlade-MOSFET ein.
related: topic.params
---
Die Vorladung endet früher, wenn die Ausgangsspannung nicht mehr steigt. Liegt der Ausgang danach unter der Hälfte des Sollwerts, meldet das BMS Kurzschluss (Event 5 Bit 5, nur mit Funktionsschalter 4, Bit 5).
