import { describe, expect, it } from "vitest";
import { decideUi, lockProps, type UiAction, type UiFacts } from "./locks";

const base: UiFacts = { source: null, chosen: "A", subject: "A" };
const state = (a: UiAction, f: Partial<UiFacts>) => {
  const d = decideUi(a, { ...base, ...f });
  return !d.visible ? "hidden" : d.locked ? `locked:${d.reason?.key}` : d.note ? `open:${d.note.key}` : "open";
};

describe("installation card", () => {
  it("wiring: fixed only while this installation runs over RS485, for the next direct connection over MQTT", () => {
    expect(state("installation.editWiring", { source: "bus" })).toBe("locked:lock.wiringLive");
    expect(state("installation.editWiring", { source: "mqtt" })).toBe("open:lock.nextBus");
    expect(state("installation.editWiring", { source: null })).toBe("open");
    // another installation's card: free whatever runs
    expect(state("installation.editWiring", { source: "bus", subject: "B" })).toBe("open");
    expect(state("installation.editWiring", { source: null, loaded: false })).toBe("locked:lock.loading");
  });
  it("packs: always editable once loaded, for the next connection while connected over RS485", () => {
    expect(state("installation.editPacks", { source: "bus" })).toBe("open:lock.nextConnect");
    expect(state("installation.editPacks", { source: "mqtt" })).toBe("open");
    expect(state("installation.editPacks", { source: null, subject: "B" })).toBe("open");
    expect(state("installation.editPacks", { loaded: false })).toBe("locked:lock.loading");
    expect(state("installation.editPacks", { busy: true })).toBe("locked:lock.busy");
  });
  it("delete and merge away: never the connected installation, whichever source", () => {
    for (const a of ["installation.delete", "installation.mergeAway"] as const) {
      expect(state(a, { source: "bus" })).toBe("locked:lock.connectedInstallation");
      expect(state(a, { source: "mqtt" })).toBe("locked:lock.connectedInstallation");
      expect(state(a, { source: null })).toBe("open");
      expect(state(a, { source: "bus", subject: "B" })).toBe("open");
      expect(state(a, { busy: true })).toBe("locked:lock.busy");
    }
  });
  it("choose: only another installation while nothing is connected", () => {
    expect(state("installation.choose", { subject: "B" })).toBe("open");
    expect(state("installation.choose", {})).toBe("hidden");
    expect(state("installation.choose", { subject: "B", source: "mqtt" })).toBe("hidden");
  });
});

describe("sidebar connection", () => {
  it("fields stay as the connection was opened, and while another action holds the port", () => {
    expect(state("connection.editFields", { linkPhase: "idle" })).toBe("open");
    for (const p of ["connecting", "open", "lost"] as const) expect(state("connection.editFields", { linkPhase: p })).toBe("locked:lock.connectionOpen");
    expect(state("connection.editFields", { linkPhase: "idle", portHeld: true })).toBe("locked:lock.portHeld");
  });
  it("the pack picker only for „Nur ansehen“ on RS485; with an installation its setting as text", () => {
    expect(state("connection.packsPicker", { chosen: null, way: "bus", linkPhase: "idle" })).toBe("open");
    expect(state("connection.packsPicker", { chosen: null, way: "bus", linkPhase: "open" })).toBe("locked:lock.connectionOpen");
    expect(state("connection.packsPicker", { chosen: "A", way: "bus" })).toBe("hidden");
    expect(state("connection.packsPicker", { chosen: null, way: "mqtt" })).toBe("hidden");
    expect(state("connection.packsText", { chosen: "A", way: "bus" })).toBe("open");
    expect(state("connection.packsText", { chosen: "A", way: "mqtt" })).toBe("hidden");
    expect(state("connection.packsText", { chosen: null, way: "bus" })).toBe("hidden");
  });
  it("lockProps gives disabled and the reason as title", () => {
    const t = (k: string) => `T(${k})`;
    expect(lockProps(decideUi("connection.editFields", { ...base, linkPhase: "open" }), t)).toEqual({ disabled: true, title: "T(lock.connectionOpen)" });
    expect(lockProps(decideUi("connection.editFields", { ...base, linkPhase: "idle" }), t)).toEqual({ disabled: false });
  });
});

