import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import { connectedForOther, endBeforeSwitch, explain, fillFields, firstLine, noAnswer, partialLine } from "./connCheck";

const t = i18n.getFixedT("de");
const none = { answered: null, noise: false, intra: false, pack_at_9600: false };

describe("connection check", () => {
  it("reads the backend's no-answer error and nothing else", () => {
    expect(noAnswer('{"kind":"no-answer","noise":true,"intra":false}')).toEqual({ noise: true, intra: false, busy: false });
    expect(noAnswer(new Error('{"kind":"no-answer","noise":false,"intra":true}'))).toEqual({ noise: false, intra: true, busy: false });
    // a bus that never got quiet: nothing was sent, said apart from silence
    const busy = noAnswer('{"kind":"no-answer","noise":false,"intra":false,"busy":true}')!;
    expect(busy.busy).toBe(true);
    expect(firstLine("can", busy, t)).not.toBe(firstLine("can", { noise: false, intra: false }, t));
    expect(noAnswer("serial port: busy")).toBeNull();
    expect(noAnswer('{"kind":"installation-busy"}')).toBeNull();
  });

  it("offers the CAN socket when a pack answers there", () => {
    const r = explain("pack", { noise: false, intra: false }, { ...none, answered: 0 }, t);
    expect(r.offer).toBe("can");
    expect(r.text).toContain("Über den RS485-1/2-Bus (19200 Baud)");
    expect(r.text).toContain("CAN-Buchse (RS485, 9600 Baud) antwortet Pack 00");
  });

  it("offers RS485-1/2 when a pack answers at 19200 baud", () => {
    const r = explain("can", { noise: true, intra: false }, { ...none, answered: 3 }, t);
    expect(r.offer).toBe("pack");
    expect(r.text).toContain("nicht lesbar");
    expect(r.text).toContain("Pack 03");
  });

  it("an answer at 9600 baud from RS485-1/2 (old firmware) is no CAN socket and offers nothing", () => {
    const r = explain("pack", { noise: false, intra: false }, { ...none, answered: 1, pack_at_9600: true }, t);
    expect(r.offer).toBeNull();
    expect(r.text).toContain("16.04.06");
  });

  it("nothing anywhere: names the states, not only the port", () => {
    const r = explain("pack", { noise: false, intra: false }, none, t);
    expect(r.offer).toBeNull();
    expect(r.text).toContain("Auch mit 9600 Baud");
    expect(r.text).toContain("Bootloader");
  });

  it("a polling master is said as such", () => {
    expect(explain("pack", { noise: false, intra: true }, none, t).text).toContain("Master fragt hier seine Slaves ab");
  });

  it("a gateway gets the gateway hint, no probe and no offer", () => {
    const r = explain("can", { noise: false, intra: false }, null, t);
    expect(r.offer).toBeNull();
    expect(r.text).toContain("Port des Gateways");
  });

  it("names the silent packs of a fixed count", () => {
    expect(partialLine([0, 1, 2], [0, 1], t)).toBe("2 von 3 Packs antworten, keine Antwort von 02.");
    expect(partialLine([0, 1], [0, 1], t)).toBe("");
  });
});

describe("last connection per installation", () => {
  it("keeps what worked per installation and ignores broken entries", async () => {
    const { lastConn, saveLastConn } = await import("./connCheck");
    const store = new Map<string, string>();
    globalThis.localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) } as Storage;
    saveLastConn("inst-a", { kind: "tcp", target: "192.0.2.10:4196", bus: "can", packs: 0 });
    saveLastConn("inst-b", { kind: "serial", target: "/dev/cu.test", bus: "pack", packs: 1 });
    expect(lastConn("inst-a")).toEqual({ kind: "tcp", target: "192.0.2.10:4196", bus: "can", packs: 0 });
    expect(lastConn("inst-b")?.bus).toBe("pack");
    expect(lastConn("inst-c")).toBeNull();
    localStorage.setItem("conn.last.inst-c", "{\"kind\":\"usb\",\"target\":\"x\",\"bus\":\"can\",\"packs\":1}");
    expect(lastConn("inst-c")).toBeNull();
    localStorage.setItem("conn.last.inst-c", "nonsense");
    expect(lastConn("inst-c")).toBeNull();
  });
});

