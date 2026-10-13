# OpenBMS Monitor

Cross-platform monitor for the Seplos BMS V2.0 (10C/10E, EMU1101). The monitor is read only unless maintenance functions are switched on (firmware view). The separate `openbms-flash` CLI can write firmware only after its own validation and explicit confirmation. Pre-release (0.1), public repo. Protocol knowledge comes from [seplos-emu1101-docs](https://github.com/privatecoder/seplos-emu1101-docs); the README describes features and the two buses.

## Layout

- `crates/openbms-proto`: frames, checksums, Seplos ASCII commands (telemetry 0x42, status 0x44, params 0x47, intra-pack 0x5A, serial numbers 0xA2/0xA4), Modbus RTU (master system values, battery serial number), parameter decoding.
- `crates/openbms-transport`: serial and TCP transport (5 s connect timeout, write timeout).
- `crates/openbms-cli`: read-only command-line poller for debugging.
- `crates/openbms-flash`: standalone EMU1101 updater (probe, `.ehex` validation and dry-run, confirmed flash); the app's firmware view uses it as a library (`app/src-tauri/src/firmware.rs`).
- `app/src-tauri`: Tauri 2 backend. `lib.rs` holds connection, polling thread, watchdog and all commands; `history.rs` the SQLite history (recording, retention/compaction, series, export/import); `firmware.rs` firmware commands, progress events, flash guard; `maint.rs` / `maint_store.rs` the pack bus panel's backend (live status, pack scan, master marker over the CAN socket, right-bank check, bootloader-pending records; design `docs/superpowers/specs/2026-10-13-simple-flash-flow.md`).
- `app/src`: React 19 + Tailwind 4 frontend.
  - `views/Firmware.tsx` and `firmware.ts`: firmware update view (behind maintenance functions) and its step logic; `views/PackBusPanel.tsx`, `maint.ts`, `maintStore.ts`: the one-screen pack bus panel, installations (address book) and the banner.
  - `api.ts`: typed Tauri commands, plus a demo API (`isDemo`, data from `demo/capture.json`) used when running in a plain browser.
  - `views/`: Dashboard (overview, towers, pairs), PackDetail, Diagnosis, Parameters, History, Recording, Cells, CellAssign, GroupEditor, Sidebar.
  - `pairs.ts`, `diag/`, `cells/` (datasheet rules, pack and parameter checks), `params.ts`, `groups.ts`, `history/` (charts, client-side helpers).
  - `i18n.ts`: all UI strings, German and English side by side.
  - `help.tsx`: info icons and help pages, rendered from `help/<lang>/<id>.md`.
- `help/`: **generated**, do not edit. Source is `tools/gen_help.py` (topics, alarms) and `tools/param_help.py` (parameters, switch bits).
- `data/cells/cells.json` + `data/schema/cells.schema.json`: bundled cell database; rules in `data/cells/README.md`.

## Commands

```sh
cd app
npx tsc --noEmit -p .          # type check
npm test                       # vitest
npx tauri dev                  # desktop app with hot reload
npm run dev                    # browser only, demo data
cargo test --workspace         # Rust tests (from repo root)
cargo clippy --workspace
python3 tools/gen_help.py      # regenerate help/ after editing the help tables
```

## Conventions

- Every user-visible string exists in German and English (`i18n.ts`, both help languages). German is the primary language of the user.
- Every value or feature that needs explaining gets a help topic in `gen_help.py` and an `InfoIcon` next to it. Help explains relationships and what to do, not only definitions.
- Content is generic: help texts, examples, code comments and test names never cite a particular installation (no dates, pack numbers or readings from someone's bank). Use neutral examples ("pack A / pack B, 40 mV at 10 A").
- No private files in the repo (own parameter exports, poll logs, recordings). Tests use synthetic data.
- No documentation of modified firmware here; that lives elsewhere.
- No database or settings migrations before the first release: on an unsupported schema the old file is moved aside.
- Match the surrounding code: short comments that explain why, compact components, Tailwind tokens from `index.css` (`text-muted`, `bg-sunken`, `text-alarm`, …).
- Checks that compare packs wait until every found pack has answered, so a partial picture right after connecting does not raise false warnings.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Pushing to `main` is routine.

## Open roadmap

- CI and builds for macOS and Windows, first pre-release.
- Writing parameters (last, behind explicit confirmation).
