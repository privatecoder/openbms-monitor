import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { accessRows, bestDecision, bestState, cellTitle, decide, group0Plan, reasonText, ROUTES, type Facts, type Group0Plan, type Proof } from "./access";
import VECTORS from "./generated/access.vectors.json";
import PARAM_FILE from "./generated/param_file.json";
import type { AccessDecision } from "./api";

const d = (action: string, channel: "uart0" | "uart1" | "uart2", state: AccessDecision["state"], reasons: AccessDecision["reasons"] = []): AccessDecision =>
  ({ route: { action, channel, wire: "W", implemented: state !== "not-implemented" }, state, reasons });

describe("access matrix texts", () => {
  it("groups the ways per action in the backend's order", () => {
    const rows = accessRows([d("read-parameters", "uart0", "ready"), d("read-parameters", "uart1", "unknown"), d("flash", "uart0", "ready")]);
    expect(rows.map((r) => [r.action, Object.keys(r.cells)])).toEqual([["read-parameters", ["uart0", "uart1"]], ["flash", ["uart0"]]]);
    // two ways on one channel: the cell shows the better one
    const two = accessRows([d("read-serials", "uart1", "blocked"), d("read-serials", "uart1", "ready")]);
    expect(bestState(two[0].cells.uart1!)).toBe("ready");
  });
  it("names the firmware need of the way the cell's state comes from", async () => {
    const { needText, routeNeed, routeOf } = await import("./access");
    const de = i18n.getFixedT("de");
    // the serial numbers on the CAN socket: binary A2/A4 (EMU1101-EXT) blocked, the OEM master's Modbus string ready
    const [ext, oem] = ROUTES.filter((r) => r.action === "read-serials" && r.channel === "uart1");
    const ways: AccessDecision[] = [{ route: ext, state: "blocked", reasons: [] }, { route: oem, state: "ready", reasons: [] }];
    expect(bestDecision(ways).route).toBe(oem);
    expect(needText(de, routeNeed(routeOf(bestDecision(ways).route)!))).toBe("ab OEM 16.06.11");
    expect(needText(de, routeNeed(routeOf(ways[0].route)!))).toBe("ab EMU1101-EXT Light 1.5.4");
  });
  it("names every reason in both languages", async () => {
    for (const lng of ["de", "en"]) {
      await i18n.changeLanguage(lng);
      const t = i18n.t.bind(i18n);
      for (const r of [
        { id: "firmware", need: { kind: "patch-from", version: [1, 5] }, known: "unknown" }, { id: "firmware", need: { kind: "image-flag", bit: 1 }, known: "no" },
        { id: "firmware", need: { kind: "image-flag", bit: 2 }, known: "unknown" }, { id: "firmware", need: { kind: "image-flag", bit: 5 }, known: "no" },
        { id: "bit", group: 0, bit: 0, want: true, known: false }, { id: "bit", group: 7, bit: 3, want: true, known: null },
        { id: "master-only" }, { id: "quiet-bus" }, { id: "local-board" }, { id: "not-implemented" }, { id: "view-only" },
        { id: "marker", on: false, known: "foreign" }, { id: "marker", on: true, known: null },
        { id: "base", from: [16, 6, 4], known: [16, 6, 3] }, { id: "base", from: [16, 6, 11], known: null },
        { id: "with-slaves", known: 0 }, { id: "with-slaves", known: null },
      ] as AccessDecision["reasons"]) {
        const text = reasonText(t, r);
        expect(text.length, JSON.stringify(r)).toBeGreaterThan(5);
        expect(text, JSON.stringify(r)).not.toMatch(/acc\./);
      }
      expect(cellTitle(t, [d("set-can-protocol", "uart2", "not-implemented", [{ id: "local-board" }])])).toMatch(/^W: .+\n· /);
      // a flag the texts do not name yet (a later build) still reads as a sentence with its bit, the version as „1.7“
      expect(reasonText(t, { id: "firmware", need: { kind: "image-flag", bit: 5 }, known: "no" })).toMatch(/5/);
      expect(reasonText(t, { id: "firmware", need: { kind: "patch-from", version: [1, 7] }, known: "no" })).toMatch(/1\.7/);
    }
  });
});

describe("the catalog evaluated as in Rust", () => {
  it("decides every route like access::decide for every vector", () => {
    for (const v of VECTORS.decide) {
      const got = ROUTES.map((r) => decide(r, v.facts as Facts));
      expect(got).toEqual(v.decisions);
    }
    expect(VECTORS.decide.length).toBeGreaterThan(50);
  });
  it("plans Group0 like access::group0_plan", () => {
    for (const v of VECTORS.group0) {
      expect(group0Plan(v.channel as "uart0" | "uart1" | "uart2", v.proof as Proof | null)).toEqual(v.plan as Group0Plan);
    }
  });
});

