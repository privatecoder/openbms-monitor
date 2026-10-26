"""Help for the BMS parameters (0x47) and function switches, imported by gen_help.py.

Effects as in the stock firmware 16.06.12/16.06.13. id: (de title, de short, en title, en short)."""

PARAMS = {
# cell voltage
"param.cell_high_voltage_alarm": ("Zell-Hochspannungs-Warnung", "Warnung, wenn eine Zelle diesen Wert erreicht. Haben alle Packs eine Lade-Warnung, senkt der Master die Ladegrenze auf 10 A gesamt. Außerdem Grundlage der 100-%-Regel des SOC (Zellen × diesem Wert, begrenzt auf 3,45–3,50 V je Zelle).",
 "Cell high-voltage warning", "Warning when a cell reaches this value. If all packs have a charge warning, the master lowers the charge limit to 10 A in total. Also the basis of the SOC's 100 % rule (cells × this value, limited to 3.45–3.50 V per cell)."),
"param.cell_high_voltage_recovery": ("Zell-Hochspannungs-Warnung aus", "Die Warnung endet, wenn die höchste Zelle wieder unter diesen Wert fällt.",
 "Cell high-voltage warning off", "The warning ends when the highest cell falls below this value again."),
"param.cell_low_voltage_alarm": ("Zell-Unterspannungs-Warnung", "Warnung, wenn eine Zelle diesen Wert erreicht. Sie stellt auch den SOC nach (siehe unten).",
 "Cell low-voltage warning", "Warning when a cell reaches this value. It also corrects the SOC (see below)."),
"param.cell_low_voltage_recovery": ("Zell-Unterspannungs-Warnung aus", "Die Warnung endet, wenn die niedrigste Zelle wieder über diesen Wert steigt.",
 "Cell low-voltage warning off", "The warning ends when the lowest cell rises above this value again."),
"param.cell_overvoltage_protection": ("Zell-Überspannungsschutz", "Erreicht eine Zelle diesen Wert, öffnet der Lade-MOSFET und der SOC springt auf 100 %. Liegt eine Zelle 30 mV darüber, meldet der Master Ladegrenze 0.",
 "Cell over-voltage protection", "When a cell reaches this value, the charge MOSFET opens and the SOC jumps to 100 %. With a cell 30 mV above it, the master reports a charge limit of 0."),
"param.cell_overvoltage_recovery": ("Zell-Überspannungsschutz aus", "Laden ist wieder erlaubt, wenn alle Zellen unter diesem Wert liegen. Entladen mit mehr als 10 A für etwa 10 s hebt den Schutz sofort auf.",
 "Cell over-voltage protection off", "Charging is allowed again when all cells are below this value. Discharging with more than 10 A for about 10 s releases the protection at once."),
"param.cell_undervoltage_protection": ("Zell-Unterspannungsschutz", "Erreicht eine Zelle diesen Wert, öffnet der Entlade-MOSFET und der SOC springt auf 0 %.",
 "Cell under-voltage protection", "When a cell reaches this value, the discharge MOSFET opens and the SOC jumps to 0 %."),
"param.cell_undervoltage_recovery": ("Zell-Unterspannungsschutz aus", "Entladen ist wieder erlaubt, wenn alle Zellen über diesem Wert liegen.",
 "Cell under-voltage protection off", "Discharging is allowed again when all cells are above this value."),
"param.balancing_start_voltage": ("Balancing ab Zellspannung", "Eine Zelle wird nur balanciert, wenn sie mindestens diese Spannung hat. Mit 3,40 V balanciert das BMS praktisch nur am Ende einer Ladung.",
 "Balancing from cell voltage", "A cell is only balanced when it has at least this voltage. With 3.40 V the BMS practically balances only at the end of a charge."),
"param.cell_low_voltage_charging_forbidden": ("Laden verboten unter (Zelle)", "Liegt eine Zelle darunter, ist Laden gesperrt. Wirkt nur, wenn die automatische Ladeaktivierung (Funktionsschalter 6, Bit 4) aus ist.",
 "Charging forbidden below (cell)", "With a cell below this value, charging is blocked. Only effective while automatic charge activation (function switch 6, bit 4) is off."),
# pack voltage
"param.pack_high_voltage_alarm": ("Pack-Hochspannungs-Warnung", "Warnung ab dieser Pack-Spannung. Haben alle Packs eine Lade-Warnung, senkt der Master die Ladegrenze auf 10 A gesamt.",
 "Pack high-voltage warning", "Warning from this pack voltage. If all packs have a charge warning, the master lowers the charge limit to 10 A in total."),
"param.pack_high_voltage_recovery": ("Pack-Hochspannungs-Warnung aus", "Die Warnung endet unter dieser Pack-Spannung.",
 "Pack high-voltage warning off", "The warning ends below this pack voltage."),
"param.pack_low_voltage_alarm": ("Pack-Unterspannungs-Warnung", "Warnung ab dieser Pack-Spannung. Der Master meldet diesen Wert außerdem als Entladeschlussspannung (DVL) an den Wechselrichter.",
 "Pack low-voltage warning", "Warning from this pack voltage. The master also reports this value to the inverter as discharge voltage limit (DVL)."),
"param.pack_low_voltage_recovery": ("Pack-Unterspannungs-Warnung aus", "Die Warnung endet über dieser Pack-Spannung.",
 "Pack low-voltage warning off", "The warning ends above this pack voltage."),
"param.pack_overvoltage_protection": ("Pack-Überspannungsschutz", "Der Master meldet diesen Wert als Ladespannung (CVL) an den Wechselrichter. Als Schutz öffnet er den Lade-MOSFET, sofern Funktionsschalter 1, Bit 5 an ist.",
 "Pack over-voltage protection", "The master reports this value to the inverter as charge voltage (CVL). As a protection it opens the charge MOSFET, provided function switch 1, bit 5 is on."),
"param.pack_overvoltage_recovery": ("Pack-Überspannungsschutz aus", "Laden ist wieder erlaubt unter dieser Pack-Spannung.",
 "Pack over-voltage protection off", "Charging is allowed again below this pack voltage."),
"param.pack_undervoltage_protection": ("Pack-Unterspannungsschutz", "Ab dieser Pack-Spannung öffnet der Entlade-MOSFET und der SOC springt auf 0 %.",
 "Pack under-voltage protection", "From this pack voltage the discharge MOSFET opens and the SOC jumps to 0 %."),
"param.pack_undervoltage_recovery": ("Pack-Unterspannungsschutz aus", "Entladen ist wieder erlaubt über dieser Pack-Spannung.",
 "Pack under-voltage protection off", "Discharging is allowed again above this pack voltage."),
"param.charger_overvoltage_protection": ("Ladegeräte-Überspannung", "Liegt am Ausgang (P+/P−) länger als 5 s mindestens diese Spannung, ist Laden gesperrt.",
 "Charger over-voltage", "With at least this voltage at the output (P+/P−) for more than 5 s, charging is blocked."),
"param.charger_overvoltage_recovery": ("Ladegeräte-Überspannung aus", "Die Sperre endet unter dieser Spannung am Ausgang.",
 "Charger over-voltage off", "The block ends below this voltage at the output."),
}

# cell, ambient and power temperatures: (key stem, de name, en name, kind)
_TEMPS = [
    ("charging_high_temperature", "Laden: Übertemperatur", "Charging: high temperature", "cell"),
    ("charging_low_temperature", "Laden: Untertemperatur", "Charging: low temperature", "cell"),
    ("charging_over_temperature", "Laden: Übertemperatur", "Charging: over-temperature", "cell"),
    ("charging_under_temperature", "Laden: Untertemperatur", "Charging: under-temperature", "cell"),
    ("discharging_high_temperature", "Entladen: Übertemperatur", "Discharging: high temperature", "cell"),
    ("discharging_low_temperature", "Entladen: Untertemperatur", "Discharging: low temperature", "cell"),
    ("discharging_over_temperature", "Entladen: Übertemperatur", "Discharging: over-temperature", "cell"),
    ("discharging_under_temperature", "Entladen: Untertemperatur", "Discharging: under-temperature", "cell"),
    ("ambient_high_temperature", "Umgebung: Übertemperatur", "Ambient: high temperature", "ambient"),
    ("ambient_low_temperature", "Umgebung: Untertemperatur", "Ambient: low temperature", "ambient"),
    ("ambient_over_temperature", "Umgebung: Übertemperatur", "Ambient: over-temperature", "ambient"),
    ("ambient_under_temperature", "Umgebung: Untertemperatur", "Ambient: under-temperature", "ambient"),
    ("power_high_temperature", "MOSFET: Übertemperatur", "MOSFET: high temperature", "power"),
    ("power_over_temperature", "MOSFET: Übertemperatur", "MOSFET: over-temperature", "power"),
]
_SENSOR = {
    "cell": ("an einem Zellfühler", "at a cell sensor"),
    "ambient": ("am Umgebungsfühler (BMS-Platine)", "at the ambient sensor (BMS board)"),
    "power": ("am Leistungsteil (MOSFET)", "at the power stage (MOSFET)"),
}
for stem, de, en, kind in _TEMPS:
    where_de, where_en = _SENSOR[kind]
    protection = stem.endswith(("over_temperature", "under_temperature"))
    if protection:
        if stem.startswith("charging"):
            eff_de, eff_en = "Laden ist gesperrt", "charging is blocked"
        elif stem.startswith("discharging"):
            eff_de, eff_en = "Entladen ist gesperrt", "discharging is blocked"
        else:
            eff_de, eff_en = "Laden und Entladen sind gesperrt", "charging and discharging are blocked"
        PARAMS[f"param.{stem}_protection"] = (f"{de}schutz", f"Schutz {where_de}: {eff_de}, bis der Rückkehrwert erreicht ist.",
            f"{en} protection", f"Protection {where_en}: {eff_en} until the recovery value is reached.")
        PARAMS[f"param.{stem}_recovery"] = (f"{de}schutz aus", "Rückkehrwert: Der Schutz endet, wenn dieser Wert wieder erreicht ist.",
            f"{en} protection off", "Recovery value: the protection ends when this value is reached again.")
    else:
        note_de = " Haben alle Packs eine Lade-Warnung, senkt der Master die Ladegrenze auf 10 A gesamt." if stem.startswith("charging") else ""
        note_en = " If all packs have a charge warning, the master lowers the charge limit to 10 A in total." if stem.startswith("charging") else ""
        PARAMS[f"param.{stem}_alarm"] = (f"{de}-Warnung", f"Warnung {where_de}, ohne Sperre.{note_de}",
            f"{en} warning", f"Warning {where_en}, without blocking.{note_en}")
        PARAMS[f"param.{stem}_recovery"] = (f"{de}-Warnung aus", "Rückkehrwert: Die Warnung endet, wenn dieser Wert wieder erreicht ist.",
            f"{en} warning off", "Recovery value: the warning ends when this value is reached again.")

