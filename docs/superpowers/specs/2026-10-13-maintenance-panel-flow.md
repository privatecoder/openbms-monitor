# Pack bus panel: guided flow

Status: implemented, 2026-10-13 (draft r3 reviewed; r1 review: `.orch/reviewer-codex-6-sol-maint-panel-flow-r1.md` in the knowledge base). Builds on `2026-10-12-maintenance-mode-design.md` (binding) and the stage-1 implementation (`app/src/views/PackBusPanel.tsx`, `Firmware.tsx`, `app/src/maint.ts`, backend `app/src-tauri/src/maint.rs`).

## Why

First bench run of stage 1 (single board, patched master, DIP 5 ON = 1 slave):

- All actions of the panel are visible and enabled at once. Nothing says which one is next.
- After an app restart the firmware view preselected the first serial port, the control-channel adapter. `needsReplug(control, port)` became true, the panel switched to the one-adapter End steps, the operator cleared the marker and ran the bus check on the CAN adapter ("pack communication incomplete"). The session stayed open (correct), but the operator was misled.
- After a restart the monitor in the sidebar is disconnected; the firmware view gives no hint whether that matters.

## Principles

- The panel is a numbered checklist. Each step is done (✓), current (▶) or locked (🔒 with the reason, e.g. "first step 2"). Only the current step's main action is enabled; harmless repeats (check master, measure) stay available on done steps.
- The backend stays the authority. The step logic is an additional UI rule (pure function `flowOf(...)` in `maint.ts`, unit-tested for every state below); it never replaces a backend gate.
- Choosing an address and an image (and the ports, in the setup form) is not locked by the maintenance steps: they are inputs the flow needs to know which branch applies (master flash, recovery). They are locked only while something runs (flash, pack check, measurement, End sub-step), as today. Changing any of them invalidates the measurement, the pack check and the typed confirmation.

## Steps

1. **Connections.** Read-only: "CAN socket: …" and "RS485-1/2: …", each with "not plugged in" when the serial port is not among the available ports, and a "Set up" button. The setup form is the single place to choose ports (amended 2026-10-13, see below); there is no port select in the panel or, with an installation in use, in the firmware view.
   - Done when the RS485-1/2 port is set up and plugged in, and the CAN socket is set up (or "without control channel" is ticked). Otherwise current with "set up the RS485-1/2 port", "RS485-1/2 port … is not plugged in", or, when the only serial endpoint is the control adapter, "one adapter for both buses? Confirm it in the setup".
   - The RS485-1/2 port is the installation's stored one (`packPortOf`): the first stored serial endpoint that is not the control adapter. The control adapter counts only with the one-adapter confirmation of the setup form (kept in the UI per installation).
   - The CAN adapter never becomes the RS485-1/2 port implicitly: a sidebar connection on the CAN socket bus becomes the CAN socket (control) of an installation without one, not a listed endpoint (`controlFromAttach`); the banner's "assign" sets a CAN-bus connection as the CAN socket and lists only pack-bus connections (`assignEndpoint`), and only a serial one of those can be the RS485-1/2 port. A first run via the sidebar on the CAN socket therefore shows "CAN socket: A / RS485-1/2: not set up", and the setup form opens with CAN = A and RS485-1/2 empty.
   - The RS485-1/2 port is shown in the panel header ("RS485-1/2: …") and in the End section, so it is visible before every bus action.
   - One-adapter mode (`needsReplug`) only when the RS485-1/2 port equals the control endpoint.

### Setup form (amended 2026-10-13)

Bench feedback: three port pickers (control channel, an endpoint list with "add", and a port select in step 1) were one too many; the list showed "–" while the store held both ports, and a save from that form would have dropped the pack port. Likely cause: the old form copied the endpoint list into its state once when it opened and never re-synced, while the port assignment in step 1 on the same screen changed the stored installation, so the open form showed and would have saved a stale list.

- Fields: name; **CAN socket** (control the master, 9600 baud): USB port or network address `host:port`; **RS485-1/2** (measure and flash packs, 19200 baud): USB port; "network" is shown but disabled until stage 2 (flashing through a gateway is not bench-tested yet). A stored port that is not plugged in stays visible in its select.
- The form starts from the installation as stored (re-read when it opens) and builds the saved list from the installation as stored at save time (`setupEndpoints`): the CAN key and the RS485-1/2 key first, then every other stored endpoint (e.g. a sidebar TCP endpoint); the old CAN key and the old pack key are replaced.
- A stored RS485-1/2 port can be replaced but not left empty (`setupCheck`); if it was set meanwhile (the form opened without one), the form reloads its fields from the stored installation and says so. The same serial adapter for both needs the checkbox "one adapter for both buses, I move the cable", shown only in that case.
- While a panel action runs (check, scan, measurement, End step) the form's inputs and Save are disabled; a result of an action whose RS485-1/2 port changed meanwhile is dropped.
- The backend's ownership guard (`endpoint-in-use`) still applies and is shown as text.
2. **Check master** (0x64). Required once per app run and again after a lost connection before step 3 unlocks. A stored state from an earlier run is shown as "last confirmed state" only.
   - A check that fails (no answer, no control channel, connection error) ends as `unknown` with the diagnosis — except when the stored state is `marker-unexplained`: a failed read keeps it, and only its recovery stays available (stage-1 decision 7). A failed check completes step 2 for the manual DIP way only; it never confirms a marker.
