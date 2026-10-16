import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Hash } from "lucide-react";
import { api, type SerialNumbers } from "../api";
import { refusal } from "../calibration";
import { errorText, writeError } from "../control";
import { serialInputOk, serialSetResult, serialText } from "../serial";
import { batteryFromBms, CAN_SHOWN, SERIAL_MAX } from "../bank";
import { SectionHead, StatusLine } from "./Section";
import { Button } from "./ui/button";
import { useCurrent } from "./Calibration";

type Which = "battery" | "bms";
type Note = { text: string; error: boolean } | null;

/** Battery (String 1, 0xA2/0xA3) and BMS serial number (String 2, 0xA4/0xA5) of the chosen pack, each with a write. */
export function SerialLine({ address, usable, locked, onBusy, linkKey }: { address: number | null; usable: boolean; locked?: boolean; onBusy?: (busy: boolean) => void; linkKey: string }) {
  const { t } = useTranslation();
  const [read, setRead] = useState<SerialNumbers | { error: string } | null>(null);
  const [input, setInput] = useState<Record<Which, string>>({ battery: "", bms: "" });
  const [busy, setBusy] = useState<Which | null>(null);
  const [note, setNote] = useState<Partial<Record<Which, Note>>>({});
  const [reload, setReload] = useState(0);
  const [readAt, setReadAt] = useState<Date | null>(null);
  // „BMS-Nummer übernehmen“ filled the battery field: what to know about the value (cut, CAN shows only part)
  const [fillNote, setFillNote] = useState<string | null>(null);
  const current = useCurrent(address);
  useEffect(() => onBusy?.(busy !== null), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  // nothing typed for one pack may be written to another, nor to the same address of another bank or connection
  useEffect(() => { setInput({ battery: "", bms: "" }); setNote({}); setFillNote(null); }, [address, linkKey]);
  useEffect(() => {
    setRead(null);
    if (address === null || !usable) return;
    let alive = true;
    api.serialNumbers(address).then((r) => { if (alive) { setRead(r); setReadAt(new Date()); } }, (e) => alive && setRead({ error: errorText(e, t) }));
    return () => { alive = false; };
  }, [address, usable, reload]);
  if (!usable || address === null) return null;

  const write = async (which: Which) => {
    const a = address, text = input[which].trim();
    setBusy(which); setNote((n) => ({ ...n, [which]: null }));
    try {
      const r = await api.setSerial(a, which, text);
      if (current.current !== a) return; // another pack is shown now
      if (r.value) setRead((s) => (s && "battery" in s ? { ...s, [which]: r.value } : s));
      const res = serialSetResult(text, r.value);
      const extra = r.master_unchecked ? ` ${t("snw.masterUnchecked")}`
        : r.master_updated.length ? ` ${t("snw.masterUpdated", { names: r.master_updated.join(", ") })}` : "";
      setNote((n) => ({ ...n, [which]: {
        text: (res === "done" ? t("snw.done") : res === "mismatch" ? t("snw.mismatch", { text: r.value ? serialText(r.value, which, "pack", a === 0, t).text : "" }) : t("snw.unread", { err: r.read_error ?? "" })) + extra,
        error: res === "mismatch" || r.master_unchecked,
      } }));
      if (res === "done") { setInput((i) => ({ ...i, [which]: "" })); if (which === "battery") setFillNote(null); }
    } catch (e) {
      if (current.current === a) setNote((n) => ({ ...n, [which]: { text: refusal(e) ? t("snw.refused") : String(e).startsWith("pre-read") ? t("snw.preRead") : writeError(e, t, (err) => t("snw.error", { err })), error: true } }));
    } finally { setBusy(null); }
  };

  // the battery serial is the only one the CAN socket shows (master only): offer the BMS serial for it when it is
  // empty, the placeholder or another value; the user still writes it
  const bmsValue = read && "bms" in read && read.bms.kind === "set" && !read.bms.truncated ? read.bms.value : null;
  const battery = read && "battery" in read ? read.battery : null;
  const offerBms = !!bmsValue && !!battery && (battery.kind === "empty" || battery.kind === "placeholder" || (battery.kind === "set" && battery.value !== bmsValue));
  const useBms = () => {
    if (!bmsValue) return;
    const f = batteryFromBms(bmsValue);
    setInput((i) => ({ ...i, battery: f.text }));
    setFillNote([f.cut ? t("snw.useBmsCut", { n: SERIAL_MAX }) : "", f.beyondCan ? t("snw.useBmsCan", { n: CAN_SHOWN }) : ""].filter(Boolean).join(" ") || null);
  };

  const row = (which: Which) => {
    const v = read && "battery" in read ? read[which] : undefined;
    const shown = v ? serialText(v, which, "pack", address === 0, t) : null;
    const s = input[which];
    const bad = s !== "" && !serialInputOk(s);
    const n = note[which];
    return (
      <div className="space-y-1">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <span className="w-32 text-muted">{t(`snw.${which}`)}</span>
          <span className="min-w-56 font-mono text-xs" title={shown?.title}>{read === null ? t("snw.reading") : shown?.text ?? "–"}</span>
          <input value={s} onChange={(e) => setInput((i) => ({ ...i, [which]: e.target.value }))} maxLength={40} disabled={!!busy || locked}
            placeholder={t("snw.placeholder")} aria-label={`${t(`snw.${which}`)}: ${t("snw.placeholder")}`}
            className={`h-8 w-72 rounded-md border bg-surface px-2 font-mono text-xs text-ink outline-none focus:border-charge ${bad ? "border-alarm" : "border-line"}`} />
          <Button size="sm" disabled={!!busy || locked || !serialInputOk(s)} onClick={() => write(which)}>{busy === which ? t("snw.writing") : t("snw.write")}</Button>
          {which === "battery" && offerBms && (
            <Button size="sm" variant="ghost" disabled={!!busy || locked} onClick={useBms} title={t("snw.useBmsHint")}>{t("snw.useBms")}</Button>
          )}
          {bad && <span className="text-alarm">{t("snw.invalid")}</span>}
        </div>
        {which === "battery" && fillNote && <StatusLine>{fillNote}</StatusLine>}
        {n && <StatusLine tone={n.error ? "error" : "ok"}>{n.text}</StatusLine>}
      </div>
    );
  };

  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={Hash} title={t("snw.title")} help="topic.paramWrite" onReload={() => setReload((r) => r + 1)} reloading={read === null}
        reloadDisabled={!!busy || locked} reloadLabel={t("snw.reload")} readAt={readAt} />
      {read && "error" in read && <StatusLine tone="error">{t("snw.readError", { err: read.error })}</StatusLine>}
      {row("battery")}
      {row("bms")}
      {address === 0 && <StatusLine>{t("snw.masterHint")}</StatusLine>}
    </div>
  );
}
