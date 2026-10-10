import { useCallback, useEffect, useMemo, useState } from "react";
import bundledJson from "../../../data/cells/cells.json";
import { api } from "../api";
import type { Cell } from "./types";

export const bundled = bundledJson as unknown as Cell[];
const bundledIds = new Set(bundled.map((c) => c.id));

export type Origin = "bundled" | "overridden" | "own";
export interface DbEntry { cell: Cell; origin: Origin; original?: Cell }

/** Bundled cells merged with the user's own and overridden entries (same id = override). */
export function useCellDb() {
  const [user, setUser] = useState<Cell[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.loadUserCells()
      .then((c) => setUser(Array.isArray(c) ? (c as Cell[]) : []))
      .catch((e) => setError(String(e)))
      .finally(() => setLoaded(true));
  }, []);

  const persist = useCallback(async (next: Cell[]) => {
    setUser(next);
    try { await api.saveUserCells(next); setError(null); } catch (e) { setError(String(e)); }
  }, []);

  const entries = useMemo<DbEntry[]>(() => {
    const userById = new Map(user.map((c) => [c.id, c]));
    const merged: DbEntry[] = bundled.map((c) =>
      userById.has(c.id) ? { cell: userById.get(c.id)!, origin: "overridden", original: c } : { cell: c, origin: "bundled" });
    for (const c of user) if (!bundledIds.has(c.id)) merged.push({ cell: c, origin: "own" });
    return merged.sort((a, b) => `${a.cell.manufacturer} ${a.cell.model} ${a.cell.variant ?? ""}`.localeCompare(`${b.cell.manufacturer} ${b.cell.model} ${b.cell.variant ?? ""}`));
  }, [user]);

  const save = (cell: Cell) => persist([...user.filter((c) => c.id !== cell.id), cell]);
  const remove = (id: string) => persist(user.filter((c) => c.id !== id));
  const importCells = (cells: Cell[]) => {
    const ids = new Set(cells.map((c) => c.id));
    return persist([...user.filter((c) => !ids.has(c.id)), ...cells]);
  };

  return { entries, user, loaded, error, save, remove, importCells, isBundledId: (id: string) => bundledIds.has(id) };
}
