# Several installations: pick, name, create, delete

Status: implemented, 2026-10-13 (draft r2; r1 reviewed: `.orch/reviewer-codex-6-sol-installation-picker-r1.md` in the knowledge base). Builds on `2026-10-12-maintenance-mode-design.md` (binding) and the implemented stage 1 + guided panel (`docs/superpowers/specs/2026-10-13-maintenance-panel-flow.md`). Superseded in parts by `2026-10-13-simple-flash-flow.md` (2026-10-13).

## Why

The data model already holds several installations (`maintenance.sqlite`: installations with name, CAN-socket endpoint = control channel, RS485-1/2 endpoint, master state, sessions, flash blocks). The UI has no picker: the installation shown is derived from the sidebar connection (`useMaint.attach` / `attachDecision`) or the last one in `localStorage["maint.installation"]`. Users with a lab board and a real bank, or several banks, need to see and switch between them. New installations are named "Installation n" without asking.

## Proposal

1. **Picker in the panel header.** „Anlage: [Name ▾]“ lists all installations by name, plus „Neue Anlage …“. Switching changes the shown installation (overview, steps, ports, End) and persists it as today. The picker and all actions are disabled while a panel or firmware action runs and while a switch is loading.
2. **Switching rules.**
   - An explicit switch away from an installation with open work (session, block, set marker, marker-unexplained, open sector note) is allowed; it ends or changes nothing, and the open work stays visible in the banner under its stable id (see 4).
   - In-memory phase data (inverter statement, measurement, fresh-check-done, scans, End progress, one-adapter mark) are per installation and are cleared on switch.
   - A bus action whose RS485-1/2 port belongs to another installation is locked in the UI with `installation-mismatch` and the offer to switch; the backend `port_gate` stays authoritative.
3. **Sidebar connect** keeps `attachDecision`. When no installation exists for the endpoint and none is in use, the app first asks for a name (default „Anlage n“); only on confirm it calls `maint_create_installation` (one transaction: installation + endpoint/control). On cancel nothing is created and the endpoint stays unassigned. `installation_for` (implicit creation) is no longer called from the UI.
4. **Banner for every installation with open work.**
   - Backend `maint_open_work() -> Vec<OpenWork>` with, per installation: id, name, session (kind, started, N) or none, the most severe block (reason, pack address, since), master state and its time, sector note present.
   - Severity order for blocks: `bootloader-unclear` > `update-interrupted` > `update-unclear` > `master-unclear`. Lines are sorted: blocks and `marker-unexplained` first, then open sessions by start time, then a set marker without session by state time. A block line names the block („Labor: Pack 3 eventuell im Bootloader seit 14:02“), not „maintenance open“.
   - Actions per line: „Anzeigen“ (switch) always; „Beenden“ only for the shown installation with an open session, no block and not `marker-unexplained` (then it scrolls to End); otherwise the line explains what must be cleared first.
   - Refresh immediately on app start, every installation switch/create/delete, flash start and flash end, every master check, block clearing, End, and after every panel action; additionally every 30 s. On a load error the last known list stays and the banner shows „Wartungsstatus nicht geladen“ prominently until a load succeeds.
5. **Create.** „Neue Anlage …“ opens the setup form empty (name required; CAN socket and RS485-1/2 as in the setup form) and calls `maint_create_installation(name, control, endpoints)` on save; ownership rules of `save_installation` apply (one owner per physical key; no takeover of another installation's control; open-work guard).
6. **Rename** stays in the setup form.
7. **Delete** in the setup form.
   - Allowed only without open work: no session, no uncleared block, last state not patched-on / marker-unexplained, and **no sector note** (a sector note records unexplained content of `0x23000` and must be shown again before a later patched flash — it is never discarded silently; the user resolves it first, see below).
   - Confirmation „Anlage „X“ löschen? Ihre Anschlüsse werden frei.“.
   - Backend `maint_delete_installation(id)` takes the same `port_busy` operation lock as flash/measure/End, then re-checks open work (incl. sector note) inside one transaction and refuses with `open-work`. Ended sessions and cleared blocks are moved to a small archive table (`maint_archive`: installation id, name, JSON of ended sessions and cleared blocks, deleted at) instead of being dropped; endpoint rows and the master state are deleted.
   - Endpoint moves (`save_installation`, create) also run under `port_busy`, so no flash gate can read an installation id that is deleted or loses its port before its `update-interrupted` block is written. Test: flash gate holds `port_busy` → delete is refused with busy.
   - After a successful delete the UI drops the selection, overview, fresh-check/scan results and the one-adapter mark of that id and switches to „keine Anlage“ (or asks which installation to show if others exist).
8. **Sector notes when moving endpoints.** `save_installation` refuses to move an endpoint away from an installation that has a sector note (treated as open work). Resolving a sector note: an explicit action in the setup form „Hinweis zu 0x23000 als geklärt markieren“ with confirmation text; it is archived, not deleted.
9. **Firmware view without an installation** (single pack, no maintenance) is unchanged; the picker shows „keine Anlage“ and „Neue Anlage …“.

## Answers to the r1 questions (adopted)

1. Switching with open work: yes, explicit, with the open work visible under its stable id and never silently dropped on a load error.
2. Delete: ended sessions and cleared blocks go to an archive table; an open sector note blocks deletion.
3. Foreign RS485-1/2 port: locked in the UI with `installation-mismatch` and a switch offer; backend `port_gate` stays authoritative.
4. Contradictions fixed: End only for an open session without block and without marker-unexplained; sector notes never discarded; delete/move serialised with flash under `port_busy`.

## Implementation notes

Taken during implementation where the proposal leaves room:

1. **Names** are required and unique among installations (case-insensitive), on create and on rename (`valid_name` in the store, `nameCheck` in the UI), so the picker never lists two equal entries. The suggested name is „Anlage n“ with the first free n.
2. **Commands:** `maint_installations` (picker list), `maint_open_work`, `maint_create_installation`, `maint_delete_installation`, `maint_resolve_sector_note`; `maint_installation_for` is gone from the UI and the command list (the backend keeps the implicit creation only for a single pack flashed without an installation, as before). Create, save, delete and resolve run under `port_busy` (`edit_installations`) and re-check inside one store transaction; refusals reach the UI as `endpoint-in-use`, `open-work`, `name-required`, `name-taken`, `no-sector-note`.
3. **Sector notes** count as open work for endpoint moves and delete (`holds_ports`), not for the flash port gate (`any_open`), which stays as in stage 1. Resolved notes move to `sector_notes_resolved`; deleted installations to `maint_archive` (both `CREATE TABLE IF NOT EXISTS`).
4. **Banner:** one line per installation, named by its most pressing item (block > unexplained marker > session > set marker > sector note); lines sorted as in point 4, within the first group by block severity, then time. „Anzeigen“ on the shown installation opens its panel. A set marker without session gets no „Beenden“ (End needs a session); its line says to check the master.
5. **Switching** clears this run's fresh master check and slave scans; inverter statement, measurement, End progress in the UI and the panel's choices go with the panel's remount. The one-adapter mark stays per installation id and is removed on delete.
