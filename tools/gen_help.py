"""Generate help/<lang>/<id>.md from the tables below (source of truth for v0.x help content)."""
import os

ROOT = os.path.join(os.path.dirname(__file__), "..", "help")

# id: (related, de(title, short, body), en(title, short, body))
TOPICS = {
"topic.connection": ("topic.buses", (
 "Verbindung", "So verbindet sich die App mit den Batterien: per USB-RS485-Adapter oder per RS485-zu-Ethernet-Gateway (TCP).",
 "- **Netzwerk (TCP):** Adresse des Gateways im Format `Host:Port`, z. B. `192.168.1.10:4196`. Die Baudrate stellst du am Gateway ein (CAN-Buchse 9600, RS485-1/2 19200).\n- **USB/Seriell:** Port des Adapters wählen; die Baudrate ergibt sich aus dem gewählten Bus.\n- Nach dem Verbinden sucht die App die Adressen 0–15 ab und fragt danach alle gefundenen Packs alle 2 s ab.\n- Nur ein Programm darf gleichzeitig auf dem Bus fragen. Läuft z. B. das Home-Assistant-Add-on am selben Gateway, kollidieren die Anfragen."),
 ("Connection", "How the app connects to the batteries: via a USB-RS485 adapter or an RS485-to-Ethernet gateway (TCP).",
 "- **Network (TCP):** gateway address as `host:port`, e.g. `192.168.1.10:4196`. The baud rate is configured on the gateway (CAN socket 9600, RS485-1/2 19200).\n- **USB/serial:** select the adapter's port; the baud rate follows the selected bus.\n- After connecting, the app scans addresses 0–15 and then polls all packs found every 2 s.\n- Only one program may poll the bus at a time. If e.g. the Home Assistant add-on uses the same gateway, requests collide.")),
"topic.buses": ("topic.connection, value.system", (
 "Die zwei RS485-Busse", "Die Packs haben zwei getrennte RS485-Busse. Welcher Bus angeschlossen ist, bestimmt, was die App lesen und ändern kann.",
 "| Bus | Baudrate | Was geht |\n|---|---|---|\n| **CAN-Buchse** (RS485-Adern Pin 1/2/3 bzw. 6/7/8) | 9600 | Werte und Meldungen **aller** Packs unter ihrer DIP-Adresse, Systemwerte des Masters (Modbus). Keine Parameter, keine Steuerbefehle. |\n| **RS485-1/2** | 19200 | voller Befehlssatz: Parameter lesen und schreiben, Steuerbefehle, Historie. Der **Master** (DIP 0 mit Slaves) antwortet hier **nie**, weil er selbst seine Slaves abfragt. |\n\nFür die Überwachung mehrerer Packs ist der Bus der CAN-Buchse ideal; zum Ändern von Einstellungen braucht man RS485-1/2."),
 ("The two RS485 buses", "The packs have two separate RS485 buses. Which one is connected decides what the app can read and change.",
 "| Bus | Baud | What works |\n|---|---|---|\n| **CAN socket** (RS485 wires pins 1/2/3 or 6/7/8) | 9600 | values and messages of **all** packs at their DIP address, system values of the master (Modbus). No parameters, no control commands. |\n| **RS485-1/2** | 19200 | full command set: read and write parameters, control commands, history. The **master** (DIP 0 with slaves) **never** answers here because it polls its slaves itself. |\n\nFor monitoring several packs the CAN socket bus is ideal; changing settings requires RS485-1/2.")),
"topic.alarms": ("status.mosfets", (
 "Meldungen", "Warnungen, Schutzzustände, Fehler und Hinweise, die das BMS gerade meldet.",
 "- **Warnung:** nur gemeldet. Haben alle Packs eine ladebezogene Warnung, senkt der Master die Ladegrenze auf 10 A gesamt.\n- **Schutz:** das BMS öffnet einen MOSFET (Laden oder Entladen gesperrt).\n- **Fehler:** Hardware- oder Kalibrierungsproblem.\n- **Hinweis:** Zustand, z. B. Heizung angefordert oder Warten auf Nachladung.\n\nJede Meldung hat ein eigenes Info-Icon mit Ursache und Folge."),
 ("Messages", "Warnings, protections, faults and notices the BMS currently reports.",
 "- **Warning:** reported only. If all packs have a charge-related warning, the master lowers the charge limit to 10 A in total.\n- **Protection:** the BMS opens a MOSFET (charging or discharging blocked).\n- **Fault:** hardware or calibration problem.\n- **Info:** a state, e.g. heating requested or waiting for recharge.\n\nEvery message has its own info icon with cause and effect.")),
"value.system": ("value.charge_limits, topic.buses", (
 "Systemwerte (Master)", "Werte, die der Master über alle Packs berechnet und genau so an den Wechselrichter meldet.",
 "Gelesen per Modbus vom Master (Adresse 0), nur am Bus der CAN-Buchse und ab Firmware 16.06.04. Enthalten sind Summenstrom, mittlere Spannung, System-SOC, Lade- und Entladegrenzen, Freigaben sowie höchste und niedrigste Zellspannung bzw. -temperatur mit Pack-Nummer."),
 ("System values (master)", "Values the master computes over all packs and reports exactly like this to the inverter.",
 "Read via Modbus from the master (address 0), only on the CAN socket bus and from firmware 16.06.04. Includes total current, average voltage, system SOC, charge and discharge limits, enable flags, and highest/lowest cell voltage and temperature with pack number.")),
"value.charge_limits": ("value.system", (
 "Lade- und Entladegrenzen", "CVL/CCL/DCL/DVL, die der Master an den Wechselrichter sendet.",
 "- **CVL** (Ladespannung) = Parameter 14 (Pack-Überspannungsschutz), fest.\n- **CCL** (Ladestrom) = Lade-Überstrom-Warnung (P50) ÷ 2 bzw. − 10 A, mal ladefähige Packs; 10 A gesamt, wenn alle Packs eine Lade-Warnung haben; 0 bei Zelle über OVP + 30 mV oder wenn kein Pack laden darf.\n- **DCL** (Entladestrom) = Entlade-Überstrom-Warnung (P52) − 10 A, mal entladefähige Packs.\n- **DVL** = Parameter 12."),
 ("Charge and discharge limits", "CVL/CCL/DCL/DVL the master sends to the inverter.",
 "- **CVL** (charge voltage) = parameter 14 (pack over-voltage protection), fixed.\n- **CCL** (charge current) = charge over-current warning (P50) ÷ 2 or − 10 A, times chargeable packs; 10 A in total when all packs have a charge warning; 0 when a cell exceeds OVP + 30 mV or no pack may charge.\n- **DCL** (discharge current) = discharge over-current warning (P52) − 10 A, times dischargeable packs.\n- **DVL** = parameter 12.")),
"value.soc": ("value.cycles", (
 "Ladestand (SOC)", "Vom BMS gezählter Ladestand in %. Er wird zusätzlich über feste Spannungsregeln korrigiert.",
 "- Das BMS zählt jede Sekunde den Strom (Coulomb-Zählung).\n- Beim Laden bleibt der SOC bei **98,2 %** stehen, bis eine Zelle die Zell-Hochspannungswarnung (P0) erreicht.\n- **100 %** setzt das BMS, wenn die Pack-Spannung 30 min lang ≥ Zellenzahl × P0 (3,45–3,50 V) liegt und mit weniger als 3 A geladen wird, oder bei Überspannungsschutz.\n- Weitere Sprünge auf 23/70/90/98 % bzw. 10/0 % bei passenden Spannungen."),
 ("State of charge (SOC)", "State of charge in % counted by the BMS. It is also corrected by fixed voltage rules.",
 "- The BMS integrates the current every second (coulomb counting).\n- While charging the SOC holds at **98.2 %** until a cell reaches the cell high-voltage warning (P0).\n- **100 %** is set when the pack voltage stays ≥ cells × P0 (3.45–3.50 V) for 30 min while charging with less than 3 A, or on over-voltage protection.\n- Further jumps to 23/70/90/98 % or 10/0 % at matching voltages.")),
"value.pack_voltage": ("value.cell_voltages", (
 "Spannung", "Pack-Spannung (Summe der Zellen) und Spannung am Ausgang (Port P+/P−).",
 "Liegt die Port-Spannung deutlich über der Pack-Spannung, erkennt das BMS ein Ladegerät bzw. eine Fremdspannung am Ausgang."),
 ("Voltage", "Pack voltage (sum of cells) and voltage at the output (port P+/P−).",
 "If the port voltage is clearly above the pack voltage, the BMS detects a charger or external voltage at the output.")),
"value.current": ("value.power", (
 "Strom", "Lade- (+) bzw. Entladestrom (−) des Packs.",
 "Im Leerlauf meldet das BMS im normalen Feld 0 A. Die App zeigt dann den genaueren Leerlaufstrom mit 1 mA Auflösung, den das BMS in einem eigentlich „reservierten\" Feld mitschickt. Als „Laden\" bzw. „Entladen\" gilt ein Pack ab +0,60 A bzw. −0,50 A."),
 ("Current", "Charge (+) or discharge (−) current of the pack.",
 "While idle the BMS reports 0 A in the regular field. The app then shows the more precise idle current with 1 mA resolution that the BMS sends in a nominally \"reserved\" field. A pack counts as charging/discharging from +0.60 A / −0.50 A.")),
"value.power": ("value.current", (
 "Leistung", "Strom × Pack-Spannung, von der App berechnet.", "Positiv beim Laden, negativ beim Entladen."),
 ("Power", "Current × pack voltage, calculated by the app.", "Positive while charging, negative while discharging.")),
"value.cycles": ("value.soc", (
 "Zyklen", "Zyklenzähler des BMS.", "Ein Zyklus zählt, sobald die Summe der SOC-Rückgänge beim Entladen Parameter 81 erreicht (z. B. 80 %). Es ist also keine volle Lade-/Entladung nötig."),
 ("Cycles", "Cycle counter of the BMS.", "A cycle counts once the sum of SOC decreases while discharging reaches parameter 81 (e.g. 80 %). A full charge/discharge is not required.")),
"value.energy": ("value.cycles", (
 "Energie", "Geladene und entladene Energie seit dem letzten Zurücksetzen, vom BMS gezählt.",
 "Das BMS summiert jede Sekunde Strom × Spannung, solange der Pack lädt bzw. entlädt (Schritte von 0,1 kWh). Die Zähler überstehen Neustarts, werden aber nur bei bestimmten Anlässen gespeichert (z. B. SOC-Änderung > 5 % im Standby). Ab 6 500 kWh meldet die Firmware fest 650,0 kWh."),
 ("Energy", "Charged and discharged energy since the last reset, counted by the BMS.",
 "The BMS sums current × voltage every second while the pack charges or discharges (0.1 kWh steps). The counters survive restarts but are only saved on certain occasions (e.g. SOC change > 5 % in standby). Beyond 6,500 kWh the firmware reports a fixed 650.0 kWh.")),
"value.cell_voltages": ("value.cell_delta, status.balancing", (
 "Zellspannungen", "Spannung jeder einzelnen Zelle in mV. Violett = deutlich über, blau = deutlich unter dem Median dieses Packs.",
 "Die Striche zeigen die Abweichung vom Median der Zellen dieses Packs (volle Höhe ±20 mV). Ein farbiger Strich unten bedeutet: Zelle wird balanciert."),
 ("Cell voltages", "Voltage of every cell in mV. Violet = clearly above, blue = clearly below this pack's median.",
 "The ticks show the deviation from the median of this pack's cells (full height ±20 mV). A coloured mark at the bottom means the cell is being balanced.")),
"value.cell_strip": ("value.cell_voltages, value.cell_delta", (
 "Zellabweichung", "Ein Strich pro Zelle: wie weit ihre Spannung vom Median aller Zellen aller Packs abweicht (der Wert steht im Spaltenkopf). Eine flache Linie heißt: gut ausbalanciert.",
 "- Der Median ist die mittlere Zellspannung: Die Hälfte aller Zellen liegt darüber, die Hälfte darunter. Anders als der Durchschnitt verschiebt er sich nicht durch eine einzelne Ausreißerzelle.\n- Nach oben (violett) = Zelle liegt über dem Median, nach unten (blau) = darunter.\n- Die volle Höhe entspricht ±20 mV; größere Abweichungen stoßen an den Rand.\n- Abweichungen unter 4 mV bleiben grau.\n- Ein farbiger Strich unten zeigt, dass die Zelle gerade balanciert wird.\n\nSo fällt ein Pack oder eine einzelne Zelle, die aus der Reihe tanzt, sofort auf, auch wenn alle absoluten Werte normal aussehen."),
 ("Cell deviation", "One tick per cell: how far its voltage is from the median of all cells of all packs (shown in the column header). A flat line means well balanced.",
 "- The median is the middle cell voltage: half of all cells are above it, half below. Unlike the average it is not pulled by a single outlier cell.\n- Up (violet) = cell above the median, down (blue) = below.\n- Full height equals ±20 mV; larger deviations hit the edge.\n- Deviations below 4 mV stay grey.\n- A coloured mark at the bottom shows the cell is being balanced.\n\nThis way a pack or a single cell that is out of line stands out immediately, even when all absolute values look normal.")),
"value.cell_delta": ("value.cell_voltages", (
 "Zelldifferenz", "Abstand zwischen höchster und niedrigster Zelle.",
 "Ab Parameter 60 (z. B. 0,5 V) meldet das BMS einen Zelldifferenz-Fehler. Balancing startet erst, wenn die Differenz Parameter 62 überschreitet und eine Zelle über Parameter 8 liegt."),
 ("Cell difference", "Difference between the highest and lowest cell.",
 "From parameter 60 (e.g. 0.5 V) the BMS reports a cell difference fault. Balancing only starts once the difference exceeds parameter 62 and a cell is above parameter 8.")),
"value.temperatures": ("topic.alarms", (
 "Temperaturen", "Vier Zellfühler, Umgebung (BMS-Platine) und Leistungsteil (MOSFET).",
 "Bei Packs mit höchstens 8 Zellen übernehmen Fühler 3 und 4 den Wert von Fühler 2. Die Lade-Temperaturgrenzen gelten nur beim Laden, die Entlade-Grenzen nur beim Entladen."),
 ("Temperatures", "Four cell sensors, ambient (BMS board) and power stage (MOSFET).",
 "On packs with 8 cells or fewer, sensors 3 and 4 copy sensor 2. Charge temperature limits apply only while charging, discharge limits only while discharging.")),
"status.mosfets": ("topic.alarms", (
 "Schaltzustand", "Welche Leistungsschalter des Packs gerade eingeschaltet sind.",
 "- **Entlade-/Lade-MOSFET:** aus, solange ein Schutz aktiv ist.\n- **Strombegrenzer:** Hardware-Stufe, über die bei Lade-Überstrom (bzw. optional ab 10 A) geladen wird.\n- **Heizung:** nur bei angeforderter Heizung und Laden bzw. erkanntem Ladegerät."),
 ("Switch state", "Which power switches of the pack are currently on.",
 "- **Discharge/charge MOSFET:** off while a protection is active.\n- **Current limiter:** hardware stage used for charging after charge over-current (or optionally from 10 A).\n- **Heater:** only when heating is requested and the pack is charging or a charger is detected.")),
"status.balancing": ("value.cell_delta", (
 "Balancing", "Passives Balancing: die Zelle wird über einen Widerstand leicht entladen.",
 "Nur oberhalb von Parameter 8 (z. B. 3,40 V) und wenn die Spreizung Parameter 62 überschreitet; nie beim Entladen. Mit gut gepaarten Zellen ist es daher selten aktiv."),
 ("Balancing", "Passive balancing: the cell is slightly discharged through a resistor.",
 "Only above parameter 8 (e.g. 3.40 V) and when the spread exceeds parameter 62; never while discharging. With well matched cells it is therefore rarely active.")),
}

