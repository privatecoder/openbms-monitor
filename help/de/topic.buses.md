---
title: Die zwei RS485-Busse
short: Die Packs haben zwei getrennte RS485-Busse. Welcher Bus angeschlossen ist, bestimmt, was die App lesen und ändern kann.
related: topic.connection, value.system, topic.access
---
- **CAN-Buchse (RS485)** (Adern Pin 1/2/3 bzw. 6/7/8), 9600 Baud: Werte und Meldungen **aller** Packs unter ihrer DIP-Adresse und die Systemwerte des Masters (Modbus). Jedes Pack antwortet dort mit seiner eigenen Firmware; mit der Originalfirmware gibt es dort keine Steuerbefehle.
- **RS485-1/2-Bus**, 19200 Baud: der volle Befehlssatz der Originalfirmware. Der **Master** (DIP 0 mit Slaves) antwortet hier **nie**, solange er selbst seine Slaves abfragt.

Für die Überwachung mehrerer Packs ist der Bus der CAN-Buchse ideal. Wo sich die Busse unterscheiden (jeder Weg unter „Zugriffswege“):

- **Parameter lesen**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4.
  - UART2 · Display/Bluetooth: Originalfirmware genügt; Group7.Bit3 (Displayport aktiv) = 1; nur die Platine am Displayport; die App nutzt diesen Weg nicht.
- **Parameter und Schalter schreiben**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4; Group0.Bit0 (UART1-Schreibfreigabe) = 1.
  - UART2 · Display/Bluetooth: Originalfirmware genügt; Group7.Bit3 (Displayport aktiv) = 1; nur die Platine am Displayport; die App nutzt diesen Weg nicht.
- **Interne Historie lesen**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4 mit Lesefähigkeit Historie (Bit 0 der Firmwarekennung).
- **Kalibrierung lesen**
  - UART0 · RS485-1/2: EMU1101-EXT ab Light 1.5.4 mit Lesefähigkeit Kalibrierseiten (Bit 1 der Firmwarekennung); Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4 mit Lesefähigkeit Kalibrierseiten (Bit 1 der Firmwarekennung).
- **Gespeicherten Stand und Energiezähler lesen**
  - UART0 · RS485-1/2: EMU1101-EXT ab Light 1.5.4 mit Lesefähigkeit gespeicherter Stand oder Energiezähler (Bit 2 oder 3 der Firmwarekennung); Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4 mit Lesefähigkeit gespeicherter Stand oder Energiezähler (Bit 2 oder 3 der Firmwarekennung).
- **Speicherdiagnose lesen**
  - UART0 · RS485-1/2: EMU1101-EXT ab Light 1.5.4 mit Speicherdiagnose (Imageflag Bit 5); Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4 mit Speicherdiagnose (Imageflag Bit 5).
- **Uhr stellen**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4 mit Power/RTC (Imageflag Bit 1); Freigabe in Group0.Bit3 (Power/RTC), Wert je Build (siehe Funktionsschalter 0).
- **Neustart**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4 mit Power/RTC (Imageflag Bit 1); Freigabe in Group0.Bit3 (Power/RTC), Wert je Build (siehe Funktionsschalter 0).
- **Ein/Aus**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4 mit Power/RTC (Imageflag Bit 1); Freigabe in Group0.Bit3 (Power/RTC), Wert je Build (siehe Funktionsschalter 0).
- **Seriennummern schreiben**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
- **CAN-Protokoll setzen**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART2 · Display/Bluetooth: Originalfirmware genügt; Group7.Bit3 (Displayport aktiv) = 1; nur die Platine am Displayport; die App nutzt diesen Weg nicht.
- **Kalibrieren**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
- **Heizung erzwingen**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
- **Restladung setzen**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.
- **Historie und Zähler löschen**
  - UART0 · RS485-1/2: Originalfirmware genügt; Hinweis: zuverlässig nur ohne pollenden Master.

**Seriennummern:** Jedes BMS speichert zwei: die **Batterie-SN** (im BatteryMonitor „Battery_SN“) und die **BMS-SN** der Platine („BMS_SN“). Welcher Weg welche liefert:

- **Seriennummern lesen**
  - UART0 · RS485-1/2: Originalfirmware genügt; liefert Batterie- und BMS-Seriennummer, je bis 30 Zeichen; Hinweis: zuverlässig nur ohne pollenden Master.
  - UART1 · CAN-Buchse: EMU1101-EXT ab Light 1.5.4; liefert Batterie- und BMS-Seriennummer, je bis 30 Zeichen.
  - UART1 · CAN-Buchse: Originalfirmware ab 16.06.11; nur am Master, Adresse 0; liefert nur die Batterie-Seriennummer (String 1), die ersten 22 Zeichen.

„Nicht gesetzt (Platzhalter)“ heißt: Es steht noch der Werkseintrag `EMU110110S-XXXX-…` darin, niemand hat eine eigene Nummer vergeben. Die App liest die Nummern einmal nach dem Verbinden und ändert von sich aus nichts; das Schreiben ist eine eigene Aktion unter Parameter (Wartungsfunktionen).
