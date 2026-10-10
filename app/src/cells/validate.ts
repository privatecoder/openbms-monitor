import Ajv2020 from "ajv/dist/2020";
import addFormats from "ajv-formats";
import schema from "../../../data/schema/cells.schema.json";
import type { Cell } from "./types";

const ajv = new Ajv2020({ allErrors: true, strict: false });
addFormats(ajv);
const validateDb = ajv.compile(schema);
const validateOne = ajv.getSchema(`${schema.$id}#/$defs/cell`)!;

/** Schema errors of one entry as readable lines ("/voltage/nominal_v must be number"). */
export function cellErrors(cell: unknown): string[] {
  if (validateOne(cell)) return [];
  return (validateOne.errors ?? []).map((e) => `${e.instancePath || "/"} ${e.message}${e.params && "additionalProperty" in e.params ? ` (${e.params.additionalProperty})` : ""}`);
}

export function isCellArray(data: unknown): data is Cell[] {
  return validateDb(data) as boolean;
}

export function dbErrors(data: unknown): string[] {
  if (validateDb(data)) return [];
  return (validateDb.errors ?? []).slice(0, 20).map((e) => `${e.instancePath || "/"} ${e.message}`);
}
