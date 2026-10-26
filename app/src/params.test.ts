import { describe, expect, it } from "vitest";
import { FILE_NAMES, KEYS, UNITS, deviating, exportName, fromStored, nameInputOk, parseExport, toExport, withHistory, withPlaceholders, type Parameters, type Source } from "./params";

// a typical configuration (16S, 280 Ah)
const VALUES = [3.45, 3.35, 2.9, 3.1, 3.65, 3.45, 2.7, 3.1, 3.4, 1.5, 55.2, 53.6, 46.4, 48, 56, 53.6, 43.2, 48, 63, 61,
  50, 47, 2, 5, 55, 50, -10, 0, 52, 47, -10, 3, 55, 50, -15, 0, 0, 10, 50, 47, 0, 3, 60, 55, -10, 0, 90, 85, 100, 85,
  150, 145, -155, -153, 160, -160, -300, 2000, 280, 75, 0.5, 0.3, 0.03, 0.02, 10, 16, 10, 10, 30, 60, 5, 5, 1, 10, 10,
  30, 240, 48, 15, 5, 96, 80, 10, 9, 0, 13, 0];
const SWITCHES = [0xff, 0xdf, 0xff, 0x3f, 0xbf, 0x9f, 0xbf, 0x1f];
const pack: Parameters = {
  address: 3, device_name: "1101-SP75", function_switches: SWITCHES,
  parameters: VALUES.map((value, index) => ({ index, key: KEYS[index], raw: 0, value, unit: UNITS[index] })),
};

describe("parameter file", () => {
  const patchProof = { source: "live", release: "16.06.13 MAINT 1.6.3", caps: {} as never, at: null } as Parameters["patch"];

  it("writes TOML with keys, comments from the help and hex switches, and reads its own file back", () => {
    const toml = toExport(pack);
    expect(toml).toContain('format = "openbms-parameters"\nversion = 1\ndevice = "1101-SP75"\npatched = false');
    expect(toml).toContain("# P0 · Cell high-voltage warning · V\n");
    expect(toml).toContain("\ncell_high_voltage_alarm = 3.450\n");
    expect(toml).toContain(`\n${KEYS[26]} = -10.0\n`);
    expect(toml).toContain(`\n${KEYS[50]} = 150.00\n`);
    expect(toml).toContain("\ngroup1 = 0xDF\n");
    expect(toml).not.toContain("Monomer");
    expect(toml).not.toContain("<");
    expect(new Set(FILE_NAMES).size).toBe(KEYS.length);
    const s = parseExport(toml, "x.toml");
    expect(s.error).toBeUndefined();
    expect(s.values).toEqual(VALUES);
    expect(s.switches).toEqual(SWITCHES);
    expect(s.suspectFrom).toBeNull();
    expect(s.device).toBe("1101-SP75");
    expect(s.patched).toBe(false);
    expect(parseExport(toExport({ ...pack, patch: patchProof }), "m.toml").patched).toBe(true);
  });

  it("reads by key: order and comments do not matter", () => {
    const toml = toExport(pack);
    const lines = toml.split("\n");
    const a = lines.findIndex((l) => l.startsWith(`${KEYS[1]} =`)), b = lines.findIndex((l) => l.startsWith(`${KEYS[2]} =`));
    [lines[a], lines[b]] = [lines[b], lines[a]];
    expect(parseExport(lines.join("\n"), "o.toml").values).toEqual(VALUES);
    // a group block moved: still group1 = DF, group7 = 1F
    const g = toml.replace("group1 = 0xDF", "group1 = 0xDF # moved");
    expect(parseExport(g, "g.toml").switches).toEqual(SWITCHES);
    // a parameter only in a comment does not count
    expect(parseExport(toml.replace("\ncell_high_voltage_alarm = 3.450", "\n# cell_high_voltage_alarm = 3.450"), "c.toml").error).toBe("structure");
  });

  it("refuses invalid, incomplete and unknown content", () => {
    const toml = toExport(pack);
    expect(parseExport(toml.replace("group1 = 0xDF", 'group1 = "0G"'), "h.toml").error).toBe("value");
    expect(parseExport(toml.replace("group1 = 0xDF", "group1 = 0x1FF"), "o.toml").error).toBe("value");
    expect(parseExport(toml.replace("cell_high_voltage_alarm = 3.450", "cell_high_voltage_alarm = nan"), "n.toml").error).toBe("value");
    expect(parseExport(toml.replace("cell_high_voltage_alarm = 3.450", "cell_high_voltage_alarm = inf"), "i.toml").error).toBe("value");
    expect(parseExport(toml.replace("cell_high_voltage_alarm = 3.450", 'cell_high_voltage_alarm = "3,450"'), "d.toml").error).toBe("value");
    expect(parseExport(toml.replace("cell_high_voltage_alarm = 3.450", "cell_high_voltage_alarm = 3,450"), "d2.toml").error).toBe("foreign");
    expect(parseExport(toml.replace("[function_switches]", "unknown_parameter = 1\n\n[function_switches]"), "u.toml").error).toBe("structure");
    expect(parseExport(toml.replace(/\ngroup7 = 0x1F/, ""), "g.toml").error).toBe("structure");
    expect(parseExport(toml.replace("cell_high_voltage_alarm = 3.450", "cell_high_voltage_alarm = 3.450\ncell_high_voltage_alarm = 3.5"), "dup.toml").error).toBe("foreign");
    expect(parseExport(toml.replace("version = 1", "version = 2"), "v.toml").error).toBe("foreign");
    // BatteryMonitor XML and the app's earlier XML files
    expect(parseExport('<?xml version="1.0"?><paraGroup><parameter><Name>Monomer high voltage alarm</Name><Value>3.450</Value></parameter></paraGroup>', "bm.xml").error).toBe("foreign");
  });

  it("flags a file whose values are shifted against the known order", () => {
    const shifted = { ...pack, parameters: pack.parameters.map((p, i) => ({ ...p, value: i < 2 ? p.value : VALUES[(i + 8) % VALUES.length] })) };
    expect(parseExport(toExport(shifted), "old.toml").suspectFrom).toBe(2);
  });

  it("names saved files by pack and local time", () => {
    expect(exportName(3, new Date(2026, 9, 10, 18, 5))).toBe("Parameter_pack03_2026-10-10_18-05.toml");
  });
});

