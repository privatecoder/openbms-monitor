import { describe, expect, it, vi } from "vitest";
import { isValidElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import "./i18n";
import i18n from "i18next";
import { HelpBody, helpComponents, helpTarget, helpTitle } from "./help";

const PAGE = import.meta.glob("../../help/de/guide.firmware.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const guide = Object.values(PAGE)[0].replace(/^---\n[\s\S]*?\n---\n/, "");

describe("help pages", () => {
  it("numbered steps and bullet lists render apart, nested case lists included", async () => {
    await i18n.changeLanguage("de");
    const html = renderToStaticMarkup(<HelpBody body={"1. one\n2. two\n   - **A**\n     1. inner\n   - **B**\n     1. other\n\n- plain"} onOpen={() => {}} />);
    expect(html).toMatch(/^<div[^>]*>\s*<ol class="[^"]*list-decimal[^"]*">\s*<li>one<\/li>/);
    expect(html).toMatch(/<ul class="[^"]*list-disc[^"]*">\s*<li><strong>A<\/strong>\s*<ol class="[^"]*list-decimal/);
    expect(html).toMatch(/<\/ol>\s*<ul class="[^"]*list-disc[^"]*">\s*<li>plain/);
    expect(html).not.toMatch(/<ol[^>]*list-disc|<ul[^>]*list-decimal/);
    // a real guide: its top level is one numbered list with case lists inside
    const page = renderToStaticMarkup(<HelpBody body={guide} onOpen={() => {}} />);
    expect(page.match(/<ol /g)!.length).toBeGreaterThan(5);
    expect(page).toContain("<ul ");
  });

  it("an internal link opens its page with the target's title; other links stay links", async () => {
    await i18n.changeLanguage("de");
    expect(helpTarget("#topic.maintenance")).toBe("topic.maintenance");
    expect(helpTarget("#topic.noSuch")).toBeNull();
    expect(helpTarget("https://example.org/#topic.maintenance")).toBeNull();
    const html = renderToStaticMarkup(<HelpBody body={"See [x](#topic.maintenance) and [docs](https://example.org/)."} onOpen={() => {}} />);
    expect(html).toContain(`<button type="button"`);
    expect(html).toContain(`>${helpTitle("topic.maintenance", "de")}</button>`);
    expect(html).not.toContain(">x<");
    expect(html).toContain(`<a href="https://example.org/">docs</a>`);
    expect(guide).toMatch(/\]\(#topic\.maintenance\)/);

    const onOpen = vi.fn();
    const a = helpComponents(onOpen, "en").a as (p: { href?: string; children?: unknown }) => ReactElement<{ onClick: () => void; children: unknown }>;
    const link = a({ href: "#topic.firmware", children: "ignored" });
    expect(isValidElement(link)).toBe(true);
    expect(link.props.children).toBe(helpTitle("topic.firmware", "en"));
    link.props.onClick();
    expect(onOpen).toHaveBeenCalledWith("topic.firmware");
  });
});
