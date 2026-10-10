---
title: Verbindung
short: So verbindet sich die App mit den Batterien: per USB-RS485-Adapter oder per RS485-zu-Ethernet-Gateway (TCP).
related: topic.buses
---
- **Netzwerk (TCP):** Adresse des Gateways im Format `Host:Port`, z. B. `192.168.1.10:4196`. Die Baudrate stellst du am Gateway ein (CAN-Buchse 9600, RS485-1/2 19200).
- **USB/Seriell:** Port des Adapters wählen; die Baudrate ergibt sich aus dem gewählten Bus.
- Nach dem Verbinden sucht die App die Adressen 0–15 ab und fragt danach alle gefundenen Packs alle 2 s ab.
- Kommt **60 s** lang keine gültige Antwort, gilt die Verbindung als getrennt („Verbindung verloren“). Die App verbindet dann alle 30 s automatisch neu, bis wieder Packs antworten.
- Nur ein Programm darf gleichzeitig auf dem Bus fragen. Läuft z. B. das Home-Assistant-Add-on am selben Gateway, kollidieren die Anfragen.