PARAMS.update({
"param.cell_heating_on": ("Zellheizung ein", "Die Heizung schaltet ein, wenn die kälteste Zelle diesen Wert erreicht. Nur mit Heizfolie und Funktionsschalter 3, Bit 6.",
 "Cell heating on", "Heating switches on when the coldest cell reaches this value. Only with a heating pad and function switch 3, bit 6."),
"param.cell_heating_off": ("Zellheizung aus", "Die Heizung schaltet aus, wenn die kälteste Zelle über diesem Wert liegt.",
 "Cell heating off", "Heating switches off when the coldest cell is above this value."),
# current
"param.charging_overcurrent_alarm": ("Lade-Überstrom-Warnung", "Grundlage der Ladegrenze (CCL): je Pack die Hälfte dieses Werts, oder dieser Wert − 10 A mit Funktionsschalter 7, Bit 7. Kann den Hardware-Strombegrenzer aktivieren.",
 "Charge over-current warning", "Basis of the charge current limit (CCL): half of this value per pack, or this value − 10 A with function switch 7, bit 7. Can activate the hardware current limiter."),
"param.charging_overcurrent_recovery": ("Lade-Überstrom-Warnung aus", "Die Warnung endet unter diesem Ladestrom.",
 "Charge over-current warning off", "The warning ends below this charge current."),
"param.discharging_overcurrent_alarm": ("Entlade-Überstrom-Warnung", "Grundlage der Entladegrenze (DCL): je Pack dieser Betrag − 10 A.",
 "Discharge over-current warning", "Basis of the discharge current limit (DCL): this amount − 10 A per pack."),
"param.discharging_overcurrent_recovery": ("Entlade-Überstrom-Warnung aus", "Die Warnung endet unter diesem Entladestrom.",
 "Discharge over-current warning off", "The warning ends below this discharge current."),
"param.charging_overcurrent_protection": ("Lade-Überstromschutz", "Fließt dieser Ladestrom länger als die eingestellte Verzögerung, öffnet der Lade-MOSFET.",
 "Charge over-current protection", "If this charge current flows longer than the set delay, the charge MOSFET opens."),
"param.discharging_overcurrent_protection": ("Entlade-Überstromschutz", "Fließt dieser Entladestrom länger als die eingestellte Verzögerung, öffnet der Entlade-MOSFET.",
 "Discharge over-current protection", "If this discharge current flows longer than the set delay, the discharge MOSFET opens."),
"param.transient_overcurrent_protection": ("Kurzzeit-Überstromschutz", "Sehr hoher Entladestrom: Der Entlade-MOSFET öffnet nach der kurzen Verzögerung in ms.",
 "Transient over-current protection", "Very high discharge current: the discharge MOSFET opens after the short delay in ms."),
"param.output_soft_start_delay": ("Vorladung des Ausgangs", "Beim Einschalten lädt das BMS zuerst über einen Vorladepfad, höchstens so lange. Danach prüft es auf Kurzschluss und schaltet den Entlade-MOSFET ein.",
 "Output pre-charge", "When switching on, the BMS first charges through a pre-charge path, at most this long. Then it checks for a short circuit and switches the discharge MOSFET on."),
"param.charging_overcurrent_delay": ("Verzögerung Lade-Überstromschutz", "So lange muss der Lade-Überstrom anliegen, bis der Schutz greift. Der Strombegrenzer nach einer Lade-Überstrom-Warnung greift schon nach der Hälfte.",
 "Charge over-current delay", "How long the charge over-current must last before the protection trips. The current limiter after a charge over-current warning engages after half this time."),
"param.discharging_overcurrent_delay": ("Verzögerung Entlade-Überstromschutz", "So lange muss der Entlade-Überstrom anliegen, bis der Schutz greift.",
 "Discharge over-current delay", "How long the discharge over-current must last before the protection trips."),
"param.transient_overcurrent_delay": ("Verzögerung Kurzzeit-Überstrom", "Verzögerung des Kurzzeit-Überstromschutzes, in 5-ms-Schritten geprüft.",
 "Transient over-current delay", "Delay of the transient over-current protection, checked in 5 ms steps."),
"param.overcurrent_recovery_delay": ("Wiedereinschalten nach Überstrom", "Nach dieser Zeit löscht das BMS Überstrom-, Kurzzeit- und Kurzschluss-Schutz von selbst. Laden löscht die Entlade-Schutze sofort, Entladen den Lade-Schutz.",
 "Restart after over-current", "After this time the BMS clears over-current, transient and short-circuit protection by itself. Charging clears the discharge protections at once, discharging the charge protection."),
"param.overcurrent_lock_events": ("Sperre nach Ereignissen", "Nach so vielen Kurzzeit-Überstrom- bzw. Kurzschluss-Ereignissen ohne Pause bleibt der Entlade-MOSFET aus, bis der Pack Laden erkennt. Nur mit Funktionsschalter 4, Bit 6 bzw. 7.",
 "Lock after events", "After this many transient over-current or short-circuit events without a pause, the discharge MOSFET stays off until the pack detects charging. Only with function switch 4, bit 6 or 7."),
"param.charge_current_limit_duration": ("Dauer der Strombegrenzung", "So lange lädt der Pack nach einer Lade-Überstrom-Warnung über den Hardware-Strombegrenzer statt über den Lade-MOSFET.",
 "Current limiting duration", "How long the pack charges through the hardware current limiter instead of the charge MOSFET after a charge over-current warning."),
# capacity and SOC
"param.rated_capacity": ("Nennkapazität", "Wird als Nennkapazität gemeldet. Ein Ändern kann die gelernte Vollkapazität zurücksetzen.",
 "Rated capacity", "Reported as rated capacity. Changing it can reset the learned full capacity."),
"param.remaining_capacity_setting": ("Restkapazität (Setzwert)", "Der zuletzt gespeicherte Setzwert, nicht der aktuelle Ladestand. Nur beim Einzelschreiben wird der Zähler auf diesen Wert gesetzt.",
 "Remaining capacity (set value)", "The last stored set value, not the current charge. Only writing it as a single parameter sets the counter to this value."),
"param.remaining_capacity_alarm": ("SOC-Warnung unter", "Warnung, wenn der SOC unter diesen Wert fällt.",
 "SOC warning below", "Warning when the SOC falls below this value."),
"param.remaining_capacity_protection": ("SOC-Schutz unter", "Unter diesem SOC öffnet der Entlade-MOSFET; Laden bleibt erlaubt. Der SOC hält beim Entladen 1 % darüber, bis eine Zelle die Unterspannungs-Warnung erreicht.",
 "SOC protection below", "Below this SOC the discharge MOSFET opens; charging stays allowed. While discharging, the SOC holds 1 % above it until a cell reaches the low-voltage warning."),
"param.intermittent_recharge_below": ("Nachladen erst wieder unter", "Nach 100 % ist Laden gesperrt, bis der SOC auf diesen Wert fällt. Sind alle Packs so weit, meldet der Master Ladegrenze 0. Nur mit Funktionsschalter 5, Bit 1.",
 "Recharge only below", "After 100 % charging is blocked until the SOC falls to this value. When all packs are there, the master reports a charge limit of 0. Only with function switch 5, bit 1."),
"param.cycle_cumulative_capacity": ("Zyklus nach Entladung von", "Ein Zyklus zählt, sobald die Summe der SOC-Rückgänge diesen Wert erreicht.",
 "Cycle after discharge of", "A cycle counts once the sum of SOC decreases reaches this value."),
# balancing and cell difference
"param.cell_difference_fault": ("Zelldifferenz-Fehler ab", "Fehler, wenn höchste und niedrigste Zelle so weit auseinanderliegen. Sperrt das Balancing.",
 "Cell difference fault from", "Fault when the highest and lowest cell are this far apart. Blocks balancing."),
"param.cell_difference_fault_recovery": ("Zelldifferenz-Fehler aus unter", "Der Fehler endet unter dieser Differenz.",
 "Cell difference fault off below", "The fault ends below this difference."),
"param.balancing_start_difference": ("Balancing ab Spreizung", "Das Balancing beginnt erst, wenn höchste und niedrigste Zelle weiter auseinanderliegen.",
 "Balancing from spread", "Balancing only starts when the highest and lowest cell are further apart than this."),
"param.balancing_stop_difference": ("Balancing bis Abstand", "Eine Zelle wird balanciert, solange sie mindestens so weit über der niedrigsten liegt.",
 "Balancing down to", "A cell is balanced as long as it is at least this far above the lowest."),
"param.static_balancing_time": ("Balancing ohne Laden höchstens", "Mit Funktionsschalter 6, Bit 2: Nach so vielen Stunden Balancing ohne Laden ist es bis zur nächsten Ladung gesperrt.",
 "Balancing without charging at most", "With function switch 6, bit 2: after this many hours of balancing without charging, it is blocked until the next charge."),
# pack and activation
"param.cells_in_series": ("Zellen in Serie", "Zahl der Zellen. Geht in alle Pack-Spannungsregeln des SOC ein.",
 "Cells in series", "Number of cells. Used in all pack voltage rules of the SOC."),
"param.charge_activation_window": ("Ladeaktivierung: Dauer", "So lange hebt das BMS nach einer Abschaltung wegen Unterspannung oder SOC den Schutz auf, damit ein Ladegerät laden kann.",
 "Charge activation: duration", "For this long after a shutdown for under-voltage or SOC, the BMS lifts the protection so a charger can charge."),
"param.charge_activation_interval": ("Ladeaktivierung: Intervall", "Mit Funktionsschalter 6, Bit 4 wiederholt das BMS die Ladeaktivierung in diesem Abstand.",
 "Charge activation: interval", "With function switch 6, bit 4 the BMS repeats charge activation at this interval."),
"param.charge_activation_attempts": ("Ladeaktivierung: Versuche", "Höchstzahl der Versuche; der Zähler beginnt neu, sobald geladen wird.",
 "Charge activation: attempts", "Maximum number of attempts; the counter restarts once charging happens."),
"param.work_record_interval": ("Historie im Betrieb alle", "Abstand der Einträge im Historienspeicher (max. 500), während der Pack lädt oder entlädt.",
 "History while active every", "Interval of entries in the history memory (max. 500) while the pack charges or discharges."),
"param.standby_record_interval": ("Historie im Standby alle", "Abstand der Einträge im Historienspeicher im Standby.",
 "History in standby every", "Interval of entries in the history memory in standby."),
"param.standby_shutdown_delay": ("Selbstabschaltung nach Standby", "Mit Funktionsschalter 7, Bit 1: Steht der Pack so lange im Standby ohne Spannung am Ausgang, schaltet er sich ab. Wecken per Taster oder Ladegerät.",
 "Shutdown after standby", "With function switch 7, bit 1: if the pack is in standby without voltage at the output this long, it switches off. Wake it with the button or a charger."),
"param.connection_fault_impedance": ("Verbindungsfehler-Impedanz", "Ohne Funktion: Die Firmware liest diesen Wert nie.",
 "Connection fault impedance", "No function: the firmware never reads this value."),
"param.compensation_1_cell": ("Kompensation 1: Zelle", "Nummer der Zelle, deren Spannung um Strom × Widerstand korrigiert wird, z. B. wegen eines Verbinders in ihrer Messung.",
 "Compensation 1: cell", "Number of the cell whose voltage is corrected by current × resistance, e.g. because a connector lies in its measurement."),
"param.compensation_1_resistance": ("Kompensation 1: Widerstand", "Widerstand der Korrektur; 0 = unwirksam.",
 "Compensation 1: resistance", "Resistance of the correction; 0 = no effect."),
"param.compensation_2_cell": ("Kompensation 2: Zelle", "Wie Kompensation 1, für eine zweite Zelle.",
 "Compensation 2: cell", "Like compensation 1, for a second cell."),
"param.compensation_2_resistance": ("Kompensation 2: Widerstand", "Widerstand der Korrektur; 0 = unwirksam.",
 "Compensation 2: resistance", "Resistance of the correction; 0 = no effect."),
})

