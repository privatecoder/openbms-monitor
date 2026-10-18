import { describe, expect, it } from "vitest";
import { keyParts, portKey, refusalParams, saveOffer, storedPort, withPort, packIsCanAdapter, busLine, busyMessage, busyWaitStep, defaultName, endpointKey, mergeSuggestion, pickable, preselect, errorKey, flashBlocked, latestOnly, masterLine, nameCheck, otherBank, packPortOf, packVersion, pendingBlock, portBusy, releaseFirst, setupEndpoints, setupFields, showDone } from "./maint";
import type { FirmwareImage, Installation, MasterView, Pending } from "./api";

const inst: Installation = { id: "i", name: "Bank", endpoints: [], control: { kind: "serial", path: "/dev/a" } };
const tr = (key: string, params?: Record<string, unknown>) => (params ? `${key} ${JSON.stringify(params)}` : key);
const master = (patch: Partial<MasterView>): MasterView => ({ kind: "patched", version: "16.06.13 MAINT 1.5", patch: "1.5", base: "16.06.13", build_id: "909a604e60e9225b", listed: "16.06.13 MAINT 1.5", caps: { status_split: true, uart1_params: true, uart1_identity: true, write_gate: true, start_gate: true }, marker: "off", marker_word: "FFFFFFFF", slaves: 3, may_halt: true, may_release: false, ...patch });
const image = (kind: FirmwareImage["kind"]): FirmwareImage => ({ release: "16.06.13", device: "EMU1101", bytes: 1, blocks: 1, sha256: "ab", kind, custom: kind !== "original" });
const pending = (pack: number, endpoint = "serial:/dev/p"): Pending => ({ id: pack, endpoint, pack, release: "16.06.13", since: 0 });