describe("the firmware a way needs", () => {
  it("names the first supported EMU1101-EXT build and the OEM base", async () => {
    const { needText, routeNeed } = await import("./access");
    const de = i18n.getFixedT("de");
    const need = (action: string, channel: string) => needText(de, routeNeed(ROUTES.find((r) => r.action === action && r.channel === channel)!));
    expect(need("read-parameters", "uart1")).toBe("ab EMU1101-EXT Light 1.5.4");
    expect(need("power-on-off", "uart1")).toBe("ab EMU1101-EXT Light 1.5.4");
    expect(need("read-identity", "uart0")).toBe("ab EMU1101-EXT Light 1.5.4");
    expect(need("read-diagnosis", "uart1")).toBe("ab EMU1101-EXT Light 1.5.4");
    expect(need("pack-lock-set", "uart1")).toBe("ab EMU1101-EXT Full 1.6.4");
    expect(need("hard-stop", "uart1")).toBe("ab EMU1101-EXT Full 1.6.4");
    expect(need("read-system", "uart1")).toBe("ab OEM 16.06.04");
    // the reads by their read-capability bits
    expect(need("read-history", "uart1")).toBe("ab EMU1101-EXT Light 1.5.4");
    expect(need("read-calibration", "uart0")).toBe("ab EMU1101-EXT Light 1.5.4");
    expect(need("read-soc-store", "uart1")).toBe("ab EMU1101-EXT Light 1.5.4");
    expect(need("read-history", "uart0")).toBe("");
    // the original firmware is enough
    expect(need("read-telemetry", "uart0")).toBe("");
    // the CCL decrease step by its capability bit 4: from Full 1.6.7
    expect(need("read-ccl-step", "uart1")).toBe("ab EMU1101-EXT Full 1.6.7");
    expect(need("write-ccl-step", "uart1")).toBe("ab EMU1101-EXT Full 1.6.7");
    // every way with a firmware condition gets a text
    for (const r of ROUTES) if (r.firmware.length) expect(needText(de, routeNeed(r))).toMatch(/^ab EMU1101-EXT (Light 1\.5|Full 1\.6)\.[47]/);
  });
});

describe("the hard stop choice (EMU1101-EXT Full, Group0 bits 6/7)", () => {
  it("takes the build's default while bit 7 is 1 and keeps every lower bit when set", async () => {
    const { hardStopChoice, hardStopConfigurable, withHardStop } = await import("./access");
    expect([hardStopConfigurable(0xfe), hardStopConfigurable(0x32), hardStopConfigurable(null)]).toEqual([true, false, false]);
    // FF in the FE image: the default soft stop; in an FF image the default hard stop
    expect(hardStopChoice(0xff, 0xfe)).toEqual({ enabled: false, inherited: true });
    expect(hardStopChoice(0xff, 0xff)).toEqual({ enabled: true, inherited: true });
    expect(hardStopChoice(0x40, 0xfe)).toEqual({ enabled: true, inherited: false });
    expect(hardStopChoice(0x3f, 0xff)).toEqual({ enabled: false, inherited: false });
    // one switch writes both bits; bit 5 (lock state) and the others stay
    expect(withHardStop(0xdf, true)).toBe(0x5f);
    expect(withHardStop(0xff, false)).toBe(0x3f);
  });
});


