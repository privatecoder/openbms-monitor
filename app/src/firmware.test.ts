import { describe, expect, it } from "vitest";
import { confirmationPhrase, progressFraction, steps, type FwSetup } from "./firmware";

const image: FwSetup["image"] = { release: "16.06.13", device: "EMU1101_10D_HW16", bytes: 53000, blocks: 209, sha256: "ab", kind: "original", custom: false };
const target = { device_name: "X", firmware_version: "16.04", can_protocol: "CAN" };
const base: FwSetup = { running: false, done: false, port: "/dev/x", address: 0, image, target, checkedFor: { port: "/dev/x", address: 0 }, typed: "", unlocked: true, ready: true, busy: false };

describe("firmware steps", () => {
  it("check pack needs port and image", () => {
    expect(steps({ ...base, port: "" }).inspect).toBe(false);
    expect(steps({ ...base, image: null }).inspect).toBe(false);
    expect(steps(base).inspect).toBe(true);
  });
  it("target_must_match_port_and_address", () => {
    expect(steps(base).confirm).toBe(true);
    expect(steps({ ...base, address: 1 }).confirm).toBe(false);
    expect(steps({ ...base, port: "/dev/y" }).confirm).toBe(false);
    expect(steps({ ...base, target: null }).confirm).toBe(false);
  });
  it("start_needs_exact_phrase", () => {
    expect(confirmationPhrase(3)).toBe("FLASH 3");
    expect(steps({ ...base, typed: "FLASH 0" }).start).toBe(true);
    expect(steps({ ...base, typed: " FLASH 0 " }).start).toBe(true);
    expect(steps({ ...base, typed: "flash 0" }).start).toBe(false);
    expect(steps({ ...base, typed: "FLASH 1" }).start).toBe(false);
  });
  it("custom image needs FLASH CUSTOM", () => {
    const custom: FwSetup = { ...base, image: { ...image, release: "custom", kind: "custom", custom: true } };
    expect(confirmationPhrase(2, true)).toBe("FLASH CUSTOM 2");
    expect(steps({ ...custom, typed: "FLASH CUSTOM 0" }).start).toBe(true);
    expect(steps({ ...custom, typed: "FLASH 0" }).start).toBe(false);
    expect(steps({ ...base, typed: "FLASH CUSTOM 0" }).start).toBe(false);
  });
  it("check pack and start follow the guided flow (steps 2–5 of the pack bus panel)", () => {
    const s = steps({ ...base, typed: "FLASH 0", ready: false });
    expect([s.inspect, s.confirm, s.start]).toEqual([false, false, false]);
    // choosing an image stays free during the preparation
    expect(s.image).toBe(true);
  });
  it("another action (pack check, measurement, End step) locks the inputs while it runs", () => {
    expect(steps({ ...base, typed: "FLASH 0", busy: true })).toEqual({ image: false, inspect: false, confirm: false, start: false });
  });
  it("nothing is editable while running", () => {
    const s = steps({ ...base, typed: "FLASH 0", running: true });
    expect(s).toEqual({ image: false, inspect: false, confirm: false, start: false });
  });
  it("a shown result blocks a second run with the old confirmation", () => {
    const s = steps({ ...base, typed: "FLASH 0", done: true });
    expect(s).toEqual({ image: false, inspect: false, confirm: false, start: false });
  });
  it("after New update the pack must be checked and confirmed again", () => {
    // what reset() and start() leave behind
    const cleared = { ...base, target: null, checkedFor: null, typed: "" };
    expect(steps(cleared)).toEqual({ image: true, inspect: true, confirm: false, start: false });
    expect(steps({ ...cleared, typed: "FLASH 0" }).start).toBe(false);
  });
});

describe("progress", () => {
  it("counts blocks and treats the stages around them as 0 and 1", () => {
    expect(progressFraction(null)).toBe(0);
    expect(progressFraction({ stage: "erase", block: null, total: null })).toBe(0);
    expect(progressFraction({ stage: "block", block: 0, total: 4 })).toBe(0);
    expect(progressFraction({ stage: "block", block: 2, total: 4 })).toBe(0.5);
    expect(progressFraction({ stage: "finalize", block: null, total: null })).toBe(1);
  });
});

describe("maintenance gate", () => {
  it("start stays locked while maintenance does not unlock it", () => {
    const s = { ...base, typed: "FLASH 0" };
    expect(steps({ ...s, unlocked: true }).start).toBe(true);
    expect(steps({ ...s, unlocked: false }).start).toBe(false);
  });
});
