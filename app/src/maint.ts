import type { ControlEndpoint, EndStep, Endpoint, FirmwareImage, FlashBlock, Installation, MaintError, MaintOverview, MaintProgress, MasterStateName, OpenWork } from "./api";

/** The monitor connection (sidebar): connected, paused by the app for a maintenance step (`monitor-stopped`),
 *  not connected (never, or closed by the user), or lost (link-lost or data timeout). */
export type Monitor = "connected" | "paused" | "offline" | "lost";

/** Severity of a flash block, most severe first (same order as the backend's `block_rank`). */
export const BLOCK_ORDER: FlashBlock["reason"][] = ["bootloader-unclear", "update-interrupted", "update-unclear", "master-unclear"];

export type WorkKind = "block" | "unexplained" | "session" | "marker" | "note";

/** One line of the banner: one installation with open work, named by its most pressing item. */
export interface WorkLine {
  id: string;
  name: string;
  kind: WorkKind;
  /** since when (block, session start, marker state, sector note) */
  at: number | null;
  block: FlashBlock | null;
  session: OpenWork["session"];
  /** an open session whose master state is unknown (connection lost) */
  unconfirmed: boolean;
  /** the installation shown in the panel */
  shown: boolean;
  /** "End" is offered: only for the shown installation with an open session, no block, no unexplained marker */
  end: boolean;
  /** without End: the text key that says what must be cleared first (null for a shown session) */
  why: string | null;
}

const workRank = (l: Pick<WorkLine, "kind" | "block">): number =>
  l.kind === "block" ? BLOCK_ORDER.indexOf(l.block!.reason) : l.kind === "unexplained" ? BLOCK_ORDER.length : l.kind === "session" ? 10 : l.kind === "marker" ? 11 : 12;

/** The banner's lines, one per installation with open work: blocks (by severity) and unexplained markers first, then
 *  open sessions by start time, then a set marker without session by state time, then open sector notes. "Show" is
 *  always offered; "End" only as `WorkLine.end` says. */
export function openWorkLines(list: OpenWork[], shown: string | null): WorkLine[] {
  const lines = list.map((w): WorkLine => {
    const s = w.state?.state;
    const kind: WorkKind = w.block ? "block" : s === "marker-unexplained" ? "unexplained" : w.session ? "session" : s === "patched-on" ? "marker" : "note";
    const at = kind === "block" ? w.block!.since : kind === "session" ? w.session!.started : kind === "note" ? w.sector_note_at : w.state_at;
    const isShown = w.id === shown;
    const end = isShown && kind === "session";
    const why = end ? null : kind === "session" ? "maint.openWork.why.showFirst" : `maint.openWork.why.${kind}`;
    return { id: w.id, name: w.name, kind, at: at ?? null, block: w.block, session: w.session, unconfirmed: kind === "session" && s === "unknown", shown: isShown, end, why };
  });
  return lines.sort((a, b) => workRank(a) - workRank(b) || (a.at ?? Infinity) - (b.at ?? Infinity) || a.name.localeCompare(b.name));
}

/** Accepts only the answer of the newest request (sequence ticket): an older reply that arrives late never
 *  overwrites a newer list. */
export function latestOnly() {
  let seq = 0;
  return { begin: () => ++seq, accepts: (ticket: number) => ticket === seq };
}

/** The open note on sector 0x23000 the master-plan checklist shows before flashing a maintenance or custom image onto
 *  the master (display only, no gate); null otherwise. */
export function planSectorNote(o: MaintOverview | null, address: number, kind: FirmwareImage["kind"] | null): string | null {
  return o?.sector_note && address === 0 && (kind === "maintenance" || kind === "custom") ? o.sector_note : null;
}

/** An installation can be deleted only without open work: no session, no block, no set or unexplained marker and no
 *  open note on sector 0x23000 (it is never discarded silently). The backend checks again under the port lock. */
export function deleteAllowed(o: MaintOverview | null): boolean {
  return !!o && !openWork(o) && !o.sector_note;
}

/** The name check of the create dialog and the setup form: null = fine, else the text key. Required, and not the
 *  name of another installation (case-insensitive), so the picker never lists two equal entries. */
