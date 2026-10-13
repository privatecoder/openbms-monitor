import { describe, expect, it } from "vitest";
import { keepsInverter, latestGuard, attachDecision, bannerOf, canFlash, endpointKey, errorKey, failingKey, failingText, flowOf, needsReplug, openWork, parsePacks, preselectPort, slavesOf, workUnchanged, type FlowInput } from "./maint";
import type { ControlEndpoint, MaintOverview, MaintSession } from "./api";

const inst = { id: "i", name: "Bank", endpoints: [], control: { kind: "serial" as const, path: "/dev/a" } };
const base: MaintOverview = { installation: inst, state: null, state_at: null, session: null, blocks: [], sector_note: null, end_next: null };
const session: MaintSession = { id: 1, installation: "i", kind: "marker", started: 1_000, expected_slaves: 3, done: [], failing: null };

describe("banner", () => {
  it("is hidden without session and without marker", () => {
    expect(bannerOf(base)).toBeNull();
    expect(bannerOf({ ...base, state: { state: "patched-off", slaves: 3 } })).toBeNull();
  });
  it("shows an open session with End, also after the connection was lost", () => {
    expect(bannerOf({ ...base, session, state: { state: "patched-on" } })).toMatchObject({ kind: "open", end: true, how: "marker" });
    expect(bannerOf({ ...base, session, state: { state: "unknown" } })).toMatchObject({ kind: "unconfirmed", end: true });
  });
  it("shows a marker without session, and never End for an unexplained marker", () => {
    expect(bannerOf({ ...base, state: { state: "patched-on" } })).toMatchObject({ kind: "open", end: true });
    expect(bannerOf({ ...base, state: { state: "marker-unexplained" } })).toMatchObject({ kind: "unexplained", end: false });
    expect(bannerOf({ ...base, session, state: { state: "marker-unexplained" } })).toMatchObject({ kind: "unexplained", end: false });
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
    measuredAt: null, port: "/dev/b", portOwner: "this", address: 3, imageKind: "original", scan: undefined, busy: false, running: false, ...o,
  });
  const states = (f: ReturnType<typeof flowOf>) => [f.steps.connections, f.steps.check, f.steps.inverter, f.steps.start, f.steps.measure, f.steps.flash].map((s) => s.state);
  const first = (n: number) => ({ key: "maint.flow.reason.first", params: { n } });

  it("without an installation only the firmware steps count", () => {
    const f = flowOf(input({ overview: null }));
    expect(f.steps.flash.state).toBe("current");
    expect(f.steps.connections.state).toBe("locked");
    expect(f.end.shown).toBe(false);
  });

  it("step 1: pack port chosen and assigned, control channel set or explicitly none", () => {
    expect(flowOf(input({ port: "" })).steps.connections).toEqual({ state: "current", reason: { key: "maint.flow.reason.choose-port" } });
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

describe("pack port preselection", () => {
  const withEps = (endpoints: string[], control: ControlEndpoint | null) => ({ ...inst, endpoints, control });
  it("preselects only the one candidate that is not the control adapter", () => {
    expect(preselectPort(["/dev/a", "/dev/b"], withEps(["serial:/dev/a", "serial:/dev/b"], { kind: "serial", path: "/dev/a" }))).toBe("/dev/b");
    // several candidates: ask
    expect(preselectPort(["/dev/a", "/dev/b", "/dev/c"], withEps(["serial:/dev/b", "serial:/dev/c"], { kind: "serial", path: "/dev/a" }))).toBe("");
    // a candidate that is not plugged in does not count
    expect(preselectPort(["/dev/a"], withEps(["serial:/dev/a", "serial:/dev/b"], { kind: "serial", path: "/dev/a" }))).toBe("");
  });
  it("the control adapter only when it is the installation's only serial endpoint", () => {
    expect(preselectPort(["/dev/a"], withEps(["serial:/dev/a"], { kind: "serial", path: "/dev/a" }))).toBe("/dev/a");
    expect(preselectPort(["/dev/a"], withEps(["serial:/dev/a", "tcp:gw:4196"], { kind: "serial", path: "/dev/a" }))).toBe("/dev/a");
    // another serial port is plugged in (not assigned): the user chooses
    expect(preselectPort(["/dev/a", "/dev/x"], withEps(["serial:/dev/a"], { kind: "serial", path: "/dev/a" }))).toBe("");
    // a TCP control channel is no serial endpoint
    expect(preselectPort(["/dev/b"], withEps(["serial:/dev/b"], { kind: "tcp", addr: "gw:4196" }))).toBe("/dev/b");
    // ports that belong to no installation are never preselected
    expect(preselectPort(["/dev/a", "/dev/b"], withEps([], null))).toBe("");
    expect(preselectPort(["/dev/a"], null)).toBe("");
  });
});
