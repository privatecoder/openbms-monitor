import type { TFunction } from "i18next";
import type { AccessDecision, AccessReason, Channel } from "./api";

export const CHANNELS: Channel[] = ["uart0", "uart1", "uart2"];

/** The matrix as rows: one per action, in the backend's order, with its way per channel (none where the firmware has none). */
export function accessRows(decisions: AccessDecision[]): { action: string; cells: Partial<Record<Channel, AccessDecision>> }[] {
  const rows: { action: string; cells: Partial<Record<Channel, AccessDecision>> }[] = [];
  for (const d of decisions) {
    let row = rows.find((r) => r.action === d.route.action);
    if (!row) rows.push((row = { action: d.route.action, cells: {} }));
    row.cells[d.route.channel] = d;
  }
  return rows;
}

/** One reason as text (`acc.reason.*`). */
export function reasonText(t: TFunction, r: AccessReason): string {
  switch (r.id) {
    case "firmware": return t(`acc.fw.${r.need}.${r.known}`);
    case "bit": {
      const name = t(`acc.bitName.g${r.group}b${r.bit}`, { defaultValue: "" });
      return t(r.known === null ? "acc.bitUnread" : "acc.bitWrong", { g: r.group, b: r.bit, want: r.want ? 1 : 0, now: r.known ? 1 : 0, name: name ? ` (${name})` : "" });
    }
    default: return t(`acc.reason.${r.id}`);
  }
}

/** The tooltip of a cell: the command, then every reason on its own line. */
export const cellTitle = (t: TFunction, d: AccessDecision) => [d.route.wire, ...d.reasons.map((r) => reasonText(t, r))].join("\n");