export function nameCheck(name: string, installations: Pick<Installation, "id" | "name">[], self: string | null = null): string | null {
  const n = name.trim().toLowerCase();
  if (!n) return "maint.setup.err.nameRequired";
  return installations.some((i) => i.id !== self && i.name.trim().toLowerCase() === n) ? "maint.setup.err.nameTaken" : null;
}

/** The suggested name of a new installation: „Anlage n“ with the first free n from the count + 1. */
export function defaultName(installations: Pick<Installation, "id" | "name">[], base: string): string {
  for (let n = installations.length + 1; ; n++) {
    const name = `${base} ${n}`;
    if (!nameCheck(name, installations)) return name;
  }
}

/** A sidebar connection that belongs to no installation becomes a new one (after the name dialog): on the CAN socket
 *  bus as its CAN socket, on the pack bus as its endpoint. */
export function createFromAttach(ep: Endpoint): { control: ControlEndpoint | null; endpoints: string[] } {
  return ep.bus === "can" ? { control: controlOf(ep), endpoints: [] } : { control: null, endpoints: [endpointKey(ep)] };
}

/** Whom the RS485-1/2 port belongs to (`owner`: its installation id, null = none) seen from the shown installation:
 *  a foreign port locks every bus action ("installation-mismatch") and offers to switch to its installation. The
 *  backend's port gate stays authoritative. */
