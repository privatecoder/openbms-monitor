import { describe, expect, it } from "vitest";
import type { FeedState } from "./api";
import { feedText } from "./source";

const t = (k: string, p?: Record<string, unknown>) => (p ? `${k}:${JSON.stringify(p)}` : k);
const feed = (over: Partial<FeedState>): FeedState =>
  ({ state: "live", boot: "b1", boot_changed: false, skew_ms: 0, version_error: null, link_error: null, packs: [], ...over });

describe("feed status line", () => {
  it("names off, offline, waiting, a clock offset and a version error; nothing while live", () => {
    expect(feedText(null, t)).toBeNull();
    expect(feedText(feed({}), t)).toBeNull();
    expect(feedText(feed({ state: "off" }), t)).toBe("feed.off");
    expect(feedText(feed({ state: "offline" }), t)).toBe("feed.offline");
    expect(feedText(feed({ state: "denied" }), t)).toBe("feed.denied");
    expect(feedText(feed({ state: "broker", link_error: "payload too big" }), t)).toBe('feed.broker:{"err":"payload too big"}');
    expect(feedText(feed({ state: "waiting" }), t)).toBe("feed.waiting");
    expect(feedText(feed({ skew_ms: -120_400 }), t)).toBe('feed.skew:{"n":-120}');
    expect(feedText(feed({ skew_ms: 4_000 }), t)).toBeNull();
    expect(feedText(feed({ version_error: "feed version 2 not supported" }), t)).toBe('feed.version:{"err":"feed version 2 not supported"}');
  });
});
