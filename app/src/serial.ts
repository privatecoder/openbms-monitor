import type { Bus, SerialValue } from "./api";

type T = (key: string, params?: Record<string, unknown>) => string;

/** Display text of one serial number; `title` carries the raw placeholder. `master`: address 0 of the bank. */
export function serialText(v: SerialValue | undefined, which: "battery" | "bms", bus: Bus | null, master: boolean, t: T): { text: string; title?: string } {
  if (!v) return { text: t("sn.unknown") };
  switch (v.kind) {
    case "set": return { text: v.truncated ? `${v.value} ${t("sn.first22")}` : v.value };
    case "placeholder": return { text: t("sn.placeholder"), title: v.value };
    case "empty": return { text: t("sn.empty") };
    case "no_answer": return { text: t("sn.noAnswer") };
    case "unavailable":
      // on the CAN socket bus only the master shows its battery serial number; everything else needs RS485-1/2
      return { text: bus === "can" && !(master && which === "battery") ? t("sn.packBusOnly") : t("sn.unsupported") };
  }
}

/** What 0xA3/0xA5 take: printable ASCII (0x20–0x7E), 1 to 30 characters after trimming; the app pads with spaces. */
export const serialInputOk = (s: string) => /^[\x20-\x7e]{1,30}$/.test(s.trim());

/** A serial number write the BMS accepted: the read-back shows the sent text, something else, or could not be read. */
export type SerialSetResult = "done" | "mismatch" | "unread";
export const serialSetResult = (sent: string, read: SerialValue | null): SerialSetResult =>
  read === null || read.kind === "no_answer" || read.kind === "unavailable" ? "unread"
  : "value" in read && read.value === sent.trim() ? "done" : "mismatch";