describe("firmware and maintenance", () => {
  const fw: UiFacts = { source: null, chosen: "A", image: true, port: true };
  const s = (a: UiAction, f: Partial<UiFacts>) => state(a, { ...fw, ...f });
  it("what a flash starts with stays fixed while one runs or a check holds the panel", () => {
    for (const a of ["firmware.inputs", "firmware.benchSearch", "firmware.recoverProbe"] as const) {
      expect(s(a, {})).toBe("open");
      expect(s(a, { flashRunning: true })).toBe("locked:lock.flashRunning");
      expect(s(a, { busy: true })).toBe("locked:lock.busy");
    }
    expect(s("firmware.benchSearch", { port: false })).toBe("locked:lock.noPort");
    expect(s("firmware.tab", { flashRunning: true })).toBe("locked:lock.flashRunning");
    expect(s("firmware.tab", { busy: true })).toBe("open");
  });
  it("bench flash and recovery need their target, the image, and nothing running", () => {
    expect(s("firmware.benchFlash", { target: true })).toBe("open");
    expect(s("firmware.benchFlash", { target: false })).toBe("locked:lock.noTarget");
    expect(s("firmware.benchFlash", { target: true, image: false })).toBe("locked:lock.noImage");
    expect(s("firmware.benchFlash", { target: true, flashRunning: true })).toBe("locked:lock.flashRunning");
    expect(s("firmware.recover", { target: true })).toBe("open");
    expect(s("firmware.recover", { target: false })).toBe("locked:lock.oneBoard");
    expect(s("firmware.recover", { target: true, image: false })).toBe("locked:lock.noImage");
    expect(s("firmware.recover", { target: true, busy: true })).toBe("locked:lock.busy");
  });
  it("a read of the target: not while flashing, the port is held or a read runs", () => {
    expect(s("firmware.readTarget", {})).toBe("open");
    expect(s("firmware.readTarget", { flashRunning: true })).toBe("locked:lock.flashRunning");
    expect(s("firmware.readTarget", { portHeld: true })).toBe("locked:lock.portHeld");
    expect(s("firmware.readTarget", { busy: true })).toBe("locked:lock.busy");
    expect(s("firmware.readTarget", { port: false })).toBe("locked:lock.noPort");
  });
  it("halt and release only wait for another panel action; rescan needs a quiet bus", () => {
    for (const a of ["maintenance.halt", "maintenance.release"] as const) {
      expect(s(a, { flashRunning: true })).toBe("open");
      expect(s(a, { busy: true })).toBe("locked:lock.busy");
    }
    expect(s("maintenance.rescan", { quiet: true })).toBe("open");
    expect(s("maintenance.rescan", { quiet: false })).toBe("locked:maint.packs.why.quiet");
    expect(s("maintenance.rescan", { quiet: true, flashRunning: true })).toBe("locked:lock.flashRunning");
    expect(s("maintenance.adopt", { flashRunning: true })).toBe("locked:lock.flashRunning");
    expect(s("maintenance.editPorts", { flashRunning: true })).toBe("locked:lock.flashRunning");
    expect(s("maintenance.editPorts", { busy: true })).toBe("open");
  });
  it("a pack row's flash: the reasons of flashBlocked, in its order", () => {
    const image = { kind: "original" } as never;
    const flash = { image, quiet: true, busy: false, running: false, address: 1, master: null, pending: [], port: "serial:/dev/p" };
    expect(s("maintenance.flash", { flash })).toBe("open");
    expect(s("maintenance.flash", { flash: { ...flash, quiet: false } })).toBe("locked:maint.packs.why.quiet");
    expect(s("maintenance.flash", { flash: { ...flash, running: true } })).toBe("locked:maint.packs.why.running");
    expect(s("maintenance.flash", { flash: { ...flash, image: null } })).toBe("locked:maint.packs.why.image");
    const pending = [{ id: 1, endpoint: "serial:/dev/p", pack: 3, since: 0 }] as never;
    expect(s("maintenance.flash", { flash: { ...flash, pending } })).toBe("locked:maint.err.bootloader-pending");
    // the pack the record is of may be flashed again (to finish it)
    expect(s("maintenance.flash", { flash: { ...flash, pending, address: 3 } })).toBe("open");
  });
  it("the banner: a pending record can always be cleared by hand; a marker is released unless a flash runs", () => {
    expect(s("pending.clear", { flashRunning: true })).toBe("open");
    expect(s("pending.clear", { busy: true })).toBe("locked:lock.busy");
    expect(s("marker.release", {})).toBe("open");
    expect(s("marker.release", { flashRunning: true })).toBe("locked:lock.flashRunning");
  });
});

