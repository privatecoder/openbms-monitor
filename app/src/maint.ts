import type { ControlEndpoint, Endpoint, FirmwareImage, Installation, LiveStatus, MaintError, MasterView, Pending } from "./api";

/** The monitor connection (sidebar): connected, paused by the app for a maintenance step (`monitor-stopped`),
 *  not connected (never, or closed by the user), or lost (link-lost or data timeout). */
export type Monitor = "connected" | "paused" | "offline" | "lost";

type T = (key: string, params?: Record<string, unknown>) => string;

/** Accepts only the answer of the newest request (sequence ticket): an older reply that arrives late never
 *  overwrites a newer list. */
export function latestOnly() {
  let seq = 0;
  return { begin: () => ++seq, accepts: (ticket: number) => ticket === seq };
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

/** Same key as the backend's `endpoint_key`: the physical endpoint, independent of the bus. */
export function endpointKey(ep: Endpoint): string {
  return ep.kind === "tcp" ? `tcp:${ep.addr.trim().toLowerCase()}` : `serial:${ep.path.trim()}`;
}

const controlOf = (ep: Endpoint): ControlEndpoint => (ep.kind === "tcp" ? { kind: "tcp", addr: ep.addr } : { kind: "serial", path: ep.path });
const controlKey = (c: ControlEndpoint | null) => (c ? (c.kind === "tcp" ? endpointKey({ kind: "tcp", addr: c.addr, bus: "can" }) : endpointKey({ kind: "serial", path: c.path, bus: "can" })) : null);

/** A sidebar connection that belongs to no installation becomes a new one (after the name dialog): on the CAN socket
 *  bus as its CAN socket, on the pack bus as its endpoint. */
export function createFromAttach(ep: Endpoint): { control: ControlEndpoint | null; endpoints: string[] } {
  return ep.bus === "can" ? { control: controlOf(ep), endpoints: [] } : { control: null, endpoints: [endpointKey(ep)] };
}

export type Attach = { do: "keep" } | { do: "use"; id: string } | { do: "create" } | { do: "offer-assign" };

/** What a new sidebar connection does to the installation shown (installations are an address book, an endpoint
 *  may belong to several): keep it when it lists the endpoint, else show the first one that does; an unknown
 *  endpoint is offered for the shown one, or named as a new one when none is shown. */
export function attachDecision(currentId: string | null, known: Pick<Installation, "id">[]): Attach {
  if (currentId && known.some((k) => k.id === currentId)) return { do: "keep" };
  if (known.length) return { do: "use", id: known[0].id };
  return currentId ? { do: "offer-assign" } : { do: "create" };
}

/** A connection on the CAN socket bus (sidebar) for an installation without a CAN socket: the adapter becomes its
 *  CAN socket, never a listed endpoint (which could pass for the RS485-1/2 port). null: nothing to change. */
export function controlFromAttach(inst: Installation, ep: Endpoint): Installation | null {
  if (ep.bus !== "can" || inst.control) return null;
  const key = endpointKey(ep);
  return { ...inst, control: controlOf(ep), endpoints: inst.endpoints.filter((k) => k !== key) };
}

/** Assign a connection explicitly (banner): on the CAN socket bus it sets (or keeps) the CAN socket; a pack-bus
 *  endpoint is listed. */
export function assignEndpoint(inst: Installation, ep: Endpoint): Installation {
  const key = endpointKey(ep);
  if (ep.bus === "can") {
    if (controlKey(inst.control) === key) return inst;
    return { ...inst, control: controlOf(ep), endpoints: inst.endpoints.filter((k) => k !== key) };
  }
  return inst.endpoints.includes(key) ? inst : { ...inst, endpoints: [...inst.endpoints, key] };
}

/** The installation's RS485-1/2 port (flash packs): the first stored serial endpoint that is not its CAN socket; the
 *  CAN socket adapter itself only when it is the one serial endpoint listed (one adapter moved between the buses).
 *  null: not set up. */
export function packPortOf(inst: Pick<Installation, "endpoints" | "control"> | null): string | null {
  if (!inst) return null;
  const serial = inst.endpoints.filter((k) => k.startsWith("serial:"));
  const pick = serial.find((k) => k !== controlKey(inst.control)) ?? serial[0];
  return pick ? pick.slice("serial:".length) : null;
}

/** The setup form's fields from a stored installation. */
export function setupFields(inst: Installation) {
  const c = inst.control;
  return { name: inst.name, canKind: (c?.kind ?? "serial") as ControlEndpoint["kind"], canValue: c ? (c.kind === "tcp" ? c.addr : c.path) : "", pack: packPortOf(inst) ?? "" };
}

/** The endpoint list the setup form saves: the RS485-1/2 port first (it reads back as the pack port), then every
 *  other stored endpoint except the old pack port and the CAN socket key. An empty pack port drops it. */
export function setupEndpoints(inst: Pick<Installation, "endpoints" | "control">, pack: string): string[] {
  const oldPack = packPortOf(inst);
  const oldPackKey = oldPack ? `serial:${oldPack}` : null;
  const ctl = controlKey(inst.control);
  const kept = inst.endpoints.filter((k) => k !== oldPackKey && k !== ctl);
  const out: string[] = [];
  for (const k of [pack ? endpointKey({ kind: "serial", path: pack, bus: "pack" }) : null, ...kept]) if (k && !out.includes(k)) out.push(k);
  return out;
}

const KINDS = ["connection", "unsupported", "no-answer", "rejected", "timeout", "refused", "busy", "store", "other-bank", "bootloader-pending"];

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
      case "connection": case "store": return { key: `maint.err.${m.kind}`, params: { text: m.message } };
      case "other-bank": return { key: "maint.err.other-bank", params: { serial: m.serial } };
      case "bootloader-pending": return { key: "maint.err.bootloader-pending", params: { pack: m.pack } };
      default: return { key: `maint.err.${m.kind}` };
    }
  }
  return { key: "maint.err.text", params: { text: String(e) } };
}

