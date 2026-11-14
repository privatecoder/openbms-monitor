import { describe, expect, it } from "vitest";
import { attemptAgain, failure, needsPause, outcomeOf, retried, runSeries, summarize, unclearWrites, writePack, type Outcome, type PackResult } from "./multiWrite";
import { KEYS, UNITS, type Parameters, type Source } from "./params";
import { afterUart1Write, changeOf, rawsOf, toRaw, SERIES_INDEX, type TargetPlan } from "./paramWrite";

/** A synthetic read: every raw 100, P26 as given, switch bytes as given. */
const block = (p26: number, switches: number[]): Parameters => ({ address: 1, device_name: "SYN", function_switches: switches,
  parameters: KEYS.map((key, index) => ({ index, key, raw: index === 26 ? p26 : 100, value: 0, unit: UNITS[index] })) });
const raws = (p26: number) => KEYS.map((_, i) => (i === 26 ? p26 : 100));
const SW = [1, 0, 0, 0, 0, 0, 0, 0];
// a plausible synthetic configuration (display values, P0..P86)
const GOOD = [3.45, 3.35, 2.9, 3.1, 3.65, 3.45, 2.7, 3.1, 3.4, 1.5, 55.2, 53.6, 46.4, 48, 56, 53.6, 43.2, 48, 63, 61,
  50, 47, 2, 5, 55, 50, -10, 0, 52, 47, -10, 3, 55, 50, -15, 0, 0, 10, 50, 47, 0, 3, 60, 55, -10, 0, 90, 85, 100, 85,
  150, 145, -155, -153, 160, -160, -300, 2000, 280, 75, 0.5, 0.3, 0.03, 0.02, 10, 16, 10, 10, 30, 60, 5, 5, 1, 10, 10,
  30, 240, 48, 15, 5, 96, 80, 10, 9, 0, 13, 0];
