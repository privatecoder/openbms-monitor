import type { TFunction } from "i18next";
import type { AccessDecision, AccessReason, Channel } from "./api";

export const CHANNELS: Channel[] = ["uart0", "uart1", "uart2"];

/** The matrix as rows: one per action, in the backend's order, with its ways per channel (none where the firmware has
 *  none; the CAN socket may have two, e.g. the binary serial read and the master's Modbus String 1). */
export function accessRows(decisions: AccessDecision[]): { action: string; cells: Partial<Record<Channel, AccessDecision[]>> }[] {
  const rows: { action: string; cells: Partial<Record<Channel, AccessDecision[]>> }[] = [];
  for (const d of decisions) {
    let row = rows.find((r) => r.action === d.route.action);
    if (!row) rows.push((row = { action: d.route.action, cells: {} }));
    (row.cells[d.route.channel] ??= []).push(d);
  }
  return rows;
}

const rank = { ready: 0, unknown: 1, blocked: 2, "not-implemented": 3 } as const;
/** The state a cell shows: its best way. */
export const bestState = (ds: AccessDecision[]) => ds.reduce((b, d) => (rank[d.state] < rank[b] ? d.state : b), ds[0].state);

/** One reason as text (`acc.reason.*`). */
export function reasonText(t: TFunction, r: AccessReason): string {
  switch (r.id) {
    case "firmware": return t(`acc.fw.${r.need}.${r.known}`);
    case "bit": {
      const name = t(`acc.bitName.g${r.group}b${r.bit}`, { defaultValue: "" });
      return t(r.known === null ? "acc.bitUnread" : "acc.bitWrong", { g: r.group, b: r.bit, want: r.want ? 1 : 0, now: r.known ? 1 : 0, name: name ? ` (${name})` : "" });
    }
    case "with-slaves": return t(r.known === null ? "acc.slavesUnknown" : "acc.slavesNone");
    case "marker": return t("acc.marker", { want: t(`acc.markerState.${r.on ? "on" : "off"}`), now: t(`acc.markerState.${r.known ?? "unread"}`) });
    case "base": return t(r.known ? "acc.baseLow" : "acc.baseUnknown", { from: r.from.map((n) => String(n).padStart(2, "0")).join("."), now: r.known?.map((n) => String(n).padStart(2, "0")).join(".") });
    default: return t(`acc.reason.${r.id}`);
  }
}

/** The tooltip of a cell: per way its command and state, then every reason on its own line. */
export const cellTitle = (t: TFunction, ds: AccessDecision[]) =>
  ds.map((d) => [`${d.route.wire}: ${t(`acc.state.${d.state}`)}`, ...d.reasons.map((r) => `· ${reasonText(t, r)}`)].join("\n")).join("\n\n");
