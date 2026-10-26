import type { Bus, ControlEndpoint, Endpoint, FirmwareImage, Installation, LiveStatus, MaintError, MasterView, PackProbeView, Pending } from "./api";
import { hardStopConfigurable } from "./access";
import { patchName } from "./profile";
import { refusalOf } from "./wiring";
import { decode } from "./errors";

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

export const controlKey = (c: ControlEndpoint | null) => (c ? (c.kind === "tcp" ? endpointKey({ kind: "tcp", addr: c.addr, bus: "can" }) : endpointKey({ kind: "serial", path: c.path, bus: "can" })) : null);

export type PortKind = "tcp" | "serial";

/** The key of a port as chosen in a form or the sidebar ("" = none): formed once here, then passed unchanged to the
 *  backend, the pending records and every comparison. */
export function portKey(kind: PortKind, target: string): string {
  if (!target.trim()) return "";
  return kind === "tcp" ? endpointKey({ kind, addr: target, bus: "pack" }) : endpointKey({ kind, path: target, bus: "pack" });
}

/** An endpoint key split into kind and address/port; null for anything else. */
export function keyParts(key: string | null): { kind: PortKind; target: string } | null {
  const i = key?.indexOf(":") ?? -1;
  if (!key || i < 0) return null;
  const kind = key.slice(0, i).toLowerCase();
  const target = key.slice(i + 1);
  return (kind === "tcp" || kind === "serial") && target ? { kind, target } : null;
}

/** What a key shows: the address or the port. */
export const keyLabel = (key: string) => keyParts(key)?.target ?? key;

export type Preselect = { do: "keep" } | { do: "use"; id: string } | { do: "pick" };

/** What a gateway address (TCP) does to the installation chosen before connecting. `known`: the non-archived
 *  installations it is linked to. Exactly one: that one. Several: the current one if it is among them, else the user
 *  picks. None: no change. A serial port never changes the choice (a USB adapter moves between banks). */
export function preselect(current: string | null, known: Pick<Installation, "id">[]): Preselect {
  if (known.length === 1) return known[0].id === current ? { do: "keep" } : { do: "use", id: known[0].id };
  if (known.length > 1) return known.some((k) => k.id === current) ? { do: "keep" } : { do: "pick" };
  return { do: "keep" };
}

/** The installations offered in the sidebar: the active ones, and an archived one only while it is the chosen one. */
export function pickable(list: Installation[], current: string | null): Installation[] {
  return list.filter((i) => !i.archived || i.id === current);
}

/** A legacy installation („übernommen“) whose endpoint text names an endpoint linked to exactly one other
 *  installation: that one is offered for „Mit Anlage X zusammenführen“. */
export function mergeSuggestion(inst: Installation, list: Installation[]): Installation | null {
  if (!inst.legacy) return null;
  const text = inst.legacy.trim();
  const keys = [endpointKey({ kind: "tcp", addr: text, bus: "can" }), endpointKey({ kind: "serial", path: text, bus: "pack" })];
  const owners = list.filter((o) => o.id !== inst.id && !o.legacy && (o.endpoints.some((k) => keys.includes(k)) || (o.control && keys.includes(controlKey(o.control)!))));
  return owners.length === 1 ? owners[0] : null;
}

/** The installation's RS485-1/2 port as its endpoint key: the first endpoint in stored order that is not its CAN
 *  socket, of any kind (twin of the backend's `pack_port`). Used by the editor, the sidebar, maintenance, flashing and
 *  the pending display. null: not set up. */
export function packPortOf(inst: Pick<Installation, "endpoints" | "control"> | null): string | null {
  if (!inst) return null;
  const ctl = controlKey(inst.control);
  return inst.endpoints.find((k) => k !== ctl) ?? null;
}

