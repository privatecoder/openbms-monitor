# Installations own the data

Status: approved r3, 2026-10-14 (r1/r2 reviewed: `.orch/reviewer-codex-6-sol-installations-r1.md`, `-r2.md` in the knowledge base; their decisions are taken over below).

## Why

An installation is fixed when flashing firmware, but everything else the app knows about a bank is scattered and keyed differently:

| Data | Today | Key |
|---|---|---|
| Recording (telemetry, events) | `history.sqlite` | `sessions.site` / `site_packs.site` = raw endpoint text as typed; `pack` PK `(pack,t)`, `system` PK `(t)` global |
| Installations, links, master serial, master read, bootloader pending | `maintenance.sqlite` | installation id; endpoints normalised; one endpoint may belong to several installations |
| Cell type per pack, groups, pairs, diagnosis | localStorage `cellTypes:/groups:/pairs:/diag:${site}` | site string |
| Serial numbers per pack | memory only | – |
| Firmware logs, `maint-events.log` | files in `firmware-logs/` | none |
| Parameter snapshots (branch `feat/write-params-serials`) | `history.sqlite` | installation + address |
| User cell models | `cells.user.json` | global (stays global) |

Consequences: the same USB adapter at the bench board and at the real bank mixes their recordings and assignments; compaction averages rows of different sites in the same minute; localStorage data is not exported; away from the bank nobody can say what is set in pack 07.

Goal: **the installation (stable id) is the one key for everything about a bank**, stored locally, viewable without a connection, exportable.

## Design

### 1. Choose the installation before connecting

- The sidebar shows the installation above the connection fields (select: known installations, „Neue Anlage …“, „Nur ansehen“). The selection is kept across app starts and always visible. **A serial port never changes it** (a USB adapter moves between banks). A gateway (TCP) linked to exactly one non-archived installation preselects it; linked to several → the current one if it is among them, else the user picks. Archived installations are never preselected. An endpoint not yet linked to the chosen installation is linked by the backend inside `connect`, after the port opened and before the first recorded row (one line „Anschluss wird Anlage X zugeordnet“, no dialog).
- `connect` gets the installation id; backend and recorder use it for this whole connection. Changing the selection while connected does nothing until the next connect (the select is disabled while connected). Reconnects reuse it without asking.
- **„Nur ansehen“**: temporary, nothing stored (no recording, no serials, no snapshots), no installation-bound writes (flash, parameter write). For a quick look at a foreign bank.
- Demo mode stays storage-free.
- Late answers (serials read after connect) are stored with the connection's installation id, never with a later selection.
- The bank check by the master's BMS serial (String 2) stays for flashing (pack bus only; on the CAN socket String 2 is not readable, so no identity check there).

### 2. Packs: place and board

- A pack is a **place** `(installation, address)`. A **board** at a place is recorded in `boards(installation, address, bms_serial, battery_serial, first_seen, last_seen)` from every successful serial read.
- Only a real String 2 (not empty, placeholder or no answer) identifies a board. A real serial that differs from the newest board at that place → new board row and a notice „Andere Platine an Adresse 07 (Seriennummer …)?“. Snapshots and per-pack notes from an older board are shown as „von vorheriger Platine (…)“, not as the current state. Serial not readable → „Platine ungeklärt“, no change claimed.
- The app's own successful String 2 write (0xA5) changes the serial of the existing board row and logs the old value as an alias; it is not a board swap.
- Cell type stays per place (it describes the cells, which stay when a board is swapped).

### 3. Stores

- **Keep the file name `maintenance.sqlite`**; introduce `user_version` migrations (transaction, backup copy `maintenance.sqlite.bak-<version>` before each). Existing data incl. `bootloader_pending` kept. New tables: `boards`, `place(installation, address, cell_type JSON)`, `layout(installation, groups JSON, pairs JSON)`, `diagnosis(installation, data JSON)`, `param_snapshots(installation, address, bms_serial, at, kind read|written|refused, block BLOB)`, `logs(installation, address NULL, at, kind, result, file)`, `installations.archived`.
- **`history.sqlite`: real migration, not a version bump** (a bump sets the old file aside). Before `History::open` (which compacts at start): keys become per site — `pack` PK `(site, pack, t)`, `system` PK `(site, t)` (site = installation id or legacy source id), compaction groups by site; `sessions.site` and `site_packs.site` rewritten together. Backup copy before.
- Parameter snapshots move from `history.sqlite` to `maintenance.sqlite` if the param branch put them there.
- New firmware logs are named `<UTC>_<installation-id>_pack<addr>_<what>.log` and indexed in `logs`; `maint-events.log` lines carry the installation id. Old logs are listed as „ohne Anlage“, never assigned by guessing.

### 4. Migration of old data (once, visible)

- Every old site key (history sites and the four localStorage keys, grouped by the same site string) becomes its own **legacy installation** named after the endpoint text, marked „übernommen“ — also when its endpoint is linked to exactly one installation (the same adapter may have served two banks; mixed data cannot be separated afterwards).
- If the endpoint is linked to exactly one installation, the „Anlagen“ view offers „Mit Anlage X zusammenführen“ (one click, after the user has looked). Merge adds, never overwrites: JSON settings of the target win; the legacy source's values are kept in the merge log.
- localStorage keys are removed only after a successful, logged move; the migration is idempotent (re-run safe).

### 5. Installations view (always visible)

- List of installations (archived ones hidden behind a toggle): name, endpoints, master serial, last connect. Rename, merge, **archive** (default; keeps everything), delete only for an installation without open work (no marker session, no bootloader pending) and with a clear list of what goes.
- Per installation the pack table: address, current board serials (and „vorherige Platine“ when swapped), cell type, last parameter snapshot (date, kind, link), last flash (date, image, result, log link).
- Maintenance setup keeps only maintenance parts.

### 6. Offline views

- The chosen installation (not the live connection) drives the offline views: Parameters shows per pack the newest snapshot of the current board as a column with date and kind; compare, datasheet check and XML save work; editing needs a connection to that installation. History shows the chosen installation (its own site select goes).
- The live monitor always shows the actually connected bus.

### 7. Export / import

- „Anlage exportieren“: one **ZIP** (`openbms-<name>-<date>.zip`): `manifest.json` (format version, installation id, export id, checksums), `installation.json` (installation, endpoints as information, master serial, places, boards, layout, diagnosis, snapshots, log index), `logs/…`, optional `recording.jsonl` (off by default). Streamed.
- „Anlage importieren“: unpack to a temp dir, check paths and checksums, then one DB transaction; files under unique names. Same installation id → merge into it; else new installation; same master serial on another installation → merge offered, not automatic. An import status row keyed by the export id is marked complete only after both databases and all files are done; an interrupted import is resumed with the same id (every step idempotent). Repeated imports deduplicated by export id and per-row identity (snapshot `(installation,address,bms_serial,at,kind,block hash)`, recording `(site,pack,t)`). Imported endpoints are information only, not active links.
- The existing recording export (CSV/JSONL per time span) stays.

## Phases (each releasable)

1. History keys per site + compaction per site + real migration before `History::open` (backend only, invisible; sites stay the endpoint text).
2. `maintenance.sqlite` migrations and new tables, prepared only: no active data moved, old keys still used.
3. Released together: `connect(installation_id)`, installation select before connect, „Nur ansehen“, the one-time idempotent legacy migration (history sites and localStorage keys → legacy installations), boards from serial reads, snapshots moved, Installations view, archive/merge, offline Parameters/History.
4. ZIP export/import.

## Out of scope

Sync between computers, cloud, user cell models (stay global with their own export), recording format changes beyond the keys.
