import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import bundledJson from "../../../data/cells/cells.json";
import { api } from "../api";
import type { Cell } from "./types";
import { dbErrors } from "./validate";
import { sameJson } from "./json";

export { sameJson };

export const bundled = bundledJson as unknown as Cell[];
const bundledById = new Map(bundled.map((c) => [c.id, c]));

export type Origin = "bundled" | "overridden" | "own";
export interface DbEntry { cell: Cell; origin: Origin; original?: Cell }

export interface ImportPlan { create: Cell[]; replace: Cell[]; unchanged: Cell[] }

/**
 * Bundled cells merged with the user's own and overridden entries (same id = override).
 * Mutations are refused until the user file has loaded, and refused for good if it was invalid,
 * so a failed or late load can never overwrite the stored data. A failed write rolls back.
 */
export function useCellDb() {
  const [user, setUser] = useState<Cell[]>([]);
  const [state, setState] = useState<"loading" | "ready" | "broken">("loading");
  const [error, setError] = useState<string | null>(null);
  const userRef = useRef<Cell[]>([]);
  const queue = useRef<Promise<unknown>>(Promise.resolve());

  useEffect(() => {
    api.loadUserCells()
      .then((data) => {
        const errs = dbErrors(data);
        if (errs.length) { setState("broken"); setError(`cells.user.json: ${errs.slice(0, 3).join("; ")}`); return; }
        userRef.current = data as Cell[];
        setUser(data as Cell[]);
        setState("ready");
      })
      .catch((e) => { setState("broken"); setError(String(e)); });
  }, []);

  /** Writes are serialized; state changes only after the file was written. */
  const persist = useCallback((next: Cell[]) => {
    const run = queue.current.then(async () => {
      if (state !== "ready") throw new Error(state === "loading" ? "still loading" : "the user database could not be read; not overwriting it");
      await api.saveUserCells(next);
      userRef.current = next;
      setUser(next);
      setError(null);
    });
    queue.current = run.catch(() => undefined);
    return run;
  }, [state]);

  const entries = useMemo<DbEntry[]>(() => {
    const userById = new Map(user.map((c) => [c.id, c]));
    const merged: DbEntry[] = bundled.map((c) =>
      userById.has(c.id) ? { cell: userById.get(c.id)!, origin: "overridden", original: c } : { cell: c, origin: "bundled" });
    for (const c of user) if (!bundledById.has(c.id)) merged.push({ cell: c, origin: "own" });
    return merged.sort((a, b) => `${a.cell.manufacturer} ${a.cell.model} ${a.cell.variant ?? ""}`.localeCompare(`${b.cell.manufacturer} ${b.cell.model} ${b.cell.variant ?? ""}`));
  }, [user]);

  const save = (cell: Cell) => persist([...userRef.current.filter((c) => c.id !== cell.id), cell]);
  const remove = (id: string) => persist(userRef.current.filter((c) => c.id !== id));

  /** What an import would do; entries identical to their bundled or stored version are skipped. */
  const planImport = (cells: Cell[]): ImportPlan => {
    const plan: ImportPlan = { create: [], replace: [], unchanged: [] };
    const mine = new Map(userRef.current.map((c) => [c.id, c]));
    for (const c of cells) {
      const current = mine.get(c.id) ?? bundledById.get(c.id);
      if (current && sameJson(current, c)) plan.unchanged.push(c);
      else (current ? plan.replace : plan.create).push(c);
    }
    return plan;
  };
  const applyImport = (plan: ImportPlan) => {
    const changed = [...plan.create, ...plan.replace];
    const ids = new Set(changed.map((c) => c.id));
    return persist([...userRef.current.filter((c) => !ids.has(c.id)), ...changed]);
  };

  return {
    entries, user, ready: state === "ready", loading: state === "loading", error, setError,
    save, remove, planImport, applyImport, isBundledId: (id: string) => bundledById.has(id),
  };
}
