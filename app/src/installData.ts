import { useEffect, useState } from "react";
import { api, isDemo, type InstallationSettings, type LegacyLocal, type SettingKey } from "./api";

/** Settings an installation owns (cell type per place, groups, pairs, diagnosis, notifications, devices), kept by the backend per installation.
 *  With „Nur ansehen“ (scope null) and in the browser preview they live in memory for this session only. One cache for
 *  all components, so the dashboard, the pack detail and the editors show a change at once. */
const EMPTY: InstallationSettings = { cell_types: {}, groups: null, pairs: null, diagnosis: null, notifications: null, devices: null };
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

/** One write in flight at a time: a value given meanwhile waits, and of those waiting only the latest is written (each
 *  value is the whole setting, built on the one before). Every call resolves with the error of the write that carried
 *  its value (null when it went through); `idle` runs after the last write. */
export function serialWriter<V>(write: (v: V) => Promise<unknown>, idle: () => void) {
  let running = false;
  let waiting: { v: V; done: ((e: unknown) => void)[] } | null = null;
  const run = (v: V, done: ((e: unknown) => void)[]) => {
    running = true;
    write(v).then(() => null, (e) => e ?? "error").then((err) => {
      done.forEach((d) => d(err));
      const next = waiting;
      waiting = null;
      if (next) run(next.v, next.done);
      else { running = false; idle(); }
    });
  };
  return (v: V) => new Promise<unknown>((resolve) => {
    if (!running) run(v, [resolve]);
    else if (waiting) { waiting.v = v; waiting.done.push(resolve); }
    else waiting = { v, done: [resolve] };
  });
}

// `devices` keeps revisions in time: two writes racing could store an older whole value last. Per installation one write
// at a time, and after the last one the stored value is read back into the cache.
const deviceWriters = new Map<string, (v: unknown) => Promise<unknown>>();
function deviceWriter(scope: string) {
  let w = deviceWriters.get(scope);
  if (!w) {
    // a change given while the stored value is read back wins over it
    let calls = 0;
    const write = serialWriter((v: unknown) => api.setInstallationSetting(scope, "devices", v), () => {
      const at = calls;
      api.installationSettings(scope).then((s) => {
        const cur = cache.get(scope);
        if (cur && at === calls) { cache.set(scope, { ...cur, devices: s.devices }); changed(); }
      }, (e) => console.error("devices read back:", e));
    });
    w = (v: unknown) => { calls++; return write(v); };
    deviceWriters.set(scope, w);
  }
  return w;
}

/** A cell type write: null when it went through; else its error, after `restore` got the cell types from before the
 *  change (`before`) and then, if it can be read, the stored state (read back fresh). */
export async function writeCellTypes(write: () => Promise<unknown>, reload: () => Promise<Record<string, unknown>>,
  before: Record<string, unknown>, restore: (back: Record<string, unknown>) => void): Promise<unknown> {
  try {
    await write();
    return null;
  } catch (e) {
    console.error("cell types:", e);
    restore(before);
    await reload().then(restore, (r) => console.error("cell types read back:", r));
    return e ?? "error";
  }
}

/** After a merge or the move of the old data: read again from the backend. */
export function reloadSettings() { cache.clear(); changed(); }

export function useSettings(scope: string | null) {
  const [, tick] = useState(0);
  useEffect(() => { const f = () => tick((n) => n + 1); subs.add(f); return () => { subs.delete(f); }; }, []);
  useEffect(() => { load(scope); }, [scope]);
  const k = keyOf(scope);
  const loaded = cache.has(k);
  /** A setting edited as a whole (groups, pairs, diagnosis, notifications, devices). Not before the stored ones are loaded.
   *  Resolves to null, or to the backend's error (`errorKey`) after the setting was put back as stored. */
  const save = async (key: Exclude<SettingKey, "cell_types">, value: unknown): Promise<unknown> => {
    const cur = cache.get(k);
    if (!cur) return null;
    cache.set(k, { ...cur, [key]: value });
    changed();
    if (!stored(scope)) return null;
    if (key === "devices") {
      const err = await deviceWriter(scope)(value);
      if (err !== null) console.error("setting devices:", err);
      return err;
    }
    try {
      await api.setInstallationSetting(scope, key, value);
      return null;
    } catch (e) {
      console.error(`setting ${key}:`, e);
      // put back unless a later change replaced it meanwhile
      const now = cache.get(k);
      if (now && now[key] === value) { cache.set(k, { ...now, [key]: cur[key] }); changed(); }
      return e ?? "error";
    }
  };
  /** The cell types of some places (null removes one); the other places keep theirs. Not before loaded. Resolves to
   *  null, or to the backend's error after the cache was set back to the stored cell types. */
  const saveCellTypes = async (changes: Record<number, unknown>): Promise<unknown> => {
    const cur = cache.get(k);
    if (!cur) return null;
    const next = { ...cur.cell_types };
    for (const [a, v] of Object.entries(changes)) if (v === null) delete next[a]; else next[a] = v;
    cache.set(k, { ...cur, cell_types: next });
    changed();
    if (!stored(scope)) return null;
    return writeCellTypes(() => api.setInstallationCellTypes(scope, changes), () => api.installationSettings(scope).then((s) => s.cell_types), cur.cell_types, (back) => {
      const now = cache.get(k);
      if (now) { cache.set(k, { ...now, cell_types: back }); changed(); }
    });
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
