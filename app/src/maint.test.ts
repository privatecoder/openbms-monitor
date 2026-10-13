import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { latestOnly, planSectorNote, acceptsProgress, busyMessage, busyWaitStep, onePackBlocked, planItems, portBusy, activityStatus, clause, keepsInverter, latestGuard, attachDecision, canFlash, createFromAttach, defaultName, deleteAllowed, nameCheck, openWorkLines, portLock, sectorNoteKey, endpointKey, errorKey, failingKey, failingText, flowOf, needsReplug, openWork, assignEndpoint, controlFromAttach, packPortOf, parsePacks, sameAdapter, setupCheck, setupEndpoints, setupFields, slavesOf, workUnchanged, type FlowInput } from "./maint";
import type { ControlEndpoint, MaintOverview, MaintSession, OpenWork } from "./api";

const inst = { id: "i", name: "Bank", endpoints: [], control: { kind: "serial" as const, path: "/dev/a" } };
const base: MaintOverview = { installation: inst, state: null, state_at: null, session: null, blocks: [], sector_note: null, end_next: null };
const session: MaintSession = { id: 1, installation: "i", kind: "marker", started: 1_000, expected_slaves: 3, done: [], failing: null };

describe("banner: one line per installation with open work", () => {
  const w = (id: string, patch: Partial<OpenWork>): OpenWork => ({ id, name: id.toUpperCase(), session: null, block: null, state: null, state_at: null, sector_note_at: null, ...patch });
  const ses = (started: number): OpenWork["session"] => ({ kind: "marker", started, expected_slaves: 3, failing: null });
  const blk = (reason: "update-interrupted" | "update-unclear" | "bootloader-unclear" | "master-unclear", since: number) => ({ id: since, pack: 3, reason, expect: null, since });
  it("orders blocks by severity and unexplained markers first, then sessions by start, then markers, then notes", () => {
    const list = [
      w("n", { sector_note_at: 1 }),
      w("m", { state: { state: "patched-on" }, state_at: 5 }),
      w("s2", { session: ses(20), state: { state: "patched-on" } }),
      w("s1", { session: ses(10), state: { state: "patched-on" } }),
      w("mu", { block: blk("master-unclear", 1) }),
      w("x", { state: { state: "marker-unexplained" }, state_at: 2 }),
      w("ui", { block: blk("update-interrupted", 9), session: ses(1) }),
      w("bu", { block: blk("bootloader-unclear", 50) }),
      w("uu", { block: blk("update-unclear", 3) }),
    ];
    expect(openWorkLines(list, null).map((l) => l.id)).toEqual(["bu", "ui", "uu", "mu", "x", "s1", "s2", "m", "n"]);
    expect(openWorkLines(list, null).map((l) => l.kind)).toEqual(["block", "block", "block", "block", "unexplained", "session", "session", "marker", "note"]);
  });
  it("a block line names the block, even with a session open", () => {
    const [l] = openWorkLines([w("a", { block: blk("bootloader-unclear", 7), session: ses(1) })], "a");
    expect(l).toMatchObject({ kind: "block", at: 7, end: false, why: "maint.openWork.why.block" });
  });
  it("offers End only for the shown installation with a session, no block and no unexplained marker", () => {
    const lines = openWorkLines([
      w("a", { session: ses(1), state: { state: "patched-on" } }),
      w("b", { session: ses(2) }),
      w("c", { session: ses(3), state: { state: "marker-unexplained" } }),
      w("d", { state: { state: "patched-on" } }),
      w("e", { sector_note_at: 4 }),
    ], "a");
    const by = Object.fromEntries(lines.map((l) => [l.id, l]));
    expect(by.a).toMatchObject({ end: true, why: null, shown: true });
    expect(by.b).toMatchObject({ end: false, why: "maint.openWork.why.showFirst" });
    expect(by.c).toMatchObject({ kind: "unexplained", end: false, why: "maint.openWork.why.unexplained" });
    expect(by.d).toMatchObject({ kind: "marker", end: false, why: "maint.openWork.why.marker" });
    expect(by.e).toMatchObject({ kind: "note", end: false, why: "maint.openWork.why.note" });
    // shown: a marker without session, a block or an unexplained marker never gets End either
    expect(openWorkLines([w("c", { session: ses(3), state: { state: "marker-unexplained" } })], "c")[0].end).toBe(false);
    expect(openWorkLines([w("d", { state: { state: "patched-on" } })], "d")[0].end).toBe(false);
    expect(openWorkLines([w("e", { sector_note_at: 4 })], "e")[0].end).toBe(false);
    expect(openWorkLines([w("f", { block: blk("update-unclear", 1), session: ses(1) })], "f")[0].end).toBe(false);
  });
  it("names an open session with an unknown state as unconfirmed, and it can still be ended", () => {
    expect(openWorkLines([w("a", { session: ses(1), state: { state: "unknown" } })], "a")[0]).toMatchObject({ unconfirmed: true, end: true });
  });
});

describe("installations: delete, names, connection, foreign port", () => {
  it("deletes only without session, block, marker or sector note", () => {
    expect(deleteAllowed(null)).toBe(false);
    expect(deleteAllowed(base)).toBe(true);
    expect(deleteAllowed({ ...base, state: { state: "no-0x64" } })).toBe(true);
    expect(deleteAllowed({ ...base, session })).toBe(false);
    expect(deleteAllowed({ ...base, blocks: [{ id: 1, pack: 2, reason: "update-unclear", expect: null, since: 1 }] })).toBe(false);
    expect(deleteAllowed({ ...base, state: { state: "patched-on" } })).toBe(false);
    expect(deleteAllowed({ ...base, state: { state: "marker-unexplained" } })).toBe(false);
    expect(deleteAllowed({ ...base, sector_note: "unknown image on the master" })).toBe(false);
  });
  it("requires a name that no other installation has", () => {
    const list = [{ id: "a", name: "Bank" }, { id: "b", name: "Lab" }];
    expect(nameCheck("  ", list)).toBe("maint.setup.err.nameRequired");
    expect(nameCheck(" bank ", list)).toBe("maint.setup.err.nameTaken");
    expect(nameCheck("Bank", list, "a")).toBeNull();
    expect(nameCheck("New", list)).toBeNull();
    expect(defaultName(list, "Anlage")).toBe("Anlage 3");
    expect(defaultName([...list, { id: "c", name: "Anlage 3" }], "Anlage")).toBe("Anlage 4");
  });
  it("a new connection becomes the CAN socket on the CAN socket bus, an endpoint on the pack bus", () => {
    expect(createFromAttach({ kind: "tcp", addr: "GW:4196", bus: "can" })).toEqual({ control: { kind: "tcp", addr: "GW:4196" }, endpoints: [] });
    expect(createFromAttach({ kind: "serial", path: "/dev/p", bus: "pack" })).toEqual({ control: null, endpoints: ["serial:/dev/p"] });
  });
  it("locks a foreign RS485-1/2 port and offers its installation", () => {
    const list = [{ id: "a", name: "Bank" }, { id: "b", name: "Lab" }];
    expect(portLock("a", "a", list)).toEqual({ owner: "this", switchTo: null });
    expect(portLock(null, "a", list)).toEqual({ owner: "none", switchTo: null });
    expect(portLock("b", "a", list)).toEqual({ owner: "other", switchTo: { id: "b", name: "Lab" } });
    expect(portLock("gone", "a", list)).toEqual({ owner: "other", switchTo: null });
    expect(portLock("b", null, list)).toEqual({ owner: "none", switchTo: null });
    // the flow and the flash gate lock it with installation-mismatch
    expect(canFlash({ overview: base, inverter: true, measuredAt: 1, address: 3, imageKind: "original", portOwner: portLock("b", "i", list).owner })).toEqual({ ok: false, reason: "installation-mismatch" });
  });
  it("translates the stored sector notes", () => {
    expect(sectorNoteKey("marker after first flash of a maintenance build")).toBe("maint.note.first-flash");
    expect(sectorNoteKey("unknown image on the master")).toBe("maint.note.unknown-image");
    expect(sectorNoteKey("0x23000 unexplained, recovered to original without stop")).toBe("maint.note.recovered");
    expect(sectorNoteKey("other")).toBeNull();
  });
});