/** RS485-1/2 was set to the CAN socket's adapter: the panel names it instead of watching a bus it cannot tell. */
export function packIsCanAdapter(inst: Pick<Installation, "endpoints" | "control"> | null): boolean {
  const ctl = controlKey(inst?.control ?? null);
  return !!inst && !!ctl && !packPortOf(inst) && inst.endpoints.includes(ctl);
}

/** The stored port of an installation for a bus: the CAN socket (`control`) or the RS485-1/2 port (also the CAN
 *  socket's adapter set up as RS485-1/2). */
export function storedPort(inst: Pick<Installation, "endpoints" | "control"> | null, bus: Bus): { kind: PortKind; target: string } | null {
  if (!inst) return null;
  const can = inst.control ? (inst.control.kind === "tcp" ? { kind: "tcp" as const, target: inst.control.addr } : { kind: "serial" as const, target: inst.control.path }) : null;
  if (bus === "can" || packIsCanAdapter(inst)) return can;
  return keyParts(packPortOf(inst));
}

/** The setup form's fields from a stored installation. */
export function setupFields(inst: Installation) {
  const c = inst.control;
  const pack = keyParts(packPortOf(inst) ?? (packIsCanAdapter(inst) ? controlKey(c) : null));
  return {
    name: inst.name, canKind: (c?.kind ?? "serial") as PortKind, canValue: c ? (c.kind === "tcp" ? c.addr : c.path) : "",
    packKind: pack?.kind ?? ("serial" as PortKind), packValue: pack?.target ?? "",
  };
}

/** The endpoint list the setup form saves: the RS485-1/2 port (`pack`, a key; "" drops it) first, so it reads back as
 *  the pack port, then every other stored endpoint except the old pack port and the CAN socket key. The CAN socket's
 *  own adapter as RS485-1/2 stands alone: any other endpoint would pass for the pack port (`packPortOf`). */
export function setupEndpoints(inst: Pick<Installation, "endpoints" | "control">, pack: string): string[] {
  const oldPack = packPortOf(inst);
  // the CAN socket key is dropped below unless it is chosen as RS485-1/2 again
  const ctl = controlKey(inst.control);
  if (pack && pack === ctl) return [pack];
  const kept = inst.endpoints.filter((k) => k !== oldPack && k !== ctl);
  const out: string[] = [];
  for (const k of [pack || null, ...kept]) if (k && !out.includes(k)) out.push(k);
  return out;
}

/** What „Speichern“ under the connection status stores: the endpoint as the CAN socket or as the RS485-1/2 port. */
export function withPort(inst: Installation, bus: Bus, kind: PortKind, target: string): Installation {
  if (bus === "can") return { ...inst, control: kind === "tcp" ? { kind, addr: target.trim() } : { kind, path: target } };
  return { ...inst, endpoints: setupEndpoints(inst, portKey(kind, target)) };
}

/** After a connect: offer to save its endpoint when the installation has another port stored for that bus. An empty
 *  one was filled by the connect itself (CAN socket) or is filled once the bank is decided (RS485-1/2). */
export function saveOffer(inst: Pick<Installation, "endpoints" | "control"> | null, bus: Bus, kind: PortKind, target: string): boolean {
  const stored = storedPort(inst, bus);
  return !!stored && portKey(stored.kind, stored.target) !== portKey(kind, target);
}

const KINDS = ["connection", "unsupported", "no-answer", "rejected", "start-refused", "timeout", "refused", "busy", "store", "other-bank", "bootloader-pending"];

