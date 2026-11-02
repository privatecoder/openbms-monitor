import { beforeEach, describe, expect, it, vi } from "vitest";

const calls = vi.hoisted(() => ({ check: vi.fn(), install: vi.fn() }));
const listen = vi.hoisted(() => ({ fail: false }));
vi.mock("./api", () => ({
  isDemo: true,
  api: {
    updateCheck: calls.check, updateInstall: calls.install,
    onUpdateProgress: async () => { if (listen.fail) throw new Error("no events"); return () => {}; },
  },
}));

const load = async () => { vi.resetModules(); return import("./update"); };
const info = { version: "9.0.0-beta", current: "0.2.0-beta", notes: "- x", date: null };

describe("updates", () => {
  beforeEach(() => { calls.check.mockReset(); calls.install.mockReset(); });

  it("a check finds an update or none", async () => {
    const u = await load();
    calls.check.mockResolvedValueOnce(info);
    await u.checkUpdate();
    expect(u.updateNow()).toMatchObject({ phase: "available", info });
    calls.check.mockResolvedValueOnce(null);
    await u.checkUpdate();
    expect(u.updateNow()).toMatchObject({ phase: "current", info: null });
    calls.check.mockRejectedValueOnce("offline");
    await u.checkUpdate();
    expect(u.updateNow()).toMatchObject({ phase: "error", error: "offline" });
  });

  it("a refused install keeps the update, a failed one needs a new check", async () => {
    const u = await load();
    const seen: string[] = [];
    calls.check.mockResolvedValue(info);
    await u.checkUpdate();
    calls.install.mockRejectedValueOnce("busy:flash");
    await u.installUpdate();
    expect(u.updateNow()).toMatchObject({ phase: "available", info, error: "busy:flash" });
    // state is read through the hook's snapshot function; use a fresh install attempt to observe phases
    calls.install.mockImplementationOnce(async () => { seen.push("installing"); throw "signature mismatch"; });
    await u.installUpdate();
    expect(seen).toEqual(["installing"]);
    expect(u.updateNow()).toMatchObject({ phase: "error", info: null });
    // after a failed (not refused) install nothing is pending: a second call does not reach the backend
    await u.installUpdate();
    expect(calls.install).toHaveBeenCalledTimes(2);
  });

  it("a failed progress listener does not block the install", async () => {
    const u = await load();
    calls.check.mockResolvedValue(info);
    await u.checkUpdate();
    listen.fail = true;
    calls.install.mockRejectedValueOnce("busy:pack");
    await u.installUpdate();
    listen.fail = false;
    expect(calls.install).toHaveBeenCalledTimes(1);
    expect(u.updateNow()).toMatchObject({ phase: "available", error: "busy:pack" });
  });

  it("busy errors map to their own text", async () => {
    const u = await load();
    expect(u.updateErrorKey("busy:history")).toBe("about.update.busy.history");
    expect(u.updateErrorKey("download failed")).toBe("about.update.failed");
  });
});
