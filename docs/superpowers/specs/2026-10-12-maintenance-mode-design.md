# Maintenance mode for firmware and parameters: design

Status: stage 1 implemented (local: control channel, maintenance mode, DIP way, guards). Stage 2 (remote flashing through the gateway) is pending bench tests. Design reviewed (five rounds with a second model, no open blocking or high findings), 2026-10-12.

## Goal

Firmware updates and parameter writes on the pack bus (RS485-1/2, UART0, 19200 baud) need a bus without a second sender. A master with configured slaves (DIP 5–8 ≠ 0) polls that bus continuously with CID2 0x5A and does not run its normal UART0 dispatcher, so it neither answers requests at address 0 nor executes 0x45 (restart, the first step of an update). Two ways stop the polling:

- **Maintenance mode** of the patched master firmware (16.06.13 `_MAINT`): UART1 command CID2 0x64 sets a persistent flash marker; while it is set, the master treats its slave count as 0 from its next one-second cycle on.
- **DIP 5–8 OFF** on the master (any firmware): the same effect, set by hand, read cyclically without restart.

The app offers maintenance mode where it is needed (firmware view, parameter writing), shows the DIP instruction where it is not available, unlocks risky actions only after a **fresh measurement** right before each action, and keeps an open maintenance session visible until it has been ended and verified.

Success, in two stages:

1. **Local:** USB adapters on both buses (UART1 control, UART0 work). The user can flash any pack of a multi-pack bank without touching a switch when the master is patched; with a master without 0x64 after setting DIP 5–8 OFF. For parameter writing this design delivers the gate only; the writing itself is a separate feature that builds on it.
2. **Remote:** the same through a two-channel RS485-to-Ethernet gateway (one channel per bus). This needs TCP flashing in the app (see "Transport"), released only after the gateway bench tests pass.

The app does not control the inverter. Before activating, the operator confirms that the inverter takes and gives no battery power; this is an operator statement, not a technical check, and the UI says so.

## Facts and their status

Measured on one bench board (single pack, slave simulated by DIP 5 ON), 2026-10-12:

- 0x64 exists only on UART1 (CAN socket RS485 pair, 9600 baud), only the pack at DIP address 0 handles it, only for ADR 00 (FF: no answer). Frames: status `~20004664E00202FD31`, start `~20004664A00601C35AFC46`, stop `~20004664A00600C35AFC47`. Reply INFO `[marker 0/1, effective slaves]`; format/token errors RTN 05.
- Original firmware answers an unknown CID2 such as 0x64 with RTN 04 (code `0x2B3E–0x2B44`, 16.06.13).
- The start/stop reply shows the effective slave count of the previous cycle; the new value is visible about 1 s later (`start` → `1/0` after ≈ 1.9 s with a 0.5 s status interval).
- With the marker set and DIP 5 ON: no 0x5A on UART0, the master answers 0x51/0x47 at address 0, a 0x45 restart produces no 0x5A while booting, the app answers again after ≈ 4.9 s.
- Without the marker a polling master ignores 0x45 on UART0.
- While the marker is set, 0x64 reports effective slaves = 0; the DIP count is not readable then.
- Polling: one slave ≈ one 0x5A per second (measured). With 11 slaves the master waits up to 150 ms per slave answer before the next poll, one round ≈ 1.65 s (from code, `0x13F6`/`0x14AE`, not measured on a full bank).

Derived, not measured: marker survives a power cycle (stored in flash `0x23000`; only a watchdog restart was measured). Not known: whether the bootloader keeps sector `0x23000` when the master is flashed; behaviour with a real second pack on the bus; the TCP gateway path for 0x64 and for flashing.

## Installation identity

Today the app names an installation by its endpoint (serial path or TCP `host:port`, `lib.rs` `connect`), so the UART1 and UART0 channels of one bank would count as two installations. Maintenance needs one identity across both: a stable internal **installation id** (generated, never changed) with a separately editable display name (e.g. "Bank 1"), to which the main endpoint(s) and the control endpoint are assigned explicitly in the settings. The maintenance session, `N` and the banner belong to the installation id. History keeps its endpoint-based `site` names as they are (display and existing data).

## Master state as seen by the app

The app models what it knows about the master, per installation id, and stores it locally so that an app restart or a change of connection does not lose it:

