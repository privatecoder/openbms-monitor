# Data & storage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A "Data & storage" section in the About view. It shows what the data folder holds. It can clear the
recording (one installation or all), delete migration backups, and empty the data folder (installations kept). Each
delete can first back up the data as installation ZIPs.

**Architecture:**
- **Rust module `storage.rs`:** measures the folder and runs the deletes under the existing reservations.
- **Emptying:** a marker file makes the next start empty the folder before the databases open; one SQL step on
  `maintenance.sqlite` follows right after it opens.
- **Backup:** the frontend reuses `installation_export` (one ZIP per installation) before it calls a delete.

**Tech Stack:** Rust (Tauri 2, rusqlite), React 19 + Tailwind 4, vitest.

**Spec:** `docs/superpowers/specs/2026-11-03-data-storage-design.md`

## Global Constraints

- **What emptying keeps:** installations, boards, cell places, layout, wiring, notifications and devices, the BMS
  history archive (`bms_records`), and the per-pack safety state (`bootloader_pending`, `patch_proof`,
  `hard_stop_seen`, `maint_expected`, `diagnosis`, `master_serial`, `master_read`).
- **What emptying deletes:**
  - the recording;
  - `param_snapshots` (in both databases);
  - `logs` with `firmware-logs/`;
  - `parameters/`, `exports/` and `recordings/`;
  - the `*.sqlite.bak-N` files;
  - `import_status` rows with `completed IS NOT NULL`, plus `legacy_sources` and `moves`;
  - the folders in `imports/` without an open import.
- **Clearing the recording** keeps `site_packs` and the history-side `param_snapshots`.
- **Never** delete WAL files of an open database, and never delete an open, resumable import (`completed IS NULL`).
- **Dialogs:** every delete dialog has exactly three buttons: back up as ZIP and delete / delete without backup /
  cancel. If any ZIP fails, nothing is deleted.
- **Busy:** refused while a firmware update, pack/maintenance action, BMS history read, parameter write or
  installation operation runs (`updater::busy` reasons, plus `reserve_installations`).
- **Texts:** UI in DE and EN, no register/hex jargon. Help via `tools/gen_help.py`. A CHANGELOG line under
  `## Unreleased`.
- **Rules:** nothing is public yet, no compatibility shims. "Simple over gated".

## Review Focus

1. **A recording is running while the clear happens.** Expected: the rows are gone, recording goes on in a new
   session, and nothing deleted comes back. Test in Task 1.
2. **Restore → clear → restore of the same ZIP.** Expected: the second restore brings the data back and does not
   report "already imported". Test in Task 1.
3. **The marker is present but a file is locked or missing.** Expected: the start does not fail, the marker stays
   for the next try, and the About view shows the error. Test in Task 3.
4. **An open import in `imports/` when emptying.** Expected: its folder and `import_status` row survive. Test in
   Task 3.
5. **VACUUM fails (disk full).** Expected: "data deleted, file not yet smaller", visible from the freelist after a
   restart too. Test in Task 2 (the overview reports `reclaimable` bytes).

---

### Task 1: Clearing the recording keeps the pack list and allows a restore again

**Files:**
- Modify: `app/src-tauri/src/history.rs`:
  - `clear_rows` (~703): drop `site_packs` from both delete lists;
  - `clear` (~516): return value.
- Modify: `app/src-tauri/src/maint_store.rs`: new `Store::forget_completed_imports`.
- Modify: `app/src-tauri/src/lib.rs` `history_clear` (~1044): also forget the completed imports.
- Test: in `history.rs` tests and `installation_bundle.rs` tests.

**Interfaces:**
- Produces:
  - `History::clear(&mut self, site: Option<&str>) -> Result<Cleared, String>` with
    `#[derive(Serialize)] pub struct Cleared { pub compacted: bool }`;
  - `Store::forget_completed_imports(&self, installation: Option<&str>) -> Result<(), String>`, which deletes
    `import_status` rows with `completed IS NOT NULL` (all installations when `None`).

