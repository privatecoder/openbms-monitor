import { describe, expect, it } from "vitest";
import { settingsSync } from "./settingsSync";

/** A promise to resolve or reject from the test. */
function later<T>() {
  let resolve!: (v: T) => void, reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("settingsSync", () => {
  it("drops a poll answer that was asked before a write", async () => {
    const shown: string[] = [];
    const sync = settingsSync<string>((s) => shown.push(s));
    const old = later<string>();
    const polled = sync.poll(() => old.promise);
    const wrote = sync.write(async () => "on");
    await wrote;
    old.resolve("off");
    await polled;
    expect(shown).toEqual(["on"]);
    await sync.poll(async () => "on, polled");
    expect(shown).toEqual(["on", "on, polled"]);
  });

  it("drops a poll asked while a write is under way", async () => {
    const shown: string[] = [];
    const sync = settingsSync<string>((s) => shown.push(s));
    const put = later<string>();
    const wrote = sync.write(() => put.promise);
    const polled = sync.poll(async () => "before the write landed");
    await polled;
    put.resolve("written");
    await wrote;
    expect(shown).toEqual(["written"]);
  });

  it("sends writes one after the other and shows only the newest answer", async () => {
    const shown: string[] = [];
    const sent: string[] = [];
    const sync = settingsSync<string>((s) => shown.push(s));
    const first = later<string>();
    const a = sync.write(() => { sent.push("enable"); return first.promise; });
    const b = sync.write(async () => { sent.push("interval"); return "enabled, 60 s"; });
    await flush();
    expect(sent).toEqual(["enable"]);
    first.resolve("enabled");
    await Promise.all([a, b]);
    expect(sent).toEqual(["enable", "interval"]);
    expect(shown).toEqual(["enabled, 60 s"]);
  });

  it("a failed write does not stop the next one, and polls count again afterwards", async () => {
    const shown: string[] = [];
    const sync = settingsSync<string>((s) => shown.push(s));
    const failed = sync.write(() => Promise.reject(new Error("disk full")));
    const next = sync.write(async () => "second");
    await expect(failed).rejects.toThrow("disk full");
    await next;
    await sync.poll(async () => "polled");
    expect(shown).toEqual(["second", "polled"]);
  });
});