describe("flash unlock", () => {
  const ok = { overview: { ...base, session, state: { state: "patched-on" as const } }, inverter: true, measuredAt: 5_000, address: 3, imageKind: "original" as const, portOwner: "this" as const };
  it("needs the inverter tick and a measurement in a session", () => {
    expect(canFlash(ok).ok).toBe(true);
    expect(canFlash({ ...ok, inverter: false })).toEqual({ ok: false, reason: "inverter" });
    expect(canFlash({ ...ok, measuredAt: null })).toEqual({ ok: false, reason: "measure" });
  });
  it("is blocked by an unclear update and by an unexplained marker", () => {
    expect(canFlash({ ...ok, overview: { ...ok.overview, blocks: [{ id: 1, pack: 2, reason: "update-unclear", expect: null, since: 0 }] } }).reason).toBe("update-unclear");
    expect(canFlash({ ...ok, overview: { ...base, state: { state: "marker-unexplained" } } }).reason).toBe("marker-unexplained");
  });
  it("a measurement older than the last state check does not count", () => {
    expect(canFlash({ ...ok, overview: { ...ok.overview, state_at: 6_000 }, measuredAt: 5_000 }).reason).toBe("measure");
  });
  it("a single pack without maintenance needs neither", () => {
    expect(canFlash({ ...ok, overview: { ...base, state: { state: "patched-off", slaves: 0 } }, inverter: false, measuredAt: null }).ok).toBe(true);
    expect(canFlash({ ...ok, overview: null, inverter: false, measuredAt: null, portOwner: "none" }).ok).toBe(true);
  });
  it("unassigned_port_blocks_maintenance_flash", () => {
    expect(canFlash({ ...ok, portOwner: "none" }).reason).toBe("port-not-assigned");
  });
  it("with an installation in use the port must belong to it, also without maintenance (backend port_gate)", () => {
    const idle = { ...ok, overview: { ...base, state: { state: "patched-off" as const, slaves: 0 } }, inverter: false, measuredAt: null };
    expect(canFlash({ ...idle, portOwner: "none" })).toEqual({ ok: false, reason: "port-not-assigned" });
    expect(canFlash({ ...idle, portOwner: "other" })).toEqual({ ok: false, reason: "installation-mismatch" });
    expect(canFlash({ ...ok, portOwner: "other" }).reason).toBe("installation-mismatch");
  });
  it("an interrupted update blocks like an unclear one", () => {
    expect(canFlash({ ...ok, overview: { ...ok.overview, blocks: [{ id: 2, pack: 4, reason: "update-interrupted", expect: null, since: 0 }] } }).reason).toBe("update-interrupted");
  });
  it("an unexplained marker allows only the recovery: master, original image, DIP session", () => {
    const locked = { ...base, state: { state: "marker-unexplained" as const } };
    const dip: MaintSession = { ...session, kind: "dip" };
    expect(canFlash({ ...ok, overview: locked, address: 0 }).reason).toBe("dip-off");
    expect(canFlash({ ...ok, overview: { ...locked, session: dip }, address: 0 }).ok).toBe(true);
    expect(canFlash({ ...ok, overview: { ...locked, session: dip }, address: 0, imageKind: "maintenance" }).reason).toBe("marker-unexplained");
    expect(canFlash({ ...ok, overview: { ...locked, session: dip }, address: 3 }).reason).toBe("marker-unexplained");
    expect(canFlash({ ...ok, overview: { ...locked, session: dip }, address: 0, inverter: false }).reason).toBe("inverter");
  });
});

describe("helpers", () => {
  it("asks to move the cable when control and pack port are one adapter", () => {
    expect(needsReplug({ kind: "serial", path: "/dev/a" }, "/dev/a")).toBe(true);
    expect(needsReplug({ kind: "serial", path: "/dev/a" }, "/dev/b")).toBe(false);
    expect(needsReplug({ kind: "tcp", addr: "gw:4196" }, "/dev/a")).toBe(false);
    expect(needsReplug(null, "/dev/a")).toBe(false);
  });
  it("a known endpoint of another installation never hides open work", () => {
    const other = { ...inst, id: "j", name: "Other" };
    const open = { ...base, session };
    expect(openWork(open)).toBe(true);
    expect(openWork(base)).toBe(false);
    expect(attachDecision(open, other)).toEqual({ do: "offer-switch", id: "j", name: "Other" });
    expect(attachDecision(base, other)).toEqual({ do: "use", id: "j" });
    expect(attachDecision(open, inst)).toEqual({ do: "use", id: "i" });
    expect(attachDecision(null, other)).toEqual({ do: "use", id: "j" });
    expect(attachDecision(open, null)).toEqual({ do: "offer-assign" });
    expect(attachDecision(null, null)).toEqual({ do: "create" });
  });
  it("names endpoints like the backend", () => {
    expect(endpointKey({ kind: "tcp", addr: " Gateway.local:4196 ", bus: "can" })).toBe("tcp:gateway.local:4196");
    expect(endpointKey({ kind: "serial", path: "/dev/cu.usbserial-X", bus: "pack" })).toBe("serial:/dev/cu.usbserial-X");
  });
  it("turns backend errors into text keys", () => {
    expect(errorKey({ kind: "refused", reason: "inverter" })).toEqual({ key: "maint.err.refused.inverter" });
    expect(errorKey('{"kind":"no-answer"}')).toEqual({ key: "maint.err.no-answer" });
    expect(errorKey("dip-off")).toEqual({ key: "maint.err.refused.dip-off" });
    expect(errorKey("some plain text")).toEqual({ key: "maint.err.text", params: { text: "some plain text" } });
    expect(errorKey({ kind: "timeout", marker: true, slaves: 11 })).toEqual({ key: "maint.err.timeout", params: { marker: 1, slaves: 11 } });
  });
  it("the inverter confirmation is not part of the overview", () => {
    expect(Object.keys(base)).not.toContain("inverter");
  });
});

