import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Hash, RefreshCw } from "lucide-react";
import { api, type SerialNumbers } from "../api";
import { refusal } from "../calibration";
import { errorText } from "../control";
import { InfoIcon } from "../help";
import { serialInputOk, serialSetResult, serialText } from "../serial";
import { Button } from "./ui/button";
import { useCurrent } from "./Calibration";

type Which = "battery" | "bms";
type Note = { text: string; error: boolean } | null;

/** Battery (String 1, 0xA2/0xA3) and BMS serial number (String 2, 0xA4/0xA5) of the chosen pack, each with a write. */
export function SerialLine({ address, usable, locked, onBusy }: { address: number | null; usable: boolean; locked?: boolean; onBusy?: (busy: boolean) => void }) {
  const { t } = useTranslation();
  const [read, setRead] = useState<SerialNumbers | { error: string } | null>(null);
  const [input, setInput] = useState<Record<Which, string>>({ battery: "", bms: "" });
  const [busy, setBusy] = useState<Which | null>(null);
  const [note, setNote] = useState<Partial<Record<Which, Note>>>({});
  const [reload, setReload] = useState(0);
  const current = useCurrent(address);
  useEffect(() => onBusy?.(busy !== null), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  // nothing typed for one pack may be written to another
  useEffect(() => { setInput({ battery: "", bms: "" }); setNote({}); }, [address]);
  useEffect(() => {
    setRead(null);
    if (address === null || !usable) return;
    let alive = true;
    api.serialNumbers(address).then((r) => alive && setRead(r), (e) => alive && setRead({ error: errorText(e, t) }));
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
      if (res === "done") setInput((i) => ({ ...i, [which]: "" }));
    } catch (e) {
      if (current.current === a) setNote((n) => ({ ...n, [which]: { text: refusal(e) ? t("snw.refused") : String(e).startsWith("pre-read") ? t("snw.preRead") : t("snw.error", { err: errorText(e, t) }), error: true } }));
    } finally { setBusy(null); }
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
          {bad && <span className="text-alarm">{t("snw.invalid")}</span>}
        </div>
        {n && <p className={n.error ? "text-alarm" : "text-ok"}>{n.text}</p>}
      </div>
    );
  };

  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="flex items-center gap-1.5 font-medium"><Hash className="h-4 w-4" aria-hidden />{t("snw.title")}<InfoIcon id="topic.paramWrite" /></span>
        {read && "error" in read && <span className="text-alarm">{t("snw.readError", { err: read.error })}</span>}
        <Button variant="ghost" size="icon" className="-ml-2 h-8 w-8" disabled={!!busy || locked || read === null} onClick={() => setReload((r) => r + 1)}
          aria-label={t("snw.reload")} title={t("snw.reload")}><RefreshCw className="h-4 w-4" /></Button>
      </div>
      {row("battery")}
      {row("bms")}
      {address === 0 && <p className="text-muted">{t("snw.masterHint")}</p>}
    </div>
  );
}
