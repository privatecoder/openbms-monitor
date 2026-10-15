# Right bank before writing

Status: r1 draft, 2026-10-15.

## Why

`connect` checks that some pack answers, then links the endpoint to the installation chosen in the sidebar. Nothing proves that the packs behind the port belong to that installation. When a USB adapter or a gateway is moved to another bank while the old installation stays chosen, the app records the other bank under the wrong installation and can write parameters, clock, CAN protocol, serials or control commands of bank A to bank B (Codex review `.orch/reviewer-codex-6-sol-pack-control-scope-r1.md`, finding 4). Worse, the serial read after connecting calls `board_seen`, which takes the other bank's boards as board swaps of this installation: the installation's board history is then wrong too.

The flash path already compares the master's BMS serial (0xA4 at address 0) with the installation (`maint::check_bank`). That does not help here: on RS485-1/2 a master with slaves never answers, and the CAN socket shows no BMS serial at all.

## Identity

A board is known by its BMS serial (String 2, 0xA4, `boards.bms_serial` plus `aliases`). Placeholder and empty values (`EMU110110S-XXXX…`, nothing set) identify nothing. For a pack at address X read with BMS serial S, against installation I:

- **match:** S is I's current board at X, or an earlier board of I at X (it becomes current again, as today).
- **first:** I has no board at X and S is unknown to every installation: a new place, recorded as today.
- **swap:** I has a different current board at X and S is unknown to every installation: a board swap, recorded as today, with the existing notice.
- **foreign:** S is a board (any address) of another installation and not of I. Archived installations count.
- **unchecked:** no real S (placeholder, empty, no answer, CAN socket bus).

## Design

1. **Serial read after connecting** (`serial_numbers`, the sidebar reads every pack): classify before `board_seen`. A **foreign** pack is not recorded (`board_seen` not called), and the read returns `identity: {kind: "foreign", installation: {id, name}}`. The sidebar then shows one red line for the connection: „Pack 02 gehört zur Anlage „B“. Ist hier eine andere Bank angeschlossen?“ with „Trennen“ and „Mit Anlage „B“ verbinden“ (disconnect, choose B, connect). Recording keeps running (it is only reading, and the user decides), but every write to that connection is refused (point 2). A **swap** is recorded as today and returned as `identity: {kind: "swap"}`.
2. **Before every write to a pack** (all commands under the pack reservation that send to the pack: parameters, clock, CAN protocol, calibration, serial, the five control commands; not the history read): inside the reservation and the same link, read 0xA4 of the target pack (one frame) and classify.
   - **foreign** → refuse `{"kind":"bank","state":"foreign","address":X,"installation":{…}}`: „An Adresse 02 hängt ein Pack der Anlage „B“. Nichts geschrieben.“
   - **swap not yet confirmed on this connection** → refuse `{"kind":"bank","state":"swap","address":X,"serial":S}`: „An Adresse 02 hängt eine Platine, die diese Anlage noch nicht kennt (S). Gehört sie zu dieser Anlage?“ with „Ja, Platine gehört dazu“ (records the confirmation for this connection generation and address, then the user repeats the action). One confirmation per pack and connection, not per write.
   - **match, first, unchecked** → write. Unchecked shows a muted note under the result: „Anlage nicht geprüft (Platine ohne Seriennummer)“.
   - A serial write (0xA5) itself changes S: its check uses S before the write; the app's own new value is recorded by `board_written` as today.
3. **CAN socket bus:** writes do not exist there. The master's battery serial (Modbus, first 22 characters) is compared with the battery serials of the installation's boards at address 0 and of other installations: equal to another installation's and none of I's → the same red line as in point 1 (hint only; battery serials can be changed and are informational).
4. A connection with a foreign pack keeps `foreign` until disconnect; the sidebar line stays.

## Out of scope

Proving the bank when every pack has a placeholder serial (the app cannot tell banks apart then; the help says so and suggests setting serials, which the app can do).

## Questions for review

1. Is the per-write 0xA4 read sound under the reservation and `with_link`, or can it read another pack than the one written (address echo, gateway)?
2. `foreign` across archived installations: harmful when a board really moved from an archived bank to this one (refusal until the user deletes or merges)? Is the swap confirmation the better path there?
3. Recording continues on a foreign connection: does anything else than the recorder write data under the wrong installation (parameter snapshots from reads, `board_seen` from other paths, BMS history saves)?
