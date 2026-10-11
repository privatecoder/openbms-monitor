---
title: Paar-Werte deuten
short: Wie Spannung, Strom und Ladestand der beiden Packs eines Paares zusammenhängen, und was ein Muster bedeutet.
related: topic.pairs, topic.diagnosis
---
- **Grundsatz:** Die beiden Packs hängen über zwei kurze Brücken direkt aneinander, ihre Ausgänge liegen also praktisch auf derselben Spannung. Was die beiden BMS anzeigen, unterscheidet sich nur um den Spannungsfall an Brücken und Steckern und um die Messabweichung der BMS: Auflösung 10 mV, dazu ein fester Versatz von bis zu einigen 10 mV je Gerät.
- **Der Strom folgt dem Widerstand:** Beide Packs teilen sich den Strom des Paares. Ein Pack mit zusätzlichem Widerstand im Pfad (schlechter Stecker, lockere Brücke) bekommt weniger, beim Laden wie beim Entladen.
- **So sieht ein echter Widerstand aus:** Der Pack hinter dem Widerstand bekommt weniger Strom **und** zeigt beim Laden die niedrigere Ausgangsspannung, beim Entladen die höhere. Der Abstand wächst mit dem Strom (doppelter Strom, doppelter Abstand) und kehrt sich beim Wechsel von Laden zu Entladen um.
- **So sieht ein Messversatz aus:** Der Abstand bleibt gleich, egal wie viel Strom fließt und in welche Richtung, und die Ströme teilen sich trotzdem gleichmäßig. Oder Spannung und Strom passen nicht zusammen: Der Pack mit der niedrigeren Spannung bekommt beim Laden **mehr** Strom statt weniger.
- **Größenordnung:** Ein Pack hat innen nur wenige mΩ (Zellen, MOSFETs, Shunt). 40 mV bei 10 A wären 4 mΩ zusätzlich, ungefähr noch einmal so viel wie der ganze Pack; der Strom würde sich dann deutlich ungleich teilen. Bei sauber geteiltem Strom kann ein solcher Abstand also kein Widerstand sein.
- **Ungleiche Ströme ohne Spannungsabstand:** Liegen die Ausgänge gleich, steckt der Unterschied in den Packs selbst: meist unterschiedliche Ladestände (der leerere nimmt beim Laden mehr, gibt beim Entladen weniger), sonst ein höherer Innenwiderstand (Diagnose → Im Pack).
- **Ladestand (SOC):** Das BMS zählt ihn mit, es misst ihn nicht. Zwischen etwa 20 % und 90 % ist die Zellspannung bei LiFePO4 fast flach, ein Abstand von 2–3 Punkten ist dort normal. Genau abgleichen kann sich der Wert erst, wenn ein Pack voll geladen wird.
- **Kleine Ströme:** Unter 20 A je Pack bestimmen Ausgleichsströme zwischen den Packs die Aufteilung mit, deshalb bewertet die App die Aufteilung erst darüber. Abstände von 3–10 mV liegen im Bereich der Auflösung und sagen nichts.

**Was tun**
- Abstand unter 50 mV, Ströme ähnlich → nichts.
- Abstand gleichbleibend, Ströme gleich → Messversatz, nichts tun. Zur Bestätigung mit dem Multimeter direkt an beiden Pack-Ausgängen messen: Zeigt es dasselbe, misst eines der BMS daneben.
- Abstand wächst mit dem Strom, ein Pack bekommt weniger → die Brücke im Pfad dieses Packs prüfen: beim ersten Pack des Paares (Hauptkabel +) die Minus-Brücke, beim zweiten (Hauptkabel −) die Plus-Brücke. Unter Last ist ein schlechter Stecker oft spürbar warm.

**Beispiele**
- Laden mit je etwa 10 A: Pack A zeigt 40 mV mehr als Pack B, B bekommt aber 10 A und A nur 9 A. Ein Widerstand vor B würde B weniger Strom geben, nicht mehr. Bleiben die 40 mV auch bei 30 A und beim Entladen gleich: Messversatz.
- Laden: 50 mV Abstand bei 10 A, 150 mV bei 30 A, beim Entladen umgekehrt. Der Pack mit der niedrigeren Spannung bekommt deutlich weniger Strom. Das sind rund 5 mΩ Übergangswiderstand in Brücke oder Stecker dieses Packs.
- Ladestände 42 % und 45 %, Ausgänge gleich: Beim Laden nimmt der leerere Pack etwas mehr Strom. Normal; die Werte gleichen sich beim Vollladen an.