| State | Meaning | Source |
|---|---|---|
| `patched-off` | patched firmware, marker 0, effective slaves N | 0x64 status `0/N` |
| `patched-on` | patched firmware, marker 1 | 0x64 status `1/0` |
| `no-0x64` | address 0 answered 0x64 with RTN 04 after successful UART1 communication: the firmware does not know the command (original or another build; RTN 04 does not prove which) | verified RTN 04 |
| `unknown` | no answer, connection error, gateway busy, master restarting | anything else |
| `marker-unexplained` | `1/0` where the app did not expect a marker (first status after flashing a patched build onto a master that was `no-0x64`, or a `1/0` without a session the app or the user can account for) | 0x64 status plus context |

A **maintenance session** is stored per installation id when the app starts maintenance (or the user confirms DIP 5–8 OFF): kind (`marker` or `dip`), start time, expected slave count `N` (from `patched-off` before `start`, or entered by the user when the marker was already set or the DIP way is used), and what has been done (flashed packs, written parameters). The session ends only after the end check passes (see "End").

`marker-unexplained` is a locked state: no "End" button, no `stop` offered or sent (it erases the whole 4 KiB sector `0x23000`, whose content is then unexplained), no hint to restore DIP 5–8 (with the marker still set the patched master stays in single-pack mode) and no prompt to give the inverter power back. The app explains the finding and points to the patch README: go back to the original image under DIP 5–8 OFF without `stop` (recovery row in the master-flash table), or examine the sector first. The user can turn it into a normal `patched-on` session only by stating where the marker comes from ("I set it earlier").

`no-0x64` leads to the DIP instruction as the derived way. `unknown` is never shown as `no-0x64`; the app shows a connection diagnosis. As an explicitly manual path it still offers the DIP instruction: a `dip` session then starts only after the user confirms DIP 5–8 OFF, and like every path nothing is unlocked before the fresh measurement in step 3 passes.

## Connections and channel ownership

The app has its **main channel** (sidebar) and a **control channel** to UART1 (configured under maintenance functions: TCP or USB; bus fixed to the CAN socket, 9600 baud). The backend owns all opened ports and serializes every access with one lock per physical endpoint (serial path or TCP host:port):

- One physical endpoint is never opened twice. If the main channel is on the CAN socket bus and points at the control endpoint, control exchanges go through the main connection (with the polling thread paused for the exchange), not through a second connection. Many gateways accept one TCP client per channel.
- Otherwise the control channel is opened for a **dialog** (`status` → `start` → wait for `1/0`, or `stop` → wait for `0/N`) and closed afterwards, not reopened every 0.5 s. A dialog has two budgets (connection setup, state change; see step 2); connection errors and status errors are reported separately.
- The firmware flow stops the monitor before "Check pack" (today `stop_monitor`). The control endpoint stays configured and is reopened for the end dialog; the backend does not assume the main connection still exists after a flash.
- Start/stop, the bus measurement, parameter writes and flashing are mutually exclusive with polling and with each other (extend the existing `flashing`/`port_busy` guard to one "maintenance operation" lock).

**Sequential way without a control channel** (one connection at a time), as its own guided flow: connect UART1 → status/start → wait `1/0` → disconnect → connect UART0 → measure and work → disconnect → connect UART1 → stop → wait `0/N` → connect UART0 → see polls again. The open session (with `N`) carries over the reconnects and stays visible on errors.

## Flow: prepare the bus (firmware view and parameter writing)

A panel "Pack bus" at the top of the firmware view and of the parameter view's write mode.