describe("deviating", () => {
  it("marks only the outliers when there is a clear majority", () => {
    expect([...deviating(["3.450", "3.450", "3.500", undefined, "3.450"])]).toEqual([2]);
  });
  it("marks every source when there is no majority", () => {
    expect([...deviating(["3.450", "3.500"])]).toEqual([0, 1]);
    expect([...deviating(["1", "1", "2", "2"])]).toEqual([0, 1, 2, 3]);
  });
  it("marks nothing when all agree or only one has a value", () => {
    expect(deviating(["5", "5"]).size).toBe(0);
    expect(deviating(["5", undefined]).size).toBe(0);
  });
});

describe("a late parameter history", () => {
  it("only the pack's column of the link it was asked on takes it", () => {
    const prev = { address: 1 } as unknown as Parameters;
    const cols: Source[] = [
      { id: "pack-1", kind: "pack", label: "01", link: "inst-b|2" },
      { id: "pack-2", kind: "pack", label: "02", link: "inst-a|1" },
    ];
    // asked for pack 1 on A's link; pack 1 meanwhile read on B's: nothing changes
    expect(withHistory(cols, 1, "inst-a|1", { last_written: 5, previous: prev })).toEqual(cols);
    const same = withHistory(cols, 1, "inst-b|2", { last_written: 5, previous: prev });
    expect(same[0].previous).toBe(prev);
    expect(same[1].previous).toBeUndefined();
  });
});

describe("withPlaceholders", () => {
  const col = (id: string, extra: Partial<Source> = {}): Source => ({ id, kind: "pack", label: id, ...extra });
  it("adds an empty column per pack on the bus, in pack order before the rest", () => {
    const file: Source = { id: "f", kind: "file", label: "x.xml" };
    const read = col("pack-1", { address: 1, values: [1] });
    const s = withPlaceholders([file, read], [0, 1, 2], true);
    expect(s.map((x) => x.id)).toEqual(["pack-0", "pack-1", "pack-2", "f"]);
    expect(s[0].notRead).toBe(true);
    // with its address: the column can be read alone before the first read
    expect(s.map((x) => x.address)).toEqual([0, 1, 2, undefined]);
    expect(s[1]).toBe(read);
  });
  it("keeps the array when nothing changes and drops the empty columns without a bus", () => {
    const s = withPlaceholders([], [0, 1], true);
    expect(withPlaceholders(s, [0, 1], true)).toBe(s);
    expect(withPlaceholders(s, [0, 1], false)).toEqual([]);
    const read = [col("pack-0", { values: [1] })];
    expect(withPlaceholders(read, [0], false)).toBe(read);
  });
  it("a pack no longer on the bus keeps a read column, loses its empty one", () => {
    const s = withPlaceholders([col("pack-5", { values: [1] })], [0, 1], true);
    expect(withPlaceholders(s, [0], true).map((x) => x.id)).toEqual(["pack-0", "pack-5"]);
  });
});

describe("device name", () => {
  it("takes 1–10 printable ASCII characters without outer spaces", () => {
    for (const ok of ["X", "PACK-01", "0123456789", "A B"]) expect(nameInputOk(ok), ok).toBe(true);
    for (const bad of ["", "01234567890", " X", "X ", "Ä", "A\tB", " "]) expect(nameInputOk(bad), bad).toBe(false);
  });
});

describe("a stored read", () => {
  it("keeps the patch it was taken under, so its release bits read as then", () => {
    const parameters: Parameters = { address: 0, parameters: [], function_switches: [0xff, 0, 0, 0, 0, 0, 0, 0], device_name: "SYN" };
    const patch = { source: "master" as const, release: "16.06.13 MAINT 1.6", caps: { status_split: true, uart1_params: true, uart1_identity: true, write_gate: true, start_gate: true }, at: 1 };
    const row = { address: 0, at: 1, kind: "read" as const, board: "current" as const, bms_serial: "SYN-0", parameters };
    expect(fromStored({ ...row, patch }).read?.patch).toEqual(patch);
    expect(fromStored(row).read?.patch).toBeUndefined();
    expect(fromStored({ ...row, parameters: null }).read).toBeUndefined();
  });
});
