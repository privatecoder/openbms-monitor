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
