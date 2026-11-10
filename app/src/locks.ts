import type { Source } from "./api";
import { flashBlocked } from "./maint";

/** One place for when a field or action of the UI is shown, offered or locked, and why. Components gather the facts
 *  (`UiFacts`) and render the decision; they do not phrase their own `disabled={…}` rules for what is decided here.
 *  The backend still checks every request itself (source, bus rule, reservations, pending, firmware): a decision here
 *  only keeps the UI from offering what does not fit and says why. Pure, so the rules are tested as a table. */

export type UiAction =
  // Installations view, per installation card (`subject`)
  | "installation.choose"
  | "installation.editWiring"
  | "installation.editPacks"
  | "installation.delete"
  /** merge this installation into another one (it goes away) */
  | "installation.mergeAway"
  // Sidebar connection
  /** installation, source tabs, ports, bus: the fields a connection is opened with */
  | "connection.editFields"
  /** the pack selection fields (search / count / address): only without an installation („Nur ansehen“) */
  | "connection.packsPicker"
  /** the installation's pack selection as text, changed under Installations */
  | "connection.packsText"
  // Firmware view
  /** switch between the pack bus panel and „Platine flashen“ */
  | "firmware.tab"
  /** image, „Eigene Firmware zulassen“, port, address: what a flash is started with */
  | "firmware.inputs"
  /** „Platine flashen“: search the boards on the port */
  | "firmware.benchSearch"
  /** „Platine flashen“: flash the chosen board */
  | "firmware.benchFlash"
  /** recovery: ask once whether a bootloader waits on the line */
  | "firmware.recoverProbe"
  /** recovery: write the image into the waiting bootloader */
  | "firmware.recover"
  /** a read of the target before a flash (state of charge store, release bits) */
  | "firmware.readTarget"
  // Maintenance (pack bus panel, banner)
  /** a pack row's „Prüfen & flashen“ (`flashBlocked`) */
  | "maintenance.flash"
  | "maintenance.halt"
  | "maintenance.release"
  | "maintenance.rescan"
  /** take the master serial / a board over for the installation */
  | "maintenance.adopt"
  | "maintenance.editPorts"
  /** the banner's release of a marker or discard of a latent one */
  | "marker.release"
  /** the banner's „cleared by hand“ of a pending bootloader record */
  | "pending.clear";

export interface UiFacts {
  /** the source of the open connection: "bus" (RS485), "mqtt" (the add-on's feed), null: none */
  source: Source | null;
  /** the installation chosen in the sidebar (the one a connection belongs to); null: „Nur ansehen“ */
  chosen: string | null;
  /** the installation a control belongs to (a card in Installations) */
  subject?: string | null;
  /** the sidebar's source tab */
  way?: "bus" | "mqtt";
  /** the sidebar is connecting, connected, or retrying a lost connection: its fields stay as the connection was opened */
  linkPhase?: "idle" | "connecting" | "open" | "lost";
  /** another action holds the port (a flash, a maintenance action, a pack check) */
  portHeld?: boolean;
  /** a local action of the component runs (a rename, merge, save …) */
  busy?: boolean;
  /** the subject's settings are loaded (nothing is saved over settings not read yet) */
  loaded?: boolean;
  /** a flash or a recovery runs (until its end event) */
  flashRunning?: boolean;
  /** a firmware image is chosen */
  image?: boolean;
  /** the port an action works on is set */
  port?: boolean;
  /** „Platine flashen“: a board is chosen; recovery: a bootloader answered and „only this board“ is confirmed */
  target?: boolean;
  /** maintenance: the pack bus is quiet (no master polling) */
  quiet?: boolean;
  /** a pack row's flash: everything `flashBlocked` weighs (image, quiet, busy, running, address, master, pending, port) */
  flash?: Parameters<typeof flashBlocked>[0];
}

export interface UiReason { key: string; params?: Record<string, unknown> }
/** `note`: a hint shown with an offered control (e.g. „gilt ab der nächsten Verbindung“). */
export interface UiDecision { visible: boolean; locked: boolean; reason?: UiReason; note?: UiReason }

const show = (note?: UiReason): UiDecision => ({ visible: true, locked: false, ...(note ? { note } : {}) });
const lock = (key: string): UiDecision => ({ visible: true, locked: true, reason: { key } });
const hide: UiDecision = { visible: false, locked: true };