/** Backend errors arrive as a MaintError object, as its JSON text, as a bare refusal key, or as plain text. */
export function errorKey(e: unknown): { key: string; params?: Record<string, unknown> } {
  // the bus rule sent nothing (`wiring.rs`), whatever action it stopped
  const r = refusalOf(e);
  if (r) return { key: `wiring.refused.${r}` };
  // a bare key (`refused`) is a refusal of the store or a maintenance step
  const v: unknown = decode(e) ?? (typeof e === "string" && /^[a-z0-9-]+$/.test(e) ? { kind: "refused", reason: e } : e);
  if (v && typeof v === "object" && "kind" in v && KINDS.includes(String((v as MaintError).kind))) {
    const m = v as MaintError;
    switch (m.kind) {
      case "refused": return { key: `maint.err.refused.${m.reason}` };
      case "timeout": return { key: "maint.err.timeout", params: { marker: m.marker, slaves: m.slaves } };
      case "start-refused": return { key: `maint.err.start.${m.reason}` };
      case "rejected": return { key: "maint.err.rejected", params: { rtn: m.rtn.toString(16).padStart(2, "0") } };
      case "connection": case "store": return { key: `maint.err.${m.kind}`, params: { text: m.message } };
      case "other-bank": return { key: "maint.err.other-bank", params: { serial: m.serial } };
      case "bootloader-pending": return { key: "maint.err.bootloader-pending", params: { pack: m.pack, port: keyLabel(m.endpoint ?? "") } };
      default: return { key: `maint.err.${m.kind}` };
    }
  }
  return { key: "maint.err.text", params: { text: String(e) } };
}

/** The serial number of an `other-bank` refusal (to offer „Seriennummer übernehmen“), else null. */
export function otherBank(e: unknown): string | null {
  const v: unknown = decode(e);
  return v && typeof v === "object" && (v as MaintError).kind === "other-bank" ? (v as { serial: string }).serial : null;
}

/** The backend's refusal of `connect` while another operation holds the port: `{kind: "busy", action}` with the
 *  running maintenance operation ("live", "scan", "halt", "release", "flash", "pack" for an action on a pack; null
 *  when it has none), and the refusal "installation-in-use" as "installation". Else null. */