# Function switches: switch.<group>.<bit>. Groups 1-5 enable the messages of events 2-6; a bit at 0 switches the
# message and the reactions that depend on the event off (rules reading a parameter or value directly stay). Bits without an entry have no function.
SWITCHES = {
"switch.0.0": ("Schreiben über die CAN-Buchse erlauben", "Patch-Firmware: 1 erlaubt das Schreiben der Parameter über die CAN-Buchse, 0 sperrt es. Entsperren nur über den RS485-1/2-Bus oder am Display.",
 "Allow writing via the CAN socket", "Patched firmware: 1 allows writing the parameters via the CAN socket, 0 locks it. Unlock only via the RS485-1/2 bus or on the display."),
"switch.0.2": ("Wartungsstart über die CAN-Buchse erlauben", "Patch-Firmware: 1 erlaubt „Master anhalten“ über die CAN-Buchse, 0 verweigert es; freigeben geht immer. Ändern nur über den RS485-1/2-Bus oder am Display.",
 "Allow maintenance start via the CAN socket", "Patched firmware: 1 allows “Halt master” via the CAN socket, 0 refuses it; releasing always works. Change only via the RS485-1/2 bus or on the display."),
# TEMP(maint-1.6): the bit 3 texts name the first 1.6 (reversed polarity); drop that part with maint::temp16
"switch.0.3": ("Ein/Aus, Neustart und Uhr über die CAN-Buchse erlauben", "Patch-Firmware ab MAINT 1.5.3/1.6.3: 1 (an) erlaubt Ein/Aus, Neustart und Uhr stellen dort gemeinsam, 0 (aus) sperrt sie; Uhr lesen geht immer. Ändern nur über den RS485-1/2-Bus oder am Display. Nur beim vorübergehend unterstützten ersten MAINT 1.6 ist es umgekehrt.",
 "Allow on/off, restart and clock via the CAN socket", "Patched firmware from MAINT 1.5.3/1.6.3: 1 (on) allows on/off, restart and setting the clock there together, 0 (off) locks them; reading the clock always works. Change only via the RS485-1/2 bus or on the display. Only the first MAINT 1.6, supported for now, works the other way round."),
"switch.0.6": ("Hard-Stop bei Nachladepause", "Patch-Firmware MAINT 1.6.3: die Wahl des Hard-Stops, wenn Bit 7 auf 0 steht (1 an, 0 aus). Gesetzt nur über den eigenen Schalter unter Parameter, nie durch eine Bearbeitung oder Datei.",
 "Hard stop at recharge pause", "Patched firmware MAINT 1.6.3: the hard stop choice while bit 7 is 0 (1 on, 0 off). Set only by its own switch under Parameters, never by an edit or a file."),
"switch.0.7": ("Hard-Stop: Voreinstellung des Builds", "Patch-Firmware MAINT 1.6.3: 1 übernimmt die Voreinstellung des Images (wie beim Altwert FF), 0 gilt Bit 6.",
 "Hard stop: the build's default", "Patched firmware MAINT 1.6.3: 1 takes the image's default (as with the old value FF), 0 applies bit 6."),
"switch.0.4": ("Erweitertes Aus", "Patch-Firmware mit Packsperre (ab MAINT 1.6.3), gilt für gezieltes Aus über den RS485-1/2-Bus und die CAN-Buchse: 1 (an, wie beim Altwert FF) schaltet das erweiterte Aus ein: Gezieltes Ausschalten sperrt dann Lade- und Entladepfad einschließlich Strombegrenzer und Vorladung, und die Sperre bleibt über Neustarts erhalten. 0 (aus): ein gezieltes Aus bleibt das bisherige Soft-Aus. Einschaltbroadcasts des Masters und Parameterimporte lösen die Sperre nicht; zum Freigeben dieses Pack gezielt einschalten.",
 "Extended off", "Patched firmware with the pack cutoff (from MAINT 1.6.3), applies to a direct off via the RS485-1/2 bus and the CAN socket: 1 (on, as with the old value FF) switches the extended off on: a direct off then locks the charge and discharge path including the current limiter and precharge, and the lock survives restarts. 0 (off): a direct off stays the previous soft off. On broadcasts of the master and parameter imports do not release the lock; switch this pack on directly to release it."),
"switch.0.3p": ("Freigabe für Ein/Aus, Neustart und Uhr über die CAN-Buchse", "Patch-Firmware ab MAINT 1.5.3 und 1.6.3 mit Ein/Aus und Uhr über die CAN-Buchse: 1 (an, wie beim Altwert FF) erlaubt Ein/Aus, Neustart und Uhr stellen dort gemeinsam, 0 (aus) sperrt sie; Uhr lesen geht immer. Ändern nur über den RS485-1/2-Bus oder am Display.",
 "Release of on/off, restart and clock via the CAN socket", "Patched firmware from MAINT 1.5.3 and 1.6.3 with on/off and the clock via the CAN socket: 1 (on, as with the old value FF) allows on/off, restart and setting the clock there together, 0 (off) locks them; reading the clock always works. Change only via the RS485-1/2 bus or on the display."),
"switch.0.5": ("Keine individuelle Packsperre", "Patch-Firmware mit Packsperre (MAINT 1.6.3), nur Status: 1 (an), solange keine gespeicherte Sperre besteht; 0 (aus), solange das Pack gesperrt ist, auch bei ausgeschaltetem erweitertem Aus. Nie bearbeitbar; gezieltes Einschalten dieses Packs löst die Sperre, Parameterimporte nicht.",
 "No individual pack cutoff", "Patched firmware with the pack cutoff (MAINT 1.6.3), status only: 1 (on) while no stored lock exists; 0 (off) while the pack is locked, also with the extended off switched off. Never editable; switching this pack on directly releases the lock, parameter imports do not."),
"switch.0.1": ("Prüfung der Temperaturfühler", "Fühler unter −45 °C, ab 100 °C oder mit 20 K Spreizung melden einen Fühlerfehler; die Heizung schaltet dann aus.",
 "Temperature sensor check", "Sensors below −45 °C, from 100 °C or 20 K apart report a sensor fault; heating then switches off."),
}
_MASKS = {
    1: [("Zell-Hochspannungs-Warnung", "Cell high-voltage warning"), ("Zell-Überspannungsschutz", "Cell over-voltage protection"),
        ("Zell-Unterspannungs-Warnung", "Cell low-voltage warning"), ("Zell-Unterspannungsschutz", "Cell under-voltage protection"),
        ("Pack-Hochspannungs-Warnung", "Pack high-voltage warning"), ("Pack-Überspannungsschutz", "Pack over-voltage protection"),
        ("Pack-Unterspannungs-Warnung", "Pack low-voltage warning"), ("Pack-Unterspannungsschutz", "Pack under-voltage protection")],
    2: [("Laden: Übertemperatur-Warnung", "Charging: high temperature warning"), ("Laden: Übertemperaturschutz", "Charging: over-temperature protection"),
        ("Laden: Untertemperatur-Warnung", "Charging: low temperature warning"), ("Laden: Untertemperaturschutz", "Charging: under-temperature protection"),
        ("Entladen: Übertemperatur-Warnung", "Discharging: high temperature warning"), ("Entladen: Übertemperaturschutz", "Discharging: over-temperature protection"),
        ("Entladen: Untertemperatur-Warnung", "Discharging: low temperature warning"), ("Entladen: Untertemperaturschutz", "Discharging: under-temperature protection")],
    3: [("Umgebung: Übertemperatur-Warnung", "Ambient: high temperature warning"), ("Umgebung: Übertemperaturschutz", "Ambient: over-temperature protection"),
        ("Umgebung: Untertemperatur-Warnung", "Ambient: low temperature warning"), ("Umgebung: Untertemperaturschutz", "Ambient: under-temperature protection"),
        ("MOSFET: Übertemperaturschutz", "MOSFET: over-temperature protection"), ("MOSFET: Übertemperatur-Warnung", "MOSFET: high temperature warning")],
    4: [("Lade-Überstrom-Warnung", "Charge over-current warning"), ("Lade-Überstromschutz", "Charge over-current protection"),
        ("Entlade-Überstrom-Warnung", "Discharge over-current warning"), ("Entlade-Überstromschutz", "Discharge over-current protection"),
        ("Kurzzeit-Überstrom", "Transient over-current"), ("Kurzschluss-Erkennung", "Short-circuit detection")],
    5: [("Ladegeräte-Überspannung", "Charger over-voltage"), None, ("SOC-Warnung", "SOC warning"), ("SOC-Schutz", "SOC protection")],
}
for g, bits in _MASKS.items():
    for b, names in enumerate(bits):
        if names:
            SWITCHES[f"switch.{g}.{b}"] = (names[0], "Freigabe dieser Meldung. Aus = keine Meldung und keine Reaktion (kein MOSFET-Abschalten, keine Folge für Ladegrenze oder SOC).",
                names[1], "Enables this message. Off = no message and no reaction (no MOSFET switching, no effect on charge limit or SOC).")
