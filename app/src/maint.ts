import type { ControlEndpoint, Endpoint, FirmwareImage, Installation, MaintError, MaintOverview } from "./api";

/** What the banner above every view shows; null = nothing open. */
export interface Banner {
  kind: "open" | "unconfirmed" | "unexplained";
  /** "End" is offered (never for an unexplained marker) */
  end: boolean;
  how: "marker" | "dip";
  since: number | null;
}

export function bannerOf(o: MaintOverview | null): Banner | null {
  if (!o) return null;
  const s = o.state?.state;
  if (s === "marker-unexplained") return { kind: "unexplained", end: false, how: "marker", since: o.session?.started ?? null };
  if (o.session) return { kind: s === "unknown" ? "unconfirmed" : "open", end: true, how: o.session.kind, since: o.session.started };
  if (s === "patched-on") return { kind: "open", end: true, how: "marker", since: null };
  return null;
}

/** Open maintenance work: a session, a block, or a set marker (same rule as the backend's `open_work`). */
export function openWork(o: MaintOverview | null): boolean {
  const s = o?.state?.state;
  return !!o && (!!o.session || o.blocks.length > 0 || s === "patched-on" || s === "marker-unexplained");
}

export type Attach = { do: "use"; id: string } | { do: "create" } | { do: "offer-assign" } | { do: "offer-switch"; id: string; name: string };

/** What a new connection does to the installation in use: switch only when nothing open would vanish.
 *  `unloadedId`: an installation is in use but its overview could not be loaded; that counts as open work. */
export function attachDecision(current: MaintOverview | null, known: Installation | null, unloadedId: string | null = null): Attach {
  const inUse = !!current || !!unloadedId;
  const same = !!known && (known.id === current?.installation.id || known.id === unloadedId);
  const mayReplace = !unloadedId && !openWork(current);
  if (known && (!inUse || same || mayReplace)) return { do: "use", id: known.id };
  if (known) return { do: "offer-switch", id: known.id, name: known.name };
  return inUse ? { do: "offer-assign" } : { do: "create" };
}

/** Latest-wins guard for overview requests: only the newest request for the installation still in use may store its answer. */
export function latestGuard() {
  let n = 0;
  return {
    begin: () => ++n,
    accepts: (ticket: number, requestedId: string, currentId: string | null) => ticket === n && requestedId === currentId,
  };
}

/** Same key as the backend's `endpoint_key`: the physical endpoint, independent of the bus. */
export function endpointKey(ep: Endpoint): string {
  return ep.kind === "tcp" ? `tcp:${ep.addr.trim().toLowerCase()}` : `serial:${ep.path.trim()}`;
}

/** Control channel and pack port on one USB adapter: the cable must be moved between the steps. */
export function needsReplug(control: ControlEndpoint | null, packPort: string): boolean {
  return control?.kind === "serial" && control.path === packPort;
}

export interface FlashCheck {
  overview: MaintOverview | null;
  inverter: boolean;
  measuredAt: number | null;
  address: number;
  imageKind: FirmwareImage["kind"];
  /** the pack port belongs to the overview's installation */
  portAssigned: boolean;
}

/** Mirrors the backend gate so the start button stays honest; the backend checks everything again. */
export function canFlash({ overview: o, inverter, measuredAt, address, imageKind, portAssigned }: FlashCheck): { ok: boolean; reason?: string } {
  if (!o) return { ok: true };
  if (o.blocks.length) return { ok: false, reason: o.blocks[0].reason };
  const s = o.state?.state;
  // the recovery row: the master back to the original image under DIP 5–8 OFF, without stop
  const recovery = s === "marker-unexplained" && address === 0 && imageKind === "original";
  if (s === "marker-unexplained" && !recovery) return { ok: false, reason: "marker-unexplained" };
  if (recovery && !o.session) return { ok: false, reason: "dip-off" };
  const maintenance = recovery || !!o.session || s === "patched-on";
  if (!maintenance) return { ok: true };
  if (!portAssigned) return { ok: false, reason: "port-not-assigned" };
  if (!inverter) return { ok: false, reason: "inverter" };
  // a measurement counts only if it is newer than the last confirmed state
  if (measuredAt === null || (o.state_at !== null && measuredAt < o.state_at)) return { ok: false, reason: "measure" };
  return { ok: true };
}

const KINDS = ["connection", "unsupported", "no-answer", "rejected", "timeout", "refused", "busy", "measure", "store"];

/** Backend errors arrive as a MaintError object, as its JSON text, as a bare refusal key, or as plain text. */
export function errorKey(e: unknown): { key: string; params?: Record<string, unknown> } {
  let v: unknown = e;
  if (typeof e === "string") {
    try { v = JSON.parse(e); } catch { v = /^[a-z0-9-]+$/.test(e) ? { kind: "refused", reason: e } : e; }
  }
  if (v && typeof v === "object" && "kind" in v && KINDS.includes(String((v as MaintError).kind))) {
    const m = v as MaintError;
    switch (m.kind) {
      case "refused": return { key: `maint.err.refused.${m.reason}` };
      case "timeout": return { key: "maint.err.timeout", params: { marker: Number(m.marker), slaves: m.slaves } };
      case "rejected": return { key: "maint.err.rejected", params: { rtn: m.rtn.toString(16).padStart(2, "0") } };
      case "connection": case "measure": case "store": return { key: `maint.err.${m.kind}`, params: { text: m.message } };
      default: return { key: `maint.err.${m.kind}` };
    }
  }
  return { key: "maint.err.text", params: { text: String(e) } };
}