describe("access facts as text (help and hints)", async () => {
  const { ACCESS_HINTS, PROBES, accessHint, accessText, wayConditions, factsOf } = await import("./access");
  const TEXT = (await import("./generated/access_text.json")).default as unknown as Record<string, Record<string, string>>;
  const SHARED = (await import("./i18n.access.json")).default as unknown as Record<string, unknown>;
  const CAT = (await import("./generated/access.json")).default;
  const ACTIONS = [...new Set(ROUTES.map((r) => r.action))];
  const tr = (lng: string) => i18n.getFixedT(lng);
  // the hints' own variables, filled with a neutral value
  const VARS = { version: "V" };
  const way = (action: string, channel: string, i = 0) => ROUTES.filter((r) => r.action === action && r.channel === channel)[i];
  const state = (action: string, channel: string, f: Partial<Facts>, i = 0) => decide(way(action, channel, i), factsOf(f)).state;

  it("words every action like the help generator (tools/access_help.py)", () => {
    for (const lng of ["de", "en"]) for (const a of ACTIONS) expect(accessText(tr(lng), [a]), `${lng} ${a}`).toBe(TEXT[lng][a]);
    expect(Object.keys(TEXT.de).sort()).toEqual([...ACTIONS].sort());
  });

  it("has every name and template in both languages", () => {
    const keys = (o: unknown, p = ""): string[] => Object.entries(o as Record<string, unknown>).flatMap(([k, v]) => (typeof v === "object" ? keys(v, `${p}${k}.`) : [`${p}${k}`]));
    expect(keys(SHARED.de)).toEqual(keys(SHARED.en));
    for (const lng of ["de", "en"]) {
      const has = (k: string) => expect(i18n.exists(`acc.${k}`, { lng }), `${lng} acc.${k}`).toBe(true);
      for (const a of ACTIONS) has(`act.${a}`);
      for (const r of ROUTES) {
        has(`channel.${r.channel}`);
        for (const f of r.firmware) {
          if (f.kind === "image-flag") has(`fw.flagName.b${f.bit}`);
          if (f.kind === "read-cap") for (let b = 0; b < 16; b++) if (f.mask & (1 << b)) has(`fw.capName.c${b}`);
        }
        for (const n of r.needs) if (n.kind === "bit-set" || n.kind === "bit-clear") has(`bitName.g${n.group}b${n.bit}`);
        if (r.yields) has(`doc.yields.${r.yields}`);
      }
      for (const bit of Object.values(CAT.flags)) has(`fw.flagName.b${bit}`);
      for (const bit of Object.values(CAT.caps)) has(`fw.capName.c${bit}`);
    }
  });

  it("fails loudly on a condition it has no words for", () => {
    const t = tr("de");
    const base = way("read-parameters", "uart0");
    expect(() => wayConditions(t, { ...base, needs: [{ kind: "new-kind" } as never] })).toThrow(/new-kind/);
    expect(() => wayConditions(t, { ...base, firmware: [{ kind: "new-firmware" } as never] })).toThrow(/new-firmware/);
    expect(() => accessText(t, ["no-such-action"])).toThrow(/no-such-action/);
  });

  it("fills every hint that restates catalog facts, and only those take {{access}}", () => {
    const takes = (lng: string) => {
      const out: string[] = [];
      const walk = (o: unknown, p: string) => { for (const [k, v] of Object.entries(o as Record<string, unknown>)) typeof v === "string" ? v.includes("{{access}}") && out.push(p + k) : walk(v, `${p}${k}.`); };
      walk(i18n.getResourceBundle(lng, "translation"), "");
      return out.sort();
    };
    for (const lng of ["de", "en"]) {
      expect(takes(lng)).toEqual(Object.keys(ACCESS_HINTS).sort());
      for (const k of Object.keys(ACCESS_HINTS) as (keyof typeof ACCESS_HINTS)[]) {
        const text = accessHint(tr(lng), k, ROUTES, VARS);
        expect(text, `${lng} ${k}`).not.toMatch(/\{\{|acc\.|undefined/);
        for (const a of ACCESS_HINTS[k]) expect(ACTIONS).toContain(a);
      }
    }
  });

  // the counter-examples a generic „geht“ would get wrong
  it("keeps the OEM master's String 1 apart from the patch's serial numbers", () => {
    const oemMaster = { patch: { kind: "none" }, base: [16, 6, 13], address: 0 } as Partial<Facts>;
    expect([state("read-serials", "uart1", oemMaster, 0), state("read-serials", "uart1", oemMaster, 1)]).toEqual(["blocked", "ready"]);
    const de = tr("de");
    expect(wayConditions(de, way("read-serials", "uart1", 0)).join(" ")).toMatch(/30 Zeichen/);
    expect(wayConditions(de, way("read-serials", "uart1", 1)).join(" ")).toMatch(/String 1.*22 Zeichen/);
    expect(accessText(tr("en"), ["read-serials"])).toMatch(/30 characters each.*String 1 only, 22 characters/);
    // the help keeps the scope too
    const page = Object.values(import.meta.glob("../../help/de/topic.buses.md", { query: "?raw", import: "default", eager: true }))[0] as string;
    expect(page).toMatch(/16\.06\.11; nur am Master, Adresse 0; liefert nur die Batterie-Seriennummer \(String 1\), die ersten 22 Zeichen/);
  });
  it("reads the clock over the CAN socket without the bit 3 release, sets it only with it", () => {
    const light = { patch: { kind: "version", version: [1, 5] }, flags: 0x3a, caps: 0x0f, group0: 0xf7, installation: true, address: 2 } as Partial<Facts>;
    expect([state("read-clock", "uart1", light), state("set-clock", "uart1", light)]).toEqual(["ready", "blocked"]);
    expect(accessText(tr("de"), ["read-clock"])).not.toMatch(/Bit3/);
    expect(accessText(tr("de"), ["set-clock"])).toMatch(/Freigabe Power\/RTC\)/);
  });
  it("names bit 3 of a restart over the CAN socket without a value: EXT and TEMP read it the other way round", () => {
    const ext = (g0: number) => ({ patch: { kind: "version", version: [1, 6] }, flags: 0xfe, group0: g0, installation: true, address: 1 }) as Partial<Facts>;
    const temp = (g0: number) => ({ ...ext(g0), flags: 0x06 }) as Partial<Facts>;
    expect([state("restart", "uart1", ext(0xff)), state("restart", "uart1", ext(0xf7))]).toEqual(["ready", "blocked"]);
    expect([state("restart", "uart1", temp(0xf7)), state("restart", "uart1", temp(0xff))]).toEqual(["ready", "blocked"]);
    // the hint names the release, never a bit or a value; the help names the bit and refers to the switch for the value
    for (const lng of ["de", "en"]) expect(accessText(tr(lng), ["restart"])).not.toMatch(/[Bb]it ?\d|= [01]|Group/);
    const page = Object.values(import.meta.glob("../../help/de/topic.control.md", { query: "?raw", import: "default", eager: true }))[0] as string;
    expect(page).toMatch(/Freigabe in Group0\.Bit3 \(Power\/RTC\), Wert je Build/);
    expect(page).not.toMatch(/Bit3[^.;]*= [01]/);
  });
  it("gives Light no stored pack lock", () => {
    const light = { patch: { kind: "version", version: [1, 5] }, flags: 0x3a, caps: 0x0f, group0: 0xff, installation: true, address: 1 } as Partial<Facts>;
    expect([state("pack-lock-set", "uart0", light), state("pack-lock-set", "uart1", light)]).toEqual(["blocked", "blocked"]);
    expect(accessText(tr("de"), ["pack-lock-set"])).toMatch(/EMU1101-EXT: Packsperre/);
    expect(Object.values(import.meta.glob("../../help/de/topic.control.md", { query: "?raw", import: "default", eager: true }))[0]).toMatch(/Full 1\.6\.4 mit Packsperre/);
  });
  it("needs the read capability, not the version: caps 0000 on a supported build", () => {
    const noCaps = { patch: { kind: "version", version: [1, 5] }, flags: 0x3a, caps: 0, address: 1 } as Partial<Facts>;
    expect([state("read-history", "uart1", noCaps), state("read-calibration", "uart1", noCaps), state("read-history", "uart0", noCaps)]).toEqual(["blocked", "blocked", "ready"]);
    expect(accessText(tr("de"), ["read-soc-store"])).toMatch(/EMU1101-EXT: gespeicherter Stand oder Energiezähler/);
  });
  it("names UART2 in the help but never offers it in a hint", () => {
    expect(ROUTES.filter((r) => r.channel === "uart2").every((r) => !r.implemented)).toBe(true);
    for (const lng of ["de", "en"]) for (const a of ACTIONS) expect(accessText(tr(lng), [a])).not.toMatch(/UART2/);
    const page = Object.values(import.meta.glob("../../help/de/topic.access.md", { query: "?raw", import: "default", eager: true }))[0] as string;
    expect(page).toMatch(/UART2 · Display\/Bluetooth: .*die App nutzt diesen Weg nicht/);
  });
  it("words the identity as a detection query, not as an OEM function", () => {
    expect(PROBES.has("read-identity")).toBe(true);
    for (const lng of ["de", "en"]) {
      const text = accessText(tr(lng), ["read-identity"]);
      expect(text).toContain(tr(lng)("acc.doc.short.probe"));
      expect(text).not.toMatch(/Originalfirmware|original firmware/);
      // patched firmware, not EXT alone: the supported first MAINT 1.6 (MNT format 3) answers it too
      expect(tr(lng)("acc.probe")).toMatch(/Patch-Firmware|patched firmware/);
      expect(tr(lng)("acc.probe")).not.toMatch(/EXT/);
    }
    // TEMP(maint-1.6): the first MAINT 1.6 (patch 1.6, flags 06) is asked over both ports like an EXT build
    const temp = { patch: { kind: "version", version: [1, 6] }, flags: 0x06, caps: null, address: 1 } as Partial<Facts>;
    expect([state("read-identity", "uart1", temp), state("read-identity", "uart0", temp)]).toEqual(["ready", "blocked"]);
  });
  it("leaves a way unclear without flags and caps, even with the switches read", () => {
    const f = factsOf({ patch: { kind: "version", version: [1, 5] }, group0: 0xff, group7: 0x00, address: 1 });
    const d = decide(way("read-calibration", "uart1"), f);
    expect(d.state).toBe("unknown");
    expect(d.reasons).toEqual([{ id: "firmware", need: { kind: "read-cap", mask: 2 }, known: "unknown" }]);
    expect(reasonText(tr("de"), d.reasons[0])).toBe("Lesefähigkeit Kalibrierseiten nicht belegt");
  });

  it("leaves no placeholder in the generated help and parameter file comments", () => {
    const pages = import.meta.glob("../../help/*/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
    const left = /\{\s*(?:ACCESS|MIN|BUILDS)|\{\{/i;
    expect(Object.entries(pages).filter(([, text]) => left.test(text)).map(([p]) => p)).toEqual([]);
    expect(JSON.stringify(PARAM_FILE)).not.toMatch(left);
  });

  // A route removed from the catalog must change every claim about it: the hints' own prose names no port or firmware
  // (except the three that say this pack lacks a CAN socket way), so only the generated part carries those facts.
  it("keeps the hints' prose free of port and firmware claims, so a removed way leaves no claim behind", () => {
    const negative = new Set(["clock.canBus", "bmsh.canBus", "ctl.canUnsupported"]);
    for (const lng of ["de", "en"]) {
      const t = tr(lng);
      for (const k of Object.keys(ACCESS_HINTS) as (keyof typeof ACCESS_HINTS)[]) {
        const prose = t(k, { ...VARS, access: "" });
        expect(prose, `${lng} ${k}`).not.toMatch(/EMU1101|Patch-Firmware|patched firmware|RS485|UART|Bit ?\d|bit ?\d/);
        if (!negative.has(k)) expect(prose, `${lng} ${k}`).not.toMatch(/CAN-Buchse|CAN socket/);
        else expect(prose, `${lng} ${k}`).toMatch(/nicht|not/);
        // every port the hint names comes from a way of its actions: without those ways, the port is gone from it
        const acts: readonly string[] = ACCESS_HINTS[k];
        const channels = [...new Set(ROUTES.filter((r) => acts.includes(r.action) && r.implemented).map((r) => r.channel))];
        expect(channels.length, `${lng} ${k}`).toBeGreaterThan(0);
        for (const ch of channels) {
          const port = t(`acc.doc.port.${ch}`);
          const fewer = ROUTES.filter((r) => !(r.channel === ch && acts.includes(r.action)));
          expect(accessHint(t, k, ROUTES, VARS), `${lng} ${k}`).toContain(port);
          if (!prose.includes(port)) expect(accessHint(t, k, fewer, VARS), `${lng} ${k} without ${ch}`).not.toContain(port);
        }
      }
    }
  });
  it("keeps every hint short: one or two lines, no bit numbers or versions", () => {
    for (const lng of ["de", "en"]) for (const k of Object.keys(ACCESS_HINTS) as (keyof typeof ACCESS_HINTS)[]) {
      const text = accessHint(tr(lng), k, ROUTES, VARS);
      expect(text.length, `${lng} ${k}: ${text}`).toBeLessThanOrEqual(240);
      expect(text, `${lng} ${k}`).not.toMatch(/Group\d|[Bb]it ?\d|\d\.\d\.\d/);
    }
  });
});

describe("the wording of a capability bit", () => {
  it("names the CCL bit a capability and keeps the reads read capabilities, in the reasons too", async () => {
    const { capKind } = await import("./access");
    expect([capKind(1 << 4), capKind(0x0F), capKind(1 << 1)]).toEqual(["feature", "read", "read"]);
    expect(() => capKind(0x11)).toThrow();
    const de = i18n.getFixedT("de"), en = i18n.getFixedT("en");
    const r = (mask: number, known: "no" | "unknown") => ({ id: "firmware", need: { kind: "read-cap", mask }, known }) as Parameters<typeof reasonText>[1];
    expect(reasonText(de, r(0x10, "no"))).toBe("Fähigkeit CCL-Abbaustufe meldet dieses Pack nicht");
    expect(reasonText(en, r(0x10, "unknown"))).toBe("capability CCL decrease step not proven");
    expect(reasonText(de, r(0x02, "no"))).toBe("Lesefähigkeit Kalibrierseiten meldet dieses Pack nicht");
    expect(reasonText(en, r(0x02, "unknown"))).toBe("read capability calibration pages not proven");
  });
});