describe("sidebar fields after a change of installation or bus", () => {
  const can = { kind: "tcp" as const, addr: "192.0.2.10:4196" };
  const both = { control: can, endpoints: ["serial:/dev/cu.test"] };
  const last = (bus: "can" | "pack", kind: "tcp" | "serial" = "tcp", target = "192.0.2.99:4196") => ({ kind, target, bus, packs: 4 });
  it("the stored port of the chosen bus wins over the remembered connection; that gives bus and pack count", () => {
    expect(fillFields(both, "can", null, "installation")).toEqual({ kind: "tcp", target: "192.0.2.10:4196", bus: "can", packs: null });
    expect(fillFields(both, "can", last("pack"), "installation")).toEqual({ kind: "serial", target: "/dev/cu.test", bus: "pack", packs: { mode: "count", n: 4 } });
    expect(fillFields(both, "pack", last("can"), "installation")).toEqual({ kind: "tcp", target: "192.0.2.10:4196", bus: "can", packs: { mode: "count", n: 4 } });
  });
  it("the remembered address only where no port is stored for that bus", () => {
    const canOnly = { control: can, endpoints: [] };
    expect(fillFields(canOnly, "can", last("pack", "serial", "/dev/cu.other"), "installation")).toEqual({ kind: "serial", target: "/dev/cu.other", bus: "pack", packs: { mode: "count", n: 4 } });
    // a bus change takes the remembered address of that bus only, never its pack count
    expect(fillFields(canOnly, "pack", last("pack", "serial", "/dev/cu.other"), "bus")).toEqual({ kind: "serial", target: "/dev/cu.other", bus: "pack", packs: null });
    expect(fillFields(canOnly, "pack", last("can"), "bus")).toEqual({ kind: "serial", target: "", bus: "pack", packs: null });
  });
  it("without a stored port or a remembered one: an empty USB field (a new installation takes its CAN socket)", () => {
    const none = { control: null, endpoints: [] };
    expect(fillFields(none, "pack", null, "installation")).toEqual({ kind: "serial", target: "", bus: "pack", packs: null });
    expect(fillFields(none, "can", null, "bus")).toEqual({ kind: "serial", target: "", bus: "can", packs: null });
    expect(fillFields({ control: can, endpoints: [] }, "pack", null, "installation")).toEqual({ kind: "tcp", target: "192.0.2.10:4196", bus: "can", packs: null });
    // the RS485-1/2 port by the shared rule: the first endpoint that is not the CAN socket, also a gateway
    expect(fillFields({ control: can, endpoints: ["tcp:192.0.2.10:4196", "tcp:192.0.2.11:4196"] }, "pack", null, "bus")).toEqual({ kind: "tcp", target: "192.0.2.11:4196", bus: "pack", packs: null });
    // „Nur ansehen“: nothing changes
    expect(fillFields(null, "pack", null, "installation")).toBeNull();
  });
});

describe("switching the installation while connecting", () => {
  it("cancels the running connect and waits for it before the choice changes", async () => {
    const steps: string[] = [];
    let finish = () => {};
    const run = new Promise<void>((r) => { finish = () => { steps.push("connect ended"); r(); }; });
    const switching = endBeforeSwitch({ running: () => run, cancel: () => { steps.push("cancel"); setTimeout(finish, 5); }, live: () => false, disconnect: async () => { steps.push("disconnect"); } })
      .then(() => steps.push("switch"));
    await switching;
    expect(steps).toEqual(["cancel", "connect ended", "switch"]);
  });
  it("ends a live connection, and does nothing without one", async () => {
    const steps: string[] = [];
    await endBeforeSwitch({ running: () => null, cancel: () => steps.push("cancel"), live: () => true, disconnect: async () => { steps.push("disconnect"); } });
    await endBeforeSwitch({ running: () => null, cancel: () => steps.push("cancel"), live: () => false, disconnect: async () => { steps.push("disconnect"); } });
    expect(steps).toEqual(["disconnect"]);
  });
  it("a connect that returns for another installation than the chosen one is closed", () => {
    expect(connectedForOther("inst-a", "inst-b")).toBe(true);
    expect(connectedForOther("inst-a", "inst-a")).toBe(false);
    expect(connectedForOther(null, null)).toBe(false);
  });
});

