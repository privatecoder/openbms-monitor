import { useEffect, useRef, useState } from "react";
import { actLocked, decideUi } from "../locks";
import { useTranslation } from "react-i18next";
import { OctagonPause } from "lucide-react";
import type { Parameters } from "../params";
import { hardStopChoice, hardStopConfigurable } from "../access";
import { useImageFlags } from "../imageFlags";
import { Button } from "./ui/button";
import { SectionHead, StatusLine } from "./Section";

/** `confirm`: the write did not go out, the note says why (the learned full capacity); „trotzdem“ sends it. */
type Note = { text: string; error: boolean; confirm?: boolean } | null;

/** „Hard-Stop bei Nachladepause“ (MAINT 1.6.3, image flag bit 7: this connection's flags, else the pack's own 64/03):
 *  one switch over Group0 bits 6/7 of the pack's last read. `write` sends the block of that read with only bits 6/7
 *  changed (the parameter write reads the pack again first) and says what came of it; first it names the learned full
 *  capacity an A1 resets. A note belongs to the pack and link it was made for. */
export function HardStopLine({ address, read, known, connected, linkKey, locked, onBusy, write }: {
  address: number | null; read: Parameters | undefined; known: number | null | undefined; connected: boolean; linkKey: string; locked?: boolean;
  onBusy?: (busy: boolean) => void; write: (address: number, read: Parameters, enabled: boolean, confirmed: boolean) => Promise<Note>;
}) {
  const { t } = useTranslation();
  const flags = useImageFlags(connected ? address : null, known, linkKey);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<Note>(null);
  const [pending, setPending] = useState<boolean | null>(null);
  const gen = useRef(0);
  useEffect(() => onBusy?.(busy), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { gen.current += 1; setNote(null); setPending(null); }, [address, linkKey]);
  if (address === null || !decideUi("pack.section", { source: null, chosen: null, address, connected, offered: hardStopConfigurable(flags) }).visible) return null;
  const choice = read ? hardStopChoice(read.function_switches[0], flags!) : null;
  const set = async (enabled: boolean, confirmed: boolean) => {
    if (!read) return;
    const mine = ++gen.current;
    setBusy(true); setNote(null); setPending(null);
    try {
      const n = await write(address, read, enabled, confirmed);
      if (gen.current !== mine) return;
      setNote(n);
      if (n?.confirm) setPending(enabled);
    } finally { setBusy(false); }
  };
  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={OctagonPause} title={t("hs.title")} help="topic.hardStop" />
      {!choice ? <StatusLine>{t("hs.unread")}</StatusLine> : (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <span>{t(choice.enabled ? "hs.on" : "hs.off")}{choice.inherited ? ` ${t("hs.inherited")}` : ""}</span>
          <Button size="sm" disabled={actLocked(!!busy, !!locked)} onClick={() => set(!choice.enabled, false)}>{busy ? t("hs.writing") : t(choice.enabled ? "hs.turnOff" : "hs.turnOn")}</Button>
          {pending !== null && <Button size="sm" variant="ghost" disabled={actLocked(!!busy, !!locked)} onClick={() => set(pending, true)}>{t("hs.anyway")}</Button>}
        </div>
      )}
      {note && <StatusLine tone={note.error ? "error" : "ok"}>{note.text}</StatusLine>}
      <StatusLine>{t("hs.hint")}</StatusLine>
    </div>
  );
}