describe("start-up races", () => {
  const known = { id: "j", name: "Other", endpoints: [], control: null };
  it("an installation in use whose overview is not loaded counts as open work", () => {
    expect(attachDecision(null, known, "i")).toEqual({ do: "offer-switch", id: "j", name: "Other" });
    expect(attachDecision(null, null, "i")).toEqual({ do: "offer-assign" });
    expect(attachDecision(null, { ...known, id: "i" }, "i")).toEqual({ do: "use", id: "i" });
    expect(attachDecision(null, known)).toEqual({ do: "use", id: "j" });
  });
  describe("overview request ordering", () => {
    const ov = (name: string): MaintOverview => ({ ...base, installation: { ...inst, name } });
    const A = ov("check"), B = ov("refresh");
    it("check starts, refresh starts and answers first, check answers: the check is shown", () => {
      const g = latestGuard();
      const c = g.begin("check", "i"), r = g.begin("refresh", "i");
      expect(g.accepts(r, "i", B, null)).toBe(false);
      expect(g.accepts(c, "i", A, null)).toBe(true);
    });
    it("refresh starts, check starts, refresh answers: ignored", () => {
      const g = latestGuard();
      const r = g.begin("refresh", "i"); g.begin("check", "i");
      expect(g.accepts(r, "i", B, null)).toBe(false);
    });
    it("two checks: the newer wins", () => {
      const g = latestGuard();
      const c1 = g.begin("check", "i"), c2 = g.begin("check", "i");
      expect(g.accepts(c1, "i", A, null)).toBe(false);
      expect(g.accepts(c2, "i", A, null)).toBe(true);
    });
    it("a refresh after a finished check is accepted; a newer refresh beats an older one", () => {
      const g = latestGuard();
      const c = g.begin("check", "i"); g.end(c);
      const r1 = g.begin("refresh", "i"), r2 = g.begin("refresh", "i");
      expect(g.accepts(r1, "i", B, null)).toBe(false);
      expect(g.accepts(r2, "i", B, null)).toBe(true);
    });
    it("a refresh begun during a check never wins, even if it answers after the check ended", () => {
      const g = latestGuard();
      const c = g.begin("check", "i"), r = g.begin("refresh", "i");
      expect(g.accepts(c, "i", A, null)).toBe(true);
      g.end(c);
      expect(g.accepts(r, "i", B, A)).toBe(false);
    });
    it("a failed refresh keeps the overview shown for the same id", () => {
      const g = latestGuard();
      const r = g.begin("refresh", "i");
      expect(g.accepts(r, "i", null, A)).toBe(false);
      expect(g.accepts(r, "i", null, null)).toBe(true);
    });
    it("answers for another installation are ignored", () => {
      const g = latestGuard();
      const c = g.begin("check", "i"), r = g.begin("refresh", "i");
      expect(g.accepts(c, "j", A, null)).toBe(false);
      expect(g.accepts(r, "j", B, null)).toBe(false);
    });
  });
});

describe("workUnchanged", () => {
  const start = { installation: "a", session: 3 };
  it("writes only while installation and session are the ones the action began with", () => {
    expect(workUnchanged(start, { installation: "a", session: 3 })).toBe(true);
    expect(workUnchanged(start, { installation: "b", session: 3 })).toBe(false);
    expect(workUnchanged(start, { installation: "a", session: null })).toBe(false);
    expect(workUnchanged(start, { installation: null, session: 3 })).toBe(false);
    expect(workUnchanged({ installation: "a", session: null }, { installation: "a", session: null })).toBe(true);
  });
  it("with a port: also only while the RS485-1/2 port is the one the action began with", () => {
    expect(workUnchanged({ ...start, port: "/dev/b" }, { installation: "a", session: 3, port: "/dev/b" })).toBe(true);
    expect(workUnchanged({ ...start, port: "/dev/b" }, { installation: "a", session: 3, port: "/dev/c" })).toBe(false);
    expect(workUnchanged({ ...start, port: "/dev/b" }, { installation: "a", session: 3, port: "" })).toBe(false);
  });
});

describe("pack count (master included)", () => {
  it("accepts only a whole number from 1 to 16; empty input is no value", () => {
    expect(parsePacks("")).toBeNull();
    expect(parsePacks("0")).toBeNull();
    expect(parsePacks("17")).toBeNull();
    expect(parsePacks("2.5")).toBeNull();
    expect(parsePacks("-3")).toBeNull();
    expect(parsePacks("x")).toBeNull();
    // 1 pack is selectable; the backend accepts it only when the master itself reports 0/0
    expect(parsePacks("1")).toBe(1);
    expect(parsePacks(" 16 ")).toBe(16);
  });
  it("derives the slave count the backend gets: packs minus the master", () => {
    expect(slavesOf(1)).toBe(0);
    expect(slavesOf(2)).toBe(1);
    expect(slavesOf(12)).toBe(11);
    expect(slavesOf(16)).toBe(15);
  });
});

describe("failing End checks", () => {
  it("maps the known keys to texts and leaves unknown values as they are", () => {
    expect(failingKey("pack-communication-incomplete")).toEqual({ key: "maint.failing.pack-communication-incomplete" });
    expect(failingKey("master-not-answering")).toEqual({ key: "maint.failing.master-not-answering" });
    expect(failingKey("system-values")).toEqual({ key: "maint.failing.system-values" });
    expect(failingKey("master-not-zero")).toEqual({ key: "maint.failing.master-not-zero" });
    expect(failingKey("master-unreadable")).toEqual({ key: "maint.failing.master-unreadable" });
    expect(failingKey("slave-present: 7")).toEqual({ key: "maint.failing.slave-present", params: { addr: 7 } });
    expect(failingKey('stop: {"kind":"no-answer"}')).toEqual({ key: "maint.failing.stop", inner: { key: "maint.err.no-answer" } });
    expect(failingKey("something else")).toBeNull();
  });
  it("composes the text, the stop error translated inside", () => {
    const t = (key: string, params?: Record<string, unknown>) => params ? `${key}(${JSON.stringify(params)})` : key;
    expect(failingText("system-values", t)).toBe("maint.failing.system-values");
    expect(failingText("slave-present: 7", t)).toBe('maint.failing.slave-present({"addr":7})');
    expect(failingText('stop: {"kind":"refused","reason":"no-session"}', t)).toBe('maint.failing.stop({"error":"maint.err.refused.no-session"})');
    expect(failingText("something else", t)).toBe("something else");
  });
  it("names a slave count mismatch with both numbers", () => {
    expect(failingKey("slave-count-mismatch: 0 1")).toEqual({ key: "maint.failing.slave-count-mismatch", params: { actual: 0, expected: 1 } });
  });
  it("a clause drops the sentence's own period, nothing else", () => {
    expect(clause("Der Master antwortet nicht.")).toBe("Der Master antwortet nicht");
    expect(clause("(zuletzt 0 Slaves).")).toBe("(zuletzt 0 Slaves)");
    expect(clause("Prüfe …")).toBe("Prüfe …");
    expect(clause("ohne Punkt")).toBe("ohne Punkt");
  });
});

