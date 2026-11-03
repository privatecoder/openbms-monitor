# Data & storage: overview, freeing space, backup

## Intent

The recording grows (a real 12-pack bank reached ~90 MB after a few weeks with every poll kept). The user wants to
see how much the app's data takes and, at some point, free that space again, with the option of a ZIP backup before
anything is deleted. Normal operation stays covered by the existing retention (Recording sheet: every poll for N
days, then minute values); this feature is the deliberate manual exception. Advice from a second reviewer (Codex,
`.orch/advice-storage.md`) is folded in.

## Goals

1. **Show the storage.** A "Data & storage" section in the About view (link from the Recording sheet) shows:
   - total size of the data folder and the free space of its drive;
   - per category, measured from the files (not estimated from row counts):
     - recording: `history.sqlite` plus `-wal`/`-shm`;
     - installation data: `maintenance.sqlite` plus WAL, including the BMS history archive;
     - firmware logs (`firmware-logs/`);
     - parameter files (`parameters/`);
     - exports (`exports/` and `recordings/`, the CSV/JSONL export folders);
     - migration backups (`*.sqlite.bak-N`);
     - leftover imports (`imports/`);
   - for the recording also its time span, row count and the current retention, with a link to the setting.
2. **Clear the recording**, of one installation or of all. Installations, boards, parameter states and the BMS
   history archive stay.
3. **Delete migration backups** (`*.sqlite.bak-N`) separately, with size and date. The dialog says that the way
   back before a migration goes with them.
4. **Empty the data folder.** Deletes the collected data but keeps the installations (see "What emptying keeps").
   The dialog lists what is in the folder, says plainly that files kept for outside use (exports, parameter files)
   are deleted too, and offers "Show folder" so they can be copied away first.
5. **Backup before delete.** Every delete dialog offers three buttons, no extra checkboxes:
   - "Back up as ZIP and delete";
   - "Delete without backup";
   - "Cancel".

   The backup is the existing installation export with recording (`installation_export`, one ZIP per
   installation; for "all" and for "empty the folder" one ZIP per installation that has data, into a chosen
   folder). If a backup fails, nothing is deleted.
6. **Honest result.** After an action the view shows the sizes before and after. If the rows are gone but the file
   could not be shrunk (VACUUM failed, e.g. disk full), it says "data deleted, file not yet smaller" instead of
   only logging it (today `clear_then_compact` only `eprintln!`s).

## Required fix first: a backup can be restored again

`installation_bundle::apply` returns `already: true` for an export id whose import row in `maintenance.sqlite` is
completed. `History::clear` drops the recording's `import:%` markers, but the maintenance-side `import_status` row
stays. So after restore → clear → restore, the same ZIP ends as "already imported" and brings nothing back. Fix:
when the recording of an installation is cleared (and when the folder is emptied), the completed `import_status`
rows of that installation go too, so its bundles import again. Add a test for restore → clear → restore.

## What emptying keeps (the user's decision)

The user keeps installations; parameter states and firmware logs are data that "can be deleted when in doubt".

**Kept** in `maintenance.sqlite`:
- installation definitions and their ports: `installations`, `installation_endpoints`, `installation_links`;
- the boards: `boards`, `serial_alias`, `master_serial`;
- cell types per place, layout, wiring, notifications, devices: `place`, `layout`, `wiring`, `notifications`,
  `devices`;
- the BMS history archive (`bms_records`), the user's decision: it is small and the BMS no longer holds older records;
- per-pack safety state, my call: `bootloader_pending`, `flash_blocks`/`master_state` where present,
  `patch_proof`, `hard_stop_seen`, `maint_expected`, `diagnosis`. Losing these could hide a pack left in update
  mode or a refused flash.

**Deleted:**
- the whole recording, i.e. all rows in `history.sqlite`, then VACUUM;
- parameter states: `param_snapshots` and the files in `parameters/`;
- firmware logs: the `logs` table and the files in `firmware-logs/`;
- `exports/`;
- the migration backups;
- the bookkeeping rows `import_status`, `legacy_sources` and `moves`;
- `imports/`, but never an import that is open or can still be resumed; those stay, with a note;

