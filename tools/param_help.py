"""Help for the BMS parameters (0x47) and function switches, imported by gen_help.py.

Effects as in the stock firmware 16.06.12/16.06.13. id: (de title, de short, en title, en short)."""

PARAMS = {
# cell voltage
"param.cell_high_voltage_alarm": ("Zell-Hochspannungs-Warnung", "Warnung, wenn eine Zelle diesen Wert erreicht. Haben alle Packs eine Lade-Warnung, senkt der Master die Ladegrenze auf 10 A gesamt. Außerdem Grundlage der 100-%-Regel des SOC (Zellen × diesem Wert, begrenzt auf 3,45–3,50 V je Zelle).",
 "Cell high-voltage warning", "Warning when a cell reaches this value. If all packs have a charge warning, the master lowers the charge limit to 10 A in total. Also the basis of the SOC's 100 % rule (cells × this value, limited to 3.45–3.50 V per cell)."),
"param.cell_high_voltage_recovery": ("Zell-Hochspannungs-Warnung aus", "Die Warnung endet, wenn die höchste Zelle wieder unter diesen Wert fällt.",
 "Cell high-voltage warning off", "The warning ends when the highest cell falls below this value again."),
"param.cell_low_voltage_alarm": ("Zell-Unterspannungs-Warnung", "Warnung, wenn eine Zelle diesen Wert erreicht. Setzt den SOC auf 10 %, wenn er noch höher steht; darüber hält der SOC beim Entladen bei 6 %.",
 "Cell low-voltage warning", "Warning when a cell reaches this value. Sets the SOC to 10 % if it is still higher; above it, the SOC holds at 6 % while discharging."),
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
# message and its reaction off completely. Bits without an entry have no function.
SWITCHES = {
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
"switch.7.7": ("Ladegrenze: Warnung − 10 A", "An: Ladegrenze je Pack = Lade-Überstrom-Warnung − 10 A. Aus: die Hälfte davon.",
 "Charge limit: warning − 10 A", "On: charge limit per pack = charge over-current warning − 10 A. Off: half of it."),
})