describe("parameters: one write at a time", () => {
  const pf: UiFacts = { source: null, chosen: null, canReadParams: true, editing: null };
  const s = (a: UiAction, f: Partial<UiFacts>) => state(a, { ...pf, ...f });
  it("read: connected with packs, not while reading, editing, or writing parameters or a control", () => {
    expect(s("params.read", {})).toBe("open");
    expect(s("params.read", { canReadParams: false })).toBe("locked:lock.notConnected");
    expect(s("params.read", { reading: true })).toBe("locked:lock.reading");
    expect(s("params.read", { editing: 0 })).toBe("locked:lock.editOpen");
    for (const k of ["params", "multi", "ctl"] as const) expect(s("params.read", { writes: { [k]: true } })).toBe("locked:lock.writeRunning");
    // a clock write does not hold the read
    expect(s("params.read", { writes: { clock: true } })).toBe("open");
  });
  it("an editor opens only on a writable column with nothing else running", () => {
    expect(s("params.editStart", { columnWritable: true })).toBe("open");
    expect(s("params.editStart", { columnWritable: false })).toBe("locked:lock.notWritable");
    expect(s("params.editStart", { columnWritable: true, editing: 2 })).toBe("locked:lock.editOpen");
    expect(s("params.editStart", { columnWritable: true, writes: { clock: true } })).toBe("locked:lock.writeRunning");
    expect(s("params.editField", { writes: { multi: true } })).toBe("locked:lock.writeRunning");
    expect(s("params.editField", { writes: { clock: true } })).toBe("open");
  });
  it("sections: each waits for the others; name for a read, Hard-Stop for the editor of its pack", () => {
    expect(s("params.section", { section: "clock", writes: { clock: true } })).toBe("open");
    expect(s("params.section", { section: "clock", writes: { serial: true } })).toBe("locked:lock.writeRunning");
    expect(s("params.section", { writes: { clock: true } })).toBe("locked:lock.writeRunning");
    expect(s("params.section", {})).toBe("open");
    expect(s("params.section", { section: "name", reading: true })).toBe("locked:lock.reading");
    expect(s("params.section", { section: "hs", editing: 3, address: 3 })).toBe("locked:lock.editOpen");
    expect(s("params.section", { section: "hs", editing: 3, address: 4 })).toBe("open");
  });
  it("restore: free for the paused pack of a run, else only beside the parameter write", () => {
    expect(s("params.restore", { address: 2, writes: { params: true } })).toBe("open");
    expect(s("params.restore", { address: 2, writes: { multi: true } })).toBe("locked:lock.writeRunning");
    expect(s("params.restore", { address: 2, pausedAt: 2, writes: { multi: true } })).toBe("open");
    expect(s("params.restore", { address: 3, pausedAt: 2, writes: { multi: true } })).toBe("locked:lock.writeRunning");
  });
});

describe("asking a BMS directly", () => {
  const s = (a: UiAction, f: Partial<UiFacts>) => state(a, { source: "bus", chosen: null, bus: "can", profile: true, address: 3, ...f });
  it("only over the monitor's own RS485 connection, never over the add-on's feed", () => {
    expect(s("bms.talk", {})).toBe("open");
    expect(s("bms.talk", { source: "mqtt" })).toBe("hidden");
    expect(s("bms.talk", { source: null })).toBe("hidden");
  });
  it("the pack view's binary serial read and a slave's 64/03: CAN socket with a profile", () => {
    expect(s("pack.askSerials", {})).toBe("open");
    expect(s("pack.askSerials", { bus: "pack" })).toBe("hidden");
    expect(s("pack.askSerials", { profile: false })).toBe("hidden");
    expect(s("pack.askSerials", { source: "mqtt" })).toBe("hidden");
    expect(s("pack.probe", {})).toBe("open");
    expect(s("pack.probe", { address: 0 })).toBe("hidden");
    expect(s("pack.probe", { busy: true })).toBe("locked:lock.busy");
    expect(s("pack.probe", { source: "mqtt" })).toBe("hidden");
  });
});
