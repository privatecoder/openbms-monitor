import { describe, expect, it } from "vitest";
import { packIsCanAdapter, assignEndpoint, attachDecision, busLine, busyMessage, busyWaitStep, controlFromAttach, createFromAttach, defaultName, endpointKey, errorKey, flashBlocked, latestOnly, masterLine, nameCheck, otherBank, packPortOf, pendingBlock, portBusy, releaseFirst, setupEndpoints, setupFields, showDone } from "./maint";
import type { FirmwareImage, Installation, MasterView, Pending } from "./api";

const inst: Installation = { id: "i", name: "Bank", endpoints: [], control: { kind: "serial", path: "/dev/a" } };
const tr = (key: string, params?: Record<string, unknown>) => (params ? `${key} ${JSON.stringify(params)}` : key);
const master = (patch: Partial<MasterView>): MasterView => ({ kind: "maint", version: "16.06.13 MAINT 1.1", build_id: "fe381985bfff8ad0", marker: "off", marker_word: "FFFFFFFF", slaves: 3, may_halt: true, may_release: false, ...patch });
const image = (kind: FirmwareImage["kind"]): FirmwareImage => ({ release: "16.06.13", device: "EMU1101", bytes: 1, blocks: 1, sha256: "ab", kind, custom: kind !== "original" });
const pending = (pack: number, endpoint = "serial:/dev/p"): Pending => ({ id: pack, endpoint, pack, release: "16.06.13", since: 0 });

describe("installations: an address book", () => {
  it("requires a name that no other installation has", () => {
    expect(nameCheck("  ", [])).toBe("maint.setup.err.nameRequired");
    expect(nameCheck("bank ", [inst])).toBe("maint.setup.err.nameTaken");
    expect(nameCheck("Bank", [inst], "i")).toBeNull();
    expect(defaultName([{ id: "x", name: "Anlage 2" }], "Anlage")).toBe("Anlage 3");
  });
  it("a new connection: keep the shown one when it lists the endpoint, else the first that does, else offer or create", () => {
    const a = { id: "a" }, b = { id: "b" };
    expect(attachDecision("b", [a, b])).toEqual({ do: "keep" });
    expect(attachDecision("c", [a, b])).toEqual({ do: "use", id: "a" });
    expect(attachDecision(null, [b])).toEqual({ do: "use", id: "b" });
    expect(attachDecision("c", [])).toEqual({ do: "offer-assign" });
    expect(attachDecision(null, [])).toEqual({ do: "create" });
  });
  it("a connection becomes the CAN socket on the CAN socket bus, an endpoint on the pack bus", () => {
    expect(createFromAttach({ kind: "tcp", addr: "gw:4196", bus: "can" })).toEqual({ control: { kind: "tcp", addr: "gw:4196" }, endpoints: [] });
    expect(createFromAttach({ kind: "serial", path: "/dev/p", bus: "pack" })).toEqual({ control: null, endpoints: ["serial:/dev/p"] });
    const none = { ...inst, control: null, endpoints: ["tcp:gw:4196"] };
    expect(controlFromAttach(none, { kind: "tcp", addr: "GW:4196", bus: "can" })).toEqual({ ...none, control: { kind: "tcp", addr: "GW:4196" }, endpoints: [] });
    expect(controlFromAttach(inst, { kind: "tcp", addr: "gw:4196", bus: "can" })).toBeNull();
    expect(assignEndpoint(inst, { kind: "serial", path: "/dev/p", bus: "pack" }).endpoints).toEqual(["serial:/dev/p"]);
    expect(assignEndpoint(inst, { kind: "serial", path: "/dev/a", bus: "can" })).toBe(inst);
  });
  it("the RS485-1/2 port is the first serial endpoint that is not the CAN socket; the CAN adapter itself is named, not used", () => {
    expect(packPortOf(null)).toBeNull();
    expect(packPortOf(inst)).toBeNull();
    expect(packPortOf({ ...inst, endpoints: ["tcp:gw:4196", "serial:/dev/a", "serial:/dev/p"] })).toBe("/dev/p");
    expect(packPortOf({ ...inst, endpoints: ["serial:/dev/a"] })).toBeNull();
    expect(packIsCanAdapter({ ...inst, endpoints: ["serial:/dev/a"] })).toBe(true);
    expect(packIsCanAdapter({ ...inst, endpoints: ["serial:/dev/a", "serial:/dev/p"] })).toBe(false);
    expect(packIsCanAdapter(inst)).toBe(false);
    expect(setupFields({ ...inst, endpoints: ["serial:/dev/a"] }).pack).toBe("/dev/a");
  });
  it("the setup saves the pack port first and keeps other endpoints", () => {
    const stored = { ...inst, endpoints: ["serial:/dev/a", "serial:/dev/p", "tcp:gw:4196"] };
    expect(setupFields(stored)).toEqual({ name: "Bank", canKind: "serial", canValue: "/dev/a", pack: "/dev/p" });
    expect(setupEndpoints(stored, "/dev/q")).toEqual(["serial:/dev/q", "tcp:gw:4196"]);
    expect(setupEndpoints(stored, "")).toEqual(["tcp:gw:4196"]);
    // one adapter for both buses: the CAN adapter is listed as the pack port
    expect(setupEndpoints(stored, "/dev/a")).toEqual(["serial:/dev/a", "tcp:gw:4196"]);
    expect(packIsCanAdapter({ ...inst, endpoints: setupEndpoints(stored, "/dev/a") })).toBe(true);
  });
  it("names endpoints like the backend", () => {
    expect(endpointKey({ kind: "tcp", addr: " Gateway.local:4196 ", bus: "can" })).toBe("tcp:gateway.local:4196");
    expect(endpointKey({ kind: "serial", path: "/dev/cu.usbserial-X", bus: "pack" })).toBe("serial:/dev/cu.usbserial-X");
  });
});

