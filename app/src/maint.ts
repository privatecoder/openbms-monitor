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

export interface Ticket { kind: "refresh" | "check"; id: string; seq: number; /** a refresh that began while a check for its id was in flight */ overlapped: boolean }

/** Ordering of overview requests. A check (fresh 0x64 read) has precedence over a refresh (stored state):
 *  a check answer counts unless a newer check for the same id began; a refresh answer is ignored while a check
 *  for that id is in flight, after a check for it began later, or after a newer refresh began. A failed refresh
 *  never replaces an overview already shown for the same id. Answers for another installation are ignored. */
export function latestGuard() {
  let seq = 0;
  const lastCheck = new Map<string, number>();
  const lastRefresh = new Map<string, number>();
  const inFlight = new Map<string, number>();
  return {
    begin(kind: Ticket["kind"], id: string): Ticket {
      const t: Ticket = { kind, id, seq: ++seq, overlapped: kind === "refresh" && (inFlight.get(id) ?? 0) > 0 };
      if (kind === "check") { lastCheck.set(id, t.seq); inFlight.set(id, (inFlight.get(id) ?? 0) + 1); }
      else lastRefresh.set(id, t.seq);
      return t;
    },
    /** call once when the request has answered or failed */
    end(t: Ticket) { if (t.kind === "check") inFlight.set(t.id, Math.max(0, (inFlight.get(t.id) ?? 1) - 1)); },
    accepts(t: Ticket, currentId: string | null, answer: MaintOverview | null, shown: MaintOverview | null): boolean {
      if (t.id !== currentId) return false;
      if (t.kind === "check") return (lastCheck.get(t.id) ?? 0) <= t.seq;
      const checks = (inFlight.get(t.id) ?? 0) > 0 || (lastCheck.get(t.id) ?? 0) > t.seq;
      if (t.overlapped || checks || (lastRefresh.get(t.id) ?? 0) > t.seq) return false;
      return !(answer === null && shown?.installation.id === t.id);
    },
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

/** A running action may write its result only while the installation and session it started with are still current. */
export function workUnchanged(start: { installation: string; session: number | null }, now: { installation: string | null; session: number | null }): boolean {
  return start.installation === now.installation && start.session === now.session;
}

export interface FlashCheck {
  overview: MaintOverview | null;
  inverter: boolean;
  measuredAt: number | null;
  address: number;
  imageKind: FirmwareImage["kind"];
  /** whom the pack port belongs to: the overview's installation, no installation, or another one */
  portOwner: "this" | "none" | "other";
}

/** Mirrors the backend gate so the start button stays honest; the backend checks everything again. */
export function canFlash({ overview: o, inverter, measuredAt, address, imageKind, portOwner }: FlashCheck): { ok: boolean; reason?: string } {
  if (!o) return { ok: true };
  // the backend's port gate: with an installation in use the pack port must belong to it, maintenance or not
  if (portOwner === "none") return { ok: false, reason: "port-not-assigned" };
  if (portOwner === "other") return { ok: false, reason: "installation-mismatch" };
  if (o.blocks.length) return { ok: false, reason: o.blocks[0].reason };
  const s = o.state?.state;
  // the recovery row: the master back to the original image under DIP 5–8 OFF, without stop
  const recovery = s === "marker-unexplained" && address === 0 && imageKind === "original";
  if (s === "marker-unexplained" && !recovery) return { ok: false, reason: "marker-unexplained" };
  if (recovery && !o.session) return { ok: false, reason: "dip-off" };
  const maintenance = recovery || !!o.session || s === "patched-on";
  if (!maintenance) return { ok: true };
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

/** The slave count per DIP as typed: a whole number from 1 to 15, else null (the backend refuses anything else). */
export function parseSlaves(text: string): number | null {
  const v = text.trim();
  if (!/^\d+$/.test(v)) return null;
  const n = Number(v);
  return n >= 1 && n <= 15 ? n : null;
}

const FAILING = ["pack-communication-incomplete", "master-not-answering", "system-values"];

/** Text key of a failing End check (EndResult.failing, session.failing); "stop: <MaintError JSON>" carries the
 *  dialog's error. null for a value this UI does not know: it is shown as is. */
export function failingKey(f: string): { key: string; inner?: { key: string; params?: Record<string, unknown> } } | null {
  if (FAILING.includes(f)) return { key: `maint.failing.${f}` };
  if (f.startsWith("stop: ")) return { key: "maint.failing.stop", inner: errorKey(f.slice("stop: ".length)) };
  return null;
}

export function failingText(f: string, t: (key: string, params?: Record<string, unknown>) => string): string {
  const k = failingKey(f);
  if (!k) return f;
  return k.inner ? t(k.key, { error: t(k.inner.key, k.inner.params) }) : t(k.key);
}
