import { useSyncExternalStore } from "react";
import { api, isDemo, type UpdateInfo, type UpdateProgress } from "./api";

/** App updates: one state for the About view and the sidebar hint. The check only reads; installing is a click. */
export interface UpdateState {
  phase: "idle" | "checking" | "current" | "available" | "installing" | "error";
  info: UpdateInfo | null;
  progress: UpdateProgress | null;
  error: string | null;
  /** where the error came from: a failed check says so in the status line, only the raw error follows */
  failed: "check" | "install" | null;
  checkedAt: number | null;
  /** automatic checks on (stored per computer) */
  auto: boolean;
}

const AUTO_KEY = "update.auto";
/** automatic checks: shortly after the start, then twice a day */
const FIRST_MS = 10_000;
const EVERY_MS = 12 * 3_600_000;

const storedAuto = () => { try { return localStorage.getItem(AUTO_KEY) !== "0"; } catch { return true; } };
let state: UpdateState = { phase: "idle", info: null, progress: null, error: null, failed: null, checkedAt: null, auto: storedAuto() };
const subs = new Set<() => void>();
const set = (s: Partial<UpdateState>) => { state = { ...state, ...s }; subs.forEach((f) => f()); };

/** the current state outside React (tests) */
export const updateNow = () => state;
export const useUpdate = () => useSyncExternalStore((f) => { subs.add(f); return () => { subs.delete(f); }; }, () => state);

export async function checkUpdate(): Promise<void> {
  if (state.phase === "checking" || state.phase === "installing") return;
  set({ phase: "checking", error: null, failed: null });
  try {
    const info = await api.updateCheck();
    set({ phase: info ? "available" : "current", info, checkedAt: Date.now() });
  } catch (e) {
    set({ phase: "error", error: String(e), failed: "check", checkedAt: Date.now() });
  }
}

/** Download and install; on success the app restarts (Windows: the installer ends it). Busy: back to "available". */
export async function installUpdate(): Promise<void> {
  if (state.phase !== "available") return;
  set({ phase: "installing", progress: null, error: null, failed: null });
  let stop: (() => void) | null = null;
  try {
    // without progress the install still works; a failed listener must not leave the store stuck in "installing"
    stop = await api.onUpdateProgress((p) => set({ progress: p })).catch(() => null);
    await api.updateInstall();
  } catch (e) {
    const msg = String(e);
    // a refused install keeps the found update; a failed download or signature needs a new check
    set(msg.startsWith("busy:") ? { phase: "available", error: msg, failed: "install" } : { phase: "error", error: msg, failed: "install", info: null });
  } finally {
    stop?.();
  }
}

export function setAutoCheck(on: boolean) {
  try { localStorage.setItem(AUTO_KEY, on ? "1" : "0"); } catch { /* the setting only lives this session */ }
  set({ auto: on });
}

let started = false;
/** Start the automatic checks once per app run (not in the browser preview). */
export function startUpdateChecks() {
  if (started || isDemo) return;
  started = true;
  const tick = () => { if (state.auto) void checkUpdate(); };
  setTimeout(tick, FIRST_MS);
  setInterval(tick, EVERY_MS);
}

/** The user-facing text of an update error. */
export function updateErrorKey(error: string): string {
  if (error.startsWith("busy:")) return `about.update.busy.${error.slice(5)}`;
  return "about.update.failed";
}