describe("backend errors", () => {
  it("turns them into text keys", () => {
    expect(errorKey({ kind: "refused", reason: "release-master-first" })).toEqual({ key: "maint.err.refused.release-master-first" });
    expect(errorKey('{"kind":"no-answer"}')).toEqual({ key: "maint.err.no-answer" });
    expect(errorKey('{"kind":"other-bank","serial":"SN-B"}')).toEqual({ key: "maint.err.other-bank", params: { serial: "SN-B" } });
    expect(errorKey({ kind: "bootloader-pending", pack: 3 })).toEqual({ key: "maint.err.bootloader-pending", params: { pack: 3 } });
    expect(errorKey("some plain text")).toEqual({ key: "maint.err.text", params: { text: "some plain text" } });
    expect(errorKey({ kind: "timeout", marker: true, slaves: 11 })).toEqual({ key: "maint.err.timeout", params: { marker: 1, slaves: 11 } });
  });
  it("offers to take over the serial number only for an other-bank refusal", () => {
    expect(otherBank('{"kind":"other-bank","serial":"SN-B"}')).toBe("SN-B");
    expect(otherBank({ kind: "other-bank", serial: "SN-B" })).toBe("SN-B");
    expect(otherBank('{"kind":"busy"}')).toBeNull();
    expect(otherBank("text")).toBeNull();
  });
  it("a busy port names the running operation", () => {
    expect(portBusy('{"kind":"busy","action":"live"}')).toEqual({ action: "live" });
    expect(portBusy("nope")).toBeNull();
    expect(busyMessage("live", false, tr)).toBe('conn.busyWait {"what":"conn.busyAction.live"}');
    expect(busyMessage("check", true, tr)).toBe('conn.busyGaveUp {"what":"conn.busyAction.other"}');
    const w = { token: 1, current: 1, locked: false, auto: false, firmwareOpen: false, now: 0, until: 10 };
    expect(busyWaitStep(w)).toBe("wait");
    expect(busyWaitStep({ ...w, auto: true, firmwareOpen: true })).toBe("stop");
    expect(busyWaitStep({ ...w, now: 10 })).toBe("give-up");
  });
  it("accepts only the newest list", () => {
    const l = latestOnly();
    const a = l.begin(), b = l.begin();
    expect([l.accepts(a), l.accepts(b)]).toEqual([false, true]);
  });
});