## Builder decisions (my calls, settled after the builder check)

- **Clear the recording** deletes the values, events, sessions and site rows of the installation (as
  `History::clear` does today). It keeps the history-side `param_snapshots` and `site_packs` (the installation's pack
  list); only "empty the folder" removes parameter states. The restore test asserts that.
- **Serialisation.** All history writes go through the `History` mutex, so clear and VACUUM do not race the poll
  writes. Recording continues afterwards in a new session; no extra poll stop is needed.
- **Emptying the folder at startup.** The marker `pending-empty` only says "empty". On the next start, in `setup`
  before either database is opened, the app:
  1. deletes `history.sqlite` with its `-wal`/`-shm` files (it is recreated empty);
  2. deletes the migration backups, `firmware-logs/`, `parameters/`, `exports/`, `recordings/`, and the imports in
     `imports/` that are not open;
  3. opens `maintenance.sqlite` and deletes the rows listed under "Deleted" in one transaction;
  4. removes the marker.

  Then the start continues normally, including the usual reconnect. If a step fails, the marker stays and the next
  start tries again; the About view shows the error.
- **Open imports.** An import can still be resumed when its `import_status` row has `completed IS NULL`. Such
  imports and their folders are always kept, also when the folder is emptied.
- **Migration backups.** They are only written at start, so deleting them while the app runs needs no reservation
  beyond the busy check.
- **Which installations "have data".** An installation has data, and gets a ZIP, when it has recording rows,
  firmware logs or BMS records. The ZIPs keep the existing export file names; a name that already exists gets a
  numeric suffix. If any ZIP fails, nothing at all is deleted.
- **What the ZIP lacks.** The ZIP does not hold parameter files, parameter states or `exports/`. The "empty"
  dialog says so next to "Show folder".
- **Sizes after an action.** They are kept in memory only. "File not yet smaller" is worked out from the current
  state, `PRAGMA freelist_count` times the page size, so it holds across restarts.
- **The BMS archive** shows as its own line (record count) under installation data, because it is inside
  `maintenance.sqlite` and has no separate file size.

## Constraints

- **When the delete runs.** Clearing the recording and deleting migration backups run while the app runs, in the
  background. They hold the existing installation reservation (`control::reserve_installations`, for one
  installation or `ALL`), so no import, merge, export or app update runs at the same time. Recording for the
  affected installation pauses until the delete is done.
- **Emptying the folder.** It also deletes files that open handles may hold (Windows). So the app writes a marker
  (`pending-empty` in the data folder, listing what to delete) and restarts. On the next start, before any
  database is opened, it applies the marker and removes it.
- **Running operations.** Refused with the existing busy reasons while one of these runs: a firmware update, a pack
  or maintenance action, a BMS history read, a parameter write, or an import, merge or export.
- **WAL.** Never delete WAL files by hand while a database is open.
- **Disk space for VACUUM.** VACUUM may need up to twice the database size in free space. If the drive has less,
  the dialog says so; the rows can still be deleted, and the file shrinks later.
- **No new defaults.** No change to the default retention, no automatic ZIP series, no restore of a whole data
  folder.
- "Simple over gated": one dialog per action, the scope named in it, three buttons. Nothing is public yet, so no
  compatibility shims are needed.
- **Texts.** UI in German and English. The help text on retention and freeing space comes via
  `tools/gen_help.py`. CHANGELOG entry under Unreleased.

## Non-goals

- A file manager.
- Deleting per pack or by table.
- Automatic archiving.
- A full data-folder backup with restore. That would be a separate project with its own restore path.
- Changing installation delete/archive (they stay in the Installations view).

## Left to the builder

- Where the Rust code lives (a new `storage.rs` is fine).
- Exact command names.
- How sizes are walked (plain `std::fs` is fine).
- The progress display.
- How the ZIP folder is picked (the existing save dialog pattern).
