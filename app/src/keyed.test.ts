import { describe, expect, it } from "vitest";
import { forKey } from "./keyed";
import { runExclusive } from "./exclusive";

describe("results with their query", () => {
  it("only the answer to the current query counts", () => {
    const r = { key: "inst-a|30", value: 1 };
    expect(forKey(r, "inst-a|30")).toBe(1);
    // another installation or period: no result, not the earlier one
    expect(forKey(r, "inst-b|30")).toBeNull();
    expect(forKey(r, "inst-a|7")).toBeNull();
    expect(forKey(null, "inst-a|30")).toBeNull();
  });
});

describe("one save at a time", () => {
  it("a second click before the answer starts nothing", async () => {
    const flag = { current: false };
    let runs = 0, finish: () => void = () => {};
    const save = () => runExclusive(flag, () => { runs++; return new Promise<string>((res) => { finish = () => res("ok"); }); });
    const first = save(), second = save();
    expect(runs).toBe(1);
    expect(await second).toBeUndefined();
    finish();
    expect(await first).toBe("ok");
    // after the answer the next one runs
    const third = save();
    expect(runs).toBe(2);
    finish();
    expect(await third).toBe("ok");
  });
});