describe("live status", () => {
  it("names a quiet bus, a polling master, other traffic and no connection", () => {
    const live = (bus: "quiet" | "polling" | "traffic" | "no-port" | "error", polled: number[] = []) => ({ bus, polled, bus_error: null, master: null, master_error: null });
    expect(busLine(live("quiet"), false, tr)).toEqual({ tone: "ok", text: "maint.live.quiet" });
    expect(busLine(live("polling", [1, 2, 3]), false, tr)).toEqual({ tone: "warn", text: 'maint.live.polling {"count":3}' });
    expect(busLine(live("traffic"), false, tr).tone).toBe("warn");
    expect(busLine(live("no-port"), false, tr).text).toBe("maint.live.noPort");
    expect(busLine(live("error"), false, tr).text).toBe("maint.live.noPort");
    // a flash runs: the status line gives way to its progress
    expect(busLine(live("polling", [1]), true, tr).text).toBe("maint.live.running");
  });
  it("shows the master's version, marker and slaves; a foreign marker word and an unknown build are alarms", () => {
    expect(masterLine(master({}), tr).tone).toBe("muted");
    expect(masterLine(master({ marker: "foreign", marker_word: "12345678" }), tr).tone).toBe("alarm");
    expect(masterLine(master({ kind: "unknown-build", may_halt: false }), tr).tone).toBe("alarm");
    expect(masterLine(master({ kind: "unknown" }), tr).text).toBe("maint.live.masterUnknown");
  });
});

describe("flash per pack", () => {
  const ok = { image: image("original"), quiet: true, busy: false, running: false, address: 3, master: null, pending: [], port: "/dev/p" };
  it("needs an image and a quiet bus; nothing while a flash or another action runs", () => {
    expect(flashBlocked(ok)).toBeNull();
    expect(flashBlocked({ ...ok, image: null })?.key).toBe("maint.packs.why.image");
    expect(flashBlocked({ ...ok, quiet: false })?.key).toBe("maint.packs.why.quiet");
    expect(flashBlocked({ ...ok, busy: true })?.key).toBe("maint.packs.why.busy");
    expect(flashBlocked({ ...ok, running: true })?.key).toBe("maint.packs.why.running");
  });
  it("a pending pack on the same endpoint blocks every other address there, not itself and not other endpoints", () => {
    expect(pendingBlock([pending(5)], "/dev/p", 3)?.pack).toBe(5);
    expect(pendingBlock([pending(5)], "/dev/p", 5)).toBeNull();
    expect(pendingBlock([pending(5, "serial:/dev/q")], "/dev/p", 3)).toBeNull();
    expect(flashBlocked({ ...ok, pending: [pending(5)] })).toEqual({ key: "maint.err.bootloader-pending", params: { pack: 5 } });
  });
  it("C': a master with its marker on gets a non-maintenance image only after its release", () => {
    const on = master({ marker: "on", marker_word: "00000000", slaves: 0, may_halt: false, may_release: true });
    expect(releaseFirst(on, 0, "original")).toBe(true);
    expect(releaseFirst(on, 0, "custom")).toBe(true);
    expect(releaseFirst(on, 0, "maintenance")).toBe(false);
    expect(releaseFirst(on, 3, "original")).toBe(false);
    expect(releaseFirst(master({}), 0, "original")).toBe(false);
    expect(releaseFirst(null, 0, "original")).toBe(false);
    expect(flashBlocked({ ...ok, address: 0, master: on })?.key).toBe("maint.packs.why.releaseFirst");
    expect(flashBlocked({ ...ok, address: 0, master: on, image: image("maintenance") })).toBeNull();
  });
  it("„Fertig“ after a flash or while the marker is on", () => {
    expect(showDone(false, null)).toBe(false);
    expect(showDone(true, null)).toBe(true);
    expect(showDone(false, master({ marker: "on" }))).toBe(true);
    expect(showDone(false, master({}))).toBe(false);
  });
});
