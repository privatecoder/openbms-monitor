---
title: Systemwerte (Master)
short: Werte, die der Master über alle Packs berechnet und genau so an den Wechselrichter meldet.
related: value.charge_limits, topic.buses
---
Gelesen per Modbus vom Master (Adresse 0), nur am Bus der CAN-Buchse und ab Firmware 16.06.04. Enthalten sind Summenstrom, mittlere Spannung, System-SOC, Lade- und Entladegrenzen, Freigaben sowie höchste und niedrigste Zellspannung bzw. -temperatur mit Pack-Nummer.
