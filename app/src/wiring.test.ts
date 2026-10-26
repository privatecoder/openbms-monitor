import { describe, expect, it } from "vitest";
import { pollsBus, refusalOf } from "./wiring";

describe("wiring", () => {
  it("names the bus the inverter polls", () => {
    expect(pollsBus("uart0")).toBe("pack");
    expect(pollsBus("uart1-master")).toBe("can");
    expect(pollsBus("uart1-each")).toBe("can");
    expect(pollsBus("can")).toBeNull();
    expect(pollsBus("none")).toBeNull();
    expect(pollsBus(null)).toBeNull();
  });

  it("finds the bus rule's refusal in a connect error and inside another action's text", () => {
    expect(refusalOf('{"code":"bus-inverter"}')).toBe("bus-inverter");
    expect(refusalOf('{"kind":"connection","message":"send refused: bus-polled"}')).toBe("bus-polled");
    expect(refusalOf(new Error("send refused: wiring-unset"))).toBe("wiring-unset");
    expect(refusalOf('{"code":"no-answer","noise":false}')).toBeNull();
    expect(refusalOf("no answer within 1s")).toBeNull();
  });
});
