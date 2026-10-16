# Parameters to several packs, health report, notifications

Status: r2, 2026-10-16 (r1 reviewed: `.orch/reviewer-codex-6-sol-multi-write-health-alerts-r1.md` in the knowledge base; findings marked „review n“).

Three independent features; one document because they share the review. Each is shipped on its own.

## A. Write a parameter change to several packs

### Why

A bank of many packs usually needs the same change on every pack (e.g. a temperature limit). Today a change is written to the one pack whose column is being edited; the same change on twelve packs means twelve edits. The clock already has „Uhr aller Packs stellen“ (series bound to installation and connection since `1bd1764`); parameters lack that.

### Design

- **Read columns carry their link (review 9).** `ParametersView.read` tags each read column with `linkNow()` at the start and drops an answer that arrives after the link changed. Only columns of the current link are offered as targets.
- In the edit bar of the Parameters view (column being edited, pencil in the column head), next to „Auf Pack NN schreiben“: **„Auch auf weitere Packs …“** opens a pick list of the other packs **that have a read column of this connection** (fresh 0x47 shown in the view). Packs without a read column are listed greyed out with „erst lesen“. Quick choices: „alle gelesenen“, „keine“.
- **What is written to the other packs is the change, not the source block.** The change is the set of parameters and switch bits the user changed against the source pack's read (`edited` vs `base`). For each target pack T: `base_T` = T's read column, `edited_T` = `base_T` with exactly those parameters/bits set to the edited values. The existing backend command `write_parameters(address, installation, base_T, edited_T, restore=false)` does the rest per pack, unchanged: pack reservation, bank check (`check_target`), fresh 0x47, rebase (stale → nothing written for that pack), 0xA1, snapshots, read-back. Byte 0 (address) and everything not changed stay the target's own.
- **Checks per target before sending:** the same blockers and cross-checks as the single write (`paramWrite.ts`: range, 0xA1 rules, the P58 capacity rule, datasheet check if chosen) are evaluated on `edited_T`. A target that fails is skipped with its reason; the others go on. One confirmation for the whole run lists the change once and the target packs („P26 −10 °C → 0 °C auf Pack 00, 01, …, 11“).
- **Capacity reset per target (review 2).** Every accepted 0xA1 resets the learned full capacity to P58 when they differ by ≥ 10 Ah (`capacityResets`), also when P58 is not part of the change. Before the confirmation the app takes each target's learned full capacity from this link's live telemetry (fresh, same `linkNow()`), else reads it (`packLive`), and names the affected packs in the confirmation („Bei Pack 03, 07 setzt das Schreiben die gelernte Vollkapazität auf P58 zurück“); a pack whose value could not be read is named as „Wirkung auf die Vollkapazität ungeklärt“.
- **Order and stopping:** the source pack first, then the targets in address order, one after another (never in parallel). The series takes `linkNow()` at the start and stops before the next pack if installation or connection changed (as the clock series). A „Abbrechen“ button stops before the next pack. **A refusal (review 1):** after RTN 06 the switches sent may already be live in RAM but not stored (`ParamWrite::Refused { stored }`). The series **pauses at that pack** and shows the existing `RefusedPanel` for it (differing switches, „Gespeicherte Schalter wiederherstellen“); the user then chooses „Weiter mit den übrigen Packs“ or „Abbrechen“. A refusal without differing switches is only reported and the series goes on.
- **Result:** one line per pack under the bar: „geschrieben und rückgelesen“, „nicht übernommen: P26“ (read-back differs), „abgelehnt (RTN 06 …)“, „übersprungen: <Grund>“ (blocker, stale, bank, busy), „nicht erreicht“ (I/O / no answer), „abgebrochen“. The read columns are replaced by the read-backs as for a single write.
- Master with slaves on RS485-1/2: a polling master does not answer (known); its line says so with the existing hint (maintenance halt / DIP 5–8).

## B. Health report from the recording

### Why

How a bank ages shows over weeks: cell spread per pack, how the current splits within each pair under load, how fast cycles count. The recording (`history.sqlite`, table `pack`) already holds every value; nothing evaluates it over time.

### Design

A new tab **„Bericht“** in the Verlauf view (or a section under it), for the chosen installation and a period (30 days default; 7 / 30 / 90 / all). Read-only, from the recording only; works offline.

**Compacted minutes (review 3, 7).** `compact_packs` keeps max of cell_max, min of cell_min and the mean current per minute, so spread and rest are not recoverable from old minute rows. A migration adds to `pack`: `spread_lo`, `spread_hi` (min/max of cell_max − cell_min over the rows of the minute; for a raw row both equal its own spread) and `i_lo`, `i_hi` (min/max current). New rows fill them; compaction aggregates them (min/max), and from now on keeps the **last** cycle count of the minute instead of the max. Old compacted rows have NULL there and are left out of spread/rest figures; the report says from which date on spread data exist.