describe("panel action progress", () => {
  const t = (key: string, params?: Record<string, unknown>) => params ? `${key}(${JSON.stringify(params)})` : key;
  const running = { installation: "a", key: "scan", action: "scan", running: true };
  it("takes only events of the running action of its installation", () => {
    expect(acceptsProgress(running, { installation: "a", action: "scan", phase: "scan", detail: null })).toBe(true);
    expect(acceptsProgress(running, { installation: "b", action: "scan", phase: "scan", detail: null })).toBe(false);
    expect(acceptsProgress(running, { installation: "a", action: "end", phase: "bus", detail: null })).toBe(false);
    expect(acceptsProgress({ ...running, running: false }, { installation: "a", action: "scan", phase: "scan", detail: null })).toBe(false);
    expect(acceptsProgress(null, { installation: "a", action: "scan", phase: "scan", detail: null })).toBe(false);
  });
  it("names the latest phase, else the action's generic text, then the result", () => {
    expect(activityStatus(running, t)).toEqual({ kind: "running", text: "maint.progress.action.scan" });
    const scan = { ...running, phase: { installation: "a", action: "scan", phase: "scan", detail: { address: 7, of: 15, pass: 1, passes: 2 } } };
    expect(activityStatus(scan, t)).toEqual({ kind: "running", text: 'maint.progress.phase.scan({"address":7,"of":15,"pass":1,"passes":2})' });
    expect(activityStatus({ ...running, running: false, result: { ok: true, text: "fertig" } }, t)).toEqual({ kind: "ok", text: "fertig" });
    expect(activityStatus({ ...running, running: false, result: { ok: false, text: "kaputt" } }, t)).toEqual({ kind: "failed", text: "kaputt" });
    expect(activityStatus({ ...running, running: false }, t)).toBeNull();
  });
});

describe("inverter statement per work phase", () => {
  it("survives the session its activation opened, nothing else", () => {
    expect(keepsInverter({ installation: "a", session: null }, { installation: "a", session: 3 })).toBe(true);
    expect(keepsInverter({ installation: "a", session: 3 }, { installation: "a", session: 3 })).toBe(true);
    expect(keepsInverter({ installation: "a", session: 3 }, { installation: "a", session: null })).toBe(false);
    expect(keepsInverter({ installation: "a", session: 3 }, { installation: "a", session: 4 })).toBe(false);
    expect(keepsInverter({ installation: "a", session: null }, { installation: "b", session: 3 })).toBe(false);
  });
});

