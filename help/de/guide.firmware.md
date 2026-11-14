---
title: Anleitung: Bank auf neue Firmware bringen
short: Schritt für Schritt vom Anschluss bis zum geprüften Update: welcher Fall zur Bank passt, in welcher Reihenfolge geflasht wird und was zu tun ist, wenn etwas stoppt.
related: topic.firmware, topic.maintenance, topic.patchGates, topic.wiring
---
Die Anleitung nennt Reihenfolge, Entscheidung und erwartetes Ergebnis; Bedingungen und Hintergründe stehen in den verlinkten Themen. Geflasht wird über den RS485-1/2-Bus.

1. Unten in der Seitenleiste die Wartungsfunktionen einschalten (Werkzeug-Symbol) und die Ansicht Firmware öffnen. Erwartet: die Tabs „RS485-1/2-Bus“ und „Platine flashen“; für die Bank gilt der Tab „RS485-1/2-Bus“. → [Firmware-Update](#topic.firmware)
2. In der Seitenleiste die Anlage der Bank wählen (nicht „Nur ansehen“). Sie braucht unter Anlagen → Anschlüsse einen Anschluss für den RS485-1/2-Bus; „Master anhalten“ (Fall C unten) geht über die CAN-Buchse, dafür braucht sie auch diesen Anschluss. Erwartet: Der Tab zeigt oben beide Anschlüsse und darunter die Statuszeile. → [Anlagen](#topic.installations)
3. Prüfen, dass bei der Anlage angegeben ist, wo der Wechselrichter angeschlossen ist; ohne die Angabe sendet die App nichts. Fragt der Wechselrichter den Bus ab, den das Update braucht, seine Kommunikation trennen und in der Seitenleiste „Wechselrichter-Kommunikation ist getrennt bzw. pausiert“ ankreuzen. → [Wechselrichter-Anschluss](#topic.wiring)
4. Den Bus ruhig machen. Nur den Fall ausführen, der zur Bank passt: → [Wartung am RS485-1/2-Bus](#topic.maintenance)
   - **A – einzelnes Pack**
     1. Nichts zu tun: Ein Pack ohne Slaves fragt nicht ab. Erwartet: Die Statuszeile zeigt „Bus ruhig“.
   - **B – Bank mit Original-Firmware auf dem Master**
     1. Am Master DIP 5–8 auf OFF stellen und die alte Stellung notieren. Erwartet: Die Statuszeile wird von selbst grün („Bus ruhig“).
   - **C – Bank mit Patch-Firmware auf dem Master (EMU1101-EXT ab Light 1.5.4 / Full 1.6.4)**
     1. „Master anhalten“ drücken. Erwartet: Der Master meldet 0 Slaves, die Statuszeile zeigt „Bus ruhig“.
     2. Lehnt der Master ab oder bietet die App „Master anhalten“ nicht an, nennt sie den Grund; dann den Bus wie in Fall B über DIP 5–8 ruhig machen und auch danach wie in Fall B weitermachen. → [Patch-Firmware: Freigabebits](#topic.patchGates)
5. Über der Packliste einmal das Image wählen („.ehex-Datei wählen“). Ein Image, das keine freigegebene Original-Firmware ist (auch EMU1101-EXT), braucht „Eigene Firmware zulassen“; bei Patch-Firmware zeigt die App vorher, was die Freigabebits danach bedeuten. → [Firmware-Update](#topic.firmware)
6. Nur in Fall C, gleich nach der Wahl des Images entscheiden: Hat das neue Image für den Master keine Patch-Funktionen (Original oder eigene Firmware ohne Wartungsfunktion), gilt beim Abschluss C′: Den Master jetzt weder wählen noch flashen, erst die Slaves; er kommt nach „Master freigeben“ dran. Die App zeigt das an der Zeile des Masters und hält dort „Prüfen & flashen“ gesperrt. → [Wartung am RS485-1/2-Bus](#topic.maintenance)
7. Die Packliste abwarten: Jedes Pack, das antwortet, steht mit Version und „Prüfen & flashen“ da; fehlt eins, „Neu suchen“. Meldet die App eine andere Bank am Anschluss, erst das klären. → [Wartung am RS485-1/2-Bus](#topic.maintenance)
8. Flashen. Nur eine der beiden Arten: → [Wartung am RS485-1/2-Bus](#topic.maintenance)
   - **Ein Pack**
     1. In seiner Zeile „Prüfen & flashen“: Die App prüft Bus, Anlage und Ziel und spielt danach direkt auf, ohne weiteren Dialog. Während des Updates weder Strom noch Kabel trennen.
   - **Mehrere Packs als Serie**
     1. Die Packs ankreuzen (oder „Alle“; bei C′ den Master wieder abwählen), dann „Ausgewählte flashen“, den angezeigten Bestätigungstext eintippen und „Serie starten“: `FLASH <n> PACKS` bei freigegebener Original-Firmware, `FLASH CUSTOM <n> PACKS` bei jedem anderen Image, auch EMU1101-EXT. Erwartet: erst die Slaves nach Adresse, der Master zuletzt; jedes Pack wird davor geprüft und danach verglichen.
9. Das Ergebnis lesen: Ein einzelnes Update zeigt „Update erfolgreich“ und den Vergleich vorher/nachher; eine Serie am Ende „Geflasht · gestoppt · nicht geflasht“ und je Pack eine Zeile. Erwartet: keine Abweichung. Stoppt etwas, weiter beim letzten Schritt. → [Firmware-Update](#topic.firmware)
10. Den Bus dem Master zurückgeben, nur im Fall von oben (bei C entweder C oder C′): → [Wartung am RS485-1/2-Bus](#topic.maintenance)
    - **A**
      1. Nichts zu tun.
    - **B**
      1. Am Master DIP 5–8 zurückstellen. Erwartet: Die Statuszeile zeigt wieder „Master fragt N Slaves ab“.
    - **C – der Master behält Patch-Funktionen (oder wurde nicht geflasht)**
      1. Unter „Fertig“ „Master freigeben“; hat die App keine Slave-Zahl behalten, fragt sie danach. Erwartet: „Marker beendet“; ob der Master wieder abfragt, zeigt die Statuszeile.
    - **C′ – statt C: das neue Master-Image hat keine Patch-Funktionen**
      1. Am Master DIP 5–8 auf OFF stellen (alte Stellung notieren).
      2. Unter „Fertig“ „DIP 5–8 am Master stehen auf OFF“ ankreuzen und „Master freigeben“, solange noch die Patch-Firmware läuft. Erwartet: „Marker beendet. Der Master arbeitet ohne Slaves“, Bus ruhig; erst dann wird „Prüfen & flashen“ am Master frei.
      3. Den Master flashen wie oben.
      4. Am Master DIP 5–8 zurückstellen. Erwartet: „Master fragt N Slaves ab“.
11. Nur wenn etwas stoppt, den passenden Fall:
    - **Serie gestoppt**
      1. Die Zeile des gestoppten Packs lesen: Sie nennt den Grund; nicht erreichte Packs stehen als „nicht geflasht“. Nichts blind wiederholen. → [Wartung am RS485-1/2-Bus](#topic.maintenance)
      2. Die Ursache klären, dann die übrigen Packs einzeln oder in einer neuen Serie flashen und danach den Bus zurückgeben wie oben.
    - **Serie offen, läuft aber nicht mehr**
      1. Etwa nachdem die Ansicht geschlossen wurde: „Offene Serie beenden“. Nur als letztes Mittel die App neu starten, und nur, während kein Update läuft. → [Wartung am RS485-1/2-Bus](#topic.maintenance)
    - **Pack hängt eventuell im Bootloader**
      1. Oben steht „Pack X an …: Update abgebrochen …“. Das Log behalten (Ordner `firmware-logs`), nichts blind wiederholen; solange der Hinweis steht, flasht die App kein anderes Pack. → [Firmware-Update](#topic.firmware)
      2. Antwortet die Anwendung des Packs wieder („Neu suchen“), dieses Pack zuerst allein flashen.
      3. Antwortet es nicht: nur diese eine Platine an den Anschluss hängen, im Tab „Platine flashen“ unter „Bootloader-Wiederherstellung“ „Bootloader prüfen“, dann „Wiederherstellen“. Der Bootloader kennt keine Adresse, deshalb wirklich nur diese Platine.
      4. Ist das Pack abgeklemmt oder repariert, den Hinweis mit „Ja, Pack X ist abgeklemmt oder repariert“ schließen. → [Wartung am RS485-1/2-Bus](#topic.maintenance)
