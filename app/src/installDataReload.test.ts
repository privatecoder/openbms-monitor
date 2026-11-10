import { describe, expect, it, vi } from "vitest";

// the backend: installation A has a stored pack selection
const settings = vi.fn(async (_id: string) => ({ cell_types: {}, groups: null, pairs: null, diagnosis: null, notifications: null, devices: null, wiring: null, packs: { mode: "count", n: 12 } }));
vi.mock("./api", () => ({ isDemo: false, api: { installationSettings: (id: string) => settings(id) } }));

describe("reloadSettings", () => {
  it("reads again what was loaded, so a component that mounted earlier is not left without its settings", async () => {
    const { loadedSettings, reloadSettings, useSettings } = await import("./installData");
    void useSettings; // the hook only triggers the first load; here through a reload of a key loaded before
    const flush = () => new Promise((r) => setTimeout(r, 0));
    // load A once the way the hook does (through a reload of an empty cache nothing is read)
    reloadSettings();
    expect(loadedSettings("A")).toBeNull();
    // simulate the first load
    const mod = await import("./installData");
    mod.__loadForTest("A");
    await flush();
    expect(loadedSettings("A")?.packs).toEqual({ mode: "count", n: 12 });
    const before = settings.mock.calls.length;
    reloadSettings();
    // emptied, and read again at once
    await flush();
    expect(settings.mock.calls.length).toBe(before + 1);
    expect(loadedSettings("A")?.packs).toEqual({ mode: "count", n: 12 });
  });
});

describe("reloadSettings during a read", () => {
  it("drops a read that started before it and reads again", async () => {
    const { loadedSettings, reloadSettings, __loadForTest } = await import("./installData");
    const flush = () => new Promise((r) => setTimeout(r, 0));
    // B's first read answers with the old state, but only after the reload; the second read gives the new one
    let release: (() => void) | null = null;
    let n = 0;
    settings.mockImplementation(async (id: string) => {
      // only B's reads count (A from the test before is read again too)
      if (id === "B") n += 1;
      const packs = id === "B" && n === 1 ? null : { mode: "count", n: 12 };
      if (id === "B" && n === 1) await new Promise<void>((r) => { release = r; });
      return { cell_types: {}, groups: null, pairs: null, diagnosis: null, notifications: null, devices: null, wiring: null, packs, id } as never;
    });
    __loadForTest("B");
    await flush();
    reloadSettings();
    release!();
    await flush(); await flush();
    expect(n).toBe(2);
    expect(loadedSettings("B")?.packs).toEqual({ mode: "count", n: 12 });
  });
});
