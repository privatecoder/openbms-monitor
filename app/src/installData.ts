import { useEffect, useState } from "react";
import { api, isDemo, type InstallationSettings, type LegacyLocal, type SettingKey } from "./api";

/** Settings an installation owns (cell type per place, groups, pairs, diagnosis), kept by the backend per installation.
 *  With „Nur ansehen“ (scope null) and in the browser preview they live in memory for this session only. One cache for
 *  all components, so the dashboard, the pack detail and the editors show a change at once. */
const EMPTY: InstallationSettings = { cell_types: {}, groups: null, pairs: null, diagnosis: null };
const cache = new Map<string, InstallationSettings>();
const loading = new Set<string>();
const subs = new Set<() => void>();
const keyOf = (scope: string | null) => scope ?? "";
const stored = (scope: string | null): scope is string => !!scope && !isDemo;
const changed = () => subs.forEach((f) => f());

// a failed load is not cached (nothing may be saved over settings the app could not read): asked again shortly
function load(scope: string | null) {
  const k = keyOf(scope);
  if (cache.has(k) || loading.has(k)) return;
  if (!stored(scope)) { cache.set(k, EMPTY); return; }
  loading.add(k);
  api.installationSettings(scope).then((s) => cache.set(k, s), (e) => {
    console.error("installation settings:", e);
    setTimeout(() => load(scope), 5000);
  }).finally(() => { loading.delete(k); changed(); });
}

/** After a merge or the move of the old data: read again from the backend. */
export function reloadSettings() { cache.clear(); changed(); }

export function useSettings(scope: string | null) {
  const [, tick] = useState(0);
  useEffect(() => { const f = () => tick((n) => n + 1); subs.add(f); return () => { subs.delete(f); }; }, []);
  useEffect(() => { load(scope); }, [scope]);
  const k = keyOf(scope);
  const loaded = cache.has(k);
  /** A setting edited as a whole (groups, pairs, diagnosis). Not before the stored ones are loaded. */
  const save = (key: Exclude<SettingKey, "cell_types">, value: unknown) => {
    const cur = cache.get(k);
    if (!cur) return;
    cache.set(k, { ...cur, [key]: value });
    changed();
    if (stored(scope)) api.setInstallationSetting(scope, key, value).catch((e) => console.error(`setting ${key}:`, e));
  };
  /** The cell types of some places (null removes one); the other places keep theirs. Not before loaded. */
  const saveCellTypes = (changes: Record<number, unknown>) => {
    const cur = cache.get(k);
    if (!cur) return;
    const next = { ...cur.cell_types };
    for (const [a, v] of Object.entries(changes)) if (v === null) delete next[a]; else next[a] = v;
    cache.set(k, { ...cur, cell_types: next });
    changed();
    if (stored(scope)) api.setInstallationCellTypes(scope, changes).catch((e) => console.error("cell types:", e));
  };
  return { settings: cache.get(k) ?? EMPTY, loaded, save, saveCellTypes };
}

// --- the old browser storage, moved once into the backend -------------------------------------------------------

const PREFIXES: [string, keyof LegacyLocal][] = [["cellTypes:", "cellTypes"], ["groups:", "groups"], ["pairs:", "pairs"], ["diag:", "diag"]];

/** The settings earlier versions kept in the browser storage, per site key (the endpoint text). */
export function collectLegacy(storage: Pick<Storage, "length" | "key" | "getItem">): Record<string, LegacyLocal> {
  const out: Record<string, LegacyLocal> = {};
  for (let i = 0; i < storage.length; i++) {
    const key = storage.key(i);
    const p = key && PREFIXES.find(([prefix]) => key.startsWith(prefix));
    if (!key || !p) continue;
    const site = key.slice(p[0].length), value = storage.getItem(key);
    if (value !== null) (out[site] ??= {})[p[1]] = value;
  }
  return out;
}

/** The storage keys of a site key that was moved. */
export const legacyKeys = (site: string) => PREFIXES.map(([prefix]) => `${prefix}${site}`);

/** Move the old data into installations of their own (the backend does it once; repeated starts are safe). The old
 *  storage keys go only after the backend reports their site done. Waits while the recording's file is prepared. */
export async function migrateLegacy(): Promise<string[]> {
  if (isDemo) return [];
  for (;;) {
    try {
      const done = await api.installationsMigrate(collectLegacy(localStorage));
      for (const site of done) for (const k of legacyKeys(site)) localStorage.removeItem(k);
      if (done.length) reloadSettings();
      return done;
    } catch (e) {
      if (!String(typeof e === "string" ? e : JSON.stringify(e)).includes("history-preparing")) throw e;
      await new Promise((r) => setTimeout(r, 2000));
    }
  }
}
