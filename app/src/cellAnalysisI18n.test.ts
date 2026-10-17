import { describe, expect, it } from "vitest";
import i18n from "./i18n";
import type { CellClass, CellReason } from "./api";

const source = Object.values(import.meta.glob("./views/CellAnalysis.tsx", { query: "?raw", import: "default", eager: true }) as Record<string, string>)[0];

describe("cell analysis texts", () => {
  it("every literal key, class, reason and period exists in German and English", () => {
    const used = new Set([...source.matchAll(/["`](cell\.[A-Za-z0-9.]+)["`]/g)].map((m) => m[1]));
    const classes: CellClass[] = ["ok", "rest_ok", "soc_offset", "rest_offset", "capacity", "ambiguous", "resistance", "measurement", "none"];
    const reasons: CellReason["key"][] = ["within", "partial", "rest_only", "rest", "same_sign", "capacity", "ambiguous", "resistance", "offset", "jumps", "no_rest",
      "top_only", "bottom_only", "pattern", "r_slight"];
    classes.forEach((c) => used.add(`cell.class.${c}`));
    reasons.forEach((r) => used.add(`cell.reason.${r}`));
    ["1", "7", "30", "90"].forEach((p) => used.add(`cell.period.${p}`));
    ["top", "bottom", "r"].forEach((m) => used.add(`cell.missing.${m}`));
    expect(used.size).toBeGreaterThan(45);
    for (const lng of ["de", "en"]) for (const k of used) expect(i18n.exists(k, { lng }) || i18n.exists(`${k}_other`, { lng }), `${lng} ${k}`).toBe(true);
  });
});