Per pack, per day (local days):
- **Cell spread** (cell_max − cell_min, mV): median and 95th percentile over the day's rows, and the same restricted to **rest** (`i_lo` > −2 A and `i_hi` < 2 A; for raw rows also the row before at rest, to leave out load sag; for minute rows `spread_hi` is used). Rest spread is the comparable number; under load it shows as secondary.
- **Cycles**: the day's last value; the report shows cycles per 30 days per pack (slope over the period, **split into segments where the counter drops**, e.g. after „Historie und Zähler löschen“; no rate across a reset; review 7) and flags a pack whose rate is more than 25 % above the bank median.
- **Temperature**: daily max of temp_max.

Per pair (pairs as stored for the installation, `pairs.ts`), per day:
- **Current share under load (review 6)**: only raw rows (not compacted minutes), each plus-pack row matched with the minus-pack row nearest in time, at most 3 s apart; the pair's total |current| ≥ 2 × 20 A (`PAIR_MIN_A` per pack): mean share of the plus pack and the number of matched samples. A share outside 50 % ± 5 points over at least 30 minutes of samples in a day is marked, unless the **live equalizing rule with its direction** applies (`pairs.ts`: lower SOC gives less when discharging, higher SOC takes less when charging, difference ≥ `EQUALIZE_SOC`); the rule is ported with the same test vectors. Pair figures therefore cover only the raw-data period (`keep_full_days`); the report says so.
- The output voltage gap is **not** in the report (review 5): the recording has the pack voltage, not the port voltage the live rule compares.

Display: one table per section (packs × summary over the period: last value, trend arrow, flag) and a small line chart per selected pack/pair (reuse `history/Chart.tsx`). A flag says in words what is unusual („Pack 08 zählt 30 % mehr Zyklen als der Median“, „Paar 00/01: Pack 00 trägt 58 % bei Last“). Export of the report as CSV.

Backend: one command `health_report(site, from, to)` that runs the aggregation in SQL on the reader connection (not the recorder's write connection) off the async runtime (`spawn_blocking`) and returns per-day aggregates; pair minutes are built in SQL by minute bucket (`t / 60000`) and pack. Compacted ranges (minute values after `keep_full_days`) are fine: they are already per minute. Rows of other installations never mix (keyed by site).

## C. Notifications

### Why

An app that runs all day on a gateway connection sees a protection trip, a pack going silent or a growing spread long before anyone opens it.

### Design

- macOS/Windows notification via `tauri-plugin-notification` (new dependency, permission asked by the OS on first use).
- Settings per installation (stored with the installation settings, `useSettings`), off by default, a small block in the Verlauf → Aufzeichnung view or the installation settings: on/off; spread threshold in mV (default 50 mV, at rest as in B); minutes without data before „Verbindung verloren“ (default 5).
- **Evaluated in the backend poll thread (review 4, answer 3)**, where each answer is fresh and tied to its link generation; the UI's merged live state (which keeps old values on failed polls) is not used. Settings are a new installation setting key `notifications` in frontend types, backend read/write and installation export/import (review 8); the switch is only offered once that works.
- **Only for confirmed data (review 4):** on the pack bus a trigger counts only for a pack whose bank verdict is *known* for this link (not unconfirmed new/moved/foreign, not unchecked). On the CAN socket bus there is no per-pack check: notifications run only if the master's battery serial (String 1, read at connect) equals the one stored for this installation; otherwise the settings show „Für diese Verbindung aus: Bank nicht geprüft (CAN-Buchse)“.
- Triggers, from these poll answers (no extra bus traffic):
  1. a **protection** or **fault** alarm becomes active on a pack (`Severity` "protection"/"fault"); one notification per alarm start („Pack 03: Zell-Überspannungsschutz“), not again until it has cleared;
  2. a pack that answered stops answering for 3 polls in a row („Pack 05 antwortet nicht mehr“), again only after it answered;
  3. rest cell spread of a pack above the threshold for 5 minutes; re-armed when it falls below threshold − 10 mV;
  4. connection lost for the set minutes (the existing „Verbindung verloren“ state).
- At most one notification per pack and trigger per 10 minutes; several packs at once are joined into one notification („Zell-Überspannungsschutz an Pack 03, 04“).
- Clicking the notification brings the app to the front (no deep link).

## Out of scope

Writing different values to different packs in one run; scheduling a report; push to phone/e-mail/MQTT; notifications while the app is closed.

## Review questions

1. A: can a multi-pack run write something to a pack that the single write would not (address byte, P58 rule, a switch bit of the target the user never saw)?
2. B: anything in the aggregation that would mislead (rest detection, pair minutes across compaction or gaps)?
3. C: can a notification fire for data of another bank or installation?
