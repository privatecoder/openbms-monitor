import { describe, expect, it, vi } from "vitest";
import { backupPaths, backupThenDelete } from "./storage";

const a = { id: "1", name: "A" };
const b = { id: "2", name: "B" };

describe("backup before delete", () => {
  it("deletes only after every backup succeeded", async () => {
    const order: string[] = [];
    const exp = vi.fn(async (id: string) => { order.push(`zip ${id}`); });
    const del = vi.fn(async () => { order.push("delete"); return 7; });
    await expect(backupThenDelete([a, b], "/out", exp, del)).resolves.toBe(7);
    expect(order).toEqual(["zip 1", "zip 2", "delete"]);
  });

  it("a failed backup deletes nothing", async () => {
    const exp = vi.fn(async (id: string) => { if (id === "2") throw new Error("disk full"); });
    const del = vi.fn(async () => 0);
    await expect(backupThenDelete([a, b], "/out", exp, del)).rejects.toThrow("disk full");
    expect(del).not.toHaveBeenCalled();
  });

  it("same names get a suffix", () => {
    const now = new Date(2026, 10, 3);
    expect(backupPaths([a, { id: "3", name: "A" }], "/out/", now)).toEqual(["/out/openbms-A-2026-11-03.zip", "/out/openbms-A-2026-11-03 (2).zip"]);
  });
});