SWITCHES.update({
"switch.3.6": ("Zellheizung", "Heizausgang bei Kälte, nur beim Laden bzw. mit erkanntem Ladegerät. Gibt auch die Mindest-Ladegrenze zum Heizen frei.",
 "Cell heating", "Heating output in the cold, only while charging or with a detected charger. Also enables the minimum charge limit for heating."),
"switch.3.7": ("Schwerfehler-Meldung", "Meldet schwere Fehler (Fühlerfehler, Zelle weit über Schutz oder unter Unterspannung) bis zum Neustart; schaltet ohne Heizung den Relais-Ausgang.",
 "Severe fault message", "Reports severe faults (sensor fault, cell far beyond protection or below under-voltage) until restart; without heating it switches the relay output."),
"switch.4.6": ("Sperre nach Kurzzeit-Überströmen", "Nach mehreren Kurzzeit-Überstrom-Ereignissen bleibt der Entlade-MOSFET aus, bis der Pack Laden erkennt.",
 "Lock after transient over-currents", "After several transient over-current events the discharge MOSFET stays off until the pack detects charging."),
"switch.4.7": ("Sperre nach Kurzschlüssen", "Nach mehreren Kurzschluss-Ereignissen bleibt der Entlade-MOSFET aus, bis der Pack Laden erkennt.",
 "Lock after short circuits", "After several short-circuit events the discharge MOSFET stays off until the pack detects charging."),
"switch.5.1": ("Intermittierende Nachladung", "Nach 100 % wird erst wieder geladen, wenn der SOC unter den eingestellten Wert fällt. Sind alle Packs so weit, meldet der Master Ladegrenze 0.",
 "Intermittent recharge", "After 100 %, charging resumes only when the SOC falls below the set value. When all packs are there, the master reports a charge limit of 0."),
"switch.5.4": ("Laden verboten bei tiefer Zelle", "Ladesperre unter der eingestellten Zellspannung; wirkt nur, wenn die automatische Ladeaktivierung aus ist.",
 "No charging with a deep cell", "Charge block below the set cell voltage; only effective while automatic charge activation is off."),
"switch.5.6": ("Ausgangs-Verbindungsfehler", "Hardware-Eingang: Ist er 5 s aktiv, schaltet der Entlade-MOSFET aus.",
 "Output connection fault", "Hardware input: if it is active for 5 s, the discharge MOSFET switches off."),
"switch.5.7": ("Meldung der Strombegrenzung", "Interne Rückmeldung des Strombegrenzers.",
 "Current limiter feedback", "Internal feedback of the current limiter."),
"switch.6.0": ("Balancing", "Schaltet das Balancing ein.", "Balancing", "Switches balancing on."),
"switch.6.1": ("Balancing auch im Leerlauf", "Sonst nur beim Laden; beim Entladen nie.", "Balancing also when idle", "Otherwise only while charging; never while discharging."),
"switch.6.2": ("Zeitlimit für Balancing ohne Laden", "Begrenzt das Balancing ohne Laden auf die eingestellten Stunden.", "Time limit for balancing without charging", "Limits balancing without charging to the set hours."),
"switch.6.3": ("Kein Balancing bei Umgebungs-Meldung", "Sperrt das Balancing bei Umgebungs-Temperaturwarnung oder -schutz.", "No balancing with ambient message", "Blocks balancing during an ambient temperature warning or protection."),
"switch.6.4": ("Automatische Ladeaktivierung", "Hebt nach Unterspannungs- oder SOC-Abschaltung den Schutz zeitweise auf, damit geladen werden kann. Schaltet die Ladesperre bei tiefer Zelle ab.",
 "Automatic charge activation", "After an under-voltage or SOC shutdown, lifts the protection at times so charging is possible. Disables the charge block for a deep cell."),
"switch.6.5": ("Manuelle Ladeaktivierung", "Einmalige Ladeaktivierung nach dem Einschalten.", "Manual charge activation", "One charge activation after switching on."),
"switch.6.6": ("Aktive Strombegrenzung", "Laden ab 10 A sofort über den Hardware-Strombegrenzer.", "Active current limiting", "Charging from 10 A goes through the hardware current limiter at once."),
"switch.6.7": ("Passive Strombegrenzung", "Nach einer Lade-Überstrom-Warnung lädt der Pack eine Zeit lang über den Hardware-Strombegrenzer.",
 "Passive current limiting", "After a charge over-current warning the pack charges through the hardware current limiter for a while."),
"switch.7.0": ("Externer Ein/Aus-Schalter", "Wertet einen externen Schalter am Eingang aus. Ohne angeschlossenen Schalter entscheidet der Ruhepegel.",
 "External on/off switch", "Evaluates an external switch at the input. Without a switch connected, the idle level decides."),
"switch.7.1": ("Selbstabschaltung im Standby", "Schaltet den Pack nach der eingestellten Standby-Zeit ohne Spannung am Ausgang ab.",
 "Shutdown in standby", "Switches the pack off after the set standby time without voltage at the output."),
"switch.7.3": ("Display-Schnittstelle", "Binärprotokoll an RS232/TTL, vermutlich für das LCD-Display.", "Display interface", "Binary protocol on RS232/TTL, probably for the LCD display."),
"switch.7.4": ("Erweitertes Display-Format", "Binärprotokoll im erweiterten Format.", "Extended display format", "Binary protocol in the extended format."),
"switch.7.5": ("Automatische Adressvergabe", "Ignoriert die DIP-Adresse und vergibt Adressen über eine Puls-Kette.",
 "Automatic addressing", "Ignores the DIP address and assigns addresses over a pulse chain."),
"switch.7.6": ("Automatische Adresse 0 vermeiden", "Nur zusammen mit Bit 5 (automatische Adressvergabe): Ergibt die Puls-Kette Adresse 0, nimmt das Pack 1; es wird so nicht Master. Ohne Bit 5 ohne Wirkung. Code sicher (0x9466, 16.06.13), der Zweck ist eine Deutung.",
 "Avoid automatic address 0", "Only together with bit 5 (automatic addressing): if the pulse chain gives address 0, the pack takes 1, so it does not become master. Without bit 5 no effect. Code certain (0x9466, 16.06.13), the purpose is an interpretation."),
"switch.7.7": ("Ladegrenze: Warnung − 10 A", "An: Ladegrenze je Pack = Lade-Überstrom-Warnung − 10 A. Aus: die Hälfte davon.",
 "Charge limit: warning − 10 A", "On: charge limit per pack = charge over-current warning − 10 A. Off: half of it."),
})

