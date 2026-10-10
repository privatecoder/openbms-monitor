import { describe, expect, it } from "vitest";
import cells from "../../../data/cells/cells.json";
import { cellErrors, dbErrors } from "./validate";

describe("schema validation in the app", () => {
  it("accepts the bundled database", () => expect(dbErrors(cells)).toEqual([]));
  it("accepts a bundled entry and rejects a broken one", () => {
    expect(cellErrors(cells[0])).toEqual([]);
    const broken = structuredClone(cells[0]) as Record<string, unknown>;
    (broken.voltage as Record<string, unknown>).nominal_v = null;
    expect(cellErrors(broken).join()).toMatch(/nominal_v/);
  });
});
