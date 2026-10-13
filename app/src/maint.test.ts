import { describe, expect, it } from "vitest";
import { latestGuard, attachDecision, bannerOf, canFlash, endpointKey, errorKey, needsReplug, openWork } from "./maint";
import type { MaintOverview, MaintSession } from "./api";

const inst = { id: "i", name: "Bank", endpoints: [], control: { kind: "serial" as const, path: "/dev/a" } };
const base: MaintOverview = { installation: inst, state: null, state_at: null, session: null, blocks: [], sector_note: null };
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
  const ok = { overview: { ...base, session, state: { state: "patched-on" as const } }, inverter: true, measuredAt: 5_000, address: 3, imageKind: "original" as const, portAssigned: true };
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
    expect(canFlash({ ...ok, overview: { ...base, state: { state: "patched-off", slaves: 0 } }, inverter: false, measuredAt: null, portAssigned: false }).ok).toBe(true);
    expect(canFlash({ ...ok, overview: null, inverter: false, measuredAt: null, portAssigned: false }).ok).toBe(true);
  });
  it("unassigned_port_blocks_maintenance_flash", () => {
    expect(canFlash({ ...ok, portAssigned: false }).reason).toBe("port-not-assigned");
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