/** The serial number of an `other-bank` refusal (to offer „Seriennummer übernehmen“), else null. */
export function otherBank(e: unknown): string | null {
  let v: unknown = e;
  if (typeof e === "string") { try { v = JSON.parse(e); } catch { return null; } }
  return v && typeof v === "object" && (v as MaintError).kind === "other-bank" ? (v as { serial: string }).serial : null;
}

/** The backend's refusal of `connect` while another operation holds the port: `{kind: "busy", action}` with the
 *  running maintenance operation ("live", "scan", "halt", "release", "flash"; null when it has none). Else null. */
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

const BUSY_ACTIONS = ["live", "scan", "halt", "release", "flash"];

/** "Port busy: the pack bus is being watched." and what happens next: waiting for it, or given up after the wait. */
export function busyMessage(action: string | null, gaveUp: boolean, t: T): string {
  const what = t(`conn.busyAction.${action && BUSY_ACTIONS.includes(action) ? action : "other"}`);
  return t(gaveUp ? "conn.busyGaveUp" : "conn.busyWait", { what });
}

export type Tone = "ok" | "warn" | "muted" | "alarm";

/** The panel's status line for the pack bus (live, every 2 s). During a flash the progress takes its place. */
export function busLine(live: LiveStatus | null, running: boolean, t: T): { tone: Tone; text: string } {
  if (running) return { tone: "muted", text: t("maint.live.running") };
  if (!live) return { tone: "muted", text: t("maint.live.waiting") };
  switch (live.bus) {
    case "quiet": return { tone: "ok", text: t("maint.live.quiet") };
    case "polling": return { tone: "warn", text: t("maint.live.polling", { count: live.polled.filter((a) => a !== 0).length }) };
    case "traffic": return { tone: "warn", text: t("maint.live.traffic") };
    default: return { tone: "muted", text: t("maint.live.noPort") };
  }
}

/** The master's part of the status line (with a CAN socket): version, marker and the slaves it polls. */
export function masterLine(m: MasterView, t: T): { tone: Tone; text: string } {
  const slaves = m.slaves !== null ? t("maint.live.slaves", { count: m.slaves }) : null;
  const marker = m.marker === "on" ? t("maint.live.markerOn") : m.marker === "off" ? t("maint.live.markerOff") : m.marker === "foreign" ? t("maint.live.markerForeign", { word: m.marker_word }) : null;
  const join = (...parts: (string | null)[]) => parts.filter(Boolean).join(" · ");
  switch (m.kind) {
    case "maint": return { tone: m.marker === "foreign" ? "alarm" : "muted", text: join(m.version, marker, slaves) };
    case "unknown-build": return { tone: "alarm", text: join(t("maint.live.unknownBuild", { version: m.version, id: m.build_id }), marker, slaves) };
    case "legacy": return { tone: "muted", text: join(t("maint.live.legacy"), marker, slaves) };
    case "no-0x64": return { tone: "muted", text: t("maint.live.no0x64") };
    default: return { tone: "muted", text: t("maint.live.masterUnknown") };
  }
}

/** Scenario C': the master (address 0) with a trusted maintenance build and its marker on gets an image that is
 *  not a maintenance build only after „Master freigeben“ (the backend refuses it too). */
export function releaseFirst(master: MasterView | null, address: number, kind: FirmwareImage["kind"] | null): boolean {
  return address === 0 && !!kind && kind !== "maintenance" && !!master?.may_release;
}

/** A pending bootloader record on this endpoint that refuses flashing `address` (another pack), or null. */
export function pendingBlock(pending: Pending[], port: string, address: number): Pending | null {
  const key = port ? endpointKey({ kind: "serial", path: port, bus: "pack" }) : null;
  return pending.find((p) => p.endpoint === key && p.pack !== address) ?? null;
}

/** Why a pack row's „Flashen“ is not usable now (text key and params), or null. The backend checks again. */
export function flashBlocked(f: { image: FirmwareImage | null; quiet: boolean; busy: boolean; running: boolean; address: number; master: MasterView | null; pending: Pending[]; port: string }): { key: string; params?: Record<string, unknown> } | null {
  if (f.running) return { key: "maint.packs.why.running" };
  if (!f.image) return { key: "maint.packs.why.image" };
  if (f.busy) return { key: "maint.packs.why.busy" };
  if (!f.quiet) return { key: "maint.packs.why.quiet" };
  const p = pendingBlock(f.pending, f.port, f.address);
  if (p) return { key: "maint.err.bootloader-pending", params: { pack: p.pack } };
  if (releaseFirst(f.master, f.address, f.image.kind)) return { key: "maint.packs.why.releaseFirst" };
  return null;
}

/** „Fertig“ is shown after at least one flash in this panel, or while the master reports its marker on. */
export function showDone(flashed: boolean, master: MasterView | null): boolean {
  return flashed || master?.marker === "on";
}
