---
title: Zell-Unterspannungs-Warnung
short: Warnung, wenn eine Zelle diesen Wert erreicht. Sie stellt auch den SOC nach (siehe unten).
related: topic.params
---
Meldung: Event 2 Bit 2 (Freigabe Funktionsschalter 1, Bit 2). Der SOC springt auf (SOC-Warnung + SOC-Schutz) / 2, wenn eine Zelle diesen Wert erreicht und der SOC noch mindestens bei der SOC-Warnung steht. Solange alle Zellen darüber liegen, zählt der SOC beim Entladen nicht unter SOC-Schutz + 1 % (geprüft in ganzen Prozent).
Außerdem die Leer-Marke für das Lernen der Kapazität. Als entladebezogene Warnung nimmt sie den Pack aus der Summe der Entladegrenze (DCL); warnen alle Packs, meldet der Master DCL = 0. Schutzstufe: Zell-Unterspannungsschutz.
