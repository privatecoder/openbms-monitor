---
title: Anleitung: Erste Einrichtung
short: Von der neuen Anlage bis zur bestätigten Bank: Anlage anlegen, Anschlüsse eintragen, verbinden und die Packs der Bank zuordnen.
related: topic.installations, topic.connection, topic.wiring, topic.buses
---
Alles, was die App über eine Bank speichert, gehört zu einer Anlage. Die Schritte führen einmal durch die Einrichtung; die Bedingungen stehen in den verlinkten Themen.

1. Eine Anlage anlegen: in Anlagen (oder in der Seitenleiste unter „Anlage“) „Neue Anlage …“, einen Namen eingeben und „Anlegen“. Erwartet: Die neue Anlage ist in der Seitenleiste gewählt. → [Anlagen](#topic.installations)
2. Ihre Anschlüsse eintragen: in Anlagen die Karte öffnen, bei „Anschlüsse“ „Ändern“, dann die CAN-Buchse (RS485) und, wenn geflasht werden soll, den RS485-1/2-Bus, jeweils Netzwerk (Gateway `Host:Port`) oder USB; „Speichern“. Ohne Eintrag ordnet die App den Anschluss beim ersten Verbinden zu. → [Die zwei RS485-Busse](#topic.buses)
3. Auf derselben Karte „Wechselrichter angeschlossen über“ wählen. Ohne die Angabe sendet die App für diese Anlage nichts. → [Wechselrichter-Anschluss](#topic.wiring)
4. Bei „Pack-Abfrage über RS485“ „suchen“ lassen oder Anzahl bzw. feste Adresse setzen; das gilt ab der nächsten Verbindung. → [Verbindung](#topic.connection)
5. Verbinden: In der Seitenleiste unter „Verbindung“ ist die Anlage gewählt; Bus wählen (die App füllt den gespeicherten Anschluss ein), dann „Verbinden“. Erwartet: „Verbunden“ und die gefundenen Packs. Antwortet nichts, sagt die App, was sie gehört hat. → [Verbindung](#topic.connection)
6. Die Bank bestätigen, je nach Zeile in der Seitenleiste: → [Anlagen](#topic.installations)
   - **„Diese Anlage kennt die Platinen … noch nicht“**
     1. Sind es die Packs dieser Bank, „Ja, übernehmen“. Erwartet: Die Aufzeichnung beginnt, in Anlagen stehen die Platinen je Adresse.
   - **Rote Zeile: „… gehört zur Anlage „B““**
     1. Hängt hier eine andere Bank, „Trennen“ oder „Mit „B“ verbinden“; nur für eine wirklich umgezogene Platine „Trotzdem … übernehmen“.
   - **„Pack(s) ohne Seriennummer“**
     1. Die Bank erkennt die App an der BMS-Seriennummer (String 2); die Batterie-Seriennummer (String 1) ersetzt sie nicht. Unter Parameter → Seriennummern je Pack eine eindeutige BMS-Seriennummer setzen, dann neu verbinden. Seriennummern schreiben geht über den RS485-1/2-Bus: Eine Verbindung an der CAN-Buchse allein genügt dafür nicht. → [Parameter und Seriennummern schreiben](#topic.paramWrite)
7. Danach nach Bedarf den Packs unter Zellen ihren Zelltyp zuweisen; damit vergleicht die App die Werte mit dem Datenblatt. → [Zelltypen](#topic.cellAssign)