# Longer explanations (help page body and the comments in the app's parameter files): firmware effects that the short
# text does not carry. id: (de, en). Plain text, may contain "\n" between sentences groups; no Markdown headings.
PARAM_DETAILS = {
# cell voltage
"param.cell_high_voltage_alarm": (
 "Meldung: Event 2 Bit 0 (Freigabe Funktionsschalter 1, Bit 0). Für die Ladegrenze fällt jeder warnende Pack aus der Summe; erst wenn keiner übrig ist, gilt 10 A gesamt.\nSOC: Oberhalb 98,2 % zählt der SOC beim Laden nur weiter, wenn eine Zelle diesen Wert erreicht. 100 % setzt die Firmware erst, wenn die Pack-Spannung mindestens Zellen × diesem Wert (begrenzt auf 3,45–3,50 V je Zelle) und die niedrigste Zelle mindestens 3,350 V beträgt, beides durchgehend über 30 min, seit mehr als 30 s kein Ladestrom ab 3 A floss und der SOC unter 99 % liegt. Außerdem die Voll-Marke für das Lernen der Kapazität. Hysterese: Rückkehrwert „Zell-Hochspannungs-Warnung aus“.",
 "Message: event 2 bit 0 (enable: function switch 1, bit 0). For the charge limit every warning pack drops out of the sum; only when none is left does 10 A in total apply.\nSOC: above 98.2 % the SOC only keeps counting up while charging if a cell reaches this value. The firmware sets 100 % only when the pack voltage is at or above cells × this value (limited to 3.45–3.50 V per cell) and the lowest cell at or above 3.350 V, both throughout more than 30 min, no charge current of 3 A or more for over 30 s, and the SOC below 99 %. Also the full mark for capacity learning. Hysteresis: recovery value “Cell high-voltage warning off”."),
"param.cell_high_voltage_recovery": (
 "Außerdem: Im Standby driftet der SOC nicht, solange eine Zelle mindestens diesen Wert hat.",
 "Also: in standby the SOC does not drift as long as a cell is at or above this value."),
"param.cell_low_voltage_alarm": (
 "Meldung: Event 2 Bit 2 (Freigabe Funktionsschalter 1, Bit 2). Der SOC springt auf (SOC-Warnung + SOC-Schutz) / 2, wenn eine Zelle diesen Wert erreicht und der SOC noch mindestens bei der SOC-Warnung steht. Solange alle Zellen darüber liegen, zählt der SOC beim Entladen nicht unter SOC-Schutz + 1 % (geprüft in ganzen Prozent).\nAußerdem die Leer-Marke für das Lernen der Kapazität. Als entladebezogene Warnung nimmt sie den Pack aus der Summe der Entladegrenze (DCL); warnen alle Packs, meldet der Master DCL = 0. Schutzstufe: Zell-Unterspannungsschutz.",
 "Message: event 2 bit 2 (enable: function switch 1, bit 2). The SOC jumps to (SOC warning + SOC protection) / 2 when a cell reaches this value and the SOC is still at or above the SOC warning. As long as all cells are above it, the SOC does not count below SOC protection + 1 % while discharging (checked in whole percent).\nAlso the empty mark for capacity learning. As a discharge-related warning it removes the pack from the discharge limit (DCL) sum; if all packs warn, the master reports DCL = 0. Protection stage: cell under-voltage protection."),
"param.cell_low_voltage_recovery": (
 "Außerdem löscht die Firmware das Voll-Kennzeichen des SOC, wenn die höchste Zelle auf diesen Wert oder darunter fällt.",
 "The firmware also clears the SOC's full flag when the highest cell falls to or below this value."),
"param.cell_overvoltage_protection": (
 "Meldung: Event 2 Bit 1 (Freigabe Funktionsschalter 1, Bit 1). Den SOC auf 100 % setzt der Schutz nur beim Laden. Rückkehr: „Zell-Überspannungsschutz aus“. Ist der Pack-Überspannungsschutz maskiert, ist dies der einzige Überladeschutz.",
 "Message: event 2 bit 1 (enable: function switch 1, bit 1). The protection sets the SOC to 100 % only while charging. Recovery: “Cell over-voltage protection off”. If pack over-voltage protection is masked, this is the only overcharge protection."),
"param.cell_undervoltage_protection": (
 "Meldung: Event 2 Bit 3 (Freigabe Funktionsschalter 1, Bit 3). Rückkehr: „Zell-Unterspannungsschutz aus“. Danach kann die Ladeaktivierung (Dauer, Intervall, Versuche) den Schutz zeitweise aufheben, damit ein Ladegerät laden kann.",
 "Message: event 2 bit 3 (enable: function switch 1, bit 3). Recovery: “Cell under-voltage protection off”. Afterwards charge activation (duration, interval, attempts) can lift the protection at times so a charger can charge."),
"param.balancing_start_voltage": (
 "Weitere Bedingungen: Die Spreizung muss über „Balancing ab Spreizung“ liegen, und die Zelle muss mindestens „Balancing bis Abstand“ über der niedrigsten liegen. Funktionsschalter 6 legt fest, ob und wann balanciert wird.",
 "Further conditions: the spread must exceed “Balancing from spread”, and the cell must be at least “Balancing down to” above the lowest. Function switch 6 decides whether and when balancing runs."),
"param.cell_low_voltage_charging_forbidden": (
 "Meldung: Event 6 Bit 4 (Freigabe Funktionsschalter 5, Bit 4). Liegt eine Zelle auf oder unter diesem Wert, bricht auch die Ladeaktivierung ab.",
 "Message: event 6 bit 4 (enable: function switch 5, bit 4). With a cell at or below this value, charge activation is aborted as well."),
# pack voltage
"param.pack_high_voltage_alarm": (
 "Meldung: Event 2 Bit 4 (Freigabe Funktionsschalter 1, Bit 4). Für die Ladegrenze fällt jeder warnende Pack aus der Summe. Kein Einfluss auf den SOC: Die SOC-Regeln rechnen mit Zellen × Zell-Hochspannungs-Warnung, nicht mit diesem Wert.",
 "Message: event 2 bit 4 (enable: function switch 1, bit 4). For the charge limit every warning pack drops out of the sum. No influence on the SOC: the SOC rules use cells × cell high-voltage warning, not this value."),
"param.pack_low_voltage_alarm": (
 "Meldung: Event 2 Bit 6 (Freigabe Funktionsschalter 1, Bit 6). Als entladebezogene Warnung nimmt sie den Pack aus der Summe der Entladegrenze (DCL). Schutzstufe: Pack-Unterspannungsschutz.",
 "Message: event 2 bit 6 (enable: function switch 1, bit 6). As a discharge-related warning it removes the pack from the discharge limit (DCL) sum. Protection stage: pack under-voltage protection."),
"param.pack_overvoltage_protection": (
 "Als CVL gilt der Wert immer, auch wenn der Schutz maskiert ist. Als Schutz: Event 2 Bit 5; der Lade-MOSFET öffnet und der SOC springt auf 100 %. Ist Bit 5 aus, löst der Schutz nie aus und setzt den SOC auch nicht auf 100 %.",
 "As CVL the value always applies, even with the protection masked. As a protection: event 2 bit 5; the charge MOSFET opens and the SOC jumps to 100 %. With bit 5 off, the protection never trips and does not set the SOC to 100 % either."),
"param.pack_undervoltage_protection": (
 "Meldung: Event 2 Bit 7 (Freigabe Funktionsschalter 1, Bit 7). Rückkehr: „Pack-Unterspannungsschutz aus“. Vgl. Zell-Unterspannungsschutz und Pack-Unterspannungs-Warnung.",
 "Message: event 2 bit 7 (enable: function switch 1, bit 7). Recovery: “Pack under-voltage protection off”. Compare cell under-voltage protection and pack low-voltage warning."),
"param.charger_overvoltage_protection": (
 "Meldung: Event 6 Bit 0 (Freigabe Funktionsschalter 5, Bit 0). Gemessen wird die Spannung am Port P+/P−, nicht die Zellsumme.",
 "Message: event 6 bit 0 (enable: function switch 5, bit 0). The voltage is measured at the P+/P− port, not the cell sum."),
# cell temperature
"param.charging_high_temperature_alarm": (
 "Meldung: Event 3 Bit 0 (Freigabe Funktionsschalter 2, Bit 0), gemeldet nur beim Laden bzw. mit erkanntem Ladegerät. Für die Ladegrenze fällt jeder warnende Pack aus der Summe. Schutzstufe: Laden: Übertemperaturschutz.",
 "Message: event 3 bit 0 (enable: function switch 2, bit 0), reported only while charging or with a detected charger. For the charge limit every warning pack drops out of the sum. Protection stage: charging over-temperature protection."),
"param.charging_low_temperature_alarm": (
 "Meldung: Event 3 Bit 2 (Freigabe Funktionsschalter 2, Bit 2), gemeldet nur beim Laden bzw. mit erkanntem Ladegerät. Für die Ladegrenze fällt jeder warnende Pack aus der Summe. Schutzstufe: Laden: Untertemperaturschutz.",
 "Message: event 3 bit 2 (enable: function switch 2, bit 2), reported only while charging or with a detected charger. For the charge limit every warning pack drops out of the sum. Protection stage: charging under-temperature protection."),
"param.charging_over_temperature_protection": (
 "Meldung: Event 3 Bit 1 (Freigabe Funktionsschalter 2, Bit 1). Gemeldet wird der Schutz nur in Laderichtung; die Ladesperre prüft ab 16.06.11 aber das rohe Schutzbit unabhängig von der Stromrichtung, sperrt also auch während des Entladens.",
 "Message: event 3 bit 1 (enable: function switch 2, bit 1). The protection is reported only in the charging direction; from 16.06.11 the charge lock tests the raw protection bit regardless of the current direction, so it also blocks while discharging."),
"param.charging_under_temperature_protection": (
 "Meldung: Event 3 Bit 3 (Freigabe Funktionsschalter 2, Bit 3). Gemeldet wird der Schutz nur in Laderichtung; die Ladesperre prüft ab 16.06.11 aber das rohe Schutzbit unabhängig von der Stromrichtung.\nLiFePO4-Zellen sind laut Datenblatt meist erst ab 0 °C zum Laden freigegeben; ein tieferer Wert verlässt sich auf Wechselrichter und Ladegrenze.",
 "Message: event 3 bit 3 (enable: function switch 2, bit 3). The protection is reported only in the charging direction; from 16.06.11 the charge lock tests the raw protection bit regardless of the current direction.\nLiFePO4 cells are usually rated for charging only from 0 °C; a lower value relies on the inverter and the charge limit."),
"param.discharging_high_temperature_alarm": (
 "Meldung: Event 3 Bit 4 (Freigabe Funktionsschalter 2, Bit 4), gemeldet nur beim Entladen. Als entladebezogene Warnung nimmt sie den Pack aus der Summe der Entladegrenze (DCL). Schutzstufe: Entladen: Übertemperaturschutz.",
 "Message: event 3 bit 4 (enable: function switch 2, bit 4), reported only while discharging. As a discharge-related warning it removes the pack from the discharge limit (DCL) sum. Protection stage: discharging over-temperature protection."),
"param.discharging_low_temperature_alarm": (
 "Meldung: Event 3 Bit 6 (Freigabe Funktionsschalter 2, Bit 6), gemeldet nur beim Entladen. Als entladebezogene Warnung nimmt sie den Pack aus der Summe der Entladegrenze (DCL). Schutzstufe: Entladen: Untertemperaturschutz.",
 "Message: event 3 bit 6 (enable: function switch 2, bit 6), reported only while discharging. As a discharge-related warning it removes the pack from the discharge limit (DCL) sum. Protection stage: discharging under-temperature protection."),
"param.discharging_over_temperature_protection": (
 "Meldung: Event 3 Bit 5 (Freigabe Funktionsschalter 2, Bit 5). Der Entlade-MOSFET öffnet. Gemeldet wird der Schutz nur in Entladerichtung; die Entladesperre prüft ab 16.06.11 aber das rohe Schutzbit unabhängig von der Stromrichtung.",
 "Message: event 3 bit 5 (enable: function switch 2, bit 5). The discharge MOSFET opens. The protection is reported only in the discharging direction; from 16.06.11 the discharge lock tests the raw protection bit regardless of the current direction."),
"param.discharging_under_temperature_protection": (
 "Meldung: Event 3 Bit 7 (Freigabe Funktionsschalter 2, Bit 7). Der Entlade-MOSFET öffnet. Gemeldet wird der Schutz nur in Entladerichtung; die Entladesperre prüft ab 16.06.11 aber das rohe Schutzbit unabhängig von der Stromrichtung.",
 "Message: event 3 bit 7 (enable: function switch 2, bit 7). The discharge MOSFET opens. The protection is reported only in the discharging direction; from 16.06.11 the discharge lock tests the raw protection bit regardless of the current direction."),
"param.cell_heating_on": (
 "Anforderung: Event 4 Bit 6 (Freigabe Funktionsschalter 3, Bit 6). Der Heizausgang schaltet nur beim Laden bzw. mit erkanntem Ladegerät. Solange geheizt werden soll, meldet der Master mindestens Packs × 10 A als Ladegrenze, auch wenn sie sonst 0 wäre.",
 "Request: event 4 bit 6 (enable: function switch 3, bit 6). The heating output switches only while charging or with a detected charger. While heating is requested, the master reports at least packs × 10 A as charge limit, even if it would otherwise be 0."),
# ambient and power stage
"param.ambient_high_temperature_alarm": (
 "Meldung: Event 4 Bit 0 (Freigabe Funktionsschalter 3, Bit 0). Solange eine Umgebungs-Warnung oder ein Umgebungs-Schutz aktiv ist, verdoppeln sich die Stromschwellen, ab denen das BMS Laden bzw. Entladen erkennt, und mit Funktionsschalter 6, Bit 3 ist das Balancing gesperrt. Zählt für Lade- und Entladegrenze als Warnung (der Pack fällt aus der Summe).",
 "Message: event 4 bit 0 (enable: function switch 3, bit 0). While an ambient warning or protection is active, the current thresholds at which the BMS detects charging or discharging double, and with function switch 6, bit 3 balancing is blocked. Counts as a warning for the charge and discharge limit (the pack drops out of the sum)."),
"param.ambient_low_temperature_alarm": (
 "Meldung: Event 4 Bit 2 (Freigabe Funktionsschalter 3, Bit 2). Wirkt wie die Umgebungs-Übertemperatur-Warnung: doppelte Erkennungsschwellen für Laden/Entladen, Balancing-Sperre mit Funktionsschalter 6, Bit 3, Pack fällt aus der Summe von Lade- und Entladegrenze.",
 "Message: event 4 bit 2 (enable: function switch 3, bit 2). Works like the ambient high-temperature warning: doubled detection thresholds for charging/discharging, balancing block with function switch 6, bit 3, the pack drops out of the charge and discharge limit sums."),
"param.ambient_over_temperature_protection": (
 "Meldung: Event 4 Bit 1 (Freigabe Funktionsschalter 3, Bit 1). „Ambient“ und „Environmental“ im Export sind derselbe Fühler auf der BMS-Platine.",
 "Message: event 4 bit 1 (enable: function switch 3, bit 1). “Ambient” and “Environmental” in the export are the same sensor on the BMS board."),
"param.ambient_under_temperature_protection": (
 "Meldung: Event 4 Bit 3 (Freigabe Funktionsschalter 3, Bit 3). „Ambient“ und „Environmental“ im Export sind derselbe Fühler auf der BMS-Platine.",
 "Message: event 4 bit 3 (enable: function switch 3, bit 3). “Ambient” and “Environmental” in the export are the same sensor on the BMS board."),
"param.power_high_temperature_alarm": (
 "Meldung: Event 4 Bit 5 (Freigabe Funktionsschalter 3, Bit 5). Zählt für Lade- und Entladegrenze als Warnung (der Pack fällt aus der Summe). Schutzstufe: MOSFET-Übertemperaturschutz.",
 "Message: event 4 bit 5 (enable: function switch 3, bit 5). Counts as a warning for the charge and discharge limit (the pack drops out of the sum). Protection stage: MOSFET over-temperature protection."),
"param.power_over_temperature_protection": (
 "Meldung: Event 4 Bit 4 (Freigabe Funktionsschalter 3, Bit 4).",
 "Message: event 4 bit 4 (enable: function switch 3, bit 4)."),
# current
"param.charging_overcurrent_alarm": (
 "Meldung: Event 5 Bit 0 (Freigabe Funktionsschalter 4, Bit 0). Die Ladegrenze des Masters ist dieser Anteil × ladefähige Packs ohne ladebezogene Warnung. Mit Funktionsschalter 6, Bit 7 schaltet die Warnung nach der halben Verzögerung des Lade-Überstromschutzes auf den Hardware-Strombegrenzer. Schutzstufe: Lade-Überstromschutz.",
 "Message: event 5 bit 0 (enable: function switch 4, bit 0). The master's charge limit is this share × chargeable packs without a charge-related warning. With function switch 6, bit 7 the warning switches to the hardware current limiter after half the charge over-current protection delay. Protection stage: charge over-current protection."),
"param.discharging_overcurrent_alarm": (
 "Meldung: Event 5 Bit 2 (Freigabe Funktionsschalter 4, Bit 2). Der Wert ist negativ (Entladen). Abzug von 10 A nur bei Beträgen über 20 A. Die Entladegrenze des Masters ist dieser Anteil × entladefähige Packs ohne entladebezogene Warnung; warnen alle, ist sie 0. Schutzstufe: Entlade-Überstromschutz.",
 "Message: event 5 bit 2 (enable: function switch 4, bit 2). The value is negative (discharging). The 10 A deduction applies only to amounts above 20 A. The master's discharge limit is this share × dischargeable packs without a discharge-related warning; if all warn, it is 0. Protection stage: discharge over-current protection."),
"param.charging_overcurrent_protection": (
 "Meldung: Event 5 Bit 1 (Freigabe Funktionsschalter 4, Bit 1). Wieder frei nach „Wiedereinschalten nach Überstrom“ oder sofort, wenn Entladestrom fließt.",
 "Message: event 5 bit 1 (enable: function switch 4, bit 1). Released after “Restart after over-current” or at once when discharge current flows."),
"param.discharging_overcurrent_protection": (
 "Meldung: Event 5 Bit 3 (Freigabe Funktionsschalter 4, Bit 3). Wieder frei nach „Wiedereinschalten nach Überstrom“ oder sofort, wenn Ladestrom fließt.",
 "Message: event 5 bit 3 (enable: function switch 4, bit 3). Released after “Restart after over-current” or at once when charge current flows."),
"param.transient_overcurrent_protection": (
 "Meldung: Event 5 Bit 4 (Freigabe Funktionsschalter 4, Bit 4). Wieder frei nach „Wiedereinschalten nach Überstrom“. Eine dauerhafte Sperre nach wiederholten Ereignissen gibt es nur mit Funktionsschalter 4, Bit 6.",
 "Message: event 5 bit 4 (enable: function switch 4, bit 4). Released after “Restart after over-current”. A lasting lock after repeated events exists only with function switch 4, bit 6."),
"param.output_soft_start_delay": (
 "Die Vorladung endet früher, wenn die Ausgangsspannung nicht mehr steigt. Liegt der Ausgang danach unter der Hälfte des Sollwerts, meldet das BMS Kurzschluss (Event 5 Bit 5, nur mit Funktionsschalter 4, Bit 5).",
 "Pre-charge ends earlier when the output voltage stops rising. If the output is then below half the target, the BMS reports a short circuit (event 5 bit 5, only with function switch 4, bit 5)."),
"param.charging_overcurrent_delay": (
 "Die verkürzte Zeit für den Strombegrenzer gilt nur mit Funktionsschalter 6, Bit 7 (passive Strombegrenzung).",
 "The shorter time for the current limiter applies only with function switch 6, bit 7 (passive current limiting)."),
"param.overcurrent_lock_events": (
 "Gezählt werden Ereignisse ohne 301 s Pause dazwischen. Die Sperre meldet Event 5 Bit 6 (Kurzzeit-Überstrom) bzw. Bit 7 (Kurzschluss). Der normale Lade- und Entlade-Überstromschutz zählt nicht mit.",
 "Events count when there is no 301 s pause between them. The lock reports event 5 bit 6 (transient over-current) or bit 7 (short circuit). Normal charge and discharge over-current protection does not count."),
"param.charge_current_limit_duration": (
 "Nur mit Funktionsschalter 6, Bit 7 (Event 7 Bit 7). Die Begrenzung beginnt, wenn die Lade-Überstrom-Warnung die halbe Verzögerung des Lade-Überstromschutzes ansteht, und endet auch nach 5 s ohne Laden. Mit Funktionsschalter 6, Bit 6 ist diese passive Begrenzung wirkungslos.",
 "Only with function switch 6, bit 7 (event 7 bit 7). Limiting starts when the charge over-current warning lasts half the charge over-current protection delay, and also ends after 5 s without charging. With function switch 6, bit 6 this passive limiting has no effect."),
# capacity and SOC
"param.rated_capacity": (
 "Wird in 0x42 als „Rated capacity“ gemeldet. Schreiben des ganzen Parameterblocks setzt die gelernte Vollkapazität auf diesen Wert, wenn beide um mindestens 10 Ah abweichen. Sind beim Start beide gespeicherten Kapazitätsdatensätze ungültig, gilt dieser Wert als Vollkapazität.",
 "Reported in 0x42 as “Rated capacity”. Writing the whole parameter block sets the learned full capacity to this value if the two differ by at least 10 Ah. If both stored capacity records are invalid at start, this value serves as the full capacity."),
"param.remaining_capacity_setting": (
 "Angabe in Ah, keine Prozent. Beim Einzelschreiben setzt das BMS den laufenden Zähler auf den geschriebenen Wert, nur wenn er die Vollkapazität nicht übersteigt. Schreiben des ganzen Parameterblocks speichert nur das Feld. Sind beim Start beide gespeicherten Kapazitätsdatensätze ungültig, startet die Restkapazität mit diesem Wert.",
 "Given in Ah, not percent. A single write sets the running counter to the written value, only if it does not exceed the full capacity. Writing the whole parameter block only stores the field. If both stored capacity records are invalid at start, the remaining capacity starts from this value."),
"param.remaining_capacity_alarm": (
 "Meldung: Event 6 Bit 2 (Freigabe Funktionsschalter 5, Bit 2). Teil der SOC-Regel bei Zell-Unterspannungs-Warnung ((SOC-Warnung + SOC-Schutz) / 2). Als entladebezogene Warnung nimmt sie den Pack aus der Summe der Entladegrenze; liegen alle Packs darunter, meldet der Master DCL = 0, der Wechselrichter hört also schon hier auf zu entladen.",
 "Message: event 6 bit 2 (enable: function switch 5, bit 2). Part of the SOC rule at the cell low-voltage warning ((SOC warning + SOC protection) / 2). As a discharge-related warning it removes the pack from the discharge limit sum; if all packs are below it, the master reports DCL = 0, so the inverter already stops discharging here."),
"param.remaining_capacity_protection": (
 "Meldung: Event 6 Bit 3 (Freigabe Funktionsschalter 5, Bit 3). Verglichen wird mit dem auf ganze Prozent gerundeten SOC. Die Ladeaktivierung hebt den Schutz periodisch kurz auf.",
 "Message: event 6 bit 3 (enable: function switch 5, bit 3). The comparison uses the SOC rounded to whole percent. Charge activation briefly lifts the protection at intervals."),
"param.intermittent_recharge_below": (
 "Meldung: Event 6 Bit 1 (Freigabe Funktionsschalter 5, Bit 1). Fordert ein Pack Heizung an, hebt das die Ladegrenze 0 auf den Heizungs-Mindeststrom. Über diesem Wert driftet der SOC im Standby zusätzlich um 0,2 A.\nOriginal-Firmware: Der wartende Pack selbst sperrt seinen Lade-MOSFET nicht, nur der Master senkt die Ladegrenze. Patch-Firmware mit Hard-Stop öffnet den Lade-MOSFET des wartenden Packs (bei MAINT 1.6.3 per Funktionsschalter 0, Bit 6/7 wählbar).",
 "Message: event 6 bit 1 (enable: function switch 5, bit 1). If a pack requests heating, the charge limit 0 is raised to the heating minimum. Above this value the SOC additionally drifts by 0.2 A in standby.\nOriginal firmware: the waiting pack itself does not lock its charge MOSFET, only the master lowers the charge limit. Patched firmware with hard stop opens the waiting pack's charge MOSFET (selectable with function switch 0, bits 6/7 on MAINT 1.6.3)."),
"param.cycle_cumulative_capacity": (
 "Gezählt werden nur SOC-Rückgänge beim Entladen.",
 "Only SOC decreases while discharging count."),
# balancing and cell difference
"param.cell_difference_fault": (
 "Meldung: Event 1 Bit 4 (Fehler), nicht maskierbar: Die Firmware wertet sie immer aus. In der Original-Firmware hat Funktionsschalter 0, Bit 4 darauf keinen Einfluss; in MAINT 1.6.3 ist dieses Bit das erweiterte Aus und hat mit der Zelldifferenz ebenfalls nichts zu tun. Rückkehr: „Zelldifferenz-Fehler aus unter“.",
 "Message: event 1 bit 4 (fault), not maskable: the firmware always evaluates it. In the original firmware function switch 0, bit 4 has no influence on it; in MAINT 1.6.3 that bit is the extended off and has nothing to do with the cell difference either. Recovery: “Cell difference fault off below”."),
"param.static_balancing_time": (
 "Der Zähler läuft, solange Balancing freigegeben ist und nicht geladen wird; Laden setzt ihn zurück. Die Sperre meldet Event 7 Bit 2.",
 "The counter runs while balancing is enabled and the pack is not charging; charging resets it. The block reports event 7 bit 2."),
# pack and activation
"param.cells_in_series": (
 "Zellzahl in 0x42/0x44 und Faktor aller Pack-Spannungsregeln des SOC (Zellen × 3,30/3,37/3,40 V bzw. × Zell-Hochspannungs-Warnung). Bei höchstens 8 Zellen übernehmen die Temperaturfühler 3/4 den Wert von Fühler 2.",
 "Cell count in 0x42/0x44 and factor of all pack voltage rules of the SOC (cells × 3.30/3.37/3.40 V or × cell high-voltage warning). With 8 cells or fewer, temperature sensors 3/4 take the value of sensor 2."),
"param.charge_activation_window": (
 "In diesem Fenster löscht das BMS den Schutz jede Sekunde, damit die MOSFETs schließen (Event 7 Bit 4/5). Automatisch nur mit Funktionsschalter 6, Bit 4, einmal nach dem Einschalten mit Bit 5.",
 "Within this window the BMS clears the protection every second so the MOSFETs close (event 7 bit 4/5). Automatically only with function switch 6, bit 4, once after switching on with bit 5."),
"param.charge_activation_interval": (
 "Wiederholt wird, solange Zell-Unterspannungsschutz, SOC 0 % oder SOC-Schutz besteht. Die Einheit „When“ im Export bedeutet Stunden.",
 "It repeats as long as cell under-voltage protection, SOC 0 % or SOC protection persists. The unit “When” in the export means hours."),
"param.charge_activation_attempts": (
 "Der Zähler beginnt neu bei Laden mit mindestens 3 A für 5 s oder wenn die niedrigste Zelle über „Zell-Unterspannungsschutz aus“ steigt.",
 "The counter restarts on charging with at least 3 A for 5 s or when the lowest cell rises above “Cell under-voltage protection off”."),
"param.work_record_interval": (
 "Die Historie liegt im EEPROM und ist mit 0x4B auslesbar. Zusätzlich schreibt das BMS sofort einen Eintrag bei jeder Änderung von Systemstatus oder Meldungen; der Zähler beginnt danach von vorn.",
 "The history is kept in EEPROM and can be read with 0x4B. In addition the BMS writes an entry at once on every change of system status or messages; the counter then starts over."),
"param.standby_shutdown_delay": (
 "Bedingungen: durchgehend Standby ohne Fremdspannung am Port und höchste Zelle unter „Zell-Überspannungsschutz aus“. Dann 60 s Laden verboten, danach Aus, Zustand im internen Flash sichern und Versorgung abschalten (Letzteres wahrscheinlich). Ein Ladegerät (Port über „Pack-Überspannungsschutz aus“) oder der Taster brechen ab. Die Einheit „When“ bedeutet Stunden.",
 "Conditions: continuous standby without external voltage at the port and the highest cell below “Cell over-voltage protection off”. Then 60 s charging forbidden, then off, state saved to internal flash and supply switched off (the latter probable). A charger (port above “Pack over-voltage protection off”) or the button aborts. The unit “When” means hours."),
"param.connection_fault_impedance": (
 "Der Ausgangs-Verbindungsfehler (Event 6 Bit 6) kommt von einem Hardware-Eingang (mindestens 5 s aktiv), nur mit Funktionsschalter 5, Bit 6.",
 "The output connection fault (event 6 bit 6) comes from a hardware input (active for at least 5 s), only with function switch 5, bit 6."),
"param.compensation_1_resistance": (
 "Intern in 0,1 mΩ. Korrektur in mV = Strom [A] × Widerstand [mΩ], abgezogen von genau der eingestellten Zelle: Beim Laden wird sie kleiner, beim Entladen größer angezeigt.",
 "Internally in 0.1 mΩ. Correction in mV = current [A] × resistance [mΩ], subtracted from exactly the set cell: it reads lower while charging and higher while discharging."),
"param.compensation_2_resistance": (
 "Wie Kompensation 1: Korrektur in mV = Strom [A] × Widerstand [mΩ], intern in 0,1 mΩ.",
 "Like compensation 1: correction in mV = current [A] × resistance [mΩ], internally in 0.1 mΩ."),
}

