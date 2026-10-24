import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { OctagonPause } from "lucide-react";
import type { Parameters } from "../params";
import { hardStopChoice, hardStopConfigurable } from "../access";
import { Button } from "./ui/button";
import { SectionHead, StatusLine } from "./Section";

type Note = { text: string; error: boolean } | null;

/** „Hard-Stop bei Nachladepause“ (MAINT 1.6.3, image flag bit 7): one switch over Group0 bits 6/7 of the pack's last
 *  read. `write` sends the block of that read with only bits 6/7 changed (the parameter write reads the pack again
 *  first) and says what came of it. */
export function HardStopLine({ address, read, flags, usable, locked, onBusy, write }: {
  address: number | null; read: Parameters | undefined; flags: number | null | undefined; usable: boolean; locked?: boolean;
  onBusy?: (busy: boolean) => void; write: (address: number, read: Parameters, enabled: boolean) => Promise<Note>;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<Note>(null);
  useEffect(() => onBusy?.(busy), [busy]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => setNote(null), [address]);
  if (address === null || !usable || !hardStopConfigurable(flags)) return null;
  const choice = read ? hardStopChoice(read.function_switches[0], flags!) : null;
  const set = async (enabled: boolean) => {
    if (!read) return;
    setBusy(true); setNote(null);
    try { setNote(await write(address, read, enabled)); } finally { setBusy(false); }
  };
  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={OctagonPause} title={t("hs.title")} help="topic.hardStop" />
      {!choice ? <StatusLine>{t("hs.unread")}</StatusLine> : (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <span>{t(choice.enabled ? "hs.on" : "hs.off")}{choice.inherited ? ` ${t("hs.inherited")}` : ""}</span>
          <Button size="sm" disabled={busy || locked} onClick={() => set(!choice.enabled)}>{busy ? t("hs.writing") : t(choice.enabled ? "hs.turnOff" : "hs.turnOn")}</Button>
        </div>
      )}
      {note && <StatusLine tone={note.error ? "error" : "ok"}>{note.text}</StatusLine>}
      <StatusLine>{t("hs.hint")}</StatusLine>
    </div>
  );
}
