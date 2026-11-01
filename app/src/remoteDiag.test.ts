import { describe, expect, it } from "vitest";
import type { CalPageRead, Coefficients, SocRecord, SocStoreRead } from "./api";
import { calRows, socView } from "./remoteDiag";

const k = (cell: number, gainB: number, zeroA = -100): Coefficients => ({ cells: Array.from({ length: 16 }, (_, i) => cell + i), voltage: [61000, 61001], current_gain: [50000, gainB], zero: [zeroA, 200] });
const active = (c: Coefficients): CalPageRead => ({ page: "active", error: null, value: { kind: "active", invalid_bits: 0, cell_count: 16, coefficients: c, reference: [1, 2], raw_sample: [0, 0], current_ma: [0, 0] } });
const stored = (page: "primary" | "backup", c: Coefficients | null): CalPageRead => ({ page, error: null,
  value: { kind: "stored", page, class: c ? "valid" : "invalid", crc_ok: true, declared_len: 51, raw: "00", coefficients: c, invalid_bits: c ? 0 : null } });

describe("calibration rows", () => {
  it("marks differing values, ignores the forced active gain B, leaves undecoded copies empty", () => {
    const rows = calRows([active(k(60000, 62000)), stored("primary", k(60000, 61500)), stored("backup", k(60000, 61500, -90))]);
    expect(rows).toHaveLength(16 + 2 + 4);
    expect(rows[0]).toEqual({ key: "cell", n: 1, values: [60000, 60000, 60000], differs: false });
    // the active gain B is set by the firmware: the same stored copies do not differ
    expect(rows.find((r) => r.key === "gainB")).toMatchObject({ values: [62000, 61500, 61500], differs: false });
    expect(rows.find((r) => r.key === "zeroA")).toMatchObject({ values: [-100, -100, -90], differs: true });
    // an invalid backup and a missing page: no values, nothing compared against them
    const partial = calRows([active(k(60000, 62000)), stored("backup", null)]);
    expect(partial[0]).toMatchObject({ values: [60000, null, null], differs: false });
    expect(calRows([])).toHaveLength(6);
  });
});

const rec = (remaining: number, full: number): SocRecord => ({ remaining_ah: remaining, full_ah: full, charged_kwh: 1, discharged_kwh: 1, cycles: 3, soc_reference: 0, soc_decrease: 0 });
const read = (boot: SocStoreRead["store"]["boot"], cls: [string, string], records = true): SocStoreRead => ({
  records, energy: true, checkpoint: false,
  store: { class: cls as SocStoreRead["store"]["class"], compare: "different", crc_ok: [true, true], boot, live_p58_ah: 280, live_p59_ah: 140, live_charged_kwh: 7000, live_discharged_kwh: 6500,
    live_remaining_ah: 176.4, live_full_ah: 280, live_soc: 63, raw: ["", ""], records: [cls[0] === "valid" ? rec(114.8, 280) : null, cls[1] === "valid" ? rec(140, 280) : null] },
});

describe("stored SOC", () => {
  it("takes only the copy the boot choice names, and only when strictly valid", () => {
    const v = socView(read("primary", ["valid", "valid"]));
    expect(v.stored).toBeCloseTo(41);
    expect(v.delta).toBeCloseTo(-22);
    expect(socView(read("backup", ["valid", "valid"])).stored).toBeCloseTo(50);
    // boot takes a primary the firmware accepts but of the wrong shape: no number, not the valid backup instead
    expect(socView(read("primary", ["invalid", "valid"]))).toMatchObject({ stored: null, delta: null, record: null });
    expect(socView(read("fallback", ["erased", "erased"])).stored).toBeNull();
    // without read-capability bit 2 the records are not vouched for
    expect(socView(read("primary", ["valid", "valid"], false)).stored).toBeNull();
  });
});
