import { describe, expect, it } from "vitest";
import cells from "../../../data/cells/cells.json";
import { dbErrors } from "./validate";
import { sameJson, unverifyChanged } from "./json";
import type { Cell } from "./types";

const db = cells as unknown as Cell[];
const lf280k = db.find((c) => c.id === "eve-lf280k-rev-b-2021")!;

describe("review guards", () => {
  it("rejects duplicate ids and inverted bands", () => {
    expect(dbErrors([lf280k, lf280k]).join()).toMatch(/duplicate id/);
    const bad = structuredClone(lf280k);
    bad.charge_derating = { basis: "C", points: [{ temp_min_c: 30, temp_max_c: 10, value: 0.5 }, { temp_c: 5, soc_min_pct: 80, soc_max_pct: 20, value: 0.1 }] };
    const errs = dbErrors([bad]).join();
    expect(errs).toMatch(/temp_min_c > temp_max_c/);
    expect(errs).toMatch(/soc_min_pct > soc_max_pct/);
  });
  it("drops 'verified' when a value changes but its proof was left as it was", () => {
    const next = structuredClone(lf280k);
    next.voltage!.charge_cutoff_v = 3.6;
    const out = unverifyChanged(next, lf280k);
    expect(out.provenance!["voltage.charge_cutoff_v"].verified).toBe(false);
    expect(out.provenance!["voltage.nominal_v"].verified).toBe(true);
  });
  it("keeps 'verified' when the proof was updated together with the value", () => {
    const next = structuredClone(lf280k);
    next.voltage!.charge_cutoff_v = 3.6;
    next.provenance!["voltage.charge_cutoff_v"] = { verified: true, note: "rechecked p. 4" };
    expect(unverifyChanged(next, lf280k).provenance!["voltage.charge_cutoff_v"].verified).toBe(true);
  });
  it("invalidates untouched proofs when the datasheet URL changes", () => {
    const next = structuredClone(lf280k);
    next.datasheet.url = "https://example.com/other.pdf";
    expect(Object.values(unverifyChanged(next, lf280k).provenance!).every((p) => !p.verified)).toBe(true);
  });
  it("invalidates untouched proofs when the datasheet revision or hash changes", () => {
    const next = structuredClone(lf280k);
    next.datasheet.revision = "C";
    expect(unverifyChanged(next, lf280k).provenance!["voltage.nominal_v"].verified).toBe(false);
  });
  it("compares JSON independent of key order", () => {
    expect(sameJson({ a: 1, b: [1, { c: 2 }] }, { b: [1, { c: 2 }], a: 1 })).toBe(true);
    expect(sameJson({ a: 1 }, { a: 1, b: undefined })).toBe(false);
  });
});
