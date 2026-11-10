import { useEffect, useState } from "react";
import { actLocked } from "../locks";
import { useTranslation } from "react-i18next";
import { Tag } from "lucide-react";
import { api } from "../api";
import { refusal } from "../calibration";
import { writeError } from "../control";
import { nameInputOk, type Parameters } from "../params";
import { capacityResets, RATED_INDEX } from "../paramWrite";
import { SectionHead, StatusLine } from "./Section";
import { Button } from "./ui/button";
import { useCurrent } from "./Calibration";

type Note = { text: string; tone: "ok" | "warn" | "error" } | null;

/** The device name of the chosen pack (bytes 159–168 of the parameter block): written with the block read fresh, only
 *  over RS485-1/2. `read`: the parameter column of this connection, its name is what the pack must still hold. */
export function NameLine({ address, usable, locked, onBusy, linkKey, read, pending, onRead }: {
  address: number | null; usable: boolean; locked?: boolean; onBusy?: (busy: boolean) => void; linkKey: string;
  read: Parameters | undefined;
  /** a refused parameter write left its switches live on this pack: any A1 now would store them, so none goes out */
  pending: boolean;
  /** `written`: the BMS acknowledged a write (the write history of the pack changed) */
  onRead: (address: number, p: Parameters | null, written: boolean) => void;
}) {
  const { t, i18n } = useTranslation();
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<Note>(null);
  // the learned full capacity this write resets to P58 (null: not readable), asked once before sending
  const [ask, setAsk] = useState<{ full: number | null; rated: number } | null>(null);
  const current = useCurrent(`${linkKey}|${address}`);
  useEffect(() => onBusy?.(busy), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { setInput(""); setNote(null); setAsk(null); }, [address, linkKey]);
  // a refusal left switches live meanwhile: a confirmation asked before must not send now
  useEffect(() => { if (pending) setAsk(null); }, [pending]);
  if (!usable || address === null) return null;

  const nf = new Intl.NumberFormat(i18n.language, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const name = input.trim();
  const ok = nameInputOk(name) && !!read && name !== read.device_name && !pending;

  const write = async (confirmed: boolean) => {
    if (!read || pending) return;
    const a = address, key = `${linkKey}|${a}`, expected = read.device_name;
    setBusy(true); setNote(null);
    try {
      // every A1 sets the learned full capacity to P58 when they differ by 10 Ah or more: said before sending
      if (!confirmed) {
        const rated = read.parameters[RATED_INDEX].raw;
        const full = await api.packFullCapacity(a).catch(() => null);
        if (current.current !== key) return;
        if (full === null || capacityResets(full, rated)) { setAsk({ full, rated }); return; }
      }
      setAsk(null);
      const r = await api.writeName(a, expected, name);
      if (current.current !== key) return;
      if (r.kind === "written") {
        onRead(a, r.read_back, true);
        const back = r.read_back?.device_name;
        setNote(back === undefined ? { text: t("nmw.unread", { err: r.read_error ?? "" }), tone: "warn" }
          : back === name ? { text: t("nmw.done"), tone: "ok" }
          : { text: t("nmw.mismatch", { name: back }), tone: "error" });
        if (back === name) setInput("");
      } else if (r.kind === "stale") {
        onRead(a, r.fresh, false);
        setNote({ text: t("nmw.stale", { name: r.fresh.device_name }), tone: "warn" });
      } else if (r.kind === "unchanged") {
        onRead(a, r.fresh, false);
        setNote({ text: t("nmw.unchanged"), tone: "ok" });
      } else if (r.kind === "refused") {
        setNote({ text: t("nmw.refused"), tone: "error" });
      } else {
        if (r.read_back) onRead(a, r.read_back, false);
        setNote({ text: t(r.read_back?.device_name === name ? "nmw.unclearLive" : "nmw.unclear", { err: r.error }), tone: "error" });
      }
    } catch (e) {
      if (current.current === key) setNote({ text: refusal(e) ? t("nmw.refused") : writeError(e, t, (err) => t("nmw.error", { err })), tone: "error" });
    } finally { setBusy(false); }
  };

  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={Tag} title={t("nmw.title")} help="topic.paramWrite" />
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="w-32 text-muted">{t("nmw.current")}</span>
        <span className="min-w-56 font-mono text-xs">{read ? read.device_name || t("nmw.empty") : t("nmw.notRead")}</span>
        <input value={input} onChange={(e) => { setInput(e.target.value); setAsk(null); }} maxLength={12} disabled={actLocked(!!busy, !!locked) || !read}
          placeholder={t("nmw.placeholder")} aria-label={t("nmw.placeholder")}
          className={`h-8 w-40 rounded-md border bg-surface px-2 font-mono text-xs text-ink outline-none focus:border-charge ${name && !nameInputOk(name) ? "border-alarm" : "border-line"}`} />
        <Button size="sm" disabled={actLocked(!!busy, !!locked) || !ok} onClick={() => void write(false)}>{busy ? t("nmw.writing") : t("nmw.write")}</Button>
        {name !== "" && !nameInputOk(name) && <span className="text-alarm">{t("nmw.invalid")}</span>}
      </div>
      {ask && (
        <div className="flex flex-wrap items-center gap-2">
          <StatusLine tone="warn">{ask.full === null ? t("nmw.capacityUnknown", { rated: nf.format(ask.rated / 100) })
            : t("nmw.capacityResets", { full: nf.format(ask.full), rated: nf.format(ask.rated / 100) })}</StatusLine>
          <Button size="sm" disabled={actLocked(!!busy, !!locked) || pending} onClick={() => void write(true)}>{t("nmw.writeAnyway")}</Button>
          <Button size="sm" variant="ghost" disabled={actLocked(!!busy, false)} onClick={() => setAsk(null)}>{t("ctl.cancel")}</Button>
        </div>
      )}
      {pending && <StatusLine tone="warn">{t("nmw.pending")}</StatusLine>}
      {note && <StatusLine tone={note.tone}>{note.text}</StatusLine>}
      <p className="max-w-3xl text-muted">{t("nmw.hint")}</p>
    </div>
  );
}
