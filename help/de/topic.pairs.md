---
title: Paare
short: Zwei Packs mit diagonalem Anschluss: Hauptkabel + am ersten Pack, Hauptkabel − am zweiten, dazwischen zwei kurze Brücken.
related: topic.pairRead, topic.diagnosis
---
- Jeder Pack hat eine Brücke in seinem Pfad: der erste die Minus-Brücke, der zweite die Plus-Brücke. Die Hauptkabel teilen sich beide.
- **Summe eines Paares deutlich unter den anderen Paaren** (unter 85 % des Medians) → die gemeinsamen Hauptkabel des Paares und ihre Anschlüsse prüfen. Entfällt, wenn der stärkere Pack normal viel bekommt; dann erklärt der schwache Pack allein die kleine Summe.
- **Ein Pack bekommt deutlich weniger als sein Partner** (unter 70 %) → die Brücke in seinem Pfad, ihre Stecker und den Pack selbst prüfen.
- Die Prüfung läuft nur unter Last (Summe ab 3 A).
- **Bewertung:** über den mittleren Strom der letzten 60 s, erst ab 20 A je Pack (Median aller Packs). Darunter bestimmen Ausgleichsströme die Aufteilung: Packs mit unterschiedlichem Ladestand gleichen sich über die Brücken aus, gemessen wurden bis ~5 A bei ruhender Anlage. Bei 10 A je Pack wirkte dadurch sogar der falsche Pack schwach. Bewertet wird außerdem erst, wenn alle Packs einmal geantwortet haben. Ein Hinweis erscheint unter 70 % bzw. 85 % und verschwindet erst über 75 % bzw. 90 %, damit er bei Werten nahe der Grenze nicht springt.
- **Ausgleich:** Liegen die Ladestände eines Paares 3 Prozentpunkte oder mehr auseinander, gibt beim Entladen der leerere Pack weniger ab und nimmt beim Laden der vollere weniger auf. Dann steht „Ladestände gleichen sich an“ statt einer Warnung.
- **Ausgänge auseinander:** Beide Packs eines Paares hängen an denselben Brücken; ihre Ausgangsspannungen unterscheiden sich nur um den Spannungsfall an Brücken und Steckern (plus einige 10 mV Messabweichung). Ab 100 mV (Mittel über 60 s) wird das rot: Übergangswiderstand in Brücke oder Stecker. Gesunde Paare liegen unter 50 mV; der defekte Stecker am 2026-10-10 zeigte 170–390 mV. Gilt unabhängig von der Last.
- **Gleichbleibend oder wachsend:** Die App merkt sich den Abstand über bis zu 6 Stunden. Hat sich der Strom dabei um mindestens 20 A je Pack verändert, legt sie eine Gerade durch die Werte. Bleibt der Abstand über den ganzen Strombereich innerhalb von 20 mV gleich, misst ein BMS einfach etwas anders: **Messversatz, kein Widerstand**, auch nicht rot. Wächst er mit dem Strom, steht der Widerstand dabei (≈ mΩ). Beispiel 2026-10-10, Paar 04/05: 40–47 mV von −24 A bis +35 A, die Ströme sauber geteilt, also Messversatz.

Paare legst du unter Übersicht → Gruppen bearbeiten fest; „Benachbarte Packs zu Paaren zusammenfassen“ bildet 00/01, 02/03 usw.
