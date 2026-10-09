---
title: System values (master)
short: Values the master computes over all packs and reports exactly like this to the inverter.
related: value.charge_limits, topic.buses
---
Read via Modbus from the master (address 0), only on the CAN socket bus and from firmware 16.06.04. Includes total current, average voltage, system SOC, charge and discharge limits, enable flags, and highest/lowest cell voltage and temperature with pack number.
