# Right bank before writing

Status: r2 implemented (branch feat/bank-identity), 2026-10-15 (r1 reviewed: `.orch/reviewer-codex-6-sol-bank-identity-r1.md` in the knowledge base).

## Why

`connect` checks that some pack answers, then links the endpoint to the installation chosen in the sidebar. Nothing proves that the packs behind the port belong to that installation. When a USB adapter or a gateway is moved to another bank while the old installation stays chosen, the app records the other bank under the wrong installation and can write parameters, clock, CAN protocol, serials or control commands of bank A to bank B (Codex review `.orch/reviewer-codex-6-sol-pack-control-scope-r1.md`, finding 4). Worse, the serial read after connecting calls `board_seen`, which takes the other bank's boards as board swaps of this installation: the installation's board history is then wrong too.

The flash path already compares the master's BMS serial (0xA4 at address 0) with the installation (`maint::check_bank`). That does not help here: on RS485-1/2 a master with slaves never answers, and the CAN socket shows no BMS serial at all.

## Identity (r2, after review `.orch/reviewer-codex-6-sol-bank-identity-r1.md`)

A board is known by its BMS serial (String 2, 0xA4, `boards.bms_serial` plus `aliases`; at address 0 also `master_serial`). Placeholder and empty values identify nothing. A pack at address X read with BMS serial S, against installation I, is one of:

- **known:** S is a board of I at X (current or earlier; an earlier one becomes current as today), or at X = 0 S equals `I.master_serial`.
- **unconfirmed:** everything else with a real S. Its kind decides only the text:
  - *new:* S unknown to every installation (a new place, a swap, or a new installation's first connect),
  - *moved:* S is a board of I at another address (DIP changed),
  - *foreign:* S is a board or the master serial of another installation, archived ones included.
  A board that is known at X in I and also elsewhere (e.g. after importing a copy of an installation) counts as known (ruling: copies must not block; the harm needs two banks with the same written serial).
- **unchecked:** a valid answer with an empty or placeholder S. Writes go ahead with the note „Anlage nicht geprüft (Platine ohne Seriennummer)“.
- **failed:** no answer, invalid answer, I/O or store error. Writes are refused („Anlage nicht geprüft: … Nichts geschrieben.“); this never counts as unchecked.

## Design (r2)

1. **Nothing is recorded for an unconfirmed board until the user confirms it.** `serial_numbers` classifies before `board_seen` and calls it only for *known* boards (and `unchecked` as today: it records nothing). It returns `identity: {kind, address, serial, installation?}`.
2. **One line in the sidebar per connection** lists the unconfirmed packs after the serial read, with one button:
   - only *new*/*moved*: „Diese Anlage kennt die Platinen an 00, 03 noch nicht (neu bzw. umgesteckt). Gehören sie zu „I“?“ → „Ja, übernehmen“ records them with `board_seen` (moved ones as today's board at the new place, history kept);
   - any *foreign*: red, „Pack 02 gehört zur Anlage „B“. Ist hier eine andere Bank angeschlossen?“ → „Trennen“, „Mit „B“ verbinden“, and smaller „Trotzdem zu „I“ übernehmen“ (a board really moved from B, also archived: recorded in I, B keeps its history).
   One confirmation covers all listed packs of this connection (a new installation with 12 packs is one click, not twelve).
3. **Before every write** (parameters, clock, CAN protocol, calibration, serial, the five control commands): inside the reservation and `with_link`, **first** read 0xA4 of the target pack, before any other frame or snapshot of that action (in `write_parameters` before the fresh 0x47). known/unchecked → go on; unconfirmed → refuse `{"kind":"bank","state":"unconfirmed","address":X,"why":"new|moved|foreign",…}` with the text pointing to the sidebar line; failed → refuse. A serial write checks S before its 0xA5; its own new value is recorded by `board_written` as today.
4. **Foreign connection:** while a *foreign* pack is unconfirmed, the connection records nothing under I: the recorder session is not started or is stopped for this link, parameter reads store no snapshots, `maint::read_master` stores no marker state. Data read before the serial read finished (first polls) is dropped (the recorder starts only after the serial read of the first pack, or keeps its first rows pending until the identity is decided — implementer's choice, the simpler one). Confirming the takeover or reconnecting with the right installation lifts it.
5. **CAN socket bus:** no writes there. The master's battery serial (Modbus) equal to another installation's address-0 battery serial and none of I's → the same red line (hint only); recording continues.
6. **Setting serials (added during implementation):**
   - When the serial read after connecting finds packs whose String 2 is empty or a placeholder (*unchecked*), the sidebar shows one muted line per connection: „Packs ohne Seriennummer (00, 02): die App kann nicht prüfen, ob hier die richtige Bank hängt. Seriennummern setzen unter Parameter → Seriennummern.“ with a link that opens the Parameters view. The help (installations, where boards and serials are explained) says that serials identify the bank, that self-built packs or missing factory values should get one, and suggests setting the battery serial (String 1) too: it is the only serial the CAN socket shows (master only, Modbus, first 22 characters, firmware ≥ 16.06.11).
   - Parameters → Serial numbers: when String 2 is set and String 1 is empty, a placeholder or another value, the battery row offers „BMS-Nummer übernehmen“, which fills the battery field with String 2 (the user still clicks „Schreiben“; nothing is written automatically). A value longer than the field (30 characters) is cut with a note; one longer than 22 characters gets the note that the CAN socket shows only the first 22.

## Implementation notes

- Recorder (point 4): on RS485-1/2 the recorder starts *held* at connect; rows polled while held are dropped, not kept pending. The first serial read that finds no foreign pack (known, new, moved, unchecked or failed) releases it; a foreign pack holds it again (rows recorded between the first read and that one stay). On the CAN socket nothing is held.
- Parameter snapshots are skipped for every pack in the connection's unconfirmed set (not only on a foreign connection); `board_written` after the app's own serial write is recorded also on a foreign connection, since its target was checked just before.
- The write result carries `unchecked` (`{value, unchecked}`); the UI's API wrapper hands on the value and shows „Anlage nicht geprüft (Platine ohne Seriennummer)“ in the Parameters view for that pack.
- A restart reads 0xA4 before its 0x51 pre-check: a silent pack is now refused as „Anlage nicht geprüft: Pack antwortet nicht“ instead of „nicht erreichbar“.

## Out of scope

Proving the bank when every pack has a placeholder serial (the app cannot tell banks apart then; the help says so and suggests setting serials, which the app can do).

## Review r1 questions (answered, kept for reference)

1. Is the per-write 0xA4 read sound under the reservation and `with_link`, or can it read another pack than the one written (address echo, gateway)?
2. `foreign` across archived installations: harmful when a board really moved from an archived bank to this one (refusal until the user deletes or merges)? Is the swap confirmation the better path there?
3. Recording continues on a foreign connection: does anything else than the recorder write data under the wrong installation (parameter snapshots from reads, `board_seen` from other paths, BMS history saves)?