describe("a fixed pack count", () => {
  it("keeps its fast path when its addresses answer, else searches", async () => {
    const { fixedFallback, fixedSearchedLine } = await import("./connCheck");
    const { default: i18n } = await import("./i18n");
    // the connect check answered at 01, one pack set (address 00): search at once
    expect(fixedFallback([0], 1, null)).toBe("search");
    expect(fixedFallback([0, 1, 2], 1, null)).toBe("keep");
    // the device query of the fixed addresses found none of them: search
    expect(fixedFallback([0], 0, [])).toBe("search");
    // „Anzahl n“ incomplete: search (a pack may sit outside 0..n-1, e.g. addresses 1–12 with „Anzahl 12“)
    expect(fixedFallback([0, 1], 0, [1])).toBe("search");
    expect(fixedFallback(Array.from({ length: 12 }, (_, i) => i), 1, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11])).toBe("search");
    expect(fixedFallback([0, 1], 0, [0, 1])).toBe("keep");
    const de = i18n.getFixedT("de");
    expect(fixedSearchedLine([0], [1], de)).toBe("Keine Antwort an Adresse 00; gesucht: Pack 01 gefunden.");
    expect(fixedSearchedLine([0, 1, 2], [], de)).toBe("Keine Antwort an Adresse 00–02; gesucht: kein Pack gefunden.");
    expect(fixedSearchedLine([0, 5], [1, 12], de)).toBe("Keine Antwort an Adresse 00, 05; gesucht: Packs 01, 12 gefunden.");
    expect(fixedSearchedLine([0], [3, 4], i18n.getFixedT("en"))).toBe("No answer at address 00; searched: packs 03, 04 found.");
  });
});

describe("which packs a connection polls", () => {
  it("search, a count or one fixed address, and the old stored count migrates", async () => {
    const { parseSel, selAddresses, selText } = await import("./connCheck");
    expect(selAddresses({ mode: "search" })).toBeNull();
    expect(selAddresses({ mode: "count", n: 3 })).toEqual([0, 1, 2]);
    expect(selAddresses({ mode: "address", k: 1 })).toEqual([1]);
    for (const sel of [{ mode: "search" as const }, { mode: "count" as const, n: 12 }, { mode: "address" as const, k: 15 }]) expect(parseSel(selText(sel))).toEqual(sel);
    // migration of conn.packs: 0 → search, n → count n
    expect(parseSel(null, "0")).toEqual({ mode: "search" });
    expect(parseSel(null, "4")).toEqual({ mode: "count", n: 4 });
    expect(parseSel(null, null)).toEqual({ mode: "search" });
    // out of range or broken: search
    expect(parseSel("address:16")).toEqual({ mode: "search" });
    expect(parseSel("count:0")).toEqual({ mode: "search" });
    expect(parseSel("x")).toEqual({ mode: "search" });
  });
  it("a fixed address keeps its fast path, else searches", async () => {
    const { fixedFallback, fixedSearchedLine } = await import("./connCheck");
    const { default: i18n } = await import("./i18n");
    expect(fixedFallback([1], 1, null)).toBe("keep");
    expect(fixedFallback([1], 3, null)).toBe("search");
    expect(fixedFallback([1], 1, [], "address")).toBe("search");
    expect(fixedFallback([1], 1, [1], "address")).toBe("keep");
    expect(fixedSearchedLine([1], [3], i18n.getFixedT("de"))).toBe("Keine Antwort an Adresse 01; gesucht: Pack 03 gefunden.");
  });
  it("a remembered connection gives its selection, also an old one with a count", async () => {
    const { fillFields } = await import("./connCheck");
    const inst = { control: null, endpoints: [] };
    expect(fillFields(inst, "pack", { kind: "serial", target: "/dev/x", bus: "pack", sel: "address:1" }, "installation")?.packs).toEqual({ mode: "address", k: 1 });
    expect(fillFields(inst, "pack", { kind: "serial", target: "/dev/x", bus: "pack", packs: 2 }, "installation")?.packs).toEqual({ mode: "count", n: 2 });
  });
});