describe("guided flow (flowOf)", () => {
  const dip: MaintSession = { ...session, kind: "dip" };
  const ov = (o: Partial<MaintOverview>): MaintOverview => ({ ...base, state_at: 1_000, ...o });
  // the pack port /dev/b is chosen and assigned, the control channel is /dev/a; nothing checked yet
  const input = (o: Partial<FlowInput> = {}): FlowInput => ({
    overview: ov({ state: { state: "patched-off", slaves: 7 } }), fresh: null, noControl: false, singlePack: false, inverter: false,
    measuredAt: null, port: "/dev/b", portAvailable: true, portOwner: "this", address: 3, imageKind: "original", scan: undefined, busy: false, running: false, ...o,
  });
  const states = (f: ReturnType<typeof flowOf>) => [f.steps.connections, f.steps.check, f.steps.inverter, f.steps.start, f.steps.measure, f.steps.flash].map((s) => s.state);
  const first = (n: number) => ({ key: "maint.flow.reason.first", params: { n } });

  it("without an installation only the firmware steps count", () => {
    const f = flowOf(input({ overview: null }));
    expect(f.steps.flash.state).toBe("current");
    expect(f.steps.connections.state).toBe("locked");
    expect(f.end.shown).toBe(false);
  });

  it("step 1: RS485-1/2 port set up and plugged in, control channel set or explicitly none", () => {
    expect(flowOf(input({ port: "" })).steps.connections).toEqual({ state: "current", reason: { key: "maint.flow.reason.setup-pack" } });
    expect(flowOf(input({ portAvailable: false })).steps.connections).toEqual({ state: "current", reason: { key: "maint.flow.reason.pack-unplugged", params: { port: "/dev/b" } } });
    expect(flowOf(input({ portAvailable: false })).steps.check).toEqual({ state: "locked", reason: first(1) });
    expect(flowOf(input({ portOwner: "none" })).steps.connections.reason?.key).toBe("maint.err.refused.port-not-assigned");
    expect(flowOf(input({ portOwner: "other" })).steps.connections.reason?.key).toBe("maint.err.refused.installation-mismatch");
    const noCtl = { ...inst, control: null };
    expect(flowOf(input({ overview: ov({ installation: noCtl }) })).steps.connections.reason?.key).toBe("maint.flow.reason.control-or-none");
    expect(flowOf(input({ overview: ov({ installation: noCtl }), noControl: true })).steps.connections.state).toBe("done");
    // everything after the current step is locked with "first step 1"
    expect(flowOf(input({ port: "" })).steps.check).toEqual({ state: "locked", reason: first(1) });
  });

  it("after an app restart or a lost connection the stored state is only the last confirmed one: check first", () => {
    const f = flowOf(input({ overview: ov({ session, state: { state: "patched-on" } }), inverter: true, measuredAt: 5_000 }));
    expect(f.state).toBeNull();
    expect(states(f)).toEqual(["done", "current", "locked", "locked", "locked", "locked"]);
    expect(f.steps.check.reason?.key).toBe("maint.flow.reason.check-needed");
    expect(f.steps.flash.reason).toEqual(first(2));
    // the open session can still be ended (End checks itself)
    expect(f.end).toMatchObject({ shown: true, state: "current" });
    // End needs the RS485-1/2 port plugged in
    const g = flowOf(input({ overview: ov({ session, state: { state: "patched-on" } }), portAvailable: false }));
    expect(g.end).toMatchObject({ shown: true, state: "locked", reason: { key: "maint.flow.reason.pack-unplugged", params: { port: "/dev/b" } } });
    expect(flowOf(input({ overview: ov({ session, state: { state: "patched-on" } }), port: "" })).end.reason?.key).toBe("maint.flow.reason.setup-pack");
  });

  it("patched-off with N > 0: activate (recommended) or the DIP way, after the inverter statement", () => {
    const f = flowOf(input({ fresh: "ok" }));
    expect(f.branch).toBe("activate");
    expect(states(f)).toEqual(["done", "done", "current", "locked", "locked", "locked"]);
    expect(f.steps.start.reason).toEqual(first(3));
    const g = flowOf(input({ fresh: "ok", inverter: true }));
    expect(states(g)).toEqual(["done", "done", "done", "current", "locked", "locked"]);
    expect(g.steps.measure.reason).toEqual(first(4));
  });

  it("no-0x64 and unknown take the manual DIP way", () => {
    expect(flowOf(input({ fresh: "ok", overview: ov({ state: { state: "no-0x64" } }) })).branch).toBe("dip");
    expect(flowOf(input({ fresh: "ok", overview: ov({ state: { state: "unknown" } }) })).branch).toBe("dip");
    // a failed check (e.g. no control channel answer) ends as unknown, whatever was stored before
    const f = flowOf(input({ fresh: "failed" }));
    expect(f.state).toBe("unknown");
    expect(f.branch).toBe("dip");
    expect(f.steps.check.state).toBe("done");
  });

  it("patched-off 0/0 without session: first the slave check, the inverter waits for its result", () => {
    const zero = ov({ state: { state: "patched-off", slaves: 0 } });
    const f = flowOf(input({ fresh: "ok", overview: zero, address: 0 }));
    expect(f.branch).toBe("single-pack");
    expect(f.scanNeeded).toBe(true);
    expect(f.steps.inverter).toEqual({ state: "locked", reason: { key: "maint.flow.reason.after-scan" } });
    expect(f.steps.start.state).toBe("current");
    expect(f.steps.flash.state).toBe("locked");
    // a cleared scan result (new check, lost or closed connection) asks again
    expect(flowOf(input({ fresh: "ok", overview: zero, address: 0, scan: undefined, inverter: true, measuredAt: 5_000 })).scanNeeded).toBe(true);
  });

  it("0/0 and no slave answers: a single pack, nothing to prepare for the original or maintenance image", () => {
    const zero = ov({ state: { state: "patched-off", slaves: 0 } });
    const f = flowOf(input({ fresh: "ok", overview: zero, address: 0, scan: null }));
    expect(f.branch).toBe("single-pack");
    expect(f.scanNeeded).toBe(false);
    // only step 4 is not needed: a master flash cuts CAN to the inverter, and the bus is measured anyway
    expect(states(f)).toEqual(["done", "done", "current", "locked", "locked", "locked"]);
    const g = flowOf(input({ fresh: "ok", overview: zero, address: 0, scan: null, inverter: true }));
    expect(states(g)).toEqual(["done", "done", "done", "done", "current", "locked"]);
    expect([g.steps.inverter.notNeeded, g.steps.start.notNeeded, g.steps.measure.notNeeded]).toEqual([undefined, true, undefined]);
    expect(flowOf(input({ fresh: "ok", overview: zero, address: 0, scan: null, inverter: true, measuredAt: 5_000, imageKind: "maintenance" })).steps.flash.state).toBe("current");
    // a custom image on a single pack still needs the DIP session (with the single-pack proof)
    const c = flowOf(input({ fresh: "ok", overview: zero, address: 0, scan: null, imageKind: "custom" }));
    expect(c.branch).toBe("dip");
    expect(c.steps.inverter.state).toBe("current");
  });

  it("0/0 and a slave answers: pack count unclear, only the DIP session", () => {
    const f = flowOf(input({ fresh: "ok", overview: ov({ state: { state: "patched-off", slaves: 0 } }), scan: 5 }));
    expect(f.branch).toBe("dip");
    expect(f.packsUnclear).toBe(true);
    expect(f.steps.inverter.state).toBe("current");
  });

  it("patched-on with an open marker session: step 4 done, then measure, then flash", () => {
    const on = ov({ session, state: { state: "patched-on" } });
    const f = flowOf(input({ fresh: "ok", overview: on, inverter: true }));
    expect(f.branch).toBe("session");
    expect(states(f)).toEqual(["done", "done", "done", "done", "current", "locked"]);
    expect(f.steps.flash.reason).toEqual(first(5));
    expect(states(flowOf(input({ fresh: "ok", overview: on, inverter: true, measuredAt: 5_000 })))).toEqual(["done", "done", "done", "done", "done", "current"]);
    // a measurement older than the last state check does not count
    expect(flowOf(input({ fresh: "ok", overview: on, inverter: true, measuredAt: 500 })).steps.measure.state).toBe("current");
    expect(flowOf(input({ fresh: "ok", overview: on, inverter: false, measuredAt: 5_000 })).steps.inverter.state).toBe("current");
  });

  it("patched-on without a session is not accounted for: step 4 stays locked", () => {
    const f = flowOf(input({ fresh: "ok", overview: ov({ state: { state: "patched-on" } }), inverter: true }));
    expect(f.branch).toBeNull();
    expect(f.steps.start).toEqual({ state: "current", reason: { key: "maint.flow.reason.patched-on-no-session" } });
    expect(f.steps.flash.state).toBe("locked");
    expect(f.end.shown).toBe(false);
  });

  it("a DIP session continues with the inverter and the measurement", () => {
    const f = flowOf(input({ fresh: "failed", overview: ov({ session: dip, state: { state: "unknown" } }), inverter: true, measuredAt: 5_000 }));
    expect(f.branch).toBe("session");
    expect(f.steps.flash.state).toBe("current");
  });

  it("a marker session with unknown state locks steps 3 to 6 until a check shows the marker", () => {
    const lost = ov({ session, state: { state: "unknown" } });
    for (const fresh of ["ok", "failed"] as const) {
      const f = flowOf(input({ fresh, overview: lost, inverter: true, measuredAt: 5_000 }));
      expect(f.steps.check).toEqual({ state: "current", reason: { key: "maint.flow.reason.marker-unknown" } });
      expect(states(f).slice(2)).toEqual(["locked", "locked", "locked", "locked"]);
    }
    expect(flowOf(input({ fresh: "ok", overview: ov({ session, state: { state: "patched-on" } }), inverter: true, measuredAt: 5_000 })).steps.flash.state).toBe("current");
  });

  it("a marker session whose master no longer reports the marker (e.g. after End's stop) locks measure and flash", () => {
    const stopped = ov({ session, state: { state: "patched-off", slaves: 7 }, end_next: "bus" });
    const f = flowOf(input({ fresh: "ok", overview: stopped, inverter: true, measuredAt: 5_000 }));
    const r = { key: "maint.flow.reason.marker-not-on" };
    expect([f.steps.measure, f.steps.flash]).toEqual([{ state: "locked", reason: r }, { state: "locked", reason: r }]);
    // End continues
    expect(f.end).toMatchObject({ shown: true, state: "current", next: "bus" });
  });

  it("marker-unexplained without session: explain or start the recovery; End not offered", () => {
    const f = flowOf(input({ fresh: "ok", overview: ov({ state: { state: "marker-unexplained" } }) }));
    expect(f.branch).toBe("unexplained");
    expect(f.steps.inverter).toEqual({ state: "locked", reason: { key: "maint.flow.reason.after-recovery" } });
    expect(f.steps.start.state).toBe("current");
    expect(f.end.shown).toBe(false);
    // a failed read keeps the state
    expect(flowOf(input({ fresh: "failed", overview: ov({ state: { state: "marker-unexplained" } }) })).state).toBe("marker-unexplained");
    // in a marker session: only the statement, End locked
    const m = flowOf(input({ fresh: "ok", overview: ov({ session, state: { state: "marker-unexplained" } }) }));
    expect(m.branch).toBe("unexplained");
    expect(m.end).toMatchObject({ shown: true, state: "locked", reason: { key: "maint.err.refused.marker-unexplained" } });
  });

  it("recovery: the DIP session unlocks steps 3, 5 and the flash only for address 0 with the original image", () => {
    const rec = ov({ session: dip, state: { state: "marker-unexplained" } });
    const ok = flowOf(input({ fresh: "ok", overview: rec, address: 0, imageKind: "original" }));
    expect(ok.branch).toBe("recovery");
    expect(states(ok)).toEqual(["done", "done", "current", "done", "locked", "locked"]);
    expect(flowOf(input({ fresh: "ok", overview: rec, address: 0, inverter: true, measuredAt: 5_000 })).steps.flash.state).toBe("current");
    const only = { key: "maint.flow.reason.recovery-only-original" };
    for (const other of [{ address: 3 }, { address: 0, imageKind: "maintenance" as const }, { address: 0, imageKind: null }]) {
      const f = flowOf(input({ fresh: "ok", overview: rec, inverter: true, measuredAt: 5_000, ...other }));
      expect([f.steps.inverter, f.steps.measure, f.steps.flash]).toEqual([{ state: "locked", reason: only }, { state: "locked", reason: only }, { state: "locked", reason: only }]);
    }
    // End stays locked until the recovery flash produced no-0x64
    expect(ok.end).toMatchObject({ shown: true, state: "locked" });
    expect(flowOf(input({ fresh: "ok", overview: ov({ session: dip, state: { state: "no-0x64" } }) })).end.state).toBe("current");
  });

  it("an open block locks measure, flash and End with the block as the reason", () => {
    const blocked = ov({ session, state: { state: "patched-on" }, blocks: [{ id: 1, pack: 4, reason: "update-unclear", expect: null, since: 0 }] });
    const f = flowOf(input({ fresh: "ok", overview: blocked, inverter: true, measuredAt: 5_000 }));
    const r = { key: "maint.err.refused.update-unclear" };
    expect([f.steps.measure, f.steps.flash]).toEqual([{ state: "locked", reason: r }, { state: "locked", reason: r }]);
    expect(f.end).toMatchObject({ state: "locked", reason: r });
  });

  it("one adapter for both buses: End follows the backend's next step", () => {
    const one = ov({ installation: { ...inst, control: { kind: "serial", path: "/dev/b" } }, session, state: { state: "patched-on" }, end_next: "bus" });
    const f = flowOf(input({ fresh: "ok", overview: one }));
    expect(f.replug).toBe(true);
    expect(f.end.next).toBe("bus");
    expect(flowOf(input({ fresh: "ok", overview: ov({ session, state: { state: "patched-on" }, end_next: "stop" }) })).replug).toBe(false);
  });

  it("without a control channel: the DIP way, or a single pack stated by the user", () => {
    const noCtl = ov({ installation: { ...inst, control: null }, state: null });
    const f = flowOf(input({ overview: noCtl, noControl: true }));
    expect(f.steps.check.state).toBe("done");
    expect(f.branch).toBe("dip");
    expect(f.singlePackOffered).toBe(true);
    const s = flowOf(input({ overview: noCtl, noControl: true, singlePack: true }));
    expect(s.branch).toBe("single-pack");
    // steps 3 and 5 stay required
    expect(states(s)).toEqual(["done", "done", "current", "locked", "locked", "locked"]);
    expect(states(flowOf(input({ overview: noCtl, noControl: true, singlePack: true, inverter: true, measuredAt: 5_000 })))).toEqual(["done", "done", "done", "done", "done", "current"]);
    // stored unknown: still offered
    expect(flowOf(input({ overview: ov({ installation: { ...inst, control: null }, state: { state: "unknown" } }), noControl: true, singlePack: true })).branch).toBe("single-pack");
    // only without a session and only with no stored state or unknown
    for (const o of [ov({ installation: { ...inst, control: null }, state: { state: "no-0x64" } }), ov({ installation: { ...inst, control: null }, state: null, session: dip })]) {
      const x = flowOf(input({ overview: o, noControl: true, singlePack: true }));
      expect(x.singlePackOffered).toBe(false);
      expect(x.branch).not.toBe("single-pack");
    }
  });

  it("while something runs every step is locked; a busy panel only disables the actions", () => {
    const f = flowOf(input({ fresh: "ok", running: true }));
    expect(states(f).every((s) => s === "locked")).toBe(true);
    expect(f.steps.check.reason?.key).toBe("maint.flow.reason.running");
    expect(flowOf(input({ fresh: "ok", busy: true })).idle).toBe(false);
    expect(flowOf(input({ fresh: "ok" })).idle).toBe(true);
  });
});

