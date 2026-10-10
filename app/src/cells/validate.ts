import Ajv2020 from "ajv/dist/2020";
import addFormats from "ajv-formats";
import schema from "../../../data/schema/cells.schema.json";
import type { Cell, DeratingPoint } from "./types";

const ajv = new Ajv2020({ allErrors: true, strict: false });
addFormats(ajv);
const validateDb = ajv.compile(schema);
const validateOne = ajv.getSchema(`${schema.$id}#/$defs/cell`)!;

const fmtErr = (e: { instancePath: string; message?: string; params?: Record<string, unknown> }) =>
  `${e.instancePath || "/"} ${e.message}${e.params && "additionalProperty" in e.params ? ` (${e.params.additionalProperty})` : ""}`;

/** Checks the schema cannot express: bands with min > max match nothing and would silently forbid charging. */
function semanticErrors(cell: Cell, at = ""): string[] {
  const out: string[] = [];
  (cell.charge_derating?.points ?? []).forEach((p: DeratingPoint, i) => {
    if (p.temp_min_c !== undefined && p.temp_max_c !== undefined && p.temp_min_c > p.temp_max_c) out.push(`${at}/charge_derating/points/${i} temp_min_c > temp_max_c`);
    if (p.soc_min_pct !== undefined && p.soc_max_pct !== undefined && p.soc_min_pct > p.soc_max_pct) out.push(`${at}/charge_derating/points/${i} soc_min_pct > soc_max_pct`);
  });
  return out;
}

/** Schema and semantic errors of one entry as readable lines ("/voltage/nominal_v must be number"). */
export function cellErrors(cell: unknown): string[] {
  if (!validateOne(cell)) return (validateOne.errors ?? []).map(fmtErr);
  return semanticErrors(cell as Cell);
}

/** Errors of a whole database (array); also rejects duplicate ids. */
export function dbErrors(data: unknown): string[] {
  if (!validateDb(data)) return (validateDb.errors ?? []).slice(0, 20).map(fmtErr);
  const cells = data as Cell[];
  const seen = new Set<string>(), dup = new Set<string>();
  for (const c of cells) (seen.has(c.id) ? dup : seen).add(c.id);
  return [
    ...[...dup].map((id) => `duplicate id "${id}"`),
    ...cells.flatMap((c, i) => semanticErrors(c, `/${i}`)),
  ];
}
