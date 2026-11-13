# Ports of an installation: one place, used by the sidebar, RS485-1/2 over the network

Status: r2, 2026-10-16 (r1 reviewed: `.orch/reviewer-codex-6-sol-installation-ports-r1.md` in the knowledge base; findings marked „review n“).

## Why

An installation already stores a CAN socket endpoint (`control`, serial or TCP) and pack-bus endpoints (`endpoints`, keys `serial:…`/`tcp:…`, „the first serial one is the RS485-1/2 port“). They can only be edited in Firmware → „Pack-Bus“ → „Einrichten“ (`MaintSetup`), because they were first used for maintenance and flashing. The sidebar connection does not use them: it keeps one global address/port, and since today the last working connection per installation (`connCheck.ts` `lastConn`) with a fallback to the stored ports (`storedConn`). The RS485-1/2 port can only be a USB port in the setup (`PackKindSelect` disables „Netzwerk“) and the backend opens it only as serial (`maint::open_pack_for`, `firmware::open_port`), although the monitor connection over TCP on the pack bus works.

## Design

1. **Ports in the Installations view.** Each installation card gets „Anschlüsse“ with two rows, each with kind (Netzwerk / USB) and address or port:
   - **CAN-Buchse (9600 Baud)** → `control`.
   - **RS485-1/2 (19200 Baud)** → the pack-bus port: the first pack-bus endpoint (`tcp:` or `serial:`), other stored endpoints kept as today (`setupEndpoints` generalised to both kinds).
   The editor is the existing `MaintSetup` form moved there (same save path `maint_save_installation`, same refusals). Firmware → „Pack-Bus“ shows the ports read-only with a link „In Anlagen ändern“.
2. **The sidebar uses them.** Choosing the bus fills kind and address/port from the installation: „CAN-Buchse“ → `control`, „RS485-1/2“ → the pack-bus port, if stored. Switching the installation (not connected): the remembered last connection wins (`lastConn`), else the stored port of the remembered/current bus, else the CAN socket. A user edit of the address field is kept until the bus or installation changes.
3. **Offer to save at connect.** After a successful connect with an installation whose stored port for that bus is empty or different: a line under the connection status „Als RS485-1/2-Anschluss von „A“ speichern?“ / „… als CAN-Buchse …“ with „Speichern“ and „Nein“ (no repeat for the same endpoint in this session). RS485-1/2 endpoints are still linked automatically at connect as today (bank decided); the offer covers the CAN socket and replacing a stored RS485-1/2 port. Never saved silently over an existing one.
4. **RS485-1/2 over the network, released for testing.** The pack-bus port may be TCP everywhere:
   - setup: „Netzwerk“ selectable for RS485-1/2, with the note „Über ein Netzwerk-Gateway ungetestet: der Gateway-Kanal muss auf 19200 Baud stehen.“;
   - backend: `open_pack_for` and `firmware::open_port` take an endpoint key (`serial:…`/`tcp:…`) and open `Endpoint::Tcp` for `tcp:`; `firmware_inspect`/`firmware_start`/maintenance commands receive the key instead of a bare path (the serial case unchanged); `must_stop_monitor` compares keys, so a monitor on the same TCP endpoint is stopped as for serial;
   - flashing over TCP: `openbms-flash` already handles split TCP answers; before `firmware_start` over TCP the confirmation line adds „Update über Netzwerk-Gateway: ungetestet“. No other change to the flash flow.

## r2 rulings (review)

- **One selection rule (review 5):** the RS485-1/2 port is the first endpoint in stored order that is not the CAN socket's key, of any kind (`tcp:` or `serial:`). One helper in the frontend (`packPortOf` generalised) and its twin in the backend; used by the editor, the sidebar, `storedConn`, Firmware, maintenance and the pending display. Tested on a mixed, ordered list.
- **One canonical key (review 2):** the endpoint key (`tcp:<addr lowercased>` / `serial:<path>`, `maint_store::endpoint_key`) is formed once where the user's choice enters (editor, sidebar, Firmware/maintenance command arguments) and passed unchanged through opening, `bootloader_pending` (`pending_on`, `add_pending`, `clear_answered`), saving, resolving and UI comparison (`pendingHere`, `pendingBlock`). Test: a TCP flash fails after erase; another pack on the same key is refused.
- **No silent saving at connect (review 1):** `connect`/`bank::settle` link an endpoint automatically only where the installation has **no** stored port of that bus (first connect). A CAN-socket connection is never entered as a pack-bus endpoint; it goes to `control` only when `control` is empty. In every other case nothing is stored and the UI shows the offer (point 3); only „Speichern“ stores.
- **No replacing a port in use (review 3):** `maint_save_installation` takes the bus reservation (`reserve_bus`, as `connect`) and refuses to change `control` while a master marker of that installation is set (maintenance not ended), and to change or drop the RS485-1/2 port while a `bootloader_pending` entry exists on it, with the reason („Wartung erst beenden“, „Abgebrochenes Update an … erst abschließen“).
- **Sidebar: stored ports win (review 4):** on installation or bus change the sidebar puts in the installation's stored port for the chosen bus; the remembered last connection supplies only the bus and the pack count, and its address/port only where the installation has no stored port for that bus. Without either, the USB field is emptied (no port of another installation stays). The `keepPort` rule goes. „Nein“ to the offer changes nothing stored.

## UI fixes in the same go (user test)

- Firmware → „Pack-Bus“: while the editor is open, the lines „RS485-1/2 nicht eingerichtet“ and the bus status box are hidden; otherwise one line „Kein RS485-1/2-Anschluss für <Anlage>: unter Anlagen einrichten“ replaces both.
- Port lists (sidebar, editor, Firmware) leave out macOS system ports that are never an RS485 adapter: `cu.debug-console`, `cu.Bluetooth-Incoming-Port`, `cu.wlan-debug` (and their `tty.` twins).
- The pack search on the pack bus („Neu suchen“) gets „Abbrechen“ (stops before the next address).

## Out of scope

Two simultaneous connections (CAN socket and RS485-1/2 at once) in the sidebar; changing the gateway's baud rate from the app.

## Review questions

1. Can moving the editor or generalising `setupEndpoints`/`packPortOf` lose or misassign stored endpoints (an endpoint shared by several installations, the CAN adapter chosen as RS485-1/2, `bank.rs` linking at connect)?
2. Anything in the TCP pack-bus path for maintenance/flash that can harm (monitor not stopped on the same gateway port, a TCP read of 0 bytes = closed mid-flash, timing of the bootloader window over the network)?
