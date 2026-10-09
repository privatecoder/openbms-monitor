import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "../lib/utils";

/** Poll interval of the backend (see Sidebar startPolling). */
export const POLL_MS = 2000;

/** Current time, re-rendered every second. */
export function useNow(intervalMs = 1000) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}

/**
 * Shows that data is flowing: a dot that pulses on every update and the age of the last update.
 * Turns amber when updates are late and red when they have stopped, so a frozen connection is obvious.
 */
export function LiveIndicator({ updated, expected, className }: { updated: number; expected?: number; className?: string }) {
  const { t } = useTranslation();
  const now = useNow();
  if (!updated) return null;
  const age = Math.max(0, Math.round((now - updated) / 1000));
  // Thresholds follow the measured update interval (more packs = longer polling round).
  const every = expected ?? POLL_MS;
  const late = Math.max(5_000, every * 2.5), stale = Math.max(30_000, every * 6);
  const state = now - updated < late ? "live" : now - updated < stale ? "late" : "stale";
  const tone = { live: "text-ok", late: "text-discharge", stale: "text-alarm" }[state];
  const dot = { live: "bg-ok", late: "bg-discharge", stale: "bg-alarm" }[state];
  return (
    <span className={cn("inline-flex items-center gap-2 text-sm", tone, className)} role="status" aria-live="polite">
      <span className="relative flex h-2 w-2">
        {state === "live" && <span key={updated} className="ping-once absolute inset-0 rounded-full bg-ok" />}
        <span className={cn("relative h-2 w-2 rounded-full", dot)} />
      </span>
      {state === "live" ? t("live.live", { s: age }) : t("live.stale", { s: age })}
    </span>
  );
}

/** Briefly highlights its content whenever `value` changes. */
export function Tick({ value, children, className }: { value: unknown; children: ReactNode; className?: string }) {
  const prev = useRef(value);
  const [n, setN] = useState(0);
  useEffect(() => {
    if (prev.current !== value) {
      prev.current = value;
      setN((x) => x + 1);
    }
  }, [value]);
  return <span key={n} className={cn(n > 0 && "value-tick", className)}>{children}</span>;
}