export function portBusy(e: unknown): { action: string | null } | null {
  const v: unknown = decode(e);
  if (!v || typeof v !== "object") return null;
  // a merge, delete, export or import of the installation: waited for like a busy port
  const r = v as { kind?: unknown; reason?: unknown };
  if (r.kind === "refused" && r.reason === "installation-in-use") return { action: "installation" };
  if (r.kind !== "busy") return null;
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

const BUSY_ACTIONS = ["live", "scan", "halt", "release", "flash", "pack"];

/** "Port busy: the pack bus is being watched." and what happens next: waiting for it, or given up after the wait. */
export function busyMessage(action: string | null, gaveUp: boolean, t: T): string {
  if (action === "installation") return t(gaveUp ? "conn.installationGaveUp" : "conn.installationWait", { what: t("maint.err.refused.installation-in-use") });
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
/** A quiet port while the master, read over the CAN socket, says it polls its slaves: the port hears nothing, it is not
 *  on the pack bus (polling would be heard at least once a second). */
export function notOnBus(bus: string | undefined, m: MasterView | null): boolean {
  return bus === "quiet" && m?.kind === "patched" && m.marker === "off" && (m.slaves ?? 0) > 0;
}

export function masterLine(m: MasterView, t: T): { tone: Tone; text: string } {
  const slaves = m.slaves !== null ? t("maint.live.slaves", { count: m.slaves }) : null;
  const marker = m.marker === "on" ? t("maint.live.markerOn") : m.marker === "off" ? t("maint.live.markerOff") : m.marker === "foreign" ? t("maint.live.markerForeign", { word: m.marker_word }) : null;
  const join = (...parts: (string | null)[]) => parts.filter(Boolean).join(" · ");
  switch (m.kind) {
    case "patched": return { tone: m.marker === "foreign" ? "alarm" : "muted", text: join(patchName(m, t), marker, slaves) };
    case "unknown-patch": return { tone: "alarm", text: t("maint.live.unknownPatch", { version: m.version ?? "" }) };
    case "no-0x64": return { tone: "muted", text: t("maint.live.no0x64") };
    default: return { tone: "muted", text: t("maint.live.masterUnknown") };
  }
}

/** The version shown for a pack: 0x51 only knows major.minor (e.g. 16.06); for the master (address 0) a recognised
 *  patch read over the CAN socket gives the full build. */
export function packVersion(address: number, firmware: string, master: MasterView | null, t?: T): string {
  return address === 0 && master?.kind === "patched" && master.version ? (t ? patchName(master, t) : master.listed ?? master.version) : firmware;
}

/** Scenario C': the master (address 0) with a trusted maintenance build and its marker on gets an image that is
 *  not a maintenance build only after „Master freigeben“ (the backend refuses it too). */
export function releaseFirst(master: MasterView | null, address: number, kind: FirmwareImage["kind"] | null): boolean {
  return address === 0 && !!kind && kind !== "maintenance" && !!master?.may_release;
}

/** A pending bootloader record on this endpoint (`port`: its key) that refuses flashing `address` (another pack), or
 *  null. */
export function pendingBlock(pending: Pending[], port: string, address: number): Pending | null {
  return port ? pending.find((p) => p.endpoint !== port || p.pack !== address) ?? null : null;
}

/** The text params of a refused save: the port of an open bootloader record of the installation ("pending-on-port"). */
export function refusalParams(inst: Pick<Installation, "endpoints">, pending: Pending[], params?: Record<string, unknown>): Record<string, unknown> {
  const open = inst.endpoints.find((ep) => pending.some((p) => p.endpoint === ep));
  return { ...params, port: open ? keyLabel(open) : "" };
}

/** Why a pack row's „Flashen“ is not usable now (text key and params), or null. The backend checks again. */
export function flashBlocked(f: { image: FirmwareImage | null; quiet: boolean; busy: boolean; running: boolean; address: number; master: MasterView | null; pending: Pending[]; port: string }): { key: string; params?: Record<string, unknown> } | null {
  if (f.running) return { key: "maint.packs.why.running" };
  if (!f.image) return { key: "maint.packs.why.image" };
  if (f.busy) return { key: "maint.packs.why.busy" };
  if (!f.quiet) return { key: "maint.packs.why.quiet" };
  const p = pendingBlock(f.pending, f.port, f.address);
  if (p) return { key: "maint.err.bootloader-pending", params: { pack: p.pack, port: keyLabel(p.endpoint) } };
  if (releaseFirst(f.master, f.address, f.image.kind)) return { key: "maint.packs.why.releaseFirst" };
  return null;
}

/** „Fertig“ is shown after at least one flash in this panel, or while the master reports its marker on. */
export function showDone(flashed: boolean, master: MasterView | null): boolean {
  return flashed || master?.marker === "on";
}

/** The pack list's version: with the pack's own 64/03 (proven) „Firmware 16.06.13 · MAINT 1.5 (Build 297f9f2a…, Hard-Stop
 *  aus)“; else the 0x51 version with „ohne Patch-Funktionen“ (another MNT) or „Patch nicht geprüft“. Never inferred from
 *  0x51 or from a refusal on the pack bus. */
export function packVersionText(firmware: string, probe: PackProbeView | undefined, t: T): string {
  if (probe?.state === "proven" && probe.version) {
    const [base, maint] = probe.version.split(" MAINT ");
    const chosen = hardStopConfigurable(probe.flags) ? ((probe.flags! & 1) ? "profile.hardStopChosenOn" : "profile.hardStopChosenOff") : "profile.hardStopUnknown";
    const hard = t(probe.hard_stop === true ? "profile.hardStopOn" : probe.hard_stop === false ? "profile.hardStopOff" : chosen);
    return t("maint.packs.versionFull", { base, patch: probe.listed?.split(" MAINT ")[1] ?? maint ?? "", build: probe.build_id ? `${probe.build_id.slice(0, 8)}…` : "?", hard });
  }
  // an older patch is named; another MNT format may be a patch this app does not know: not proven, no more
  const key = probe?.state === "negative" ? (probe.version ? "maint.packs.versionNoPatch" : "maint.packs.versionUnknownFormat") : "maint.packs.versionUnchecked";
  return t(key, { version: firmware });
}
