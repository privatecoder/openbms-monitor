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