describe("installation setup: CAN socket and RS485-1/2", () => {
  const can: ControlEndpoint = { kind: "serial", path: "/dev/cu.usbserial-A" };
  const withEps = (endpoints: string[], control: ControlEndpoint | null) => ({ ...inst, endpoints, control });
  // the bench case: the control adapter was registered by the sidebar connection, the pack port assigned later
  const bench = withEps(["serial:/dev/cu.usbserial-A", "serial:/dev/cu.usbmodem-B"], can);

  it("the RS485-1/2 port is the first stored serial endpoint that is not the control adapter", () => {
    expect(packPortOf(bench)).toBe("/dev/cu.usbmodem-B");
    expect(packPortOf(withEps(["tcp:gw:4196", "serial:/dev/b", "serial:/dev/c"], can))).toBe("/dev/b");
    expect(packPortOf(withEps(["serial:/dev/b"], { kind: "tcp", addr: "gw:4196" }))).toBe("/dev/b");
    expect(packPortOf(withEps(["tcp:gw:4196"], null))).toBeNull();
    expect(packPortOf(null)).toBeNull();
  });
  it("the control adapter counts as RS485-1/2 only with the one-adapter confirmation", () => {
    const one = withEps(["serial:/dev/cu.usbserial-A"], can);
    expect(packPortOf(one)).toBeNull();
    expect(packPortOf(one, true)).toBe("/dev/cu.usbserial-A");
    // a second adapter wins over the confirmation; a TCP control channel is no adapter
    expect(packPortOf(bench, true)).toBe("/dev/cu.usbmodem-B");
    expect(packPortOf(withEps([], { kind: "tcp", addr: "gw:4196" }), true)).toBeNull();
  });
  it("saving keeps both stored ports of the bench case (the form shows both, nothing is dropped)", () => {
    expect(packPortOf(bench)).toBe("/dev/cu.usbmodem-B");
    expect(setupEndpoints(bench, can, "/dev/cu.usbmodem-B").sort()).toEqual(["serial:/dev/cu.usbmodem-B", "serial:/dev/cu.usbserial-A"]);
    // and the saved list reads back as the same RS485-1/2 port
    expect(packPortOf({ ...bench, endpoints: setupEndpoints(bench, can, "/dev/cu.usbmodem-B") })).toBe("/dev/cu.usbmodem-B");
  });
  it("a new RS485-1/2 port replaces the old one; other endpoints of the installation stay", () => {
    const i = withEps(["tcp:gw:4196", "serial:/dev/cu.usbserial-A", "serial:/dev/b"], can);
    const eps = setupEndpoints(i, can, "/dev/c");
    expect(eps).toContain("tcp:gw:4196");
    expect(eps).toContain("serial:/dev/cu.usbserial-A");
    expect(eps).toContain("serial:/dev/c");
    expect(eps).not.toContain("serial:/dev/b");
    expect(packPortOf({ ...i, endpoints: eps })).toBe("/dev/c");
  });
  it("a new CAN socket replaces the old control key; a network CAN socket is listed by its address", () => {
    const eps = setupEndpoints(bench, { kind: "tcp", addr: "GW:4196" }, "/dev/cu.usbmodem-B");
    expect(eps).toEqual(["tcp:gw:4196", "serial:/dev/cu.usbmodem-B"]);
    expect(setupEndpoints(withEps([], null), null, "/dev/b")).toEqual(["serial:/dev/b"]);
  });
  it("the RS485-1/2 port is listed before any other serial endpoint, so it reads back as the pack port", () => {
    // an extra serial endpoint (e.g. a sidebar connection) stays, after the pack port
    const i = withEps(["serial:/dev/x", "serial:/dev/b"], { kind: "tcp", addr: "gw:4196" });
    expect(packPortOf(i)).toBe("/dev/x");
    const eps = setupEndpoints(i, { kind: "tcp", addr: "gw:4196" }, "/dev/b");
    expect(packPortOf({ ...i, endpoints: eps })).toBe("/dev/b");
  });
  it("one adapter for both buses: one key, refused without the explicit confirmation", () => {
    expect(sameAdapter(can, "/dev/cu.usbserial-A")).toBe(true);
    expect(sameAdapter({ kind: "tcp", addr: "gw:4196" }, "/dev/cu.usbserial-A")).toBe(false);
    expect(sameAdapter(can, "")).toBe(false);
    const stored = withEps(["serial:/dev/cu.usbserial-A"], can);
    expect(setupEndpoints(stored, can, "/dev/cu.usbserial-A", true)).toEqual(["serial:/dev/cu.usbserial-A"]);
    expect(setupCheck({ can, pack: "/dev/cu.usbserial-A", oneAdapter: false, storedPack: null })).toBe("maint.setup.err.sameAdapter");
    expect(setupCheck({ can, pack: "/dev/cu.usbserial-A", oneAdapter: true, storedPack: null })).toBeNull();
    expect(setupCheck({ can, pack: "/dev/cu.usbmodem-B", oneAdapter: false, storedPack: null })).toBeNull();
  });
  it("a pack port stored meanwhile (the form opened without one) is reported as such: the form reloads", () => {
    expect(setupCheck({ can, pack: "", oneAdapter: false, storedPack: "/dev/cu.usbmodem-B", loadedPack: "" })).toBe("maint.setup.err.packSetMeanwhile");
    expect(setupCheck({ can, pack: "", oneAdapter: false, storedPack: "/dev/cu.usbmodem-B", loadedPack: "/dev/cu.usbmodem-B" })).toBe("maint.setup.err.packRequired");
  });
  it("first run via the sidebar on the CAN socket: the adapter becomes the CAN socket, never RS485-1/2", () => {
    const created = withEps(["serial:/dev/cu.usbserial-A"], null);
    const saved = controlFromAttach(created, { kind: "serial", path: "/dev/cu.usbserial-A", bus: "can" });
    expect(saved).toEqual({ ...created, control: can, endpoints: [] });
    expect(packPortOf(saved!)).toBeNull();
    // step 1 shows "CAN socket: A / RS485-1/2: not set up"; the form pre-fills CAN = A and RS485-1/2 empty
    expect(setupFields(saved!, false)).toEqual({ name: "Bank", canKind: "serial", canValue: "/dev/cu.usbserial-A", pack: "", oneAdapter: false });
    const f = flowOf({ overview: { ...base, installation: saved! }, fresh: null, noControl: false, singlePack: false, inverter: false, measuredAt: null,
      port: packPortOf(saved!) ?? "", portAvailable: false, portOwner: "none", address: 0, imageKind: null, scan: undefined, busy: false, running: false });
    expect(f.steps.connections.reason?.key).toBe("maint.flow.reason.setup-pack");
    // a gateway on the CAN socket too
    expect(controlFromAttach(withEps(["tcp:gw:4196"], null), { kind: "tcp", addr: "GW:4196", bus: "can" })).toEqual({ ...inst, endpoints: [], control: { kind: "tcp", addr: "GW:4196" } });
    // the pack bus, or an installation with a control channel already: nothing to change
    expect(controlFromAttach(withEps(["serial:/dev/b"], null), { kind: "serial", path: "/dev/b", bus: "pack" })).toBeNull();
    expect(controlFromAttach(bench, { kind: "serial", path: "/dev/cu.usbserial-A", bus: "can" })).toBeNull();
  });
  it("banner assign: a CAN endpoint becomes the CAN socket, only a serial pack-bus endpoint can become RS485-1/2", () => {
    const empty = withEps([], null);
    const a = assignEndpoint(empty, { kind: "serial", path: "/dev/cu.usbserial-A", bus: "can" });
    expect(a).toEqual({ ...empty, control: can, endpoints: [] });
    expect(packPortOf(a)).toBeNull();
    // the CAN socket already set to it: kept, never listed as an endpoint
    expect(assignEndpoint(a, { kind: "serial", path: "/dev/cu.usbserial-A", bus: "can" })).toEqual(a);
    // a CAN endpoint on another adapter replaces the CAN socket, it does not become RS485-1/2
    const b = assignEndpoint(withEps(["serial:/dev/cu.usbmodem-B"], can), { kind: "serial", path: "/dev/c", bus: "can" });
    expect(b.control).toEqual({ kind: "serial", path: "/dev/c" });
    expect(packPortOf(b)).toBe("/dev/cu.usbmodem-B");
    // pack bus: added as an endpoint
    const p = assignEndpoint(a, { kind: "serial", path: "/dev/cu.usbmodem-B", bus: "pack" });
    expect(p.endpoints).toEqual(["serial:/dev/cu.usbmodem-B"]);
    expect(packPortOf(p)).toBe("/dev/cu.usbmodem-B");
  });
  it("one adapter without the tick: step 1 asks to confirm it in the setup", () => {
    const one = withEps(["serial:/dev/cu.usbserial-A"], can);
    const f = flowOf({ overview: { ...base, installation: one }, fresh: null, noControl: false, singlePack: false, inverter: false, measuredAt: null,
      port: packPortOf(one) ?? "", portAvailable: false, portOwner: "none", address: 0, imageKind: null, scan: undefined, busy: false, running: false });
    expect(f.steps.connections.reason?.key).toBe("maint.flow.reason.confirm-one-adapter");
  });
  it("a save never drops a stored RS485-1/2 port by leaving it empty", () => {
    expect(setupCheck({ can, pack: "", oneAdapter: false, storedPack: "/dev/cu.usbmodem-B" })).toBe("maint.setup.err.packRequired");
    // nothing stored yet: the CAN socket alone may be saved, and so may no port at all
    expect(setupCheck({ can, pack: "", oneAdapter: false, storedPack: null })).toBeNull();
    expect(setupCheck({ can: null, pack: "", oneAdapter: false, storedPack: null })).toBeNull();
    // and the endpoint list keeps the stored pack port when no new one is given
    expect(setupEndpoints(bench, can, "")).toContain("serial:/dev/cu.usbmodem-B");
  });
});

