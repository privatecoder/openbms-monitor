import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Wrench } from "lucide-react";
import { api, type MarkerView } from "../api";
import { markerError, markerLine } from "../marker";
import { useCurrent } from "./Calibration";
import { SectionHead, StatusLine } from "./Section";
import { Button } from "./ui/button";

/** „Wartungs-Marker“ next to „Steuern“: read, set and clear the marker of the chosen pack over this connection, also
 *  without an installation (a board on the bench). CAN socket: the master; RS485-1/2: MAINT 1.5.1/1.6.1, clear only. */
export function MarkerSection({ address, bus, locked, onBusy, linkKey }: {
  address: number | null; bus: "can" | "pack" | null; locked?: boolean; onBusy?: (busy: boolean) => void;
  /** installation and connection: results of an action begun on another one are dropped */
  linkKey: string;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<MarkerView | null>(null);
  const [busy, setBusy] = useState<"read" | "set" | "clear" | null>(null);
  const [note, setNote] = useState<{ text: string; tone: "ok" | "error" } | null>(null);
  const current = useCurrent(address);
  const linkNow = useCurrent(linkKey);
  const gen = useRef(0);
  const same = (a: number, lk: string) => current.current === a && linkNow.current === lk;
  useEffect(() => onBusy?.(busy !== null), [busy]); // eslint-disable-line react-hooks/exhaustive-deps

  const masterOnly = bus === "can" && address !== null && address !== 0;
  const run = async (k: "read" | "set" | "clear", a: number) => {
    const mine = ++gen.current, lk = linkNow.current;
    setBusy(k); setNote(null);
    try {
      const v = k === "read" ? await api.markerRead(a) : await api.markerSet(a, k === "set");
      if (gen.current !== mine || !same(a, lk)) return;
      setView(v);
      if (k !== "read") setNote({ text: t(k === "set" ? "mk.doneSet" : v.slaves ? "mk.doneClearPolls" : "mk.doneClear", { n: v.slaves ?? 0 }), tone: "ok" });
    } catch (e) {
      if (gen.current === mine && same(a, lk)) setNote({ text: markerError(e, t), tone: "error" });
    } finally {
      if (gen.current === mine) setBusy(null);
    }
  };
  useEffect(() => {
    gen.current++;
    setView(null); setNote(null); setBusy(null);
    if (open && address !== null && bus && !masterOnly) void run("read", address);
  }, [open, address, bus, linkKey]); // eslint-disable-line react-hooks/exhaustive-deps
  if (address === null || !bus) return null;

  const disabled = locked || busy !== null;
  const line = view ? markerLine(view, t) : null;
  return (
    <div className="space-y-3 border-t border-line pt-3 text-sm">
      <SectionHead icon={Wrench} title={t("mk.title")} help="topic.maintenance" open={open} onToggle={() => setOpen(!open)} closedHint={t("mk.closed")}
        onReload={open && !masterOnly ? () => void run("read", address) : undefined} reloading={busy === "read"} reloadDisabled={disabled} />
      {open && (
        <div className="space-y-2">
          {masterOnly && <StatusLine>{t("mk.masterOnly")}</StatusLine>}
          {line && <StatusLine tone={line.tone}>{line.text}</StatusLine>}
          {view && (view.may_set || view.may_clear) && (
            <div className="flex flex-wrap items-center gap-2">
              {view.may_set && <Button size="sm" variant="secondary" disabled={disabled} onClick={() => void run("set", address)}>{busy === "set" ? t("mk.setting") : t("mk.set")}</Button>}
              {view.may_clear && <Button size="sm" variant="secondary" disabled={disabled} onClick={() => void run("clear", address)}>{busy === "clear" ? t("mk.clearing") : t("mk.clear")}</Button>}
            </div>
          )}
          {view?.may_set && <StatusLine>{t("mk.setHint")}</StatusLine>}
          {view?.may_clear && <StatusLine>{t(view.bus === "can" ? "mk.clearHint" : "mk.clearHintPack")}</StatusLine>}
          {note && <StatusLine tone={note.tone}>{note.text}</StatusLine>}
        </div>
      )}
    </div>
  );
}