describe("installations: an address book", () => {
  it("requires a name that no other installation has", () => {
    expect(nameCheck("  ", [])).toBe("maint.setup.err.nameRequired");
    expect(nameCheck("bank ", [inst])).toBe("maint.setup.err.nameTaken");
    expect(nameCheck("Bank", [inst], "i")).toBeNull();
    expect(defaultName([{ id: "x", name: "Anlage 2" }], "Anlage")).toBe("Anlage 3");
  });
  it("a gateway preselects its one active installation; several: keep the current one or let the user pick", () => {
    const a = { id: "a" }, b = { id: "b" };
    expect(preselect("b", [a])).toEqual({ do: "use", id: "a" });
    expect(preselect(null, [a])).toEqual({ do: "use", id: "a" });
    expect(preselect("a", [a])).toEqual({ do: "keep" });
    expect(preselect("b", [a, b])).toEqual({ do: "keep" });
    expect(preselect("c", [a, b])).toEqual({ do: "pick" });
    expect(preselect(null, [a, b])).toEqual({ do: "pick" });
    // not linked: the choice stays (also „Nur ansehen“)
    expect(preselect("c", [])).toEqual({ do: "keep" });
    expect(preselect(null, [])).toEqual({ do: "keep" });
  });
  it("the sidebar offers the active installations and an archived one only while it is chosen", () => {
    const list = [inst, { ...inst, id: "old", archived: true }];
    expect(pickable(list, "i").map((i) => i.id)).toEqual(["i"]);
    expect(pickable(list, "old").map((i) => i.id)).toEqual(["i", "old"]);
  });
  it("a moved-in installation is offered for merging only into the one installation that lists its endpoint", () => {
    const legacy: Installation = { id: "l", name: "GW:4196", endpoints: [], control: null, legacy: "GW:4196" };
    const bank: Installation = { id: "b", name: "Bank", endpoints: [], control: { kind: "tcp", addr: "gw:4196" } };
    const lab: Installation = { id: "lab", name: "Lab", endpoints: ["tcp:gw:4196"], control: null };
    expect(mergeSuggestion(legacy, [legacy, bank])?.id).toBe("b");
    expect(mergeSuggestion(legacy, [legacy, bank, lab])).toBeNull();
    expect(mergeSuggestion(legacy, [legacy])).toBeNull();
    expect(mergeSuggestion(bank, [legacy, bank])).toBeNull();
    const usb: Installation = { id: "u", name: "/dev/p", endpoints: [], control: null, legacy: "/dev/p" };
    expect(mergeSuggestion(usb, [usb, { ...lab, endpoints: ["serial:/dev/p"] }])?.id).toBe("lab");
  });
  it("the RS485-1/2 port is the first endpoint in stored order that is not the CAN socket, of any kind", () => {
    expect(packPortOf(null)).toBeNull();
    expect(packPortOf(inst)).toBeNull();
    // a mixed, ordered list: the same pick as the backend's `pack_port`
    expect(packPortOf({ ...inst, endpoints: ["serial:/dev/a", "tcp:192.0.2.7:4196", "serial:/dev/p"] })).toBe("tcp:192.0.2.7:4196");
    expect(packPortOf({ ...inst, endpoints: ["serial:/dev/p", "tcp:192.0.2.7:4196"] })).toBe("serial:/dev/p");
    expect(packPortOf({ ...inst, control: { kind: "tcp", addr: "192.0.2.7:4196" }, endpoints: ["tcp:192.0.2.7:4196", "serial:/dev/p"] })).toBe("serial:/dev/p");
    expect(packPortOf({ ...inst, endpoints: ["serial:/dev/a"] })).toBeNull();
    // the CAN adapter itself is named, not used
    expect(packIsCanAdapter({ ...inst, endpoints: ["serial:/dev/a"] })).toBe(true);
    expect(packIsCanAdapter({ ...inst, endpoints: ["serial:/dev/a", "serial:/dev/p"] })).toBe(false);
    expect(packIsCanAdapter(inst)).toBe(false);
    expect(setupFields({ ...inst, endpoints: ["serial:/dev/a"] }).packValue).toBe("/dev/a");
    // the same rule for the sidebar
    expect(storedPort({ ...inst, endpoints: ["serial:/dev/a", "tcp:192.0.2.7:4196"] }, "pack")).toEqual({ kind: "tcp", target: "192.0.2.7:4196" });
    expect(storedPort(inst, "can")).toEqual({ kind: "serial", target: "/dev/a" });
    expect(storedPort(null, "can")).toBeNull();
  });
  it("forms a port key once and splits it again", () => {
    expect(portKey("tcp", " Gateway.Test:4196 ")).toBe("tcp:gateway.test:4196");
    expect(portKey("serial", "/dev/cu.test")).toBe("serial:/dev/cu.test");
    expect(portKey("tcp", " ")).toBe("");
    expect(keyParts("tcp:192.0.2.7:4196")).toEqual({ kind: "tcp", target: "192.0.2.7:4196" });
    expect(keyParts("/dev/cu.test")).toBeNull();
    expect(keyParts(null)).toBeNull();
  });
  it("the setup saves the pack port first and keeps other endpoints", () => {
    const stored = { ...inst, endpoints: ["serial:/dev/a", "serial:/dev/p", "tcp:gw:4196"] };
    expect(setupFields(stored)).toEqual({ name: "Bank", canKind: "serial", canValue: "/dev/a", packKind: "serial", packValue: "/dev/p" });
    expect(setupEndpoints(stored, "serial:/dev/q")).toEqual(["serial:/dev/q", "tcp:gw:4196"]);
    expect(setupEndpoints(stored, "")).toEqual(["tcp:gw:4196"]);
    // a network gateway as RS485-1/2: first, the old USB port goes
    expect(setupEndpoints(stored, portKey("tcp", "192.0.2.8:4196"))).toEqual(["tcp:192.0.2.8:4196", "tcp:gw:4196"]);
    expect(setupFields({ ...stored, endpoints: ["tcp:192.0.2.8:4196"] })).toMatchObject({ packKind: "tcp", packValue: "192.0.2.8:4196" });
    // one adapter for both buses: the CAN adapter alone, so no other endpoint passes for the pack port
    expect(setupEndpoints(stored, "serial:/dev/a")).toEqual(["serial:/dev/a"]);
    expect(packIsCanAdapter({ ...inst, endpoints: setupEndpoints(stored, "serial:/dev/a") })).toBe(true);
  });
  it("offers to save a connection only over another stored port of that bus", () => {
    const stored = { ...inst, control: { kind: "tcp" as const, addr: "192.0.2.10:4196" }, endpoints: ["serial:/dev/p"] };
    expect(saveOffer(stored, "can", "tcp", "192.0.2.10:4196")).toBe(false);
    expect(saveOffer(stored, "can", "tcp", "192.0.2.11:4196")).toBe(true);
    expect(saveOffer(stored, "pack", "serial", "/dev/p")).toBe(false);
    expect(saveOffer(stored, "pack", "tcp", "192.0.2.12:4196")).toBe(true);
    // the CAN socket's adapter set up as RS485-1/2 counts as stored: another port is offered, not linked
    const oneAdapter = { ...inst, endpoints: ["serial:/dev/a"] };
    expect(storedPort(oneAdapter, "pack")).toEqual({ kind: "serial", target: "/dev/a" });
    expect(saveOffer(oneAdapter, "pack", "serial", "/dev/a")).toBe(false);
    expect(saveOffer(oneAdapter, "pack", "serial", "/dev/p")).toBe(true);
    // nothing stored: the connect fills it (CAN socket) or the decided bank does (RS485-1/2)
    expect(saveOffer({ ...inst, control: null, endpoints: [] }, "can", "tcp", "192.0.2.11:4196")).toBe(false);
    expect(saveOffer({ ...inst, control: null, endpoints: [] }, "pack", "serial", "/dev/p")).toBe(false);
    // „Speichern“: the CAN socket replaced, or the new RS485-1/2 port first
    expect(withPort(stored, "can", "tcp", "192.0.2.11:4196").control).toEqual({ kind: "tcp", addr: "192.0.2.11:4196" });
    expect(withPort(stored, "pack", "tcp", "192.0.2.12:4196").endpoints).toEqual(["tcp:192.0.2.12:4196"]);
    // a refused save names the port of the open bootloader record
    expect(refusalParams({ endpoints: ["tcp:192.0.2.12:4196"] }, [pending(3, "tcp:192.0.2.12:4196")])).toEqual({ port: "192.0.2.12:4196" });
    expect(refusalParams({ endpoints: [] }, [pending(3)])).toEqual({ port: "" });
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
    expect(errorKey({ kind: "bootloader-pending", pack: 3, endpoint: "tcp:192.0.2.20:4196" })).toEqual({ key: "maint.err.bootloader-pending", params: { pack: 3, port: "192.0.2.20:4196" } });
    expect(errorKey("some plain text")).toEqual({ key: "maint.err.text", params: { text: "some plain text" } });
    expect(errorKey({ kind: "timeout", marker: "on", slaves: 11 })).toEqual({ key: "maint.err.timeout", params: { marker: "on", slaves: 11 } });
    expect(errorKey(JSON.stringify({ kind: "start-refused", reason: "start-not-allowed" }))).toEqual({ key: "maint.err.start.start-not-allowed" });
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
    expect(portBusy('{"kind":"refused","reason":"installation-in-use"}')).toEqual({ action: "installation" });
    expect(portBusy('{"kind":"refused","reason":"name-taken"}')).toBeNull();
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
    expect(masterLine(master({ kind: "unknown-patch", caps: null, may_halt: false }), tr).tone).toBe("alarm");
    // not listed, same capabilities: named by its version and base
    expect(masterLine(master({ listed: null, version: "16.07.02 MAINT 1.6", patch: "1.6", base: "16.07.02" }), tr).text).toContain('profile.unlisted {"patch":"1.6","base":"16.07.02"}');
    expect(masterLine(master({ kind: "unknown" }), tr).text).toBe("maint.live.masterUnknown");
  });
});

