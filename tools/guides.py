"""Step-by-step guides (Anleitungen), imported by gen_help.py and written as help/<lang>/guide.*.md.

A guide says order, decision and expected result; conditions and background stay in the topics its steps link to (one
target per step, rendered as an internal help link). Port and firmware facts come from the catalog placeholders
(`{VIA:…}`, `{GATE:…}`, `{MIN}`), never written by hand. Where the reader must pick one of several alternatives, the step
holds `cases`: only the one that fits is done, then the guide goes on with the next common step.

Where a guide appears in the app is decided in one place, `app/src/guides.tsx`; `topics` here are the help pages whose
related links get the guide."""


def S(de, en, help=None, cases=None):
    """A step: one or two sentences per language, an optional help id it links to and optional alternative cases."""
    return {"de": de, "en": en, "help": help, "cases": cases or []}


def C(de, en, steps):
    """A case: its label per language and its own ordered steps (alternatives: do only the case that fits)."""
    return {"de": de, "en": en, "steps": steps}


GUIDES = {
"guide.firmware": {
 "topics": ["topic.firmware", "topic.maintenance"],
 "related": ["topic.firmware", "topic.maintenance", "topic.patchGates", "topic.wiring"],
 "de": ("Anleitung: Bank auf neue Firmware bringen",
        "Schritt für Schritt vom Anschluss bis zum geprüften Update: welcher Fall zur Bank passt, in welcher Reihenfolge geflasht wird und was zu tun ist, wenn etwas stoppt.",
        "Die Anleitung nennt Reihenfolge, Entscheidung und erwartetes Ergebnis; Bedingungen und Hintergründe stehen in den verlinkten Themen. Geflasht wird {VIA:flash}."),
 "en": ("Guide: bring a bank to new firmware",
        "Step by step from the port to the checked update: which case fits the bank, in which order to flash and what to do when something stops.",
        "The guide gives the order, the decision and the expected result; conditions and background are in the linked topics. Flashing goes {VIA:flash}."),
 "steps": [
  S("Unten in der Seitenleiste die Wartungsfunktionen einschalten (Werkzeug-Symbol) und die Ansicht Firmware öffnen. Erwartet: die Tabs „RS485-1/2-Bus“ und „Platine flashen“; für die Bank gilt der Tab „RS485-1/2-Bus“.",
    "At the bottom of the sidebar switch on the maintenance functions (wrench icon) and open the Firmware view. Expected: the tabs “RS485-1/2 bus” and “Flash a board”; for the bank the “RS485-1/2 bus” tab applies.",
    "topic.firmware"),
  S("In der Seitenleiste die Anlage der Bank wählen (nicht „Nur ansehen“). Sie braucht unter Anlagen → Anschlüsse einen Anschluss für den RS485-1/2-Bus; „Master anhalten“ (Fall C unten) geht {VIA:maint-start}, dafür braucht sie auch diesen Anschluss. Erwartet: Der Tab zeigt oben beide Anschlüsse und darunter die Statuszeile.",
    "In the sidebar choose the bank's installation (not “View only”). Under Installations → Ports it needs an endpoint for the RS485-1/2 bus; “Halt master” (case C below) works {VIA:maint-start}, so for that it needs this port too. Expected: the tab shows both ports on top and the status line below them.",
    "topic.installations"),
  S("Prüfen, dass bei der Anlage angegeben ist, wo der Wechselrichter angeschlossen ist; ohne die Angabe sendet die App nichts. Fragt der Wechselrichter den Bus ab, den das Update braucht, seine Kommunikation trennen und in der Seitenleiste „Wechselrichter-Kommunikation ist getrennt bzw. pausiert“ ankreuzen.",
    "Check that the installation says where the inverter is connected; without it the app sends nothing. If the inverter polls the bus the update needs, cut its communication and tick “Inverter communication is cut or paused” in the sidebar.",
    "topic.wiring"),
  S("Den Bus ruhig machen. Nur den Fall ausführen, der zur Bank passt:",
    "Make the bus quiet. Do only the case that fits the bank:",
    "topic.maintenance", [
    C("A – einzelnes Pack", "A – single pack", [
      S("Nichts zu tun: Ein Pack ohne Slaves fragt nicht ab. Erwartet: Die Statuszeile zeigt „Bus ruhig“.",
        "Nothing to do: a pack without slaves does not poll. Expected: the status line shows “Bus quiet”."),
    ]),
    C("B – Bank mit Original-Firmware auf dem Master", "B – bank with original firmware on the master", [
      S("Am Master DIP 5–8 auf OFF stellen und die alte Stellung notieren. Erwartet: Die Statuszeile wird von selbst grün („Bus ruhig“).",
        "Set DIP 5–8 of the master OFF and note the old positions. Expected: the status line turns green by itself (“Bus quiet”)."),
    ]),
    C("C – Bank mit Patch-Firmware auf dem Master (EMU1101-EXT ab {MIN})", "C – bank with patched firmware on the master (EMU1101-EXT from {MIN})", [
      S("„Master anhalten“ drücken. Erwartet: Der Master meldet 0 Slaves, die Statuszeile zeigt „Bus ruhig“.",
        "Press “Halt master”. Expected: the master reports 0 slaves, the status line shows “Bus quiet”."),
      S("Lehnt der Master ab oder bietet die App „Master anhalten“ nicht an, nennt sie den Grund; dann den Bus wie in Fall B über DIP 5–8 ruhig machen und auch danach wie in Fall B weitermachen.",
        "If the master refuses, or the app does not offer “Halt master”, it names the reason; then make the bus quiet with DIP 5–8 as in case B and go on as in case B afterwards too.",
        "topic.patchGates"),
    ]),
  ]),
  S("Über der Packliste einmal das Image wählen („.ehex-Datei wählen“). Ein Image, das keine freigegebene Original-Firmware ist (auch EMU1101-EXT), braucht „Eigene Firmware zulassen“; bei Patch-Firmware zeigt die App vorher, was die Freigabebits danach bedeuten.",
    "Above the pack list choose the image once (“Choose .ehex file”). An image that is no released original firmware (EMU1101-EXT too) needs “Allow custom firmware”; with patched firmware the app first shows what the release bits mean afterwards.",
    "topic.firmware"),
  S("Nur in Fall C, gleich nach der Wahl des Images entscheiden: Hat das neue Image für den Master keine Patch-Funktionen (Original oder eigene Firmware ohne Wartungsfunktion), gilt beim Abschluss C′: Den Master jetzt weder wählen noch flashen, erst die Slaves; er kommt nach „Master freigeben“ dran. Die App zeigt das an der Zeile des Masters und hält dort „Prüfen & flashen“ gesperrt.",
    "Only in case C, decide right after choosing the image: if the new image for the master has no patch functions (original or custom firmware without maintenance functions), C′ applies at the end: neither pick nor flash the master now, only the slaves; it follows after “Release master”. The app shows this at the master's row and keeps “Check & flash” locked there.",
    "topic.maintenance"),
  S("Die Packliste abwarten: Jedes Pack, das antwortet, steht mit Version und „Prüfen & flashen“ da; fehlt eins, „Neu suchen“. Meldet die App eine andere Bank am Anschluss, erst das klären.",
    "Wait for the pack list: every pack that answers is listed with its version and “Check & flash”; if one is missing, “Search again”. If the app reports another bank on the port, settle that first.",
    "topic.maintenance"),
  S("Flashen. Nur eine der beiden Arten:",
    "Flash. Only one of the two ways:",
    "topic.maintenance", [
    C("Ein Pack", "One pack", [
      S("In seiner Zeile „Prüfen & flashen“: Die App prüft Bus, Anlage und Ziel und spielt danach direkt auf, ohne weiteren Dialog. Während des Updates weder Strom noch Kabel trennen.",
        "In its row “Check & flash”: the app checks bus, installation and target and then writes the firmware right away, without a further dialog. Do not disconnect power or cable during the update."),
    ]),
    C("Mehrere Packs als Serie", "Several packs as a series", [
      S("Die Packs ankreuzen (oder „Alle“; bei C′ den Master wieder abwählen), dann „Ausgewählte flashen“, den angezeigten Bestätigungstext eintippen und „Serie starten“: `FLASH <n> PACKS` bei freigegebener Original-Firmware, `FLASH CUSTOM <n> PACKS` bei jedem anderen Image, auch EMU1101-EXT. Erwartet: erst die Slaves nach Adresse, der Master zuletzt; jedes Pack wird davor geprüft und danach verglichen.",
        "Tick the packs (or “All”; with C′ untick the master again), then “Flash selected”, type the confirmation shown and “Start series”: `FLASH <n> PACKS` with released original firmware, `FLASH CUSTOM <n> PACKS` with any other image, EMU1101-EXT too. Expected: the slaves by address first, the master last; every pack is checked before and compared after."),
    ]),
  ]),
  S("Das Ergebnis lesen: Ein einzelnes Update zeigt „Update erfolgreich“ und den Vergleich vorher/nachher; eine Serie am Ende „Geflasht · gestoppt · nicht geflasht“ und je Pack eine Zeile. Erwartet: keine Abweichung. Stoppt etwas, weiter beim letzten Schritt.",
    "Read the result: a single update shows “Update complete” and the before/after comparison; a series at its end “Flashed · stopped · not flashed” and one line per pack. Expected: no difference. If something stops, go on with the last step.",
    "topic.firmware"),
  S("Den Bus dem Master zurückgeben, nur im Fall von oben (bei C entweder C oder C′):",
    "Give the bus back to the master, only in the case from above (with C either C or C′):",
    "topic.maintenance", [
    C("A", "A", [
      S("Nichts zu tun.", "Nothing to do."),
    ]),
    C("B", "B", [
      S("Am Master DIP 5–8 zurückstellen. Erwartet: Die Statuszeile zeigt wieder „Master fragt N Slaves ab“.",
        "Set DIP 5–8 of the master back. Expected: the status line shows “Master polls N slaves” again."),
    ]),
    C("C – der Master behält Patch-Funktionen (oder wurde nicht geflasht)", "C – the master keeps patch functions (or was not flashed)", [
      S("Unter „Fertig“ „Master freigeben“; hat die App keine Slave-Zahl behalten, fragt sie danach. Erwartet: „Marker beendet“; ob der Master wieder abfragt, zeigt die Statuszeile.",
        "Under “Done” press “Release master”; if the app kept no slave count, it asks for it. Expected: “Marker ended”; whether the master polls again is shown by the status line."),
    ]),
    C("C′ – statt C: das neue Master-Image hat keine Patch-Funktionen", "C′ – instead of C: the new master image has no patch functions", [
      S("Am Master DIP 5–8 auf OFF stellen (alte Stellung notieren).", "Set DIP 5–8 of the master OFF (note the old positions)."),
      S("Unter „Fertig“ „DIP 5–8 am Master stehen auf OFF“ ankreuzen und „Master freigeben“, solange noch die Patch-Firmware läuft. Erwartet: „Marker beendet. Der Master arbeitet ohne Slaves“, Bus ruhig; erst dann wird „Prüfen & flashen“ am Master frei.",
        "Under “Done” tick “DIP 5–8 of the master are OFF” and press “Release master” while the patched firmware still runs. Expected: “Marker ended. The master works without slaves”, bus quiet; only then “Check & flash” at the master becomes free."),
      S("Den Master flashen wie oben.", "Flash the master as above."),
      S("Am Master DIP 5–8 zurückstellen. Erwartet: „Master fragt N Slaves ab“.", "Set DIP 5–8 of the master back. Expected: “Master polls N slaves”."),
    ]),
  ]),
  S("Nur wenn etwas stoppt, den passenden Fall:",
    "Only if something stops, the case that fits:",
    None, [
    C("Serie gestoppt", "Series stopped", [
      S("Die Zeile des gestoppten Packs lesen: Sie nennt den Grund; nicht erreichte Packs stehen als „nicht geflasht“. Nichts blind wiederholen.",
        "Read the stopped pack's line: it names the reason; packs not reached read “not flashed”. Repeat nothing blindly.",
        "topic.maintenance"),
      S("Die Ursache klären, dann die übrigen Packs einzeln oder in einer neuen Serie flashen und danach den Bus zurückgeben wie oben.",
        "Find the cause, then flash the remaining packs on their own or in a new series, and give the bus back as above afterwards."),
    ]),
    C("Serie offen, läuft aber nicht mehr", "Series open but no longer running", [
      S("Etwa nachdem die Ansicht geschlossen wurde: „Offene Serie beenden“. Nur als letztes Mittel die App neu starten, und nur, während kein Update läuft.",
        "For example after its view was closed: “End open series”. Only as a last resort restart the app, and only while no update runs.",
        "topic.maintenance"),
    ]),
    C("Pack hängt eventuell im Bootloader", "Pack may hang in its bootloader", [
      S("Oben steht „Pack X an …: Update abgebrochen …“. Das Log behalten (Ordner `firmware-logs`), nichts blind wiederholen; solange der Hinweis steht, flasht die App kein anderes Pack.",
        "On top it says “Pack X on …: update interrupted …”. Keep the log (folder `firmware-logs`), repeat nothing blindly; while the note stands the app flashes no other pack.",
        "topic.firmware"),
      S("Antwortet die Anwendung des Packs wieder („Neu suchen“), dieses Pack zuerst allein flashen.",
        "If the pack's application answers again (“Search again”), flash this pack first on its own."),
      S("Antwortet es nicht: nur diese eine Platine an den Anschluss hängen, im Tab „Platine flashen“ unter „Bootloader-Wiederherstellung“ „Bootloader prüfen“, dann „Wiederherstellen“. Der Bootloader kennt keine Adresse, deshalb wirklich nur diese Platine.",
        "If it does not: connect only this one board to the port, in the “Flash a board” tab under “Bootloader recovery” press “Check bootloader”, then “Recover”. The bootloader knows no address, so really only this board."),
      S("Ist das Pack abgeklemmt oder repariert, den Hinweis mit „Ja, Pack X ist abgeklemmt oder repariert“ schließen.",
        "If the pack is disconnected or repaired, close the note with “Yes, pack X is disconnected or repaired”.",
        "topic.maintenance"),
    ]),
  ]),
 ],
},
"guide.paramMulti": {
 "topics": ["topic.paramWrite", "topic.params"],
 "related": ["topic.paramWrite", "topic.params", "topic.paramCheck"],
 "de": ("Anleitung: Parameter auf mehrere Packs schreiben",
        "Eine Änderung einmal machen und auf weitere Packs übertragen: lesen, ein Pack bearbeiten, Packs wählen, bestätigen und die Ergebnisliste lesen.",
        "Geschrieben wird die Änderung, nicht die ganze Spalte: Jedes Pack behält seine übrigen Werte und Schalter. Parameter schreiben geht {VIA:write-parameters}; was dafür nötig ist, steht in den verlinkten Themen."),
 "en": ("Guide: write parameters to several packs",
        "Make a change once and carry it to further packs: read, edit one pack, pick packs, confirm and read the result list.",
        "What is written is the change, not the whole column: every pack keeps its other values and switches. Writing parameters goes {VIA:write-parameters}; what it needs is in the linked topics."),
 "steps": [
  S("Mit einer Anlage verbinden, nicht mit „Nur ansehen“: Damit lässt sich lesen, aber nicht schreiben.",
    "Connect with an installation, not with “View only”: that reads but does not write.",
    "topic.connection"),
  S("In Parameter „Aus den Packs lesen“. Erwartet: eine Spalte je Pack. Ziel kann nur ein Pack sein, das auf dieser Verbindung gelesen wurde.",
    "In Parameters press “Read from packs”. Expected: one column per pack. Only a pack read on this connection can be a target.",
    "topic.params"),
  S("Ein Pack bearbeiten: im Spaltenkopf den Stift („Bearbeiten“), dann die Werte ändern. Erwartet: Geänderte Zellen sind gefärbt, die Leiste darüber listet die Änderungen; verletzt ein Wert eine Regel, nennt sie die Leiste und das Schreiben bleibt aus.",
    "Edit one pack: the pencil in its column header (“Edit”), then change the values. Expected: changed cells are coloured, the bar above lists the changes; if a value breaks a rule, the bar names it and writing stays off.",
    "topic.paramWrite"),
  S("In der Leiste „Auch auf weitere Packs …“ und die Ziele ankreuzen oder „alle gelesenen“. Nicht gelesene Packs sind ausgegraut („erst lesen“).",
    "In the bar press “Also to other packs …” and tick the targets, or “all read”. Packs not read are greyed out (“read first”).",
    "topic.paramWrite"),
  S("„Auf Pack X und n weitere schreiben …“. Die Bestätigung listet die Änderungen und die Packs, bei denen das Schreiben die gelernte Vollkapazität auf P58 zurücksetzt (der SOC springt). „Wiederholungen je Pack“ wählen (0–5, vorbelegt 2), dann „n Packs schreiben“.",
    "“Write to pack X and n more …”. The confirmation lists the changes and the packs for which the write resets the learned full capacity to P58 (the SOC jumps). Choose “Retries per pack” (0–5, preset 2), then “Write n packs”.",
    "param.rated_capacity"),
  S("Zusehen: erst das bearbeitete Pack, dann die übrigen nach Adresse, jedes mit frischem Lesen, Schreiben und Rücklesen; ein neuer Versuch setzt auf frisch gelesenem Stand auf. „Abbrechen“ wirkt vor dem nächsten Pack oder Versuch.",
    "Watch: the edited pack first, then the others by address, each with a fresh read, the write and the read-back; a new attempt builds on a freshly read state. “Cancel” acts before the next pack or attempt.",
    "topic.paramWrite"),
  S("Nur wenn die Serie anhält, weil ein Pack den Block abgelehnt hat und seine Schalter dabei aktiv wurden: erst „Gespeicherte Schalter wiederherstellen“, dann „Weiter mit den übrigen Packs“ oder „Abbrechen“.",
    "Only if the series halts because a pack refused the block and its switches became active doing so: first “Restore stored switches”, then “Go on with the other packs” or “Cancel”.",
    "topic.paramWrite"),
  S("Die Ergebnisliste lesen: Oben stehen die Zahlen (geschrieben, übersprungen nach N Versuchen, abgelehnt, ausgelassen …), darunter je Pack eine Zeile mit dem Ergebnis, dem Grund und der Zahl der Versuche. Erwartet: alle Ziele „geschrieben und rückgelesen“. Je nach Zeile:",
    "Read the result list: on top the counts (written, skipped after N attempts, refused, skipped …), below one line per pack with the outcome, the reason and the number of attempts. Expected: every target “written and read back”. Depending on the line:",
    "topic.paramWrite", [
    C("„übersprungen nach N Versuchen“ oder „Ergebnis ungeklärt“", "“skipped after N attempts” or “result unclear”", [
      S("Den genannten Grund klären, die Packs neu lesen und die Änderung für dieses Pack erneut schreiben.",
        "Settle the reason named, read the packs again and write the change to this pack once more."),
    ]),
    C("„abgelehnt“", "“refused”", [
      S("Das Pack selbst hat abgelehnt; erneutes Senden ändert nichts. Werte und Firmware dieses Packs prüfen.",
        "The pack itself refused; sending again changes nothing. Check this pack's values and firmware.",
        "topic.paramCheck"),
    ]),
    C("„übersprungen: …“ ohne Versuch", "“skipped: …” without an attempt", [
      S("Das Pack hatte die Werte schon, verletzt mit ihnen eine Regel oder hat eine offene Ablehnung; der Grund steht in der Zeile.",
        "The pack already had the values, breaks a rule with them or has a pending refusal; the reason is in the line."),
    ]),
  ]),
  S("Zum Schluss erneut „Aus den Packs lesen“ und „Nur Abweichungen“ einschalten. Erwartet: Die geänderten Werte sind in allen Zielen gleich.",
    "Finally press “Read from packs” again and switch on “Only differences”. Expected: the changed values are the same in every target.",
    "topic.params"),
 ],
},
"guide.setup": {
 "topics": ["topic.installations", "topic.connection"],
 "related": ["topic.installations", "topic.connection", "topic.wiring", "topic.buses"],
 "de": ("Anleitung: Erste Einrichtung",
        "Von der neuen Anlage bis zur bestätigten Bank: Anlage anlegen, Anschlüsse eintragen, verbinden und die Packs der Bank zuordnen.",
        "Alles, was die App über eine Bank speichert, gehört zu einer Anlage. Die Schritte führen einmal durch die Einrichtung; die Bedingungen stehen in den verlinkten Themen."),
 "en": ("Guide: first setup",
        "From a new installation to the confirmed bank: create the installation, enter its ports, connect and assign the bank's packs.",
        "Everything the app stores about a bank belongs to an installation. The steps lead once through the setup; the conditions are in the linked topics."),
 "steps": [
  S("Eine Anlage anlegen: in Anlagen (oder in der Seitenleiste unter „Anlage“) „Neue Anlage …“, einen Namen eingeben und „Anlegen“. Erwartet: Die neue Anlage ist in der Seitenleiste gewählt.",
    "Create an installation: in Installations (or in the sidebar under “Installation”) “New installation …”, enter a name and “Create”. Expected: the new installation is chosen in the sidebar.",
    "topic.installations"),
  S("Ihre Anschlüsse eintragen: in Anlagen die Karte öffnen, bei „Anschlüsse“ „Ändern“, dann die CAN-Buchse (RS485) und, wenn geflasht werden soll, den RS485-1/2-Bus, jeweils Netzwerk (Gateway `Host:Port`) oder USB; „Speichern“. Ohne Eintrag ordnet die App den Anschluss beim ersten Verbinden zu.",
    "Enter its ports: in Installations open the card, at “Ports” press “Change”, then the CAN socket (RS485) and, if packs are to be flashed, the RS485-1/2 bus, each over the network (gateway `host:port`) or USB; “Save”. Without an entry the app links the port at the first connection.",
    "topic.buses"),
  S("Auf derselben Karte „Wechselrichter angeschlossen über“ wählen. Ohne die Angabe sendet die App für diese Anlage nichts.",
    "On the same card choose “Inverter connected via”. Without it the app sends nothing for this installation.",
    "topic.wiring"),
  S("Bei „Pack-Abfrage über RS485“ „suchen“ lassen oder Anzahl bzw. feste Adresse setzen; das gilt ab der nächsten Verbindung.",
    "At “Packs polled over RS485” keep “search” or set a count or a fixed address; it applies from the next connection.",
    "topic.connection"),
  S("Verbinden: In der Seitenleiste unter „Verbindung“ ist die Anlage gewählt; Bus wählen (die App füllt den gespeicherten Anschluss ein), dann „Verbinden“. Erwartet: „Verbunden“ und die gefundenen Packs. Antwortet nichts, sagt die App, was sie gehört hat.",
    "Connect: in the sidebar under “Connection” the installation is chosen; choose the bus (the app fills in the stored port), then “Connect”. Expected: “Connected” and the packs found. If nothing answers, the app says what it heard.",
    "topic.connection"),
  S("Die Bank bestätigen, je nach Zeile in der Seitenleiste:",
    "Confirm the bank, depending on the line in the sidebar:",
    "topic.installations", [
    C("„Diese Anlage kennt die Platinen … noch nicht“", "“This installation does not know … the boards … yet”", [
      S("Sind es die Packs dieser Bank, „Ja, übernehmen“. Erwartet: Die Aufzeichnung beginnt, in Anlagen stehen die Platinen je Adresse.",
        "If they are this bank's packs, “Yes, take over”. Expected: recording starts, Installations lists the boards per address."),
    ]),
    C("Rote Zeile: „… gehört zur Anlage „B““", "Red line: “… belongs to installation “B””", [
      S("Hängt hier eine andere Bank, „Trennen“ oder „Mit „B“ verbinden“; nur für eine wirklich umgezogene Platine „Trotzdem … übernehmen“.",
        "If another bank is connected here, “Disconnect” or “Connect with “B””; only for a board that really moved “Take over into … anyway”."),
    ]),
    C("„Pack(s) ohne Seriennummer“", "“Pack(s) without a serial number”", [
      S("Die Bank erkennt die App an der BMS-Seriennummer (String 2); die Batterie-Seriennummer (String 1) ersetzt sie nicht. Unter Parameter → Seriennummern je Pack eine eindeutige BMS-Seriennummer setzen, dann neu verbinden. Seriennummern schreiben geht {VIA:write-serials}: Eine Verbindung an der CAN-Buchse allein genügt dafür nicht.",
        "The app recognises the bank by the BMS serial number (String 2); the battery serial number (String 1) does not replace it. Under Parameters → Serial numbers set a unique BMS serial number per pack, then connect again. Writing serial numbers goes {VIA:write-serials}: a connection at the CAN socket alone is not enough for it.",
        "topic.paramWrite"),
    ]),
  ]),
  S("Danach nach Bedarf den Packs unter Zellen ihren Zelltyp zuweisen; damit vergleicht die App die Werte mit dem Datenblatt.",
    "Then, as needed, assign the packs their cell type under Cells; with it the app compares the values with the datasheet.",
    "topic.cellAssign"),
 ],
},
}
