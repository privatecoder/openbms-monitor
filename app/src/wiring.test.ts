import { describe, expect, it } from "vitest";
import { inverterOf, pollsBus, refusalOf } from "./wiring";

describe("wiring", () => {
  it("names the bus the inverter polls", () => {
    expect(pollsBus("uart0")).toBe("pack");
    expect(pollsBus("uart1")).toBe("can");
    expect(pollsBus("can")).toBeNull();
    expect(pollsBus("none")).toBeNull();
    expect(pollsBus(null)).toBeNull();
  });

  it("reads the two earlier names of the CAN socket's RS485 pair as that one value", () => {
    expect(inverterOf({ inverter: "uart1-master" })).toBe("uart1");
    expect(inverterOf({ inverter: "uart1-each" })).toBe("uart1");
    expect(inverterOf({ inverter: "can" })).toBe("can");
    expect(inverterOf(null)).toBeNull();
  });

  it("finds the bus rule's refusal in a connect error and inside another action's text", () => {
    expect(refusalOf('{"kind":"bus-refused","reason":"bus-inverter"}')).toBe("bus-inverter");
    expect(refusalOf('{"kind":"connection","message":"send refused: bus-polled"}')).toBe("bus-polled");
    expect(refusalOf(new Error("send refused: wiring-unset"))).toBe("wiring-unset");
    expect(refusalOf('{"kind":"no-answer","noise":false}')).toBeNull();
    expect(refusalOf("no answer within 1s")).toBeNull();
  });
});
