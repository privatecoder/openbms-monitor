import { accessHint } from "./access";
import type { MarkerView } from "./api";
import { errorText } from "./control";
import { errorKey } from "./maint";

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
    case "no-uart0": return { text: accessHint(t, "mk.noUart0", undefined, { version }), tone: "warn" };
    case "unknown-patch": return { text: t("mk.unknownPatch", { version: version || "EMU1101-EXT" }), tone: "warn" };
    case "no-0x64": return { text: t("mk.original"), tone: "muted" };
    default: return { text: v.bus === "pack" ? accessHint(t, "mk.unknownPack") : t("mk.unknownCan"), tone: "warn" };
  }
}

const KEYS = ["marker-master-only", "marker-set-can-only", "halt-not-offered", "release-not-offered"];

/** A refusal of `marker_read`/`marker_set` as text; a start the master refused (RTN 05) with its cause. */
export function markerError(e: unknown, t: T): string {
  const s = typeof e === "string" ? e : e instanceof Error ? e.message : "";
  if (s.startsWith("start-refused:")) return t(`maint.err.start.${s.slice("start-refused:".length)}`);
  if (KEYS.includes(s)) return t(`mk.err.${s}`);
  // a refusal of the maintenance store or the master comparison (another master, its serial unreadable)
  // (bank refusals of RS485-1/2 keep the shared texts below)
  const k = errorKey(s);
  if (s.startsWith("{") && k.key !== "maint.err.text") return t(k.key, k.params);
  return t("mk.failed", { err: errorText(e, t) });
}
