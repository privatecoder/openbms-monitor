# Installation export / import as ZIP (phase 4) — plan

Spec: `docs/superpowers/specs/2026-10-14-installations-own-the-data.md` §7 (approved r3). Branch `feat/zip-export`.

## Decisions (beyond the spec)

1. **Two import commands: inspect, then apply.** `installation_import_inspect(path)` unpacks and checks, returns a preview; the user decides in a dialog; `installation_import_apply(export_id, into)` writes. The preview is what makes „same master serial → merge offered, not automatic“ possible without a second dialog later.
2. **Where an import goes** (`into`):
   - the export's installation id exists here → always into it (the preview says „wird mit Anlage X zusammengeführt“; no choice, it is the same installation);
   - else `new` (default) or, offered only when the export's master serial equals the stored master serial of an installation here, `merge:<id>` („Master-Seriennummer passt zu Anlage X: dort hinein?“).
   - A new installation keeps the export's id (so a second import of a later export of it merges by id) and its name; a name already used gets „ (Import)“, then „ (Import 2)“ …
3. **What goes into the ZIP** (`openbms-<name>-<YYYY-MM-DD>.zip`, name sanitised to `[A-Za-z0-9._-]`):
   - `manifest.json`: `{ format: 1, app, export_id, created, installation, files: { "<path>": { size, sha256 } } }`, written last.
   - `installation.json`: installation (id, name, created, archived, last_connect, legacy), `endpoints` (information only), master serial (+ at), places (cell type JSON), layout (groups, pairs), diagnosis, boards with their aliases, parameter snapshots (block as hex), log index (file, address, at, kind, result, release).
   - `logs/<file>`: every indexed firmware log of the installation that exists on disk (missing ones are listed in the index with `missing: true`).
   - `recording.jsonl` only when „Aufzeichnung mitnehmen“ is ticked (off by default): the existing JSONL export of the whole span and all packs, written to a temp file next to the target, then streamed in.
   - Not exported: `bootloader_pending`, master read/marker state, moves, legacy sources, import status, control endpoint (local hardware state, not facts about the bank).
   - Written as `<target>.part`, renamed when complete; a failure deletes the part file.
4. **Import checks before anything is written** (inspect):
   - Entry names: only `manifest.json`, `installation.json`, `recording.jsonl`, `logs/<name>` with `<name>` matching `[A-Za-z0-9._-]{1,200}` and not starting with `.`; anything else → refused (no path traversal, no absolute paths, no directories, no symlinks).
   - Size: each entry ≤ 2 GiB and the sum ≤ 4 GiB, counted while extracting (not trusted from the header); free space checked before.
   - Every file in the manifest present with matching size and SHA-256; no file outside the manifest; `format` = 1.
   - Unpacked to `<data>/imports/<export_id>/` (export_id validated like a log name). An existing complete unpack of the same id is reused after re-checking hashes.
5. **Apply, resumable** (spec: import status keyed by export id, every step idempotent). `import_status.state` is JSON `{ into, maint: bool, recording: bool, logs: bool }`:
   - **maint step** (one transaction in `maintenance.sqlite`): installation row (if new), master serial (set if none; a different one is not taken and logged), settings via the existing `add_settings` (target wins, the import's values kept in the move log), boards (dedupe by `(installation, address, bms_serial, first_seen)`; `last_seen` = max; aliases dedupe by `(board, serial, at)`), snapshots (dedupe by `(installation, address, bms_serial, at, kind, block)`), log index rows. A `moves` row `kind = "import"` with what was added and what was not taken. Sets `maint = true` in the same transaction.
   - **recording step**: the existing history import (one transaction) with site = target installation id; rows `INSERT OR IGNORE` by `(site, pack, t)` as today. To survive a crash between the history commit and the status update, the history import writes `meta('import:<export_id>')` in its own transaction and skips a file whose key is there (events would otherwise double).
   - **logs step**: each file copied to `firmware-logs/` (`<name>`; if a different file has that name: `<stem>-import-<n>.<ext>`, and the index row points to the new name); a file with equal name and equal hash is skipped. Copy to `.part`, then rename.
   - Completed only after all three: `completed = now`, temp dir removed. A completed export id → inspect says „bereits importiert am …“, apply does nothing.
   - An interrupted import (row without `completed`) → inspect of the same file resumes it; the Installations view lists unfinished imports with „Fortsetzen“.
6. **UI** (Installations view): per installation „Exportieren“ (save dialog; checkbox „Aufzeichnung mitnehmen“, shows the recording's size estimate); top „Anlage importieren …“ (open dialog → preview: name, packs/boards/snapshots/logs counts, recording span, where it goes with the choice from 2. → „Importieren“ → result line). Demo mode: hidden. Busy state disables both while running; writes are refused while the history is preparing (existing guard).
7. **Not in scope**: encryption, partial exports (time span, packs), importing old-format files other than the existing JSONL import.

## Tasks

1. `zip` crate (deflate only) in `app/src-tauri`; module `app/src-tauri/src/bundle.rs`: manifest types, name/size/hash checks, `write_zip(entries) -> manifest`, `unpack_checked(zip, dir, limits) -> Manifest`. Unit tests: traversal names, oversize, hash mismatch, extra file, missing file, symlink entry, good round trip.
2. `maint_store`: `export_data(inst) -> InstallationData` and `import_data(data, into, export_id, now) -> ImportReport` (transactional, dedupe as in 5.), import status helpers. Tests: round trip into an empty store; import twice → no duplicates; merge by id; merge into another (master serial match) with settings not taken; name collision.
3. `history`: import with a marker key (`meta('import:<id>')`), skip when present. Test: second call adds nothing (no new events, no session).
4. Commands `installation_export`, `installation_import_inspect`, `installation_import_apply`, `installation_imports_open` in `installations.rs`; log copying; temp dir handling; `.part` writing. Test of the whole flow on temp dirs (export → inspect → apply → apply again).
5. UI: buttons and dialogs in `Installations.tsx`, API bindings, i18n DE/EN, help topic `topic.installations` (export/import paragraph), `python3 tools/gen_help.py`.
6. README/CLAUDE.md lines; full test run; review.
