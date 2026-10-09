# Bundled cell database

`cells.json` is an array of cell models, validated against [`../schema/cells.schema.json`](../schema/cells.schema.json). Entries of the bundled set must have every required value marked `"verified": true` in `provenance`. Users can override bundled entries locally and add their own.

Key rules (full description in the schema):

- **No `null`.** A value the datasheet does not state is omitted; its `provenance` entry stays with `verified: true` and a note "not specified in datasheet …".
- **Rates always with basis** `C` or `P`, never amperes.
- `datasheet.source_type` is who authored the document; `datasheet.url` is where it was read.
- **`charge_derating`** holds the datasheet's "charge current by temperature" table: `basis` plus `points`, each with `value` and either `temp_c` or a band (`temp_min_c`/`temp_max_c`), optionally `soc_min_pct`/`soc_max_pct`.
  - **Between two points the lower value applies** (also between SOC columns and on shared band bounds). This is an app rule, not a manufacturer statement: only the printed points are documented, and the lower neighbour is the only value certain not to exceed what the manufacturer allows.
  - Outside the table, charging is not allowed.
  - Linear interpolation only if a datasheet explicitly allows it: `"interpolation": "linear"` with proof in `provenance`.
- `charge_min_c`/`charge_max_c` are the datasheet's range limits, not a charge permit; where a derating table exists, the effective charge range is where it allows more than 0.
