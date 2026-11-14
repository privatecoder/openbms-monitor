---
title: Anleitung: Parameter auf mehrere Packs schreiben
short: Eine Änderung einmal machen und auf weitere Packs übertragen: lesen, ein Pack bearbeiten, Packs wählen, bestätigen und die Ergebnisliste lesen.
related: topic.paramWrite, topic.params, topic.paramCheck
---
Geschrieben wird die Änderung, nicht die ganze Spalte: Jedes Pack behält seine übrigen Werte und Schalter. Parameter schreiben geht über den RS485-1/2-Bus oder die CAN-Buchse; was dafür nötig ist, steht in den verlinkten Themen.

1. Mit einer Anlage verbinden, nicht mit „Nur ansehen“: Damit lässt sich lesen, aber nicht schreiben. → [Verbindung](#topic.connection)
2. In Parameter „Aus den Packs lesen“. Erwartet: eine Spalte je Pack. Ziel kann nur ein Pack sein, das auf dieser Verbindung gelesen wurde. → [Parameter](#topic.params)
3. Ein Pack bearbeiten: im Spaltenkopf den Stift („Bearbeiten“), dann die Werte ändern. Erwartet: Geänderte Zellen sind gefärbt, die Leiste darüber listet die Änderungen; verletzt ein Wert eine Regel, nennt sie die Leiste und das Schreiben bleibt aus. → [Parameter und Seriennummern schreiben](#topic.paramWrite)
4. In der Leiste „Auch auf weitere Packs …“ und die Ziele ankreuzen oder „alle gelesenen“. Nicht gelesene Packs sind ausgegraut („erst lesen“). → [Parameter und Seriennummern schreiben](#topic.paramWrite)
5. „Auf Pack X und n weitere schreiben …“. Die Bestätigung listet die Änderungen und die Packs, bei denen das Schreiben die gelernte Vollkapazität auf P58 zurücksetzt (der SOC springt). „Wiederholungen je Pack“ wählen (0–5, vorbelegt 2), dann „n Packs schreiben“. → [Nennkapazität](#param.rated_capacity)
6. Zusehen: erst das bearbeitete Pack, dann die übrigen nach Adresse, jedes mit frischem Lesen, Schreiben und Rücklesen; ein neuer Versuch setzt auf frisch gelesenem Stand auf. „Abbrechen“ wirkt vor dem nächsten Pack oder Versuch. → [Parameter und Seriennummern schreiben](#topic.paramWrite)
7. Nur wenn die Serie anhält, weil ein Pack den Block abgelehnt hat und seine Schalter dabei aktiv wurden: erst „Gespeicherte Schalter wiederherstellen“, dann „Weiter mit den übrigen Packs“ oder „Abbrechen“. → [Parameter und Seriennummern schreiben](#topic.paramWrite)
8. Die Ergebnisliste lesen: Oben stehen die Zahlen (geschrieben, übersprungen nach N Versuchen, abgelehnt, ausgelassen …), darunter je Pack eine Zeile mit dem Ergebnis, dem Grund und der Zahl der Versuche. Erwartet: alle Ziele „geschrieben und rückgelesen“. Je nach Zeile: → [Parameter und Seriennummern schreiben](#topic.paramWrite)
   - **„übersprungen nach N Versuchen“ oder „Ergebnis ungeklärt“**
     1. Den genannten Grund klären, die Packs neu lesen und die Änderung für dieses Pack erneut schreiben.
   - **„abgelehnt“**
     1. Das Pack selbst hat abgelehnt; erneutes Senden ändert nichts. Werte und Firmware dieses Packs prüfen. → [Parameter gegen Datenblatt](#topic.paramCheck)
   - **„übersprungen: …“ ohne Versuch**
     1. Das Pack hatte die Werte schon, verletzt mit ihnen eine Regel oder hat eine offene Ablehnung; der Grund steht in der Zeile.
9. Zum Schluss erneut „Aus den Packs lesen“ und „Nur Abweichungen“ einschalten. Erwartet: Die geänderten Werte sind in allen Zielen gleich. → [Parameter](#topic.params)