# alarm id: (de title, de short, en title, en short)
ALARMS = {
"alarm.ev1.b0": ("Spannungsmessung fehlerhaft", "Die Zellspannungsmessung liefert unplausible Werte.", "Voltage sensing fault", "Cell voltage measurement returns implausible values."),
"alarm.ev1.b1": ("Temperaturfühler fehlerhaft", "Ein Fühler liegt außerhalb −45 … +100 °C oder die Spreizung ist ≥ 20 K. Die Heizung wird gesperrt.", "Temperature sensor fault", "A sensor is outside −45 … +100 °C or the spread is ≥ 20 K. Heating is disabled."),
"alarm.ev1.b2": ("Strommessung fehlerhaft", "Die Strommessung liefert unplausible Werte.", "Current sensing fault", "Current measurement returns implausible values."),
"alarm.ev1.b3": ("Taster-Fehler", "Der Taster war länger als 301 s gedrückt.", "Button fault", "The push button was pressed for more than 301 s."),
"alarm.ev1.b4": ("Zelldifferenz-Fehler", "Die Differenz zwischen höchster und niedrigster Zelle erreicht Parameter 60; aus unter Parameter 61.", "Cell difference fault", "The difference between highest and lowest cell reaches parameter 60; clears below parameter 61."),
"alarm.ev1.b5": ("Lade-MOSFET defekt", "Der Lade-Schalter reagiert nicht wie erwartet.", "Charge switch fault", "The charge switch does not respond as expected."),
"alarm.ev1.b6": ("Entlade-MOSFET defekt", "Der Entlade-Schalter reagiert nicht wie erwartet.", "Discharge switch fault", "The discharge switch does not respond as expected."),
"alarm.ev1.b7": ("Strombegrenzer defekt", "Die Strombegrenzer-Stufe reagiert nicht wie erwartet.", "Current limiter fault", "The current limiter stage does not respond as expected."),
"alarm.ev2.b0": ("Zellspannung hoch", "Eine Zelle hat die Zell-Hochspannungswarnung (P0) erreicht; Rückkehr unter P1.", "Cell voltage high", "A cell reached the cell high-voltage warning (P0); clears below P1."),
"alarm.ev2.b1": ("Zell-Überspannungsschutz", "Eine Zelle hat P4 erreicht: Laden gesperrt, SOC wird auf 100 % gesetzt. Endet unter P5 oder bei Entladen > 10 A für 10 s.", "Cell over-voltage protection", "A cell reached P4: charging blocked, SOC set to 100 %. Ends below P5 or when discharging > 10 A for 10 s."),
"alarm.ev2.b2": ("Zellspannung niedrig", "Eine Zelle hat die Zell-Unterspannungswarnung (P2) erreicht; Rückkehr über P3.", "Cell voltage low", "A cell reached the cell low-voltage warning (P2); clears above P3."),
"alarm.ev2.b3": ("Zell-Unterspannungsschutz", "Eine Zelle hat P6 erreicht: Entladen gesperrt, SOC wird auf 0 % gesetzt. Laden mit ≥ 3 A hebt den Schutz auf.", "Cell under-voltage protection", "A cell reached P6: discharging blocked, SOC set to 0 %. Charging with ≥ 3 A clears the protection."),
"alarm.ev2.b4": ("Pack-Spannung hoch", "Die Pack-Spannung hat P10 erreicht; Rückkehr unter P11.", "Pack voltage high", "The pack voltage reached P10; clears below P11."),
"alarm.ev2.b5": ("Pack-Überspannungsschutz", "Die Pack-Spannung hat P14 erreicht: Laden gesperrt. Kann per Funktionsschalter abgeschaltet sein.", "Pack over-voltage protection", "The pack voltage reached P14: charging blocked. May be disabled by a function switch."),
"alarm.ev2.b6": ("Pack-Spannung niedrig", "Die Pack-Spannung hat P12 erreicht; Rückkehr über P13.", "Pack voltage low", "The pack voltage reached P12; clears above P13."),
"alarm.ev2.b7": ("Pack-Unterspannungsschutz", "Die Pack-Spannung hat P16 erreicht: Entladen gesperrt, SOC wird auf 0 % gesetzt.", "Pack under-voltage protection", "The pack voltage reached P16: discharging blocked, SOC set to 0 %."),
"alarm.ev3.b0": ("Laden: Temperatur hoch", "Zelltemperatur beim Laden über P20.", "Charging: temperature high", "Cell temperature while charging above P20."),
"alarm.ev3.b1": ("Laden: Übertemperaturschutz", "Zelltemperatur beim Laden über P24: Laden gesperrt bis unter P25.", "Charging: over-temperature protection", "Cell temperature while charging above P24: charging blocked until below P25."),
"alarm.ev3.b2": ("Laden: Temperatur niedrig", "Zelltemperatur beim Laden unter P22.", "Charging: temperature low", "Cell temperature while charging below P22."),
"alarm.ev3.b3": ("Laden: Untertemperaturschutz", "Zelltemperatur beim Laden unter P26: Laden gesperrt bis über P27.", "Charging: under-temperature protection", "Cell temperature while charging below P26: charging blocked until above P27."),
"alarm.ev3.b4": ("Entladen: Temperatur hoch", "Zelltemperatur beim Entladen über P28.", "Discharging: temperature high", "Cell temperature while discharging above P28."),
"alarm.ev3.b5": ("Entladen: Übertemperaturschutz", "Zelltemperatur beim Entladen über P32: Entladen gesperrt bis unter P33.", "Discharging: over-temperature protection", "Cell temperature while discharging above P32: discharging blocked until below P33."),
"alarm.ev3.b6": ("Entladen: Temperatur niedrig", "Zelltemperatur beim Entladen unter P30.", "Discharging: temperature low", "Cell temperature while discharging below P30."),
"alarm.ev3.b7": ("Entladen: Untertemperaturschutz", "Zelltemperatur beim Entladen unter P34: Entladen gesperrt bis über P35.", "Discharging: under-temperature protection", "Cell temperature while discharging below P34: discharging blocked until above P35."),
"alarm.ev4.b0": ("Umgebung: Temperatur hoch", "Umgebungstemperatur (BMS-Platine) über P38.", "Ambient temperature high", "Ambient temperature (BMS board) above P38."),
"alarm.ev4.b1": ("Umgebung: Übertemperaturschutz", "Umgebungstemperatur über P42: Laden und Entladen gesperrt.", "Ambient over-temperature protection", "Ambient temperature above P42: charging and discharging blocked."),
"alarm.ev4.b2": ("Umgebung: Temperatur niedrig", "Umgebungstemperatur unter P40.", "Ambient temperature low", "Ambient temperature below P40."),
"alarm.ev4.b3": ("Umgebung: Untertemperaturschutz", "Umgebungstemperatur unter P44: Laden und Entladen gesperrt.", "Ambient under-temperature protection", "Ambient temperature below P44: charging and discharging blocked."),
"alarm.ev4.b4": ("Leistungsteil: Übertemperaturschutz", "MOSFET-Temperatur über P48: Laden und Entladen gesperrt.", "Power stage over-temperature protection", "MOSFET temperature above P48: charging and discharging blocked."),
"alarm.ev4.b5": ("Leistungsteil: Temperatur hoch", "MOSFET-Temperatur über P46.", "Power stage temperature high", "MOSFET temperature above P46."),
"alarm.ev4.b6": ("Heizung angefordert", "Niedrigste Zelltemperatur ≤ P36. Geheizt wird nur mit Ladegerät und wenn die Heizung freigeschaltet ist.", "Heating requested", "Lowest cell temperature ≤ P36. Heating only runs with a charger and when enabled."),
"alarm.ev4.b7": ("Schwerer Fehler", "Fühlerfehler, Zelle ≥ P4 + 50 mV, Zelle ≤ P6 − 200 mV oder Überstrom-Schutz. Bleibt bis zum Neustart.", "Severe fault", "Sensor fault, cell ≥ P4 + 50 mV, cell ≤ P6 − 200 mV or over-current protection. Stays until restart."),
"alarm.ev5.b0": ("Ladestrom hoch", "Ladestrom über P50. Löst ggf. den Strombegrenzer aus.", "Charge current high", "Charge current above P50. May activate the current limiter."),
"alarm.ev5.b1": ("Lade-Überstromschutz", "Ladestrom über P54 für P66 s: Laden gesperrt, automatisch wieder an nach P69 s.", "Charge over-current protection", "Charge current above P54 for P66 s: charging blocked, re-enabled after P69 s."),
"alarm.ev5.b2": ("Entladestrom hoch", "Entladestrom über P52.", "Discharge current high", "Discharge current above P52."),
"alarm.ev5.b3": ("Entlade-Überstromschutz", "Entladestrom über P55 für P67 s: Entladen gesperrt, automatisch wieder an nach P69 s.", "Discharge over-current protection", "Discharge current above P55 for P67 s: discharging blocked, re-enabled after P69 s."),
"alarm.ev5.b4": ("Kurzzeit-Überstromschutz", "Entladestrom über P56 für P68 ms.", "Transient over-current protection", "Discharge current above P56 for P68 ms."),
"alarm.ev5.b5": ("Kurzschlussschutz", "Kurzschluss am Ausgang erkannt.", "Short-circuit protection", "Short circuit at the output detected."),
"alarm.ev5.b6": ("Sperre nach Kurzzeit-Überstrom", "Nach P70 Ereignissen ohne 301 s Pause bleibt der Entlade-MOSFET aus, bis der Pack Laden erkennt.", "Transient over-current lock", "After P70 events without a 301 s pause the discharge MOSFET stays off until the pack detects charging."),
"alarm.ev5.b7": ("Sperre nach Kurzschluss", "Nach P70 Kurzschlüssen ohne 301 s Pause bleibt der Entlade-MOSFET aus, bis der Pack Laden erkennt.", "Short-circuit lock", "After P70 short circuits without a 301 s pause the discharge MOSFET stays off until the pack detects charging."),
"alarm.ev6.b0": ("Ladegerät-Überspannung", "Spannung am Ausgang ≥ P18 für > 5 s: Laden gesperrt.", "Charger over-voltage", "Output voltage ≥ P18 for > 5 s: charging blocked."),
"alarm.ev6.b1": ("Warten auf Nachladung", "Nach 100 % ist der Pack ladegesperrt, bis der SOC auf P80 fällt. Alle Packs gesperrt → Ladegrenze 0.", "Waiting for recharge", "After 100 % the pack is charge-blocked until SOC falls to P80. All packs blocked → charge limit 0."),
"alarm.ev6.b2": ("Restkapazität niedrig", "SOC unter P78.", "Remaining capacity low", "SOC below P78."),
"alarm.ev6.b3": ("Restkapazitätsschutz", "SOC unter P79: Entladen gesperrt, Laden bleibt erlaubt.", "Remaining capacity protection", "SOC below P79: discharging blocked, charging still allowed."),
"alarm.ev6.b4": ("Zelle zu tief zum Laden", "Eine Zelle liegt unter P9: Laden gesperrt (nur wenn die automatische Ladeaktivierung aus ist).", "Cell too low to charge", "A cell is below P9: charging blocked (only when automatic charge activation is off)."),
"alarm.ev6.b5": ("Verpolung", "Verpolung am Ausgang (in Firmware 16.06 nicht implementiert).", "Reverse polarity", "Reverse polarity at the output (not implemented in firmware 16.06)."),
"alarm.ev6.b6": ("Ausgangs-Verbindungsfehler", "Hardware-Eingang meldet ≥ 5 s einen Verbindungsfehler am Ausgang (nur wenn per Funktionsschalter aktiv): Laden und Entladen gesperrt.", "Output connection fault", "A hardware input reports a connection fault at the output for ≥ 5 s (only if enabled by function switch): charging and discharging blocked."),
"alarm.ev7.b4": ("Ladeaktivierung (automatisch)", "Bei Unterspannung, SOC 0 % oder SOC-Schutz öffnet das BMS alle P73 h (höchstens P74-mal) für P72 min den Ausgang, damit ein Ladegerät laden kann.", "Charge activation (automatic)", "On under-voltage, SOC 0 % or SOC protection the BMS opens the output for P72 min every P73 h (at most P74 times) so a charger can charge."),
"alarm.ev7.b5": ("Ladeaktivierung (manuell)", "Einmal nach dem Einschalten wird ein solcher Schutz für P72 min aufgehoben, wenn die Ausgangsspannung niedrig ist.", "Charge activation (manual)", "Once after power-on such a protection is lifted for P72 min if the output voltage is low."),
"alarm.ev8.b0": ("Speicherfehler", "Zugriff auf den Einstellungs- bzw. Historienspeicher fehlgeschlagen.", "Storage fault", "Access to the settings/history memory failed."),
"alarm.ev8.b1": ("Uhr-Fehler", "Die Echtzeituhr des BMS antwortet nicht.", "Clock fault", "The real-time clock of the BMS does not respond."),
"alarm.ev8.b2": ("Spannung nicht kalibriert", "Die Spannungsmessung wurde nicht kalibriert.", "Voltage not calibrated", "Voltage measurement has not been calibrated."),
"alarm.ev8.b3": ("Strom nicht kalibriert", "Die Strommessung wurde nicht kalibriert.", "Current not calibrated", "Current measurement has not been calibrated."),
"alarm.ev8.b4": ("Nullpunkt nicht kalibriert", "Der Strom-Nullpunkt wurde nicht kalibriert; die SOC-Korrektur über die Spannung ist dann aus.", "Zero point not calibrated", "The current zero point has not been calibrated; SOC voltage correction is then disabled."),
"alarm.ev8.b5": ("Uhrzeit ungültig", "Die Uhr des BMS war gestoppt (z. B. Pufferbatterie leer); Uhrzeit neu setzen.", "Clock time invalid", "The BMS clock was stopped (e.g. backup battery empty); set the time again."),
}

def write(lang, id, title, short, body, related):
    os.makedirs(os.path.join(ROOT, lang), exist_ok=True)
    with open(os.path.join(ROOT, lang, f"{id}.md"), "w", encoding="utf-8") as f:
        f.write(f"---\ntitle: {title}\nshort: {short}\nrelated: {related}\n---\n{body}\n")

for id, (related, de, en) in TOPICS.items():
    write("de", id, *de, related)
    write("en", id, *en, related)
for id, (dt, ds, et, es) in ALARMS.items():
    write("de", id, dt, ds, "", "topic.alarms")
    write("en", id, et, es, "", "topic.alarms")
print(len(TOPICS) + len(ALARMS), "entries per language")