// the sidebar's fields stay as the running connection was opened (a change would only count with the next connect)
const fieldsLock = (f: UiFacts): UiDecision =>
  f.linkPhase && f.linkPhase !== "idle" ? lock("lock.connectionOpen") : f.portHeld ? lock("lock.portHeld") : show();

// what a flash is started with stays as it was while one runs or a check holds the panel
const inputsLock = (f: UiFacts): UiDecision | null =>
  f.flashRunning ? lock("lock.flashRunning") : f.busy ? lock("lock.busy") : null;

export function decideUi(action: UiAction, f: UiFacts): UiDecision {
  const own = f.subject != null && f.subject === f.chosen;
  // the subject's connection is open (either source): its answers are recorded under it
  const ownLive = own && f.source !== null;
  switch (action) {
    case "installation.choose":
      return own || f.source !== null ? hide : show();
    case "installation.editWiring":
      if (f.loaded === false) return lock("lock.loading");
      // the bus rule is bound when an RS485 connection opens; over MQTT nothing is sent to a BMS
      if (own && f.source === "bus") return lock("lock.wiringLive");
      if (ownLive) return show({ key: "lock.nextBus" });
      return show();
    case "installation.editPacks":
      if (f.loaded === false) return lock("lock.loading");
      if (f.busy) return lock("lock.busy");
      if (own && f.source === "bus") return show({ key: "lock.nextConnect" });
      return show();
    case "installation.delete":
    case "installation.mergeAway":
      if (f.busy) return lock("lock.busy");
      if (ownLive) return lock("lock.connectedInstallation");
      return show();
    case "connection.editFields":
      return fieldsLock(f);
    case "connection.packsPicker":
      if (f.way !== "bus" || f.chosen !== null) return hide;
      return fieldsLock(f);
    case "connection.packsText":
      return f.way === "bus" && f.chosen !== null ? show() : hide;
    case "firmware.tab":
    case "maintenance.editPorts":
      return f.flashRunning ? lock("lock.flashRunning") : show();
    case "firmware.inputs":
      return inputsLock(f) ?? show();
    case "firmware.benchSearch":
      return inputsLock(f) ?? (!f.port ? lock("lock.noPort") : show());
    case "firmware.benchFlash":
      return inputsLock(f) ?? (!f.target ? lock("lock.noTarget") : !f.image ? lock("lock.noImage") : show());
    case "firmware.recoverProbe":
      return inputsLock(f) ?? (!f.port ? lock("lock.noPort") : show());
    case "firmware.recover":
      return inputsLock(f) ?? (!f.image ? lock("lock.noImage") : !f.target ? lock("lock.oneBoard") : show());
    case "firmware.readTarget":
      if (f.flashRunning) return lock("lock.flashRunning");
      if (f.portHeld) return lock("lock.portHeld");
      if (f.busy) return lock("lock.busy");
      return f.port ? show() : lock("lock.noPort");
    case "maintenance.flash": {
      if (!f.flash) return lock("lock.loading");
      const why = flashBlocked(f.flash);
      return why ? { visible: true, locked: true, reason: why } : show();
    }
    // halt and release are the way out of a maintenance: only another panel action holds them (a set marker never does)
    case "maintenance.halt":
    case "maintenance.release":
      return f.busy ? lock("lock.busy") : show();
    case "maintenance.rescan":
      if (f.busy) return lock("lock.busy");
      if (f.flashRunning) return lock("lock.flashRunning");
      return f.quiet ? show() : lock("maint.packs.why.quiet");
    case "maintenance.adopt":
    case "marker.release":
      if (f.busy) return lock("lock.busy");
      return f.flashRunning ? lock("lock.flashRunning") : show();
    // the manual clearing of a pending record stays reachable whatever else is locked by it
    case "pending.clear":
      return f.busy ? lock("lock.busy") : show();
  }
}

/** For JSX: the `disabled` of a control and its title (the reason, translated). */
export function lockProps(d: UiDecision, t: (key: string, params?: Record<string, unknown>) => string): { disabled: boolean; title?: string } {
  return d.locked ? { disabled: true, ...(d.reason ? { title: t(d.reason.key, d.reason.params) } : {}) } : { disabled: false };
}
