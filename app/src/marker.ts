import type { MarkerView } from "./api";
import { errorText } from "./control";

type T = (k: string, o?: Record<string, unknown>) => string;

/** The state line of „Wartungs-Marker“: the build and its marker, or why this connection shows none. */
export function markerLine(v: MarkerView, t: T): { text: string; tone: "ok" | "warn" | "muted" } {
  const version = v.version ?? "";
  switch (v.kind) {
    case "patched": {
      if (v.marker === null) return { text: t("mk.unread", { version }), tone: "warn" };
      const slaves = v.marker === "off" && v.slaves ? t("mk.polls", { n: v.slaves }) : "";
      return { text: t(`mk.state.${v.marker}`, { version, slaves }), tone: v.marker === "foreign" ? "warn" : "muted" };
    }
    case "no-uart0": return { text: t("mk.noUart0", { version }), tone: "warn" };
    case "unknown-patch": return { text: t("mk.unknownPatch", { version: version || "MNT" }), tone: "warn" };
    case "no-0x64": return { text: t("mk.original"), tone: "muted" };
    default: return { text: t(v.bus === "pack" ? "mk.unknownPack" : "mk.unknownCan"), tone: "warn" };
  }
}

const KEYS = ["marker-master-only", "marker-set-can-only", "halt-not-offered", "release-not-offered"];

/** A refusal of `marker_read`/`marker_set` as text; a start the master refused (RTN 05) with its cause. */
export function markerError(e: unknown, t: T): string {
  const s = typeof e === "string" ? e : e instanceof Error ? e.message : "";
  if (s.startsWith("start-refused:")) return t(`maint.err.start.${s.slice("start-refused:".length)}`);
  if (KEYS.includes(s)) return t(`mk.err.${s}`);
  return t("mk.failed", { err: errorText(e, t) });
}
