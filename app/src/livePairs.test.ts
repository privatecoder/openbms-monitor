import { describe, expect, it } from "vitest";
import { addAnswer, live, resetLivePairs } from "./livePairs";
import type { PackUpdate, Telemetry } from "./api";

const answer = (address: number, current: number, read_at: number | null): PackUpdate =>
  ({ address, telemetry: { current, port_voltage: 53.5 } as Telemetry, status: null, error: null, read_at });

describe("live pair memory", () => {
  it("keeps the read time, a minute of answers, and is emptied with the connection", () => {
    resetLivePairs();
    addAnswer(answer(0, 30, 1_000), 99_999);
    addAnswer(answer(0, 31, 50_000));
    addAnswer(answer(0, 32, 70_000));
    expect(live.samples.get(0)!.map((x) => x.t)).toEqual([50_000, 70_000]);
    // a failed read adds nothing; a backend without read times uses the receive time
    addAnswer({ ...answer(1, 5, null), telemetry: null });
    expect(live.samples.has(1)).toBe(false);
    addAnswer(answer(1, 5, null), 71_000);
    expect(live.samples.get(1)![0].t).toBe(71_000);
    resetLivePairs();
    expect(live.samples.size).toBe(0);
  });
});

describe("the pair window on the source's clock", () => {
  it("keeps samples of a source whose clock is 120 s behind", async () => {
    const { windowNow } = await import("./livePairs");
    const { pairedMeans } = await import("./pairs");
    resetLivePairs();
    const local = 1_000_000, behind = local - 120_000;
    for (let k = 0; k < 4; k++) {
      addAnswer(answer(0, 30, behind + k * 13_000), local + k * 13_000 + 100);
      addAnswer(answer(1, 30, behind + k * 13_000 + 700), local + k * 13_000 + 800);
    }
    const now = windowNow(local + 3 * 13_000 + 900);
    expect(Math.abs(now - (behind + 3 * 13_000 + 800))).toBeLessThan(500);
    expect(pairedMeans(live.samples.get(0)!, live.samples.get(1)!, now, 60_000)?.n).toBe(4);
  });

  it("does not pair reads 3.4 s apart, whatever the network delay", async () => {
    const { windowNow } = await import("./livePairs");
    const { pairedMeans } = await import("./pairs");
    resetLivePairs();
    const t0 = 5_000_000;
    for (let k = 0; k < 4; k++) {
      addAnswer(answer(0, 30, t0 + k * 13_000), t0 + k * 13_000 + 700);
      addAnswer(answer(1, 50, t0 + k * 13_000 + 3_400), t0 + k * 13_000 + 3_600);
    }
    expect(pairedMeans(live.samples.get(0)!, live.samples.get(1)!, windowNow(t0 + 60_000), 60_000)).toBeUndefined();
  });

  it("is the local clock before any answer", async () => {
    const { windowNow } = await import("./livePairs");
    resetLivePairs();
    expect(windowNow(123)).toBe(123);
  });
});

describe("the window clock with delayed sends", () => {
  it("follows the add-on's send time, not the read time, so delayed groups do not stretch the window", async () => {
    const { windowNow } = await import("./livePairs");
    const { pairedMeans } = await import("./pairs");
    resetLivePairs();
    // pairs read at 100/101, 129/130, 158/159 s, each group sent 30 s after its reads
    for (const r of [100_000, 129_000, 158_000]) {
      addAnswer({ ...answer(0, 30, r), sent_at: r + 30_000 }, r + 30_000);
      addAnswer({ ...answer(1, 30, r + 1_000), sent_at: r + 30_000 }, r + 30_000);
    }
    const now = windowNow(188_000 + 1_000);
    expect(now).toBe(189_000);
    expect(pairedMeans(live.samples.get(0)!, live.samples.get(1)!, now, 60_000)?.n ?? 0).toBeLessThan(3);
  });
});
