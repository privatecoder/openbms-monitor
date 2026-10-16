import { describe, expect, it } from "vitest";
import { failure, needsPause, outcomeOf, runSeries, type Outcome } from "./multiWrite";
import { KEYS, UNITS, type Parameters } from "./params";

/** A synthetic read: every raw 100, P26 as given, switch bytes as given. */
const block = (p26: number, switches: number[]): Parameters => ({ address: 1, device_name: "SYN", function_switches: switches,
  parameters: KEYS.map((key, index) => ({ index, key, raw: index === 26 ? p26 : 100, value: 0, unit: UNITS[index] })) });
const raws = (p26: number) => KEYS.map((_, i) => (i === 26 ? p26 : 100));
const SW = [1, 0, 0, 0, 0, 0, 0, 0];

describe("result of one pack", () => {
  it("written and read back as sent", () => {
    expect(outcomeOf({ kind: "written", read_back: block(2731, SW), read_error: null }, raws(2731), SW)).toEqual({ kind: "written" });
  });
  it("read back differs: names the parameters and switch bytes not taken", () => {
    expect(outcomeOf({ kind: "written", read_back: block(2631, [1, 0, 0, 4, 0, 0, 0, 0]), read_error: null }, raws(2731), SW))
      .toEqual({ kind: "notTaken", params: [26], switches: [3] });
  });
  it("written without a read-back", () => {
    expect(outcomeOf({ kind: "written", read_back: null, read_error: "timeout" }, raws(2731), SW)).toEqual({ kind: "unread", error: "timeout" });
  });
  it("stale is skipped", () => {
    expect(outcomeOf({ kind: "stale", fresh: block(2631, SW) }, raws(2731), SW)).toEqual({ kind: "skipped", why: "stale" });
  });
  it("a refusal counts the switch bits live but not stored", () => {
    const r = outcomeOf({ kind: "refused", refusal: "refused", stored: block(2631, [0, 0, 0, 0, 0, 0, 0, 0]) }, raws(2731), [3, 0, 0, 0, 0, 0, 0, 0]);
    expect(r).toEqual({ kind: "refused", refusal: "refused", live: 2 });
    expect(needsPause(r)).toBe(true);
    const same = outcomeOf({ kind: "refused", refusal: "refused", stored: block(2631, SW) }, raws(2731), SW);
    expect(needsPause(same)).toBe(false);
    expect(needsPause(outcomeOf({ kind: "refused", refusal: "invalid", stored: block(2631, [0, 0, 0, 0, 0, 0, 0, 0]) }, raws(2731), SW))).toBe(false);
  });
  it("errors: bank and busy skipped, silent not reached, link change cancels", () => {
    const bank = JSON.stringify({ kind: "bank", state: "unconfirmed", address: 3, why: "new" });
    expect(failure(bank)).toEqual({ kind: "skipped", why: "bank", error: bank });
    const silent = JSON.stringify({ kind: "bank", state: "failed", address: 0, error: "no-answer" });
    expect(failure(silent)).toEqual({ kind: "unreached", error: silent });
    const busy = JSON.stringify({ kind: "pack-busy", address: 2, action: "clock" });
    expect(failure(busy)).toEqual({ kind: "skipped", why: "busy", error: busy });
    expect(failure(JSON.stringify({ kind: "link-changed" }))).toEqual({ kind: "cancelled" });
    expect(failure("timeout after 1000 ms")).toEqual({ kind: "unreached", error: "timeout after 1000 ms" });
  });
});

describe("the series", () => {
  const harness = (outcomes: Record<number, Outcome>, opts: { linkAfter?: number; cancelAfter?: number; goOn?: boolean } = {}) => {
    let link = "a|1", cancelled = false;
    const written: number[] = [], paused: number[] = [], lines: [number, string][] = [];
    const run = runSeries([0, 1, 2, 3], {
      link: () => link, cancelled: () => cancelled,
      write: async (a) => { written.push(a); if (a === opts.linkAfter) link = "a|2"; if (a === opts.cancelAfter) cancelled = true; return outcomes[a] ?? { kind: "written" }; },
      pause: async (a) => { paused.push(a); return opts.goOn ?? true; },
      report: (a, o) => lines.push([a, o.kind]),
    });
    return { run, written, paused, lines };
  };

  it("writes every pack in the given order", async () => {
    const h = harness({});
    expect(await h.run).toBe("done");
    expect(h.written).toEqual([0, 1, 2, 3]);
    expect(h.paused).toEqual([]);
  });

  it("stops before the next pack when the link changed", async () => {
    const h = harness({}, { linkAfter: 1 });
    expect(await h.run).toBe("link");
    expect(h.written).toEqual([0, 1]);
    expect(h.lines).toEqual([[0, "written"], [1, "written"], [2, "cancelled"], [3, "cancelled"]]);
  });

  it("stops before the next pack when cancelled", async () => {
    const h = harness({}, { cancelAfter: 0 });
    expect(await h.run).toBe("cancelled");
    expect(h.written).toEqual([0]);
  });

  it("pauses at a refusal with live switches and goes on when asked", async () => {
    const h = harness({ 1: { kind: "refused", refusal: "refused", live: 1 } }, { goOn: true });
    expect(await h.run).toBe("done");
    expect(h.paused).toEqual([1]);
    expect(h.written).toEqual([0, 1, 2, 3]);
  });

  it("a cancel at the pause leaves the rest unwritten", async () => {
    const h = harness({ 1: { kind: "refused", refusal: "refused", live: 1 } }, { goOn: false });
    expect(await h.run).toBe("cancelled");
    expect(h.written).toEqual([0, 1]);
    expect(h.lines.slice(2)).toEqual([[2, "cancelled"], [3, "cancelled"]]);
  });

  it("a refusal without live switches is only reported", async () => {
    const h = harness({ 2: { kind: "refused", refusal: "refused", live: 0 } });
    expect(await h.run).toBe("done");
    expect(h.paused).toEqual([]);
    expect(h.written).toEqual([0, 1, 2, 3]);
  });
});
