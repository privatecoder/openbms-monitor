---
title: Guide: first setup
short: From a new installation to the confirmed bank: create the installation, enter its ports, connect and assign the bank's packs.
related: topic.installations, topic.connection, topic.wiring, topic.buses
---
Everything the app stores about a bank belongs to an installation. The steps lead once through the setup; the conditions are in the linked topics.

1. Create an installation: in Installations (or in the sidebar under “Installation”) “New installation …”, enter a name and “Create”. Expected: the new installation is chosen in the sidebar. → [Installations](#topic.installations)
2. Enter its ports: in Installations open the card, at “Ports” press “Change”, then the CAN socket (RS485) and, if packs are to be flashed, the RS485-1/2 bus, each over the network (gateway `host:port`) or USB; “Save”. Without an entry the app links the port at the first connection. → [The two RS485 buses](#topic.buses)
3. On the same card choose “Inverter connected via”. Without it the app sends nothing for this installation. → [Inverter connection](#topic.wiring)
4. At “Packs polled over RS485” keep “search” or set a count or a fixed address; it applies from the next connection. → [Connection](#topic.connection)
5. Connect: in the sidebar under “Connection” the installation is chosen; choose the bus (the app fills in the stored port), then “Connect”. Expected: “Connected” and the packs found. If nothing answers, the app says what it heard. → [Connection](#topic.connection)
6. Confirm the bank, depending on the line in the sidebar: → [Installations](#topic.installations)
   - **“This installation does not know … the boards … yet”**
     1. If they are this bank's packs, “Yes, take over”. Expected: recording starts, Installations lists the boards per address.
   - **Red line: “… belongs to installation “B””**
     1. If another bank is connected here, “Disconnect” or “Connect with “B””; only for a board that really moved “Take over into … anyway”.
   - **“Pack(s) without a serial number”**
     1. The app recognises the bank by the BMS serial number (String 2); the battery serial number (String 1) does not replace it. Under Parameters → Serial numbers set a unique BMS serial number per pack, then connect again. Writing serial numbers goes via the RS485-1/2 bus: a connection at the CAN socket alone is not enough for it. → [Writing parameters and serial numbers](#topic.paramWrite)
7. Then, as needed, assign the packs their cell type under Cells; with it the app compares the values with the datasheet. → [Cell types](#topic.cellAssign)