- [ ] **Step 1: Write failing tests**
  - `history.rs`: `clearing_keeps_the_pack_list` asserts that after `clear(Some(site))`:
    - `SELECT COUNT(*) FROM site_packs WHERE site=?` is unchanged;
    - `pack` and `events` rows for the site are 0;
    - `param_snapshots` rows are unchanged.
  - `history.rs`: `clear_reports_whether_the_file_shrank` asserts that `clear(None)?.compacted == true` on a temp
    database.
  - `installation_bundle.rs`: `a_backup_restores_again_after_the_recording_was_cleared`:
    1. export with recording, then apply (rows N);
    2. `History::clear(Some(inst))` and `Store::forget_completed_imports(Some(inst))`;
    3. inspect and apply the same ZIP again; assert `already == false` and `recording_rows == N`.
- [ ] **Step 2:** `cargo test -p openbms-monitor clear restores_again`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - `clear_then_compact` returns `Cleared { compacted }`: `compacted` is false when the compact closure errs; the
    `eprintln!` stays.
  - `history_clear` in `lib.rs` returns `Cleared` and calls `forget_completed_imports(site)` after a successful
    clear.
  - Update the `api.ts` type: `historyClear` returns `{ compacted: boolean }`.
- [ ] **Step 4:** `cargo test -p openbms-monitor`. Expected: all pass.
- [ ] **Step 5:** Commit: `Aufzeichnung leeren: Pack-Liste bleibt, Sicherung lässt sich erneut einspielen`.

### Task 2: Storage overview

**Files:**
- Create: `app/src-tauri/src/storage.rs` (module doc, overview, tests).
- Modify: `app/src-tauri/src/lib.rs`: `mod storage;`, register `storage::storage_overview`.
- Modify: `app/src-tauri/src/maint_store.rs`:
  - `Store::bms_record_count(&self) -> Result<u64, String>`;
  - `Store::open_imports(&self) -> Result<Vec<String>, String>`, the export ids with `completed IS NULL`.
- Modify: `app/src-tauri/src/history.rs`:
  - `History::span(&self) -> Option<(i64, i64)>`, the min/max `t` over `pack` and `system`;
  - `History::reclaimable(&self) -> u64`, `freelist_count * page_size`.

**Interfaces:**
- Produces:
  - `pub fn measure(dir: &Path, open_imports: &[String]) -> Vec<Category>`, pure over the file system and testable.
  - `#[derive(Serialize)] pub struct Category { pub key: &'static str, pub bytes: u64, pub files: u64 }`. Keys, in
    this order:
    - `recording`: `history.sqlite`, `-wal`, `-shm`;
    - `installations`: `maintenance.sqlite` plus WAL/SHM;
    - `firmware_logs`;
    - `parameters`;
    - `exports`: `exports/` and `recordings/`;
    - `backups`: `*.sqlite.bak-*`;
    - `imports`: only folders not in `open_imports`;
    - `other`: everything else at the top level.
  - `#[derive(Serialize)] pub struct Overview { pub dir: String, pub total: u64, pub free: Option<u64>, pub categories: Vec<Category>, pub recording_rows: u64, pub recording_span: Option<(i64, i64)>, pub reclaimable: u64, pub bms_records: u64, pub open_imports: u64, pub installations: Vec<InstallationData>, pub empty_error: Option<String> }`.
  - `#[derive(Serialize)] pub struct InstallationData { pub id: String, pub name: String, pub has_data: bool }`.
    `has_data` means recording rows, firmware logs or BMS records exist for that installation.
  - `#[tauri::command] pub async fn storage_overview(app: AppHandle, state: State<'_, AppState>) -> Result<Overview, String>`.
    The walk runs in `spawn_blocking`. `free` comes from `migrate::free_space(dir)`. `empty_error` is the text of
    `pending-empty.err` if that file exists (Task 3).

- [ ] **Step 1: Write failing test** `measure_sorts_files_into_categories`:
  - Setup: a temp dir with `history.sqlite` (10 B), `history.sqlite-wal` (5 B), `maintenance.sqlite` (7 B),
    `history.sqlite.bak-2` (3 B), `firmware-logs/a.log` (4 B), `parameters/p.xml` (2 B), `recordings/x.csv` (6 B),
    `imports/open1/f` (1 B), `imports/old/f` (8 B), `stray.txt` (9 B), and `open_imports = ["open1"]`.
  - Assert the bytes: recording 15, installations 7, backups 3, firmware_logs 4, parameters 2, exports 6,
    imports 8, other 9.
