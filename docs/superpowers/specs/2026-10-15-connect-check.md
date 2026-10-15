# Check the connection when connecting

Status: r2, 2026-10-15 (r1 reviewed: `.orch/reviewer-codex-6-sol-connect-check-r1.md` in the knowledge base).

## Why

„Verbinden“ shows „Verbunden“ as soon as the port opens. With a fixed pack count the app starts polling at once and only later, maybe, says „Kein Pack antwortet“; the status stays green until the 10-s data timeout, then it retries forever. A wrong USB port, the adapter on the CAN socket while „RS485-1/2“ is chosen (or the other way round), or a gateway port with the wrong baud rate all look like „connected, no data“.

## Design

### 1. A manual connect asks the packs first

After the port opened and before the app counts as connected (no `onConnected`, no polling, no recording row), it asks once:

- **CAN socket:** the master's Modbus system values (as today) or 0x51 of any pack address to be polled (fixed count, from the master, else 0..15), stop at the first answer.
- **RS485-1/2:** 0x51 of the addresses to be polled (fixed count, else 0..15), stop at the first answer. A master with slaves does not answer here, its slaves do; a lone master at DIP 0 answers.

One answer is enough: connected, then everything as today (polling, device query of all packs, serials). The status line then says what answered: „Pack 00 antwortet über RS485-1/2.“; when the device query of a fixed count finishes with gaps: „2 von 3 Packs antworten, keine Antwort von 02.“

Backend: `connect(endpoint, installation, addresses)` runs the check itself after opening (0x51 with 300 ms per address, CAN socket first the Modbus system read; stop at the first answer). Only on success it links the endpoint to the installation and starts the recorder session (today both happen before any answer). No answer → it closes the port and returns the error `no-answer` with `noise` (bytes arrived that were no valid frame, typical for a wrong baud rate). A new command `probe_other_bus(path, bus)` opens the USB port at the other bus's baud rate, asks the same way, closes it and returns `{ answered: Option<u8>, noise }`.

### 2. No answer: say why, offer the other bus

The port is closed again, the status stays „Nicht verbunden“, the message is red and stays until the next action:

- **USB port:** the app reopens the same port with the other bus's baud rate and asks the same way (CAN socket: Modbus + 0x51 of 0..15; RS485-1/2: 0x51 of 0..15; stop at first answer, port closed afterwards).
  - Answer there: „Keine Antwort über RS485-1/2 (19200 Baud). An der CAN-Buchse (9600 Baud) antwortet Pack 00.“ + button **„Mit CAN-Buchse verbinden“** (switches the bus and connects).
  - No answer either: „An /dev/cu.… antwortet kein Pack, weder über RS485-1/2 noch über die CAN-Buchse. Richtiger Port? Kabel an A/B? Pack eingeschaltet?“ With `noise`: „Es kommen Daten an, aber nicht lesbar“ instead of the first sentence.
- **Gateway (TCP):** the baud rate is set in the gateway, the app cannot try the other one. „Über 192.168.1.10:4196 antwortet kein Pack auf RS485-1/2 (19200 Baud). Prüfen: Port des Gateways, dort eingestellte Baudrate, Kabel.“ With `noise`: „Es kommen Daten an, aber nicht lesbar: die Baudrate des Gateways passt nicht zum Bus (CAN-Buchse 9600, RS485-1/2 19200).“

### 3. Unchanged

Automatic reconnects (lost connection) keep today's path without the check of the other bus: they retry with the last settings. Demo mode unchanged. „Nur ansehen“ the same as with an installation.

## Harm check

Only reads (0x51, Modbus read) are sent, also at the other baud rate. Nothing is linked or recorded for a connection that did not answer (today a wrong port gets linked to the installation).

## Review r1 (taken over)

- **Reachability is checked on all addresses 0..15** (the ones to be polled first, stop at the first answer), not only on the fixed count: a lone slave at DIP 11 with „1 Pack“ is reachable. The fixed count is used only for polling. No broadcast `FF` (several packs would answer at once).
- **The other-baud probe names the baud rate, not the socket.** Firmware 16.04.06 `_9600` runs RS485-1/2 at 9600 baud. An answer at 9600 is told apart with one read-only 0x47 at the answering address: answered → RS485-1/2 at 9600 baud (old firmware), no one-click switch (the app's pack bus runs at 19200); not answered → CAN socket, one-click switch offered. An answer at 19200 is RS485-1/2 (the CAN socket never runs at 19200).
- **„No answer“ is a state, not a proof of a wrong port.** A master with a configured slave count > 0 polls its slaves and never answers 0x51 on RS485-1/2, even when no slave answers; intra-pack traffic (0x5A) seen during the check is reported as „Am Bus fragt ein Master seine Slaves ab, aber kein Pack antwortet“. A pack in the bootloader or within ~5 s after a restart does not answer either; the message names these. The firmware view stays reachable without a monitor connection (it is today).
- The check itself occupies the bus for up to 16 × 300 ms; on a pack bus with a polling master this may cost a few poll rounds (no lasting effect). An external updater running on the same bus at the same time can be disturbed by any transmission; the app's own flashing holds the port and is not affected.
