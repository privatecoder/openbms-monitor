# Cell analysis per pack, external active balancers

Status: r2, 2026-10-17 (r1 reviewed: `.orch/reviewer-codex-6-sol-cell-analysis-balancers-r1.md` in the knowledge base; rulings below win over the text above them).

Two features that depend on each other: the cell analysis must know when a balancer moves the cells, and the balancer timeline is shown next to the cells. One document, one review; shipped in two steps (B needs the activity rule of A only for marking).

## A. External active balancers (devices, manual values first)

### Why

Many banks have an active balancer permanently wired to each pack (e.g. NEEY, Heltec, JK). It moves charge between cells whenever its own conditions hold, independent of the BMS's passive balancing. Without knowing it, a cell analysis reads its work as cell behaviour, and the user cannot see when it runs.

### Design

- **Devices per installation.** A new installation setting key `devices` (frontend types, backend read/write, installation export/import, like `notifications`): a list of device definitions. First and only kind for now: `active-balancer`. A definition has `id`, `name` (free, e.g. „NEEY 4A“), `kind`, `profile: null` (reserved for later connection profiles: how to connect, which values exist, how to read them; not part of this step), and the manual settings:
  - `start_mv`: cell voltage from which it works (the highest cell must be ≥ this), optional;
  - `trigger_mv`: spread (highest − lowest cell) from which it starts;
  - `stop_mv`: spread below which it stops, optional (default = `trigger_mv`, no hysteresis);
  - `when`: `always` | `charging` (current > +1 A) | `not-discharging` (current ≥ −1 A);
  - `max_a`: balance current, informative only (shown, not used in the rule).
- **Assignment: one balancer per pack.** Each pack of the installation can have at most one device assigned (`assign: {pack: device_id}`); one definition can be assigned to many packs (twelve identical balancers = one definition). Removing a definition removes its assignments after a confirmation naming the packs.
- **Editing:** a „Geräte“ block on the installation card (Installations view), like „Anschlüsse“: list of definitions with add/edit/delete, and a pack grid to assign („Pack 00–11: NEEY 4A“, quick choice „allen Packs“). Defaults when adding an active balancer: empty name, `trigger_mv` 5, `stop_mv` empty, `start_mv` empty, `when` always.
- **Activity, computed (not measured).** For each recorded raw row of a pack with an assigned balancer: active when `when` holds for the row's current, the highest cell ≥ `start_mv` (if set), and the spread ≥ `trigger_mv`, staying active until the spread < `stop_mv` (state carried row to row within one session; a gap > 3 poll periods or a session change resets to inactive). One pure function, unit-tested, used by B as well. Minute rows (compacted) use `spread_hi`/`spread_lo` and `i_lo`/`i_hi`: active if the condition holds for the minute's extremes in a way that is certain, „unbekannt“ otherwise; rows without these columns (old) are „unbekannt“.
- **Display in the pack details (`PackDetail`):** a timeline over the chosen period with two lanes: „Externer Balancer (berechnet)“ from the rule above, and „BMS-Balancing (gemeldet)“ from the BMS's per-cell balancing bits (status `balancing`), with the number of cells being balanced. Below: the share of time active in the period. The computed lane always carries „berechnet aus Zellspannungen, nicht gemessen“ as long as `profile` is null.
- **Recording of the BMS balancing bits:** a migration adds `balancing INTEGER` (bitmask) to `pack`; new raw rows fill it; compaction keeps the OR of the minute. Older rows: NULL, lane shows „keine Daten“.

## B. Cell analysis

### Why

A chart of 16 cell lines shows that cells differ, not what it means. In the flat part of the LFP curve (≈3.30–3.35 V) differences of 5–15 mV come from internal resistance and state of charge; capacity differences show only near full and near empty. The goal: say per cell whether it matches, has an SOC offset, likely less capacity, higher resistance, or looks like a measurement/contact issue, and say when the data do not allow a statement.

### Design

Read-only, from the recording (raw rows only, i.e. within `keep_full_days`); for the chosen installation, period as in the health report (7 / 30 / 90 days). Backend command `cell_analysis(site, pack, from, to)` on the reader connection, `spawn_blocking`, like `health_report`.

Per cell, deviation from the pack's **median cell of the same row** (mV), aggregated in four situations:

1. **Rest:** row and the row before with |current| < 2 A (as in the report), and no balancer active (A) and no BMS balancing bit for that cell. Median deviation → SOC offset in the flat region.
2. **Top:** rows with the highest cell ≥ 3.40 V while charging. Median deviation and the order in which cells pass 3.45 V.
3. **Bottom:** rows with the lowest cell ≤ 3.15 V while discharging. Median deviation.
4. **Load steps (resistance):** pairs of consecutive rows (≤ 3 poll periods apart) where the pack current changes by ≥ 20 A: per cell ΔU/ΔI (mΩ), median over all steps; reported relative to the pack median (%), and absolute.

Classification per cell (thresholds as constants, tested):
- **passend:** |rest| ≤ 5 mV, resistance within ±20 % of the median, top/bottom (where available) within ±15 mV;
- **Ladestand-Versatz:** top deviation and bottom deviation have the same sign (both high or both low), resistance normal;
- **Kapazität vermutlich kleiner:** high at top and low at bottom (both needed);
- **Widerstand erhöht:** resistance ≥ 130 % of the median over at least 10 steps;
- **auffällig (Messung/Kontakt?):** a constant offset that does not depend on current or SOC (deviation at rest ≈ under load ≈ top), or jumps of ≥ 20 mV between consecutive rows that no other cell shows.

A situation without enough data (fewer than 30 rows, or fewer than 10 load steps) is shown as „keine Aussage: …“ with the reason („noch keine Ladung über 3,40 V aufgezeichnet“, „zu wenige Laststufen“). Rows where a balancer of A is active are left out of rest/top/bottom and counted; the analysis says how many were left out.

Display:
- **Pack details:** a heatmap cells × time (deviation from the row median, colour scale symmetric, ±20 mV clipped), the balancer timeline of A aligned below it, and a table per cell: rest / top / bottom deviation, resistance (mΩ, %), classification with the reason in words.
- **Health report („Bericht“):** a section „Zellen“ listing per pack the cells not „passend“ with their class; CSV export extended.

## r2 rulings (review)

- **Cell positions (review 1):** the import keeps positions: a row whose `cell_voltages` holds a non-numeric entry is stored without cells (`cells` NULL), never shortened. The analysis uses only raw rows whose cell count equals the pack's usual count (the most frequent count in the period); others are counted as „ohne vollständige Zellwerte“.
- **Same session (review 2):** load-step pairs, the rest predecessor and the balancer state carried row to row are formed only within one `session`.
- **Assignments and settings in time (review 3):** `devices` keeps revisions: every change of a definition's settings and every assignment change appends `{from: ms, …}`; the activity of a row uses the revision valid at its time. Before a pack's first assignment the lane shows „kein Balancer eingetragen“ and the analysis excludes nothing for it. Deleting a definition ends its assignments at that time (`to`), the past stays computable.
- **Compacted minutes (review 4):** compaction stores `balancing` = OR of the minute and `bal_rows` = number of raw rows with any bit set (`n` is the number of rows); a minute shows „mindestens einmal balanciert, Anteil bal_rows/n“, never a count of cells at once. A row without a parsed status keeps NULL (not 0); NULL counts neither way.
- **Round trip (review 5):** `balancing`/`bal_rows` go through the installation merge, the JSONL export and import and the installation ZIP; a test runs a ZIP round trip with bits set.
- **Capacity vs resistance (review 6):** top/bottom deviations are corrected by the cell's own measured resistance: `dev − (R_cell − R_median) × I`. „Kapazität vermutlich kleiner“ needs the resistance from ≥ 10 load steps and the corrected top and bottom both beyond ±15 mV; without enough steps the class is „mehrdeutig (Widerstand oder Kapazität)“.

## Out of scope

Reading a balancer over Bluetooth or RS485 (later, via `profile`); balancers attached only now and then; writing balancer settings; per-cell capacity in Ah.

## Review questions

1. A: can the computed activity be shown or used as if measured, or leak into another installation (export/import, assignments when packs change)?
2. B: anything in the classification that would mislead (median of a row with a missing cell, load steps across a session gap or a connection change, rows of compacted minutes, the BMS balancing bits on firmware that does not report them)?
3. Migration: anything harmful in adding `balancing` to `pack` and to the compaction?