- [ ] **Step 2:** `cargo test -p openbms-monitor measure_sorts`. Expected: FAIL.
- [ ] **Step 3:** Implement `measure`, the store/history helpers and the command.
- [ ] **Step 4:** `cargo test -p openbms-monitor`. Expected: PASS.
- [ ] **Step 5:** Commit: `Speicherübersicht: Größe je Bereich, freier Platz, Aufzeichnungszeitraum`.

### Task 3: Deleting, live and at the next start

**Files:**
- Modify: `app/src-tauri/src/storage.rs`.
- Modify: `app/src-tauri/src/lib.rs`:
  - `setup` calls `storage::apply_pending_empty(&dir)` before the history thread and the store open, and
    `storage::finish_pending_empty(&store, &dir)` right after `Store::open` succeeds;
  - register the commands.
- Modify: `app/src-tauri/src/maint_store.rs`: `Store::empty_data(&self) -> Result<(), String>`, one transaction
  over the deletes in the Global Constraints.

**Interfaces:**
- Consumes: `History::clear` and `Cleared` (Task 1); `Store::forget_completed_imports` (Task 1);
  `Store::open_imports` (Task 2).
- Produces:
  - `#[tauri::command] pub async fn storage_clear_recording(state: State<'_, AppState>, installation: Option<String>) -> Result<Cleared, String>`.
    - It refuses with `busy:<reason>` while `updater::busy` reports a reason (make that fn `pub(crate)`).
    - It holds `reserve_installations(&state.ops, &[id or ALL], "clear")`; a refusal maps to `busy:installations`.
    - Then it runs `History::clear` and `forget_completed_imports`, inside `spawn_blocking`.
  - `#[tauri::command] pub fn storage_delete_backups(app: AppHandle) -> Result<u64, String>`: deletes
    `*.sqlite.bak-*` and returns the freed bytes.
  - `#[tauri::command] pub fn storage_empty(app: AppHandle, state: State<AppState>) -> Result<(), String>`.
    - It refuses under the same busy rules.
    - It writes `pending-empty` (content: the open import ids, one per line) and calls `app.restart()`.
  - `pub fn apply_pending_empty(dir: &Path)`:
    - When `pending-empty` exists, it deletes:
      - `history.sqlite` with `-wal`/`-shm`;
      - `*.sqlite.bak-*`;
      - `firmware-logs/`, `parameters/`, `exports/`, `recordings/`;
      - the folders in `imports/` not listed in the marker.
    - Errors are collected into `pending-empty.err`; it never panics.
  - `pub fn finish_pending_empty(store: &Store, dir: &Path)`:
    - When `pending-empty` exists: it calls `store.empty_data()`.
    - On success it removes `pending-empty` and `pending-empty.err`, but only if `apply` reported no error.
    - Otherwise it writes the error to `.err` and keeps the marker.

- [ ] **Step 1: Write failing tests** in `storage.rs`:
  - `emptying_keeps_installations_and_open_imports`:
    1. Setup: a temp dir with a real `Store` holding:
       - an installation and a board;
       - one `bms_records` row and one `param_snapshots` row;
       - one completed and one open `import_status` row;
       - one `logs` row;
       - files in every category, and `imports/open1/`.
    2. Write the marker `"open1\n"`, then call `apply_pending_empty` and `finish_pending_empty`.
    3. Assert:
       - the installation and the board are still there;
       - `bms_records` is 1;
       - `param_snapshots` and `logs` are 0;
       - the open `import_status` row is kept, the completed one is gone;
       - `imports/open1` exists, `history.sqlite` does not;
       - the marker and `.err` are gone.
  - `a_failed_empty_keeps_the_marker`: make `parameters` a file without write permission in a read-only parent
    (unix only, `#[cfg(unix)]`). Assert that the marker stays and `pending-empty.err` is non-empty.
  - `deleting_backups_frees_their_bytes`: two `.bak-*` files of 3 B and 4 B give a return value of 7, and both
    files are gone.