# Per function switch group (0–7): what the group as a whole does. id "group.<n>": (de, en).
GROUP_DETAILS = {
"group.0": (
 "Keine Freigabemaske. Original-Firmware: Nur Bit 1 wirkt (Prüfung der Temperaturfühler), die übrigen Bits werden nur gespeichert und gemeldet; Tasterereignis und Zelldifferenz-Fehler (Event 1) wertet die Firmware immer aus. Patch-Firmware: ab MAINT 1.5.3 sind Bit 0, 2 und 3 Freigaben für Schreiben, Wartung sowie Ein/Aus, Neustart und Uhr über die CAN-Buchse; ab MAINT 1.6.3 kommen Bit 4 (erweitertes Aus), Bit 5 (Status der Packsperre) und Bit 6/7 (Hard-Stop-Auswahl) hinzu.",
 "Not an enable mask. Original firmware: only bit 1 has an effect (temperature sensor check), the other bits are only stored and reported; the button event and the cell difference fault (event 1) are always evaluated. Patched firmware: from MAINT 1.5.3 bits 0, 2 and 3 release writing, maintenance and on/off, restart and clock via the CAN socket; MAINT 1.6.3 adds bit 4 (extended off), bit 5 (pack cutoff status) and bits 6/7 (hard stop choice)."),
"group.1": (
 "Freigabemaske für Event 2 (Zell- und Pack-Spannung). Die Firmware verknüpft das Event direkt nach der Auswertung mit dieser Maske, bevor Ladesperre, Entladesperre, SOC, Ladegrenzen oder Protokolle es lesen; ein Bit auf 0 schaltet die Meldung und alle Reaktionen ab, die am Event hängen. Regeln, die den Parameter oder die Spannung direkt lesen, bleiben aktiv: die SOC-Korrekturen mit Zell-Hochspannungs- und Zell-Unterspannungs-Warnung (100 %-Regel, Sprung bei tiefer Zelle) laufen weiter.",
 "Enable mask for event 2 (cell and pack voltage). The firmware ANDs the event with this mask right after evaluating it, before the charge lock, discharge lock, SOC, limits or protocols read it; a bit at 0 switches off the message and every reaction that depends on the event. Rules that read the parameter or the voltage directly stay active: the SOC corrections with the cell high- and low-voltage warning (100 % rule, jump at a low cell) keep working."),
"group.2": (
 "Freigabemaske für Event 3 (Zelltemperatur beim Laden und Entladen, Warnung und Schutz). Ein Bit auf 0 schaltet die Meldung und alle Reaktionen ab, die am Event hängen; Regeln, die einen Parameter oder Messwert direkt lesen, bleiben aktiv.",
 "Enable mask for event 3 (cell temperature while charging and discharging, warning and protection). A bit at 0 switches off the message and every reaction that depends on the event; rules that read a parameter or measured value directly stay active."),
"group.3": (
 "Freigabemaske für Event 4: Umgebungs- und MOSFET-Temperatur (Bit 0–5), dazu die Zellheizung (Bit 6) und die Schwerfehler-Meldung (Bit 7). Ein Bit auf 0 schaltet die Meldung und alle Reaktionen ab, die am Event hängen; Regeln, die einen Parameter oder Messwert direkt lesen, bleiben aktiv.",
 "Enable mask for event 4: ambient and MOSFET temperature (bits 0–5), plus cell heating (bit 6) and the severe fault message (bit 7). A bit at 0 switches off the message and every reaction that depends on the event; rules that read a parameter or measured value directly stay active."),
"group.4": (
 "Freigabemaske für Event 5 (Strom): Lade- und Entlade-Überstrom, Kurzzeit-Überstrom, Kurzschluss-Erkennung sowie die Sperren nach wiederholten Kurzzeit- bzw. Kurzschluss-Ereignissen. Ein Bit auf 0 schaltet die Meldung und alle Reaktionen ab, die am Event hängen; Regeln, die einen Parameter oder Messwert direkt lesen, bleiben aktiv.",
 "Enable mask for event 5 (current): charge and discharge over-current, transient over-current, short-circuit detection and the locks after repeated transient or short-circuit events. A bit at 0 switches off the message and every reaction that depends on the event; rules that read a parameter or measured value directly stay active."),
"group.5": (
 "Freigabemaske für Event 6 (Sonstiges): Ladegeräte-Überspannung, intermittierende Nachladung, SOC-Warnung und -Schutz, Ladesperre bei tiefer Zelle, Ausgangs-Verbindungsfehler, Rückmeldung des Strombegrenzers. Bit 5 (Verpolung) setzt kein Code. Ein Bit auf 0 schaltet die Meldung und alle Reaktionen ab, die am Event hängen; Regeln, die einen Parameter oder Messwert direkt lesen, bleiben aktiv.",
 "Enable mask for event 6 (miscellaneous): charger over-voltage, intermittent recharge, SOC warning and protection, charge block for a deep cell, output connection fault, current limiter feedback. Bit 5 (reverse polarity) is set by no code. A bit at 0 switches off the message and every reaction that depends on the event; rules that read a parameter or measured value directly stay active."),
"group.6": (
 "Funktionsschalter, keine Maske: Balancing (Bit 0–3), Ladeaktivierung automatisch und manuell (Bit 4/5) sowie aktive und passive Strombegrenzung über den Hardware-Strombegrenzer (Bit 6/7).",
 "Function switches, not a mask: balancing (bits 0–3), automatic and manual charge activation (bits 4/5) and active and passive limiting through the hardware current limiter (bits 6/7)."),
"group.7": (
 "Funktionsschalter, keine Maske: externer Ein/Aus-Schalter, Standby-Selbstabschaltung, Binärprotokoll an RS232/TTL und sein Format, automatische Adressvergabe samt Vermeidung von Adresse 0 (Bit 5/6) und der Modus der Ladegrenze (Bit 7). Bit 2 hat keine Funktion.",
 "Function switches, not a mask: external on/off switch, standby shutdown, binary protocol on RS232/TTL and its format, automatic addressing including avoiding address 0 (bits 5/6) and the charge limit mode (bit 7). Bit 2 has no function."),
}