describe("flash per pack", () => {
  it("shows the master's full maintenance build, else the 0x51 version", () => {
    expect(packVersion(0, "16.06", master({}))).toBe("16.06.13 MAINT 1.5");
    expect(packVersion(1, "16.06", master({}))).toBe("16.06");
    expect(packVersion(0, "16.06", master({ kind: "unknown-patch", caps: null }))).toBe("16.06");
    expect(packVersion(0, "16.06", master({ kind: "no-0x64", version: null }))).toBe("16.06");
    expect(packVersion(0, "16.06", null)).toBe("16.06");
  });

  const ok = { image: image("original"), quiet: true, busy: false, running: false, address: 3, master: null, pending: [], port: "serial:/dev/p" };
  it("needs an image and a quiet bus; nothing while a flash or another action runs", () => {
    expect(flashBlocked(ok)).toBeNull();
    expect(flashBlocked({ ...ok, image: null })?.key).toBe("maint.packs.why.image");
    expect(flashBlocked({ ...ok, quiet: false })?.key).toBe("maint.packs.why.quiet");
    expect(flashBlocked({ ...ok, busy: true })?.key).toBe("maint.packs.why.busy");
    expect(flashBlocked({ ...ok, running: true })?.key).toBe("maint.packs.why.running");
  });
  it("a pending pack blocks every other address and every other endpoint, not itself at its endpoint", () => {
    expect(pendingBlock([pending(5)], "serial:/dev/p", 3)?.pack).toBe(5);
    expect(pendingBlock([pending(5)], "serial:/dev/p", 5)).toBeNull();
    expect(pendingBlock([pending(5, "serial:/dev/q")], "serial:/dev/p", 3)?.pack).toBe(5);
    expect(pendingBlock([pending(5, "serial:/dev/q")], "serial:/dev/p", 5)?.pack).toBe(5);
    expect(pendingBlock([pending(5)], "", 3)).toBeNull();
    // a gateway: the record carries the key formed at the input, compared unchanged
    const gw = portKey("tcp", "192.0.2.20:4196");
    expect(pendingBlock([pending(5, "tcp:192.0.2.20:4196")], gw, 3)?.pack).toBe(5);
    expect(flashBlocked({ ...ok, pending: [pending(5)] })).toEqual({ key: "maint.err.bootloader-pending", params: { pack: 5, port: "/dev/p" } });
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
