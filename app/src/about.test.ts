import { describe, expect, it } from "vitest";
import pkg from "../package.json";
import changelog from "../../CHANGELOG.md?raw";
import { changelogBody } from "./views/About";

describe("about view", () => {
  it("the changelog has a section for the app's version", () => {
    expect(changelog).toMatch(new RegExp(`^## ${pkg.version.replace(/\./g, "\\.")} — \\d{4}-\\d{2}-\\d{2}$`, "m"));
  });
  it("the shown changelog starts at the first version heading", () => {
    expect(changelogBody("# Changelog\n\nintro\n\n## Unreleased\n\n- a\n")).toBe("## Unreleased\n\n- a\n");
    expect(changelogBody("no headings")).toBe("no headings");
  });
});