describe("busy port on connect", () => {
  it("reads the backend's busy refusal with the running action", () => {
    expect(portBusy('{"kind":"busy","action":"check"}')).toEqual({ action: "check" });
    expect(portBusy('{"kind":"busy","action":null}')).toEqual({ action: null });
    expect(portBusy({ kind: "busy" })).toEqual({ action: null });
    expect(portBusy("connection refused")).toBeNull();
    expect(portBusy('{"kind":"refused","reason":"no-port"}')).toBeNull();
  });
  it("names the action while waiting and when giving up, in German and English", () => {
    const tr = (lng: string) => (key: string, params?: Record<string, unknown>) => i18n.t(key, { ...params, lng });
    expect(busyMessage("check", false, tr("de"))).toBe("Port belegt: Master wird gerade geprüft. Die Verbindung wird danach aufgebaut.");
    expect(busyMessage("check", false, tr("en"))).toBe("Port busy: the master is being checked. The connection follows once it is done.");
    expect(busyMessage(null, true, tr("de"))).toMatch(/^Port belegt: .*30 s/);
    for (const lng of ["de", "en"]) {
      for (const a of ["check", "activate", "start-dip", "scan", "account-marker", "set-slaves", "measure", "end", "end-step", "clear-block", "flash", "unknown-action", null]) {
        for (const gaveUp of [false, true]) expect(busyMessage(a, gaveUp, tr(lng)), `${lng} ${a}`).not.toMatch(/conn\.|undefined/);
      }
    }
  });
});