describe("after a fallback search", () => {
  it("keeps the expected packs and adds the ones found elsewhere", async () => {
    const { mergeAfterSearch } = await import("./connCheck");
    const { default: i18n } = await import("./i18n");
    // „Anzahl 12“ (0–11), pack 5 silent at connect, pack 12 found outside: 5 stays polled, 12 joins
    const twelve = Array.from({ length: 12 }, (_, i) => i);
    expect(mergeAfterSearch(twelve, [0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12])).toEqual([...twelve, 12]);
    // „feste Adresse 0“ silent, a pack at 1: both polled
    expect(mergeAfterSearch([0], [1])).toEqual([0, 1]);
    expect(mergeAfterSearch([2, 1], [1, 3])).toEqual([1, 2, 3]);
    expect(i18n.t("conn.expectedSilent", { count: 1, list: "05", lng: "de" })).toBe("Pack 05 ohne aktuelle Antwort, wird weiter abgefragt.");
  });
});

describe("the connect's gap note", () => {
  it("names only the packs still silent and is gone once all answer", async () => {
    const { gapLine } = await import("./connCheck");
    const twelve = Array.from({ length: 12 }, (_, i) => i);
    // device query missed 1, 4, 9, 10; the search found 0, 3, 4, 6, 9, 10 (busy bus): 1 stays unanswered
    const g = { expected: twelve, missing: [1, 4, 9, 10], searched: [0, 3, 4, 6, 9, 10] };
    expect(gapLine(g, [], t)).toBe("Keine Antwort an Adresse 01, 04, 09, 10; gesucht: Packs 00, 03, 04, 06, 09, 10 gefunden. Pack 01 ohne aktuelle Antwort, wird weiter abgefragt.");
    // packs that answered the device query are never called silent; 4, 9, 10 answering by now drop out
    expect(gapLine(g, [0, 2, 3, 4, 5, 9, 10], t)).toBe("Keine Antwort an Adresse 01; gesucht: Packs 00, 03, 04, 06, 09, 10 gefunden. Pack 01 ohne aktuelle Antwort, wird weiter abgefragt.");
    expect(gapLine(g, twelve, t)).toBe("");
    // without a search: the partial line shrinks the same way
    const p = { expected: [0, 1, 2], missing: [1, 2], searched: null };
    expect(gapLine(p, [0, 2], t)).toBe(partialLine([0, 1, 2], [0, 2], t));
    expect(gapLine(p, [0, 1, 2], t)).toBe("");
  });
});

describe("a gap after the packs answered", () => {
  it("drops answered packs for good, so an emptied monitor does not bring them back", async () => {
    const { gapLine, pruneGap } = await import("./connCheck");
    const g = { expected: [0, 1, 2], missing: [1, 2], searched: [0] };
    const after = pruneGap(g, [0, 1]);
    expect(after).toEqual({ ...g, missing: [2] });
    // the monitor stopped for maintenance: no fresh values, pack 1 stays out of the note
    expect(gapLine(after!, [], t)).not.toContain("01");
    expect(pruneGap(after!, [2])).toBeNull();
    expect(pruneGap(g, [0])).toBe(g);
  });
});

describe("MQTT per installation", () => {
  const ms = (over = {}) => ({ host: "broker.example", port: 1883, user: "u", password: "p", tls: false, base: "seplos", ...over });

  it("saves a first broker, keeps an identical one, asks before replacing another", async () => {
    const { mqttSaveAction } = await import("./connCheck");
    expect(mqttSaveAction(null, ms())).toBe("save");
    expect(mqttSaveAction(ms(), ms())).toBe("none");
    expect(mqttSaveAction(ms(), ms({ host: "other.example" }))).toBe("ask");
    expect(mqttSaveAction(ms(), ms({ password: "new" }))).toBe("ask");
  });

  it("preselects the way last used with the installation, else MQTT only when it is all it has", async () => {
    const { initialWay } = await import("./connCheck");
    expect(initialWay("mqtt", false, true)).toBe("mqtt");
    expect(initialWay("tcp", true, false)).toBe("bus");
    expect(initialWay(null, true, false)).toBe("mqtt");
    expect(initialWay(null, true, true)).toBe("bus");
    expect(initialWay(null, false, false)).toBe("bus");
  });
});