const good = (p26: number) => GOOD.map((v, i) => (i === 26 ? p26 : toRaw(i, v)));
const pack = (p26: number, switches: number[]): Parameters => ({ address: 1, device_name: "SYN", function_switches: switches,
  parameters: good(p26).map((raw, index) => ({ index, key: KEYS[index], raw: raw < 0 ? raw + 0x10000 : raw, value: 0, unit: UNITS[index] })) });

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
  it("a lost answer: confirmed by the read-back is written, else unclear", () => {
    expect(outcomeOf({ kind: "unclear", error: "timeout" }, raws(2731), SW)).toEqual({ kind: "unclear", error: "timeout" });
    expect(outcomeOf({ kind: "written", read_back: block(2731, SW), read_error: null, unanswered: true }, raws(2731), SW)).toEqual({ kind: "written", unanswered: true });
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
  it("the CAN socket's E2 makes no switch live: no pause", () => {
    const r = outcomeOf({ kind: "refused", refusal: "refused", stored: block(2631, [0, 0, 0, 0, 0, 0, 0, 0]), switches_live: false }, raws(2731), [3, 0, 0, 0, 0, 0, 0, 0]);
    expect(r).toEqual({ kind: "refused", refusal: "refused", live: 0 });
    expect(needsPause(r)).toBe(false);
  });
  it("an acknowledged CAN socket write whose read-back differs stays unclear; the series goes on", async () => {
    const r = outcomeOf({ kind: "unclear", error: '{"kind":"uart1","error":"readback"}', read_back: block(2631, SW), before: block(2531, SW) }, raws(2731), SW);
    expect(r.kind).toBe("unclear");
    const seen: number[] = [];
    const end = await runSeries([1, 2], { link: () => "l", cancelled: () => false, retries: 0, wait: async () => {},
      first: async (a) => { seen.push(a); return r; }, again: async () => r, pause: async () => true, report: () => {} });
    expect([end, seen]).toEqual(["done", [1, 2]]);
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
  /** `outcomes[a]`: the outcomes of pack a's attempts in order (the last one repeats); none: written at once. */
  const harness = (outcomes: Record<number, Outcome[]>, opts: { retries?: number; linkAfter?: [number, number]; cancelAfter?: [number, number]; goOn?: boolean } = {}) => {
    let link = "a|1", cancelled = false;
    const tries: [number, number][] = [], paused: number[] = [], lines: [number, string, number, boolean][] = [];
    let waits = 0;
    const attempt = (a: number) => {
      const n = tries.filter(([x]) => x === a).length + 1;
      tries.push([a, n]);
      if (opts.linkAfter?.[0] === a && opts.linkAfter[1] === n) link = "a|2";
      if (opts.cancelAfter?.[0] === a && opts.cancelAfter[1] === n) cancelled = true;
      const list = outcomes[a] ?? [{ kind: "written" }];
      return list[Math.min(n, list.length) - 1];
    };
    const run = runSeries([0, 1, 2, 3], {
      link: () => link, cancelled: () => cancelled, retries: opts.retries ?? 2, wait: async () => { waits++; },
      first: async (a) => attempt(a), again: async (a) => attempt(a),
      pause: async (a) => { paused.push(a); return opts.goOn ?? true; },
      report: (a, r) => lines.push([a, r.out.kind, r.attempts, r.gaveUp]),
    });
    return { run, tries, paused, lines, waits: () => waits };
  };
  const written = (h: { tries: [number, number][] }) => [...new Set(h.tries.map(([a]) => a))];

  it("writes every pack in the given order", async () => {
    const h = harness({});
    expect(await h.run).toBe("done");
    expect(written(h)).toEqual([0, 1, 2, 3]);
    expect(h.paused).toEqual([]);
    expect(h.waits()).toBe(0);
  });

  it("stops before the next pack when the link changed", async () => {
    const h = harness({}, { linkAfter: [1, 1] });
    expect(await h.run).toBe("link");
    expect(written(h)).toEqual([0, 1]);
    expect(h.lines.map(([a, k]) => [a, k])).toEqual([[0, "written"], [1, "written"], [2, "cancelled"], [3, "cancelled"]]);
  });

  it("stops before the next pack when cancelled", async () => {
    const h = harness({}, { cancelAfter: [0, 1] });
    expect(await h.run).toBe("cancelled");
    expect(written(h)).toEqual([0]);
  });

  it("pauses at a refusal with live switches and goes on when asked", async () => {
    const h = harness({ 1: [{ kind: "refused", refusal: "refused", live: 1 }] }, { goOn: true });
    expect(await h.run).toBe("done");
    expect(h.paused).toEqual([1]);
    expect(written(h)).toEqual([0, 1, 2, 3]);
    expect(h.tries.filter(([a]) => a === 1)).toHaveLength(1);
  });

  it("a cancel at the pause leaves the rest unwritten", async () => {
    const h = harness({ 1: [{ kind: "refused", refusal: "refused", live: 1 }] }, { goOn: false });
    expect(await h.run).toBe("cancelled");
    expect(written(h)).toEqual([0, 1]);
    expect(h.lines.slice(2).map(([a, k]) => [a, k])).toEqual([[2, "cancelled"], [3, "cancelled"]]);
  });

  it("a refusal without live switches is only reported, not retried", async () => {
    const h = harness({ 2: [{ kind: "refused", refusal: "refused", live: 0 }] });
    expect(await h.run).toBe("done");
    expect(h.paused).toEqual([]);
    expect(h.lines[2]).toEqual([2, "refused", 1, false]);
  });

  it("succeeds on the second attempt after a pause", async () => {
    const h = harness({ 1: [{ kind: "notTaken", params: [26], switches: [] }, { kind: "written" }] });
    expect(await h.run).toBe("done");
    expect(h.lines[1]).toEqual([1, "written", 2, false]);
    expect(h.waits()).toBe(1);
  });

  it("gives up after retries + 1 attempts, then the next pack still runs", async () => {
    const h = harness({ 1: [{ kind: "unreached", error: "timeout" }] }, { retries: 2 });
    expect(await h.run).toBe("done");
    expect(h.tries.filter(([a]) => a === 1)).toHaveLength(3);
    expect(h.lines[1]).toEqual([1, "unreached", 3, true]);
    expect(written(h)).toEqual([0, 1, 2, 3]);
  });

  it("an unclear result is retried and no longer stops the series", async () => {
    const h = harness({ 0: [{ kind: "unclear", error: "timeout" }] }, { retries: 1 });
    expect(await h.run).toBe("done");
    expect(h.lines[0]).toEqual([0, "unclear", 2, true]);
    expect(written(h)).toEqual([0, 1, 2, 3]);
  });

  it("retries 0: one attempt each, a failed pack is skipped and the series goes on", async () => {
    const h = harness({ 1: [{ kind: "unclear", error: "timeout" }], 2: [{ kind: "unread", error: "timeout" }] }, { retries: 0 });
    expect(await h.run).toBe("done");
    expect(h.tries).toEqual([[0, 1], [1, 1], [2, 1], [3, 1]]);
    expect(h.lines[1]).toEqual([1, "unclear", 1, true]);
    expect(h.waits()).toBe(0);
  });

  it("a link change between attempts ends the run before the retry", async () => {
    const h = harness({ 1: [{ kind: "notTaken", params: [26], switches: [] }, { kind: "written" }] }, { linkAfter: [1, 1] });
    expect(await h.run).toBe("link");
    expect(h.tries.filter(([a]) => a === 1)).toHaveLength(1);
    expect(h.lines.map(([a, k, n, g]) => [a, k, n, g])).toEqual([[0, "written", 1, false], [1, "notTaken", 1, false], [2, "cancelled", 0, false], [3, "cancelled", 0, false]]);
  });

  it("a cancel between attempts ends the run before the retry", async () => {
    const h = harness({ 0: [{ kind: "skipped", why: "stale" }] }, { cancelAfter: [0, 1] });
    expect(await h.run).toBe("cancelled");
    expect(h.tries).toEqual([[0, 1]]);
    expect(h.lines[0]).toEqual([0, "skipped", 1, false]);
  });
});

describe("retries", () => {
  it("retries only outcomes where the write may not have landed or was not checked", () => {
    const yes: Outcome[] = [{ kind: "notTaken", params: [1], switches: [] }, { kind: "unread", error: "" }, { kind: "unreached", error: "" },
      { kind: "unclear", error: "" }, { kind: "skipped", why: "stale" }];
    const no: Outcome[] = [{ kind: "written" }, { kind: "refused", refusal: "refused", live: 0 }, { kind: "refused", refusal: "locked", live: 0 },
      { kind: "skipped", why: "bank", error: "" }, { kind: "skipped", why: "busy", error: "" }, { kind: "skipped", why: "blocked", blockers: [] },
      { kind: "skipped", why: "same" }, { kind: "skipped", why: "pending" }, { kind: "cancelled" }];
    expect(yes.map(retried)).toEqual(yes.map(() => true));
    expect(no.map(retried)).toEqual(no.map(() => false));
  });

  // the run's change: P26 from −10 to −12 °C
  const change = changeOf(good(2631), good(2611), SW, SW);
  const again = (read: () => Promise<Parameters>, stop = () => false) => {
    const sent: TargetPlan[] = [];
    const out = attemptAgain(1, change, false, { read, stop, send: async (p) => { sent.push(p); return { kind: "written" }; } });
    return { out, sent };
  };

  it("a retry writes the change on top of a fresh read, only what still differs", async () => {
    const fresh = pack(2631, [1, 0, 0, 4, 0, 0, 0, 0]);
    const a = again(async () => fresh);
    expect(await a.out).toEqual({ kind: "written" });
    expect(a.sent).toHaveLength(1);
    expect(a.sent[0].raws[26]).toBe(2611);
    expect(a.sent[0].raws[0]).toBe(rawsOf(fresh)[0]);
    expect(a.sent[0].switches).toEqual([1, 0, 0, 4, 0, 0, 0, 0]);
    expect(a.sent[0].changes).toHaveLength(1);
  });

  it("unclear, then a fresh read that already shows the change: written without a second write", async () => {
    let n = 0;
    const sent: TargetPlan[] = [];
    const r = await writePack({ retries: 2, wait: async () => {}, stop: () => false,
      first: async () => { n++; return { kind: "unclear", error: "timeout" }; },
      again: () => attemptAgain(1, change, false, { read: async () => pack(2611, SW), stop: () => false, send: async (p) => { sent.push(p); return { kind: "written" }; } }) });
    expect(r).toEqual<PackResult>({ out: { kind: "written", byRead: true }, attempts: 2, gaveUp: false });
    expect([n, sent.length]).toEqual([1, 0]);
  });

  it("a fresh read with blockers skips the pack", async () => {
    // a change of the cells in series that is not unlocked
    const series = changeOf(good(2631), good(2631).map((r, i) => (i === SERIES_INDEX ? r + 1 : r)), SW, SW);
    const r = await attemptAgain(1, series, false, { read: async () => pack(2631, SW), stop: () => false, send: async () => ({ kind: "written" }) });
    expect(r.kind === "skipped" && r.why).toBe("blocked");
    expect(retried(r)).toBe(false);
  });

  it("a failed fresh read counts as a failed attempt", async () => {
    const a = again(async () => { throw "timeout after 1000 ms"; });
    expect(await a.out).toEqual({ kind: "unread", error: "timeout after 1000 ms", fresh: true });
    expect(a.sent).toHaveLength(0);
    const r = await writePack({ retries: 1, wait: async () => {}, stop: () => false,
      first: async () => ({ kind: "unreached", error: "x" }), again: () => again(async () => { throw "timeout"; }).out });
    expect(r).toEqual({ out: { kind: "unread", error: "timeout", fresh: true }, attempts: 2, gaveUp: true });
  });

  it("a link change during the fresh read writes nothing", async () => {
    const a = again(async () => pack(2631, SW), () => true);
    expect(await a.out).toEqual({ kind: "cancelled" });
    expect(a.sent).toHaveLength(0);
  });
});

describe("summary", () => {
  it("counts written, given up, refused, skipped, stopped and cancelled packs", () => {
    expect(summarize([
      { out: { kind: "written" }, attempts: 1 }, { out: { kind: "written", byRead: true }, attempts: 2 },
      { out: { kind: "unreached", error: "" }, attempts: 3, gaveUp: true },
      { out: { kind: "refused", refusal: "refused", live: 0 }, attempts: 1 },
      { out: { kind: "skipped", why: "same" } }, { out: { kind: "skipped", why: "blocked", blockers: [] }, attempts: 2 },
      { out: { kind: "notTaken", params: [1], switches: [] }, attempts: 1 },
      { out: { kind: "cancelled" }, attempts: 0 }, {},
    ])).toEqual({ written: 2, gaveUp: 1, tries: 3, refused: 1, skipped: 2, stopped: 1, cancelled: 1 });
  });
});

describe("CAN socket refusals", () => {
  // the shapes the backend sends (`uart1_refusal`, `uart1_error`)
  const u = (error: string, more: Record<string, unknown> = {}) => JSON.stringify({ kind: "uart1", error, ...more });

  it("a fixed refusal is skipped with its reason and not retried; a missing answer is not reached", () => {
    for (const why of ["pending", "write-locked", "unsupported", "changed", "board-changed", "no-identity", "other-master", "duplicate", "unchecked"]) {
      const o = failure(u(why));
      expect(o).toEqual({ kind: "skipped", why: "uart1", error: u(why) });
      expect(retried(o)).toBe(false);
    }
    const proto = u("protocol", { code: 135 });
    expect(failure(proto)).toEqual({ kind: "skipped", why: "uart1", error: proto });
    expect(failure(u("no-answer"))).toEqual({ kind: "unreached", error: u("no-answer") });
    expect(failure(u("readback")).kind).toBe("unclear");
    expect(retried(failure(u("no-answer")))).toBe(true);
  });

  it("a refusal of the first write ends the pack after one send; the next pack still runs", async () => {
    for (const why of ["pending", "write-locked", "unsupported", "changed"]) {
      const sends: number[] = [];
      const write = async (a: number) => { sends.push(a); return a === 1 ? failure(u(why)) : { kind: "written" } as Outcome; };
      const lines: [number, PackResult][] = [];
      const end = await runSeries([1, 2], { link: () => "l", cancelled: () => false, retries: 2, wait: async () => {},
        first: write, again: write, pause: async () => true, report: (a, r) => lines.push([a, r]) });
      expect([end, sends]).toEqual(["done", [1, 2]]);
      expect(lines[0][1]).toEqual({ out: { kind: "skipped", why: "uart1", error: u(why) }, attempts: 1, gaveUp: false });
    }
  });

  it("a fresh read is classified the same way", async () => {
    const change = changeOf([1], [2], [0], [0]);
    const read = (e: string) => attemptAgain(1, change, false, { read: async () => { throw e; }, stop: () => false, send: async () => ({ kind: "written" }) });
    expect(await read(u("pending"))).toEqual({ kind: "skipped", why: "uart1", error: u("pending") });
    expect(await read(u("no-answer"))).toEqual({ kind: "unread", error: u("no-answer"), fresh: true });
  });
});

describe("stopping during a retry's fresh read", () => {
  const change = changeOf([1], [2], [0], [0]);
  const fresh: Parameters = { address: 1, device_name: "SYN", function_switches: [0],
    parameters: [{ index: 0, key: KEYS[0], raw: 1, value: 0, unit: UNITS[0] }] };
  /** Packs `addresses`; pack 1's first write is not reached, its retry read resolves (or fails) only after `during`
   *  ran; the stop wiring is the view's: cancelled or another link. */
  const run = async (addresses: number[], during: (s: { link: string; cancelled: boolean }) => void, fails = false) => {
    const st = { link: "a|1", cancelled: false };
    const sends: number[] = [], lines: [number, string][] = [];
    const end = await runSeries(addresses, {
      link: () => st.link, cancelled: () => st.cancelled, retries: 2, wait: async () => {},
      first: async (a) => { sends.push(a); return a === 1 ? { kind: "unreached", error: "timeout" } : { kind: "written" }; },
      again: (a) => attemptAgain(a, change, true, {
        read: () => new Promise<Parameters>((res, rej) => setTimeout(() => { during(st); if (fails) rej("timeout"); else res(fresh); }, 5)),
        stop: () => st.cancelled || st.link !== "a|1",
        send: async () => { sends.push(a); return { kind: "written" }; },
      }),
      pause: async () => true, report: (a, r) => lines.push([a, r.out.kind]),
    });
    return { end, sends, lines };
  };

  it("a cancel while the read is pending sends nothing more", async () => {
    const r = await run([1, 2], (s) => { s.cancelled = true; });
    expect(r.sends).toEqual([1]);
    expect(r.end).toBe("cancelled");
    expect(r.lines).toEqual([[1, "cancelled"], [2, "cancelled"]]);
  });

  it("a link change while the read is pending: nothing sent, the run ends on the link (also at the last pack)", async () => {
    for (const fails of [false, true]) {
      const notLast = await run([1, 2], (s) => { s.link = "a|2"; }, fails);
      expect([notLast.end, notLast.sends, notLast.lines]).toEqual(["link", [1], [[1, "cancelled"], [2, "cancelled"]]]);
      const last = await run([1], (s) => { s.link = "a|2"; }, fails);
      expect([last.end, last.sends, last.lines]).toEqual(["link", [1], [[1, "cancelled"]]]);
    }
  });

  it("without a stop the retry goes on as planned", async () => {
    const r = await run([1, 2], () => {});
    expect([r.end, r.sends, r.lines]).toEqual(["done", [1, 1, 2], [[1, "written"], [2, "written"]]]);
    const failed = await run([1], () => {}, true);
    expect([failed.end, failed.lines]).toEqual(["done", [[1, "unread"]]]);
  });
});

describe("a CAN socket write confirmed by the fresh read", () => {
  const p = (p26: number) => pack(p26, SW);
  const col = (read: Parameters, more: Partial<Source> = {}): Source => ({ id: "pack-1", kind: "pack", address: 1, label: "Pack 01", link: "a|1",
    read: { ...read, bms_serial: "SYN-1" }, values: rawsOf(read), switches: read.function_switches, ...more } as Source);
  const unclearAnswer = { kind: "unclear" as const, error: "timeout", before: p(2631) };

  it("books the unclear write's block before it, with the fresh read and a new time, without sending again", async () => {
    const w = unclearWrites();
    w.sent(1, unclearAnswer);
    const change = changeOf(rawsOf(p(2631)), rawsOf(p(2611)), p(0).function_switches, p(0).function_switches);
    let booked: Source | null = null, sent = 0;
    const earlier = col(p(2651), { lastWritten: 5, previous: p(2671) });
    const out = await attemptAgain(1, change, true, { read: async () => p(2611), stop: () => false, send: async () => { sent++; return { kind: "written" }; },
      confirmed: (read) => { const before = w.take(1); if (before) booked = afterUart1Write(earlier, col(read), true, before, 99); } });
    expect([out, sent]).toEqual([{ kind: "written", byRead: true }, 0]);
    expect(booked!.previous).toEqual(p(2631));
    expect(booked!.lastWritten).toBe(99);
    expect(booked!.read!.parameters[26].raw).toBe(2611);
    expect(w.take(1)).toBeUndefined();
  });

  it("a change found after an attempt that never left is not booked as the app's write", () => {
    const w = unclearWrites();
    w.sent(1, unclearAnswer);
    w.sent(1, undefined);
    expect(w.take(1)).toBeUndefined();
    w.sent(2, { kind: "written", read_back: p(2611), read_error: null });
    expect(w.take(2)).toBeUndefined();
  });

  it("the hook is called only when the read already shows the change", async () => {
    const change = changeOf(rawsOf(p(2631)), rawsOf(p(2611)), p(0).function_switches, p(0).function_switches);
    let calls = 0;
    await attemptAgain(1, change, true, { read: async () => p(2631), stop: () => false, send: async () => ({ kind: "written" }), confirmed: () => { calls++; } });
    expect(calls).toBe(0);
  });
});