- [ ] **Step 2:** `cargo test -p openbms-monitor storage`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test -p openbms-monitor && cargo clippy -p openbms-monitor -- -D warnings`. Expected:
  PASS, no warnings.
- [ ] **Step 5:** Commit: `Speicher freigeben: Aufzeichnung leeren, Migrationskopien löschen, Datenordner beim nächsten Start leeren`.

### Task 4: The About section, backup flow, texts

**Files:**
- Modify: `app/src/api.ts`:
  - types `StorageOverview`, `StorageCategory`, `InstallationData`;
  - functions `storageOverview`, `storageClearRecording(installation: string | null)`, `storageDeleteBackups`,
    `storageEmpty`, each with a demo stub;
  - the demo overview has sample sizes.
- Create: `app/src/views/Storage.tsx`: `StoragePanel` and `DeleteDialog`.
- Create: `app/src/storage.ts`: `backupThenDelete(...)`, pure logic and testable.
- Create: `app/src/storage.test.ts`.
- Modify:
  - `app/src/views/About.tsx`: replace the data-folder panel with `<StoragePanel />`;
  - `app/src/views/Recording.tsx`: a "Manage storage" link that opens About (use the existing view setter);
  - `app/src/i18n.ts`: `about.storage.*` in DE and EN;
  - `tools/gen_help.py`: a topic `topic.storage`, then run `python3 tools/gen_help.py`;
  - `CHANGELOG.md`.

**Interfaces:**
- Consumes: `api.installationExport(id, path, true)` (existing); `open({ directory: true })` from
  `@tauri-apps/plugin-dialog` for the backup folder; `save(...)` for a single installation (pattern
  `InstallationTransfer.tsx:148`, `bundleName`).
- Produces:
  - `export async function backupThenDelete(targets: { id: string; name: string }[], folder: string, exportOne: (id: string, path: string) => Promise<unknown>, del: () => Promise<T>): Promise<T>`.
  - It exports every target into `folder/bundleName(name)`, adding ` (2)`, ` (3)` … when the name is already used
    in this run. It throws on the first failed export without calling `del`.

- [ ] **Step 1: Write failing tests** in `storage.test.ts`:
  - `deletes only after every backup succeeded`: two targets, export resolves; assert `del` is called once after
    two exports.
  - `a failed backup deletes nothing`: the second export rejects; assert `del` is never called and the promise
    rejects.
  - `same names get a suffix`: two targets named "A"; assert the paths end in `A….zip` and `A… (2).zip`.
- [ ] **Step 2:** `npx vitest run src/storage.test.ts`. Expected: FAIL.
- [ ] **Step 3: Implement `storage.ts`, then the UI.**
  - **`StoragePanel`:**
    - total and free space;
    - one line per category with its size;
    - recording: time span, rows and the current retention;
    - the BMS archive as its record count;
    - "data deleted, file not yet smaller" when `reclaimable > 0` after a clear;
    - `empty_error` in red when set;
    - buttons: "Clear recording …" (pick one installation or all), "Delete migration backups …" (only when
      backups > 0), "Empty data folder …", "Show folder".
  - **`DeleteDialog`:** names the scope and the sizes, then three buttons.
    - Clear and empty back up the installations with `has_data`. For "one installation" the target is that
      installation.
    - The empty dialog also says that parameter files, parameter states and exports are not in the ZIP, and shows
      "Show folder".
    - The backup-deletion dialog says that the way back before a migration goes with them, and has no backup
      button: there is nothing to put in a ZIP.
  - **After an action:** show the sizes before and after (from a fresh `storageOverview`).
- [ ] **Step 4:** Run `npx tsc --noEmit && npx vitest run`. Expected: all pass. Then check in the browser preview
  (demo mode) at 900 px and 1280 px: no overflow, the dialogs open.
- [ ] **Step 5:** Commit: `Über: Daten & Speicher (Übersicht, Aufzeichnung leeren, Datenordner leeren, ZIP-Sicherung)`.

### Task 5: Codex review and fixes

- [ ] Write the brief `.orch/brief-storage-review.md`: the base commit before Task 1, the spec, the plan, and the
  Review Focus list. The report goes to `.orch/review-storage.md`.
- [ ] Submit it to the Codex pane, then wait for the report file.
- [ ] Fix the confirmed findings, rerun the full test suites, commit and push.