describe("pack count editor with the marker on", () => {
  it("1 pack cannot be saved while the master reports the marker", () => {
    expect(onePackBlocked(1, "patched-on")).toBe(true);
    expect(onePackBlocked(1, "marker-unexplained")).toBe(true);
    expect(onePackBlocked(2, "patched-on")).toBe(false);
    expect(onePackBlocked(null, "patched-on")).toBe(false);
    for (const s of ["patched-off", "no-0x64", "unknown", undefined] as const) expect(onePackBlocked(1, s)).toBe(false);
  });
  it("says why in German and English", () => {
    expect(i18n.t("maint.panel.onePackMarker", { lng: "de" })).toBe("1 Pack lässt sich erst eintragen, wenn der Marker gelöscht ist. „Beenden“ löscht ihn zuerst und meldet dann, dass die Packzahl nicht passt; danach hier 1 eintragen und erneut beenden.");
    expect(i18n.exists("maint.panel.onePackMarker", { lng: "en" })).toBe(true);
    for (const lng of ["de", "en"]) expect(i18n.exists("maint.panel.cancel", { lng })).toBe(true);
  });
});

describe("waiting for a busy port", () => {
  const w = { token: 3, current: 3, locked: false, auto: false, firmwareOpen: false, now: 1000, until: 31000 };
  it("keeps waiting while nothing changed and the time is not up", () => {
    expect(busyWaitStep(w)).toBe("wait");
    // the firmware view stops only the automatic reconnect, not a connect the user asked for
    expect(busyWaitStep({ ...w, firmwareOpen: true })).toBe("wait");
  });
  it("stops on cancel, disconnect, new settings or a user action (token changed), and while a flash locks the sidebar", () => {
    expect(busyWaitStep({ ...w, current: 4 })).toBe("stop");
    expect(busyWaitStep({ ...w, locked: true })).toBe("stop");
    expect(busyWaitStep({ ...w, auto: true, firmwareOpen: true })).toBe("stop");
  });
  it("gives up after the wait; a stop wins over giving up", () => {
    expect(busyWaitStep({ ...w, now: 31000 })).toBe("give-up");
    expect(busyWaitStep({ ...w, now: 31000, current: 4 })).toBe("stop");
  });
  it("has the cancel texts", () => {
    for (const lng of ["de", "en"]) for (const k of ["conn.cancel", "conn.busyCancelled"]) expect(i18n.exists(k, { lng }), `${lng} ${k}`).toBe(true);
    expect(i18n.t("conn.cancel", { lng: "de" })).toBe("Abbrechen");
  });
});

describe("master-plan checklist", () => {
  it("shows a confirmed step once, as its checkbox, in its step position", () => {
    const before = ["dip-off", "stop-to-zero", "measure"];
    expect(planItems(before)).toEqual([{ step: "dip-off", checkbox: true }, { step: "stop-to-zero", checkbox: false }, { step: "measure", checkbox: false }]);
    expect(planItems(["isolated-or-released", "measure"]).map((i) => i.step)).toEqual(["isolated-or-released", "measure"]);
    expect(planItems(before).filter((i) => i.step === "dip-off")).toHaveLength(1);
  });
});

describe("single pack texts", () => {
  it("never claim a slave count the master did not report", () => {
    for (const lng of ["de", "en"]) {
      const t = i18n.getFixedT(lng);
      const text = t("maint.panel.endOkZero");
      expect(text).not.toBe("maint.panel.endOkZero");
      expect(text).not.toMatch(/meldet 0|reports 0/);
    }
  });
});

describe("review follow-ups", () => {
  it("accepts only the newest open-work load", () => {
    const g = latestOnly();
    const first = g.begin();
    const second = g.begin();
    expect(g.accepts(first)).toBe(false);
    expect(g.accepts(second)).toBe(true);
  });
  it("shows an open sector note before a maintenance or custom image on the master", () => {
    const o = { ...base, sector_note: "unknown image on the master" };
    expect(planSectorNote(o, 0, "maintenance")).toBe("unknown image on the master");
    expect(planSectorNote(o, 0, "custom")).toBe("unknown image on the master");
    expect(planSectorNote(o, 0, "original")).toBeNull();
    expect(planSectorNote(o, 3, "custom")).toBeNull();
    expect(planSectorNote(base, 0, "custom")).toBeNull();
    for (const lng of ["de", "en"]) expect(i18n.exists("maint.plan.sectorNote", { lng })).toBe(true);
    expect(i18n.t("maint.plan.sectorNote", { lng: "de", note: "x" })).toBe("Offener Hinweis zu 0x23000: x. Vor diesem Flash klären oder bewusst fortfahren.");
  });
});