3. **Inverter statement.** Checkbox; after a restart, a lost connection or the end of a session it is empty with the reason shown. It survives the session its own activation opened.
4. **Start maintenance**, by state:
   - `patched-off`, N > 0, no session: two choices — "Activate maintenance mode" (recommended) or the manual DIP way (note positions, set DIP 5–8 OFF, confirm, enter pack count incl. master). Spec row "activate first or DIP 5–8 OFF".
   - `no-0x64` / `unknown`, no session: manual DIP way.
   - `patched-off` with 0 slaves (fresh 0/0), no session: 0/0 alone does not prove a single pack (a bank with DIP 5–8 OFF reads the same). Step 4 offers "check for slaves" (the existing UART0 slave scan, two passes). Only when no slave answers is the installation shown as a single pack and step 4 as "not needed" for a master flash with the original or maintenance image (spec row "patched-off, DIP 5–8 = 0"). Steps 3 and 5 stay required: a master flash stops CAN to the inverter. The scan result lives in memory and is cleared by every new master check and by a closed or lost connection. If a slave answers, the state is "pack count unclear: DIP 5–8 may be OFF" and only the DIP session (with its End and DIP-restore reminder) is offered. A custom image on a single pack still needs the DIP session with the single-pack proof (as implemented).
   - session open (marker or DIP), or `patched-on` with the marker accounted for: ✓, showing kind and "P packs = N slaves + 1 master" with "change".
   - `marker-unexplained`: the locked branch (explanation; "I set the marker myself" for marker sessions, or "start recovery by DIP"). See Recovery.
5. **Measure bus.** Unlocked when step 4 is done (or "not needed") and a pack port is chosen. Shows "bus quiet at hh:mm:ss"; a new state check or a flash invalidates it.
6. **Flash.** The firmware steps below are unlocked only when steps 2–5 are done. Address, port and image can be chosen any time (see Principles); "check pack" and "start" follow the flow.
7. **End maintenance** (own section): shown while a session is open; locked while a block exists or the state is `marker-unexplained`.
   - Two adapters: one "End" action (stop → bus → values).
   - One adapter: three sub-steps; the backend reports the next allowed sub-step (new field in the overview: `end_next: "stop" | "bus" | "values" | null`, derived from the in-memory progress), so the UI shows exactly that one as current after a view change or retry. After a failed sub-step the overview reports the restart point and the UI says so.

Blocks (unclear updates) are listed above the steps; while one exists, steps 5–7 are locked with the block as the reason.

## Single pack without a control channel

Without a control channel the app cannot read 0x64, and a DIP session with 1 pack needs one (single-pack proof). So step 1 has the explicit choice "work without a control channel" (step 2 then ends as the manual way), and step 4 offers, next to the DIP way, the operator statement "only this one pack is on the pack bus" — only while no session is open and the stored master state is absent or `unknown`. It marks step 4 as not needed; steps 3 and 5 stay required, and the flash's own preflight still checks the quiet bus. Both choices live in memory only and are cleared, like the inverter statement, when the connection is closed or lost.

## Marker session after `stop`

A marker session whose master no longer reports the marker (e.g. `patched-off` after End's stop sub-step) locks steps 5–6 ("check the master again"); End continues from its next sub-step.

## Recovery (`marker-unexplained`)

- Step 4 shows the recovery: note DIP positions, set DIP 5–8 OFF, confirm, start the recovery DIP session.
- After that the flow unlocks step 3 (inverter), step 5 (measure) and the firmware steps **only for address 0 with an original image**; any other address or image keeps them locked with "recovery: only the original image on the master".
- End stays locked until the recovery flash has produced `no-0x64` (state no longer `marker-unexplained`).

## Marker session with unknown state (backend fix)

A connection loss stores `unknown` while a marker session is open. Today `measure` derives the fresh-status rule from the stored state (`fresh_status(Unknown) = Unchecked`) and `prepare_flash` re-checks only for address 0, so a slave flash could pass on bus quiet + 0x47 alone.

- Backend: when the open session is a marker session, the fresh status before any flash/measure must be `1/0` regardless of the stored state (`Fresh::MarkerOn`); with one adapter for both buses (no UART1 read possible in the same command) a marker session with stored `unknown` is refused (`state-unknown`) until a successful check.
- UI: a marker session with `unknown` locks steps 3–6 until a check shows `patched-on`; a failed check clears the inverter statement.

## Monitor connection

Three distinct texts: "monitor paused for maintenance" (the app stopped it itself, `monitor-stopped`), "not connected" (never connected / closed by the user), "connection lost" (link-lost or data timeout). The maintenance steps open their connections themselves; each step reports its own success or failure.

## Answers to the r1 questions (adopted)

1. Fresh check per app run / after a connection loss before a new work phase: yes. Stored data only drives the banner.
2. Measurement before End: no; End checks itself. Blocks and `marker-unexplained` keep End locked.
3. Port preselection: only with exactly one candidate; otherwise an explicit choice. (Superseded 2026-10-13: the ports come from the setup form only; no preselection.)
4. Recovery, DIP alternative for patched-off N > 0, marker session with unknown, end progress and port visibility are covered above.