export function portLock(owner: string | null, shown: string | null, installations: Pick<Installation, "id" | "name">[]): { owner: "this" | "none" | "other"; switchTo: { id: string; name: string } | null } {
  if (!shown || owner === null) return { owner: "none", switchTo: null };
  if (owner === shown) return { owner: "this", switchTo: null };
  const other = installations.find((i) => i.id === owner);
  return { owner: "other", switchTo: other ? { id: other.id, name: other.name } : null };
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
/** With `port` (the RS485-1/2 port the action used) also only while that port is still the one in use. */
export function workUnchanged(start: { installation: string; session: number | null; port?: string }, now: { installation: string | null; session: number | null; port?: string }): boolean {
  return start.installation === now.installation && start.session === now.session && (start.port === undefined || start.port === now.port);
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

/** The backend's refusal of `connect` while another operation holds the port: `{kind: "busy", action}` with the
 *  running maintenance action ("check", …, "flash"; null when it has none, e.g. "check pack"). Else null. */
export function portBusy(e: unknown): { action: string | null } | null {
  let v: unknown = e;
  if (typeof e === "string") { try { v = JSON.parse(e); } catch { return null; } }
  if (!v || typeof v !== "object" || (v as { kind?: unknown }).kind !== "busy") return null;
  const a = (v as { action?: unknown }).action;
  return { action: typeof a === "string" ? a : null };
}

/** One round of waiting for a busy port. `token` is the wait's own generation, `current` the sidebar's now (bumped by
 *  cancel, disconnect, new settings, unmount and a user action elsewhere): another one means stop. A running flash
 *  (`locked`) stops it, and so does the firmware view for the automatic reconnect (that view needs the port). */
export function busyWaitStep(w: { token: number; current: number; locked: boolean; auto: boolean; firmwareOpen: boolean; now: number; until: number }): "wait" | "stop" | "give-up" {
  if (w.token !== w.current || w.locked || (w.auto && w.firmwareOpen)) return "stop";
  return w.now >= w.until ? "give-up" : "wait";
}

const BUSY_ACTIONS = ["check", "activate", "start-dip", "scan", "account-marker", "set-slaves", "measure", "end", "end-step", "clear-block", "flash"];

/** "Port busy: the master is being checked." and what happens next: waiting for it, or given up after the wait. */
export function busyMessage(action: string | null, gaveUp: boolean, t: (key: string, params?: Record<string, unknown>) => string): string {
  const what = t(`conn.busyAction.${action && BUSY_ACTIONS.includes(action) ? action : "other"}`);
  return t(gaveUp ? "conn.busyGaveUp" : "conn.busyWait", { what });
}

/** The number of packs on the bus as typed, master included: a whole number from 1 to 16, else null. The backend
 *  gets `slavesOf` it; 1 pack (0 slaves) it accepts only when a fresh 0x64 shows "patched, 0 slaves" or the original
 *  firmware's RTN 04, and the slave scan finds no slave. */
export function parsePacks(text: string): number | null {
  const v = text.trim();
  if (!/^\d+$/.test(v)) return null;
  const n = Number(v);
  return n >= 1 && n <= 16 ? n : null;
}

/** The master-plan checklist in plan order: a step the user confirms by a checkbox (DIP 5–8 OFF, build released or
 *  master isolated) is shown once, as that checkbox, in its position (e.g. before stop and measurement). */
export const PLAN_CHECKBOXES = ["dip-off", "isolated-or-released"];
export function planItems(before: string[]): { step: string; checkbox: boolean }[] {
  return before.map((step) => ({ step, checkbox: PLAN_CHECKBOXES.includes(step) }));
}

/** "1 pack" needs a fresh 0/0 (or RTN 04) from the master, which it cannot report while its marker is on (stored state
 *  patched-on or marker-unexplained): saving 1 would be refused, so the editor does not offer it. */
export function onePackBlocked(packs: number | null, state: MasterStateName | undefined): boolean {
  return packs === 1 && (state === "patched-on" || state === "marker-unexplained");
}

/** The slave count N (= the master's DIP 5–8 value) of a bank with `packs` packs, master included. */
export function slavesOf(packs: number): number {
  return packs - 1;
}

type T = (key: string, params?: Record<string, unknown>) => string;

/** "8 Packs = 7 Slaves + 1 Master" */
export function packsLine(packs: number, t: T): string {
  return t("maint.panel.packsEq", { packs: t("maint.panel.packCount", { count: packs }), slaves: t("maint.panel.slaveCount", { count: slavesOf(packs) }) });
}

/** The line under the pack count input: the derived slave count and the master's DIP 5–8 value. */
export function packsDerived(packs: number, t: T): string {
  return `${packsLine(packs, t)} · ${t("maint.panel.dipWas", { n: slavesOf(packs) })}`;
}

const FAILING = ["pack-communication-incomplete", "master-not-answering", "system-values", "master-not-zero", "master-unreadable"];

/** Text key of a failing End check (EndResult.failing, session.failing); "stop: <MaintError JSON>" carries the
 *  dialog's error, "slave-present: <address>" the slave found, "slave-count-mismatch: <actual> <N>" the master's
 *  slave count against the session's. null for a value this UI does not know: it is shown as is. */
export function failingKey(f: string): { key: string; params?: Record<string, unknown>; inner?: { key: string; params?: Record<string, unknown> } } | null {
  if (FAILING.includes(f)) return { key: `maint.failing.${f}` };
  const slave = /^slave-present: (\d+)$/.exec(f);
  if (slave) return { key: "maint.failing.slave-present", params: { addr: Number(slave[1]) } };
  const mismatch = /^slave-count-mismatch: (\d+) (\d+)$/.exec(f);
  if (mismatch) return { key: "maint.failing.slave-count-mismatch", params: { actual: Number(mismatch[1]), expected: Number(mismatch[2]) } };
  if (f.startsWith("stop: ")) return { key: "maint.failing.stop", inner: errorKey(f.slice("stop: ".length)) };
  return null;
}

/** A sentence used inside another one: without its own final period (an ellipsis stays). */
export function clause(text: string): string {
  return text.replace(/(?<!\.)\.$/, "");
}

/** The failing End check as a clause (no final period): it goes into „Prüfung fehlgeschlagen: …. Die Wartung bleibt offen.“ */
export function failingText(f: string, t: T): string {
  const k = failingKey(f);
  if (!k) return clause(f);
  if (k.inner) return clause(t(k.key, { error: clause(t(k.inner.key, k.inner.params)) }));
  if (k.key === "maint.failing.slave-count-mismatch") {
    const actual = Number(k.params?.actual), expected = Number(k.params?.expected);
    // count: „eingetragen ist 1 Pack“ / „eingetragen sind 2 Packs“
    return clause(t(k.key, { count: expected + 1, actual: t("maint.panel.slaveCount", { count: actual }), packs: t("maint.panel.packCount", { count: expected + 1 }), expected: t("maint.panel.slaveCount", { count: expected }) }));
  }
  return clause(t(k.key, k.params));
}

/** A pack bus panel action at its button: running (with the latest phase the backend reported), then its result,
 *  which stays until the next action or a view change. `key` names the button, `action` the backend command. */
export interface Activity {
  installation: string;
  key: string;
  action: string;
  running: boolean;
  phase?: MaintProgress;
  result?: { ok: boolean; text: string };
}

/** A progress event belongs to the running action of the same installation. */
export function acceptsProgress(a: Activity | null, ev: MaintProgress): boolean {
  return !!a && a.running && a.installation === ev.installation && a.action === ev.action;
}

/** The status line under the button: the phase while running (else the action's generic text), then the result. */
export function activityStatus(a: Activity, t: T): { kind: "running" | "ok" | "failed"; text: string } | null {
  if (a.running) return { kind: "running", text: a.phase ? t(`maint.progress.phase.${a.phase.phase}`, a.phase.detail ?? undefined) : t(`maint.progress.action.${a.action}`) };
  if (a.result) return { kind: a.result.ok ? "ok" : "failed", text: a.result.text };
  return null;
}

/** The inverter statement belongs to one work phase. It survives the session that the activation it
 *  unlocked has just opened (no session → new session, same installation); it is cleared when the
 *  installation changes, the session ends or another session takes its place. */
export function keepsInverter(prev: { installation: string | null; session: number | null }, next: { installation: string | null; session: number | null }): boolean {
  if (prev.installation !== next.installation) return false;
  return prev.session === next.session || (prev.session === null && next.session !== null);
}

const controlKey = (c: ControlEndpoint | null) => (c ? (c.kind === "tcp" ? endpointKey({ kind: "tcp", addr: c.addr, bus: "can" }) : endpointKey({ kind: "serial", path: c.path, bus: "can" })) : null);

/** The installation's RS485-1/2 port (measure and flash packs): the first stored serial endpoint, in stored order,
 *  that is not the control adapter. The control adapter itself only with the one-adapter confirmation (a USB
 *  adapter moved between both buses) and no other serial endpoint. null: not set up. */
export function packPortOf(inst: Installation | null, oneAdapter = false): string | null {
  if (!inst) return null;
  const control = controlKey(inst.control);
  const other = inst.endpoints.find((k) => k.startsWith("serial:") && k !== control);
  if (other) return other.slice("serial:".length);
  return oneAdapter && inst.control?.kind === "serial" ? inst.control.path : null;
}

/** The CAN socket and the RS485-1/2 port are the same USB adapter. */
export function sameAdapter(can: ControlEndpoint | null, pack: string): boolean {
  return !!pack && can?.kind === "serial" && can.path.trim() === pack.trim();
}

/** The setup form's check before saving: null = fine, else the text key of the refusal. A stored RS485-1/2 port
 *  can be replaced, never silently dropped; one adapter for both buses needs the explicit confirmation. */
export function setupCheck({ can, pack, oneAdapter, storedPack, loadedPack }: { can: ControlEndpoint | null; pack: string; oneAdapter: boolean; storedPack: string | null;
  /** the pack port the form was loaded with: "" while a port is stored = it was set meanwhile (the form reloads) */
  loadedPack?: string }): string | null {
  if (!pack && storedPack) return loadedPack === "" ? "maint.setup.err.packSetMeanwhile" : "maint.setup.err.packRequired";
  if (sameAdapter(can, pack) && !oneAdapter) return "maint.setup.err.sameAdapter";
  return null;
}

/** The setup form's fields from a stored installation (`oneAdapter`: the stored one-adapter confirmation). */
export function setupFields(inst: Installation, oneAdapter: boolean) {
  const c = inst.control;
  return {
    name: inst.name,
    canKind: (c?.kind ?? "serial") as ControlEndpoint["kind"],
    canValue: c ? (c.kind === "tcp" ? c.addr : c.path) : "",
    pack: packPortOf(inst, oneAdapter) ?? "",
    oneAdapter,
  };
}

const controlOf = (ep: Endpoint): ControlEndpoint => (ep.kind === "tcp" ? { kind: "tcp", addr: ep.addr } : { kind: "serial", path: ep.path });

/** A connection on the CAN socket bus (sidebar) for an installation without a control channel: the adapter becomes
 *  its CAN socket, never a listed endpoint (which could pass for the RS485-1/2 port). null: nothing to change. */
export function controlFromAttach(inst: Installation, ep: Endpoint): Installation | null {
  if (ep.bus !== "can" || inst.control) return null;
  const key = endpointKey(ep);
  return { ...inst, control: controlOf(ep), endpoints: inst.endpoints.filter((k) => k !== key) };
}

/** Assign a connection explicitly (banner): on the CAN socket bus it sets (or keeps) the CAN socket; only a
 *  pack-bus endpoint is listed, and only a serial one can then be the RS485-1/2 port. */
export function assignEndpoint(inst: Installation, ep: Endpoint): Installation {
  const key = endpointKey(ep);
  if (ep.bus === "can") {
    const endpoints = inst.endpoints.filter((k) => k !== key);
    if (controlKey(inst.control) === key) return endpoints.length === inst.endpoints.length ? inst : { ...inst, endpoints };
    return { ...inst, control: controlOf(ep), endpoints };
  }
  return inst.endpoints.includes(key) ? inst : { ...inst, endpoints: [...inst.endpoints, key] };
}

/** The endpoint list the setup form saves: the CAN socket and the RS485-1/2 port first (so the pack port reads
 *  back as the first serial endpoint), then every other stored endpoint of the installation (e.g. a sidebar
 *  connection). The old CAN key and the old pack port are replaced; an empty pack port keeps the stored one. */
export function setupEndpoints(inst: Installation, can: ControlEndpoint | null, pack: string, oneAdapter = false): string[] {
  const oldCan = controlKey(inst.control);
  const oldPackPath = packPortOf(inst, oneAdapter);
  const oldPack = oldPackPath ? endpointKey({ kind: "serial", path: oldPackPath, bus: "pack" }) : null;
  const packKey = pack ? endpointKey({ kind: "serial", path: pack, bus: "pack" }) : oldPack;
  const kept = inst.endpoints.filter((k) => k !== oldCan && k !== oldPack);
  const out: string[] = [];
  for (const k of [controlKey(can), packKey, ...kept]) if (k && !out.includes(k)) out.push(k);
  return out;
}

export type StepId = "connections" | "check" | "inverter" | "start" | "measure" | "flash";
export type StepState = "done" | "current" | "locked";
export interface Reason { key: string; params?: Record<string, unknown> }
export interface FlowStep {
  state: StepState;
  /** why it is locked, or what the current step waits for */
  reason?: Reason;
  /** done without work: this branch does not need the step */
  notNeeded?: boolean;
}
/** What step 4 offers: activate maintenance mode (or the DIP way), the DIP way, nothing (single pack), the
 *  running recovery, an open session, or the explanation of an unexplained marker. */
export type Branch = "activate" | "dip" | "single-pack" | "recovery" | "session" | "unexplained";

export interface FlowInput {
  overview: MaintOverview | null;
  /** the fresh 0x64 check of this app run (and since the last lost connection): null = none yet, "failed" =
   *  refused (e.g. no control channel); an answer stored as unknown arrives as "ok" with state unknown */
  fresh: "ok" | "failed" | null;
  /** the user works without a control channel (explicit choice in step 1) */
  noControl: boolean;
  /** without a control channel: the user stated that only this one pack is on the bus (counts only while
   *  `singlePackOffered`: no session, no stored state or unknown) */
  singlePack: boolean;
  inverter: boolean;
  measuredAt: number | null;
  /** the RS485-1/2 port in use ("" = not set up) */
  port: string;
  /** the RS485-1/2 port is plugged in (in the list of available ports) */
  portAvailable: boolean;
  portOwner: "this" | "none" | "other";
  address: number;
  imageKind: FirmwareImage["kind"] | null;
  /** single-pack slave scan of this run on the chosen port: undefined = not run, null = no slave, n = first slave */
  scan: number | null | undefined;
  /** a panel action runs */
  busy: boolean;
  /** a flash runs */
  running: boolean;
}

export interface Flow {
  /** the master state of this run; null until a fresh check (the stored one is only "last confirmed") */
  state: MasterStateName | null;
  branch: Branch | null;
  steps: Record<StepId, FlowStep>;
  /** End, its own section: shown while a session is open */
  end: { shown: boolean; state: StepState; reason?: Reason; next: EndStep | null };
  /** step 4 offers the slave check (0/0 without session, not yet checked on this port) */
  scanNeeded: boolean;
  /** 0/0 but a slave answered: DIP 5–8 may be OFF */
  packsUnclear: boolean;
  /** control channel and pack port are one adapter */
  replug: boolean;
  /** step 4 may offer the statement "only this one pack" (no control channel, no session, nothing or unknown stored) */
  singlePackOffered: boolean;
  /** no action runs: the current step's main action may be used */
  idle: boolean;
}

const r = (key: string, params?: Record<string, unknown>): Reason => (params ? { key, params } : { key });

/** The panel's guided steps. An additional UI rule on top of the backend gates, never a replacement: the backend
 *  checks everything again. Steps run in order; a step after the first incomplete one is locked ("first step n"),
 *  a deferred step waits for step 4 without holding up the others, a hard lock (block, recovery target) names
 *  its own reason. */
export function flowOf(i: FlowInput): Flow {
  const o = i.overview;
  const ids: StepId[] = ["connections", "check", "inverter", "start", "measure", "flash"];
  if (!o) {
    const steps = Object.fromEntries(ids.map((id) => [id, { state: "locked", reason: r("maint.flow.reason.no-installation") }])) as Record<StepId, FlowStep>;
    steps.flash = { state: i.running ? "locked" : "current", ...(i.running ? { reason: r("maint.flow.reason.running") } : {}) };
    return { state: null, branch: null, steps, end: { shown: false, state: "locked", next: null }, scanNeeded: false, packsUnclear: false, replug: false, singlePackOffered: false, idle: !i.busy && !i.running };
  }
  const stored = o.state?.state ?? null;
  const hasControl = !!o.installation.control;
  const manual = !hasControl && i.noControl;
  const fresh = manual ? "failed" : i.fresh;
  // a failed check ends as unknown; it never clears an unexplained marker
  const state: MasterStateName | null = fresh === null ? null : fresh === "failed" ? (stored === "marker-unexplained" ? stored : "unknown") : (stored ?? "unknown");
  const session = o.session;
  const singlePackOffered = manual && !session && (stored === null || stored === "unknown");
  const zero = state === "patched-off" && (o.state?.slaves ?? 0) === 0;
  let branch: Branch | null = null;
  if (state === "marker-unexplained") branch = session?.kind === "dip" ? "recovery" : "unexplained";
  else if (session) branch = "session";
  else if (state === "patched-on") branch = null;
  else if (state === "patched-off" && !zero) branch = "activate";
  else if (zero) branch = i.scan === undefined || (i.scan === null && i.imageKind !== "custom") ? "single-pack" : "dip";
  else if (state !== null) branch = singlePackOffered && i.singlePack ? "single-pack" : "dip";
  const scanNeeded = !session && zero && i.scan === undefined;
  const single = branch === "single-pack" && !scanNeeded;
  const recoveryTarget = i.address === 0 && i.imageKind === "original";
  const recoveryLock = branch === "recovery" && !recoveryTarget ? r("maint.flow.reason.recovery-only-original") : null;
  // a marker session whose master no longer reports the marker (e.g. patched-off after End's stop step): no new
  // measurement or flash until a check shows the marker again; End continues
  const markerLock = session?.kind === "marker" && state !== null && state !== "patched-on" && state !== "unknown" && state !== "marker-unexplained"
    ? r("maint.flow.reason.marker-not-on") : null;
  const blockLock = o.blocks.length ? r(`maint.err.refused.${o.blocks[0].reason}`) : null;
  // the RS485-1/2 port: set up (setup form) and plugged in
  // a one-adapter setup without the tick: the only serial endpoint is the control adapter
  const ctlKey = controlKey(o.installation.control);
  const serialEps = o.installation.endpoints.filter((k) => k.startsWith("serial:"));
  const unconfirmedOne = o.installation.control?.kind === "serial" && serialEps.length > 0 && serialEps.every((k) => k === ctlKey);
  const portLock = !i.port ? r(unconfirmedOne ? "maint.flow.reason.confirm-one-adapter" : "maint.flow.reason.setup-pack") : !i.portAvailable ? r("maint.flow.reason.pack-unplugged", { port: i.port }) : undefined;
  const measured = i.measuredAt !== null && (o.state_at === null || i.measuredAt >= o.state_at);

  type Eval = { complete: boolean; reason?: Reason; hard?: Reason | null; deferred?: Reason; notNeeded?: boolean };
  const evalStep: Record<StepId, () => Eval> = {
    connections: () => {
      const reason = portLock
        ?? (i.portOwner === "none" ? r("maint.err.refused.port-not-assigned")
        : i.portOwner === "other" ? r("maint.err.refused.installation-mismatch")
        : !hasControl && !i.noControl ? r("maint.flow.reason.control-or-none") : undefined);
      return { complete: !reason, reason };
    },
    check: () => {
      if (state === null) return { complete: false, reason: r("maint.flow.reason.check-needed") };
      // a lost connection stores unknown while the marker session is open: only a check showing the marker continues
      if (session?.kind === "marker" && state === "unknown") return { complete: false, reason: r("maint.flow.reason.marker-unknown") };
      return { complete: true };
    },
    inverter: () => {
      if (branch === "unexplained") return { complete: false, deferred: r("maint.flow.reason.after-recovery") };
      if (scanNeeded) return { complete: false, deferred: r("maint.flow.reason.after-scan") };
      return { complete: i.inverter, hard: recoveryLock };
    },
    start: () => {
      if (branch === null && state === "patched-on") return { complete: false, reason: r("maint.flow.reason.patched-on-no-session") };
      if (branch === "session" || branch === "recovery") return { complete: true };
      if (single) return { complete: true, notNeeded: true };
      return { complete: false };
    },
    measure: () => ({ complete: measured, hard: blockLock ?? recoveryLock ?? markerLock }),
    flash: () => ({ complete: false, hard: blockLock ?? recoveryLock ?? markerLock }),
  };

  // the recovery starts its DIP session (step 4) before the inverter statement (step 3)
  const order: StepId[] = branch === "recovery" ? ["connections", "check", "start", "inverter", "measure", "flash"] : ids;
  const steps = {} as Record<StepId, FlowStep>;
  let blocker: Reason | null = null;
  for (const id of order) {
    const n = ids.indexOf(id) + 1;
    const e = evalStep[id]();
    if (i.running) steps[id] = { state: "locked", reason: r("maint.flow.reason.running") };
    else if (e.hard) { steps[id] = { state: "locked", reason: e.hard }; blocker ??= r("maint.flow.reason.first", { n }); }
    else if (e.deferred) steps[id] = { state: "locked", reason: e.deferred };
    else if (blocker) steps[id] = { state: "locked", reason: blocker };
    else if (e.complete) steps[id] = e.notNeeded ? { state: "done", notNeeded: true } : { state: "done" };
    else { steps[id] = e.reason ? { state: "current", reason: e.reason } : { state: "current" }; blocker = r("maint.flow.reason.first", { n }); }
  }

  // End checks itself (no measurement first); it needs the RS485-1/2 port plugged in and is locked by a block or the unexplained marker
  const now = state ?? stored;
  const endLock = i.running ? r("maint.flow.reason.running")
    : blockLock
    ?? (now === "marker-unexplained" ? r("maint.err.refused.marker-unexplained") : null)
    ?? portLock ?? (i.portOwner !== "this" ? r(i.portOwner === "none" ? "maint.err.refused.port-not-assigned" : "maint.err.refused.installation-mismatch") : null);
  const end = { shown: !!session, state: (endLock ? "locked" : "current") as StepState, ...(endLock ? { reason: endLock } : {}), next: o.end_next ?? null };

  return {
    state, branch, steps, end, scanNeeded,
    packsUnclear: zero && !session && typeof i.scan === "number",
    replug: needsReplug(o.installation.control, i.port),
    singlePackOffered,
    idle: !i.busy && !i.running,
  };
}

/** The text key of a stored note on sector 0x23000 (the backend writes it in English); null: show it as is. */
export function sectorNoteKey(note: string): string | null {
  if (note.startsWith("marker after first flash")) return "maint.note.first-flash";
  if (note.startsWith("unknown image")) return "maint.note.unknown-image";
  if (note.includes("recovered to original")) return "maint.note.recovered";
  return null;
}
