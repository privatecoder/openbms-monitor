import { useSyncExternalStore } from "react";
import type { FeedState, Source } from "./api";

type T = (key: string, params?: Record<string, unknown>) => string;

/** The source of the current connection (`connect`: "bus", `connectMqtt`: "mqtt", null: not connected) and the state
 *  of the add-on's feed while the source is MQTT. */
let source: Source | null = null;
let feed: FeedState | null = null;
const listeners = new Set<() => void>();
const tell = () => { for (const l of listeners) l(); };
const subscribe = (l: () => void) => { listeners.add(l); return () => { listeners.delete(l); }; };

export function setSource(s: Source | null) {
  source = s;
  if (s !== "mqtt") feed = null;
  tell();
}

export function setFeed(f: FeedState | null) {
  feed = f;
  tell();
}

export const getSource = () => source;
export const getFeed = () => feed;
export const useSource = (): Source | null => useSyncExternalStore(subscribe, getSource, getSource);
export const useFeed = (): FeedState | null => useSyncExternalStore(subscribe, getFeed, getFeed);

/** A clock offset the status line names (ms). */
export const SKEW_SHOWN_MS = 5_000;

/** The status line under an MQTT connection; null while the feed is live and the clocks agree. */
export function feedText(f: FeedState | null, t: T): string | null {
  if (!f) return null;
  if (f.version_error) return t("feed.version", { err: f.version_error });
  if (f.state === "denied") return t("feed.denied");
  if (f.state === "broker") return t("feed.broker", { err: f.link_error ?? "" });
  if (f.state === "offline") return t("feed.offline");
  if (f.state === "off") return t("feed.off");
  if (f.state === "waiting") return t("feed.waiting");
  if (f.skew_ms !== null && Math.abs(f.skew_ms) > SKEW_SHOWN_MS) return t("feed.skew", { n: Math.round(f.skew_ms / 1000) });
  return null;
}
