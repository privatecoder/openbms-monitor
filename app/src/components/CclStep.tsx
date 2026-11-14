import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { TrendingDown } from "lucide-react";
import { api, isDemo, type Bus, type CclRead } from "../api";
import { CAP, accessHint } from "../access";
import { CCL_MAX, CCL_MIN, bankStep, masterKey, masterOffers, parseStep, writeLockedOf } from "../ccl";
import { errorText } from "../control";
import { decideUi, lockProps } from "../locks";
import { useProfile } from "../profile";
import { askBitsAgain, useReadCaps } from "../imageFlags";
import { Button } from "./ui/button";
import { SectionHead, StatusLine } from "./Section";

type Note = { text: string; tone: "ok" | "error" | "warn" } | null;

/** „EXT — CCL-Abbaustufe (Master)“ (EMU1101-EXT Full 1.6.7, capability bit 4 of the master's own 64/03), over the
 *  connection's bus (the CAN socket, or RS485-1/2 with the master's polling stopped; the backend takes the way): the master's
 *  persistent CCL decrease step, apart from the parameter table and every bulk path. Always the master (address 0),
 *  whichever pack is chosen. The value is read when the section appears and again after any change of link or master
 *  (`masterKey`); it is shown only from a confirmed read, never as a default. Saving is its own explicit action. */
export function CclStepSection({ connected, bus, installation, packs, linkKey, locked, onBusy }: {
  connected: boolean; bus: Bus | null; installation: string | null; packs: number; linkKey: string; locked?: boolean; onBusy?: (busy: boolean) => void;
}) {
  const { t } = useTranslation();
  const profile = useProfile();
  // the CAN socket: the master's profile; RS485-1/2 has none, so the master's own 64/03 there (answered only while it
  // does not poll)
  const [tick, setTick] = useState(0);
  const [checking, setChecking] = useState(false);
  const caps0 = useReadCaps(connected && bus === "pack" ? 0 : null, null, linkKey, tick);
  const offered = bus === "can" ? masterOffers(profile) : caps0 !== null && (caps0 & (1 << CAP.ccl_step)) !== 0;
  const key = masterKey(linkKey, profile);
  const [read, setRead] = useState<CclRead | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState<"read" | "save" | null>(null);
  const [note, setNote] = useState<Note>(null);
  const gen = useRef(0);
  const visible = decideUi("ccl.section", { source: connected ? "bus" : null, chosen: installation, bus, connected, offered, demo: isDemo }).visible;
  useEffect(() => onBusy?.(busy === "save"), [busy]); // eslint-disable-line react-hooks/exhaustive-deps

  const load = async (keep?: Note) => {
    const mine = ++gen.current;
    setBusy("read"); setRead(null); setNote(keep ?? null);
    try {
      const r = await api.cclStepRead();
      if (gen.current !== mine) return;
      setRead(r); setText(String(r.step.down));
    } catch (e) {
      if (gen.current === mine) setNote({ text: [keep?.text, errorText(e, t)].filter(Boolean).join(" "), tone: "error" });
    } finally {
      if (gen.current === mine) setBusy(null);
    }
  };
  // a new link, profile read or master: what was shown belongs to the master before; read the current one
  useEffect(() => {
    gen.current += 1;
    setRead(null); setText(""); setNote(null); setBusy(null);
    if (visible) void load();
  }, [key, visible]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!visible) {
    // RS485-1/2 without an answer to the master's 64/03: say why nothing is shown and offer to ask again (no write)
    const recheck = decideUi("ccl.recheck", { source: connected ? "bus" : null, chosen: installation, bus, connected, demo: isDemo, available: caps0 !== null, busy: checking });
    if (!recheck.visible) return null;
    const again = async () => {
      setChecking(true);
      try { await askBitsAgain(0, linkKey); } finally { setChecking(false); setTick((n) => n + 1); }
    };
    return (
      <div className="space-y-2 border-t border-line pt-3 text-sm">
        <SectionHead icon={TrendingDown} title={t("ccl.title")} help="topic.cclStep" />
        <StatusLine>{t("ccl.uncheckedU0")}</StatusLine>
        <Button size="sm" {...lockProps(recheck, t)} onClick={() => void again()}>{checking ? t("ccl.checking") : t("ccl.checkAgain")}</Button>
      </div>
    );
  }
  const value = parseStep(text);
  // Group0 bit 0 gates only the CAN socket's way
  const writeLocked = bus === "can" ? writeLockedOf(profile) : null;
  const save = decideUi("ccl.save", {
    source: "bus", chosen: installation, busy: busy !== null, held: !!locked, available: !!read, target: !!read?.serial,
    writeLocked: writeLocked === true, ready: value !== null, same: value !== null && value === read?.step.down,
  });
  const reread = decideUi("ccl.read", { source: "bus", chosen: installation, busy: busy !== null, held: !!locked });
  const run = async () => {
    if (value === null || !read?.serial) return;
    const mine = ++gen.current;
    setBusy("save"); setNote(null);
    try {
      const r = await api.cclStepWrite(installation, value, read.serial);
      if (gen.current !== mine) return;
      setRead({ step: r.step, serial: r.serial }); setText(String(r.step.down));
      setNote({ text: t("ccl.saved", { v: r.step.down }), tone: "ok" });
    } catch (e) {
      if (gen.current !== mine) return;
      // an open or refused save: the shown value is no longer confirmed; read it again and keep the message
      setBusy(null);
      void load({ text: errorText(e, t), tone: "error" });
      return;
    } finally {
      if (gen.current === mine) setBusy(null);
    }
  };

  return (
    <div className="space-y-2 border-t border-line pt-3 text-sm">
      <SectionHead icon={TrendingDown} title={t("ccl.title")} help="topic.cclStep" onReload={() => void load()} reloading={busy === "read"}
        reloadDisabled={reread.locked} reloadLabel={t("ccl.read")} />
      {busy === "read" ? <StatusLine>{t("ccl.reading")}</StatusLine>
        : read ? (
          <>
            <StatusLine tone="ok">{t("ccl.value", { v: read.step.down })}{read.step.down === 1 ? ` ${t("ccl.isDefault")}` : ""}</StatusLine>
            <StatusLine>{t("ccl.up", { up: read.step.up, min: read.step.min, max: read.step.max })}</StatusLine>
            {packs > 0 && <StatusLine>{t("ccl.example", { n: packs, a: bankStep(packs, read.step.down) })}</StatusLine>}
          </>
        ) : <StatusLine tone="warn">{t("ccl.unread")}</StatusLine>}
      {read && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <label className="flex items-center gap-2">
            <span>{t("ccl.field")}</span>
            <input type="number" inputMode="numeric" min={CCL_MIN} max={CCL_MAX} step={1} value={text} onChange={(e) => setText(e.target.value)}
              disabled={busy !== null} aria-invalid={value === null}
              className="h-8 w-20 rounded-md border border-line bg-surface px-2 text-sm tabular-nums text-ink outline-none focus:border-charge aria-[invalid=true]:border-alarm" />
            <span className="text-muted">{t("ccl.unit")}</span>
          </label>
          <Button size="sm" {...lockProps(save, t)} onClick={() => void run()}>{busy === "save" ? t("ccl.saving") : t("ccl.save")}</Button>
        </div>
      )}
      {read && !read.serial && <StatusLine tone="warn">{t("ccl.noSerial")}</StatusLine>}
      {note && <StatusLine tone={note.tone}>{note.text}</StatusLine>}
      <StatusLine>{accessHint(t, "ccl.hint")}</StatusLine>
    </div>
  );
}