1. **Check master.** 0x64 status over the control channel → state as above. `patched-off`: store `N`, offer "Activate maintenance mode". `patched-on`: an existing session is resumed; without one, ask the user for the DIP slave count `N` (needed for the end check) and create the session. `no-0x64`: DIP instruction (note the positions, set 5–8 OFF), the user confirms. `unknown`: diagnosis; manual DIP path as described under master state.
2. **Inverter.** Before every work phase — activating, resuming an already active marker or an open session, and again after an app restart or a lost connection — the operator ticks afresh: "Inverter: no battery charging or discharging." Text next to it: maintenance mode and DIP 5–8 OFF do not stop the inverter; the master then reports only its own pack (DCL drops to one pack at once, CCL only ramps down 1 A/s); while the master itself is flashed, CAN stops entirely. The confirmation is kept only for the current work phase in memory, never stored with the marker or the session. Without a fresh confirmation neither flashing nor writing is unlocked. Activate sends `start` and waits for `1/0`. Time budgets, both on a monotonic clock: connection setup ≤ 5 s (the transport's TCP limit), state change ≤ 3 s from the valid start/stop reply; the end dialog uses the same budgets.
3. **Measure, right before each action.** On the pack bus channel, which must be configured for RS485-1/2:
   - no bytes for 2 s (no 0x5A, no other sender),
   - 0x47 answered by address 0 (0x47 exists only on UART0, so this also proves the channel is the pack bus and the master's dispatcher runs),
   - for patched masters additionally a fresh UART1 status `1/0` taken just before.
   The result is "bus quiet at hh:mm:ss", never "bus exclusive": a restart of the master or another client can still disturb it later. The measurement is repeated immediately before every flash start and every parameter write.
   - It cannot see a pack that sits silently in its bootloader after an aborted update: bootloader frames carry no pack address, so that pack could answer or be written along. Therefore an **unclear update** ("update unclear" or "bootloader state unclear", see the abort cases) on any pack blocks every further flash on this bus until that pack answers as an application again (0x51/0x47 at its address) or the user confirms it was isolated or recovered. The block is stored with the installation id and survives an app restart. This residual risk is named in the UI.
4. **Work.**
   - *Flash:* the existing "Check pack" preflight of `openbms-flash` (quiet bus, 0x51 and 0x47 of the target) runs immediately before `0x45`, as today. One pack at a time.
   - *Write parameters (separate feature):* measurement, write and read-back (0x47) of the target run as one exclusive operation; a mismatch is reported, not retried silently.
   - *Read parameters:* not gated. Slaves answer between polls; a polling master is shown as "does not answer while it polls; prepare the bus to read it".
5. **End.**
   - Marker: `stop`, then wait for `marker = 0` and effective slaves = `N`; for `N` = 0 wait at least 1.1 s. Then on the pack bus: for `N` > 0, 0x5A polls must be seen for the configured slave addresses; for `N` = 0, no polls is correct.
   - DIP: instruction "Set DIP 5–8 back to the noted positions", then the same pack bus check against `N`.
   - For `N` = 0 there is no polling round to see: the end check is 0x47 answered by address 0 on the pack bus plus two fresh Modbus reads ≥ 2 s apart.
   - For `N` > 0, a complete polling round with answers: on the pack bus the app listens until it has seen, for every address 1…`N`, the master's 0x5A poll **and** that slave's 0x5A answer (a poll alone does not prove an answer). After that it reads the system values over Modbus twice, at least one polling round apart (≥ 2 s for `N` = 11), because the master's aggregate is only renewed after complete rounds. Only then it asks the operator to give the inverter its battery power back. Missing slaves: "switched back, pack communication incomplete", the session stays open.
   - Only now the session is closed. If any check fails, the session stays open with the failing check named.
   The app offers "End" after a successful flash or write; it does not end on its own.

## Visible state: banner

A banner above every view while a maintenance session is open or the last confirmed state is `patched-on` (for `marker-unexplained` a banner with the explanation instead, without "End"): "Maintenance of the master open since hh:mm (marker / DIP 5–8 OFF). The master reports only its own pack to the inverter." with "End". It also shows when the state is `unknown` with an open session ("not confirmed: connection lost"). It is not cleared by a disconnect, an app restart or while the master is flashed; on start-up and on every new connection on the CAN socket bus the app re-checks 0x64. For the DIP way the banner is the reminder to set the switches back; 0x64 cannot see DIP positions. The wording is "last confirmed state", not a guarantee.

## Flashing the master itself

The master can only be restarted into its bootloader while it does not poll.

| Master before | Image | Before the flash | After the flash |
|---|---|---|---|
| `patched-on` | patched | measure (step 3) | 0x64 must read `1/0`. Records whether the bootloader kept `0x23000`. `0/N`, no answer or a failed post-check: **update unclear**: keep the session open, do not prompt inverter release, show DIP 5–8 OFF as the way to quiet the bus and the recovery hint. |
| `patched-on` | original | 1. DIP 5–8 OFF (user confirms). 2. `stop`, wait for `0/0` (DIP now 0). 3. Measure: bus still quiet. Abort if 2 or 3 fails. | Master runs original firmware, quiet because of the DIP; 0x64 now answers RTN 04, as expected. Keep DIP 5–8 OFF until all maintenance work is finished, then end as DIP session. |
| `patched-off`, DIP 5–8 ≠ 0 | known patched build or original | Activate maintenance mode first (`patched-on`, rows above) or DIP 5–8 OFF. | as above |
| `patched-off`, DIP 5–8 = 0 | known patched build or original | nothing extra (master already quiet) | patched: `0/0`; original: RTN 04 |
| `no-0x64` | known patched build | DIP 5–8 OFF (DIP session) | `0/0` expected. `1/0` is possible (an old marker or other content in `0x23000`): do **not** send `stop` automatically (it erases the whole 4 KiB sector); state `marker-unexplained` (locked, see master state). With `0/0`: restore DIP at the end. |
| any | other custom image | Treat it as an image that may ignore the marker: the same hard order as for the original (DIP 5–8 OFF, then `stop` with confirmed `0/0` if the master was patched, then measure). Offered on a shared bus only if the build is released separately for that, otherwise only with the master physically isolated from the other packs. | no assumptions: state `unknown image`, re-check 0x51 and 0x64 and show what answers; DIP 5–8 stay OFF until the result is clear. |
| `no-0x64` | original | DIP 5-8 OFF (user confirms), then measure (step 3). No `stop` (the firmware has no 0x64). | Master runs the original firmware, quiet because of the DIP; 0x64 answers RTN 04, as expected. End as DIP session. |
| `no-0x64` | other custom image | The custom row below, without `stop` (nothing to stop). | as the custom row |
| `marker-unexplained` | original (recovery) | DIP 5–8 OFF (user confirms), then measure (quiet + 0x47 at address 0). **No `stop`.** | Master runs the original firmware, quiet because of the DIP. The content of `0x23000` stays unexplained and is recorded with the installation id; before a later patched flash the app shows it again. End as DIP session. |

Rule for choosing the row: the image kind decides first. An image that is neither an allowlisted original nor a known maintenance build always takes the custom row, whatever the master's state; the backend decision function is covered by a unit test for every combination.

Why DIP before returning to the original image: the original firmware ignores the marker, so the master would poll right after the flash (post-flash check fails, further slave updates blocked), and the marker would stay in flash unseen; a later patched image would mute the master at once. Clearing it under DIP OFF leaves no hidden state. This order is a hard precondition for the normal `patched-on` → original change: the app does not offer it without the confirmed `0/0` and a passed measurement. The only exception is the recovery row for `marker-unexplained` (DIP 5–8 OFF and measurement, no `stop`); a unit test covers both paths.

**Abort or power loss** in any phase:

- after `0x45` but before `R F L`: the bootloader accepts `R F L` only for ≈ 0.65–0.8 s and then starts the application by itself (measured); the application answers again ≈ 4.8 s after the restart. The app checks for that answer.
- after a confirmed `R F L` but before `W F E`: no application data written yet, but whether the bootloader returns to the application by itself is not known. The app checks for an application answer; without it the result is "bootloader state unclear" (same handling as below). No automatic repeat.
- after `W F E`: the board may have no valid application (mixed state at a 4 KiB boundary). Do not restart blindly; the session stays open and the result is "update unclear". Recovery path (repeated `R F L` after a restart or power cycle) is not part of this design and not yet measured (firmware view design, out of scope).
- during `start`/`stop`: re-read 0x64 and continue from the state found; a partly written marker word reads as `marker = 1` (any word other than all ones) and `stop` erases the sector.

The image kind comes from `openbms-flash`: allowlisted original release, a known maintenance build (code SHA-256 listed next to `known::RELEASES`), or other custom firmware.

## Transport for remote use

Stage 2 needs TCP in the firmware flow: `firmware_inspect` and `firmware_start` take an endpoint (serial or TCP) like `connect`; the UI offers the same kinds as the sidebar. Released only after these bench tests with the real gateway (lab notebook item 3a): frames sent in pieces (5/20/50 ms gaps), added latency before `R F L` and each frame, the gateway's second channel at 19200 baud on RS485-1, full flash with `openbms-flash --tcp`. Until then the TCP option is hidden behind the custom-firmware-style switch or not offered.

## Backend

- `openbms-proto::maint` (exists); `openbms-transport`: `maint_status()` exists, add `maint_set(on)`.
- `openbms-transport::connect_tcp` today waits up to 5 s **per resolved address** and does not bound name resolution; the 5 s setup budget needs one overall deadline across resolution and all addresses (or the budget is stated per address).
- App backend:
  - control endpoint settings; endpoint registry with one lock per physical endpoint; the maintenance-operation lock (extends `flashing`/`port_busy`).
  - `maint_dialog(action, expected_slaves)`: status/start/stop plus wait, one deadline, returns the final state or a typed error (connection vs status vs timeout).
  - `bus_measure(address)`: quiet + 0x47 on the pack bus channel; shares code with `openbms-flash::session` (export `quiet`), returns time and result.
  - maintenance session, blocks and installation settings per installation id in an own database file `maintenance.sqlite` (app data folder), not in the history database. Reason: `history.rs::open_db` moves the whole history file aside when its schema version changes, which would silently drop an open session or a flash block. The new file uses only `CREATE TABLE IF NOT EXISTS` and never bumps a version.
  - firmware commands take an endpoint (stage 2).

## Stage-1 decisions

Taken during implementation where this design leaves room:

1. **Pack port decides the installation.** Flash start, measurement, End and the block check find the installation from the pack port. A new connection never silently switches the installation in use; endpoints are assigned explicitly. An unassigned pack port is refused while any installation has open work.
2. **`update-interrupted`** is written before every restart into the bootloader and replaced by the result once the flash returns. A crash leaves it, and it blocks like an unclear update.
3. **Sequential way and `stop`.** A master-flash row that needs `stop` right before the restart cannot run with one adapter; it is refused there with a hint to use a separate control adapter.
4. **Unclear master update** is a flash block on address 0 (reason `master-unclear`, with the expected state), with or without a session. It stops further flashes and End and is cleared only by a master check that shows the expected state (not by 0x51/0x47, because the master's application runs) or by "isolated or recovered".
5. **Guarded endpoint ownership.** One physical endpoint belongs to exactly one installation. An adapter used as another installation's control channel is never taken over. An endpoint or control adapter never moves between installations while the source or the target has open work, and an installation with open work keeps all its endpoints and its control adapter.
6. **End in three steps** (clear the marker, check the pack bus, read the system values). With one USB adapter for both buses the panel runs them one at a time and asks to move the cable in between; with two adapters End runs them in one go.
7. **A failed read never clears `marker-unexplained`**; an unclear check after the recovery flash keeps that state too (plus a block).
8. **Stale results are discarded.** Results of panel actions that return after a switch of installation or session are not written. A refresh never overrides a fresh master check.
9. **Own database file** `maintenance.sqlite` (see Backend) and the maintenance build counts as custom firmware for the opt-in (it needs "Allow custom firmware" and `FLASH CUSTOM n`; it is still recognised by its code hash).
10. **Pack count, not slave count.** Wherever the app needs `N` from the user (DIP way, accounting for a marker, correcting `N`), the operator enters the number of packs on the bus, master included (2 to 16); the app derives `N` = packs − 1 and shows it next to the input ("8 packs = 7 slaves + 1 master · the master's DIP 5–8 were set to 7"). The backend still stores and checks `N`. A single pack (1 pack, `N` = 0) is accepted only when, inside the same command (DIP session start or `N` correction), the master itself reports "patched, marker off, 0 slaves" in a fresh 0x64 read **and** on the pack bus (quiet, 0x47 from address 0) no slave answers a 0x51 query at addresses 1–15 (two passes, 600 ms window each; fail-closed: any byte other than the exact echo of the own request counts as a slave, only a silent window is absence; refused "slave-present"). A bank master with DIP 5–8 OFF also reports 0/0; its answering slaves tell it apart. Slave packs switched off or disconnected during both checks cannot be seen; the pack count the operator enters must be the real one. Without a control channel it is refused; the commands take the pack port for the scan. Activating maintenance mode and the marker statement keep `N` ≥ 1. End with `N` = 0 needs the control channel: the pack bus step is the quiet-bus/0x47 measurement plus the same slave scan ("slave present at address a" otherwise), and the values step requires a fresh 0/0 before the two reads ("master does not report 0 slaves" otherwise).

## Out of scope

Parameter writing itself (0xA1 frames, UI); stored read/written parameter snapshots; recovery after an aborted update; automatic inverter control.

## Tests

- Unit: frames and parsing (exist); dialog loop with a fake clock and fake link (start/stop, `N` = 0 and > 0, timeout, connection error, RTN 04 vs no answer → `no-0x64` vs `unknown`); bus measurement with simulated streams (0x5A present, other bytes, master answering 0x47, wrong bus answering only 0x51).
- UI: state machine (four master states, session open/closed, sequential way), banner persistence across reconnect and restart, the master-flash table rows including abort cases.
- Bench, single board (DIP 5 ON): full local flow with USB on both buses; power cycle with the marker set; patched → patched flash of the master (does `0x23000` survive?); patched → original in DIP-first order.
- Bench, two real packs, then the full bank: 0x64 through the gateway while the monitor runs on UART1; 0xA1 with read-back to each pack while the master is quiet; flashing one slave while others listen; gateway disconnect and reconnect.
