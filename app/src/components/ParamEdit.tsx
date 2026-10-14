import { useTranslation } from "react-i18next";
import { PenLine, TriangleAlert } from "lucide-react";
import type { Parameters } from "../params";
import { parseNumber } from "../calibration";
import { InfoIcon, helpTitle } from "../help";
import { cn } from "../lib/utils";
import { KEYS, UNITS, decimals } from "../params";
import { capacityResets, fromRaw, ratedFor, switchDiff, toRaw, SERIES_INDEX, RATED_INDEX, type Blocker, type Change, type Raws } from "../paramWrite";
import type { ParamLevel } from "../cells/paramCheck";
import { Button } from "./ui/button";

/** The edit of one pack column: the read it started from and the values as edited. */
export interface Edit {
  address: number;
  base: Raws;
  baseSwitches: number[];
  raws: Raws;
  switches: number[];
  /** inputs as typed, until the field is left (then the value shows rounded to the raw resolution) */
  text: Record<number, string>;
  seriesUnlocked: boolean;
}

/** Inputs that are not a number. */
export const badInput = (e: Edit) => Object.entries(e.text).flatMap(([i, s]) => (parseNumber(s) === null ? [Number(i)] : []));

/** Typing into a cell: the raw follows every readable input. */
export function typed(e: Edit, i: number, s: string): Edit {
  const v = parseNumber(s);
  return { ...e, text: { ...e.text, [i]: s }, raws: v === null ? e.raws : e.raws.map((r, k) => (k === i ? toRaw(i, v) : r)) };
}

/** Leaving a cell: a readable input gives way to the stored value, so rounding shows. */
export function left(e: Edit, i: number): Edit {
  if (e.text[i] === undefined || parseNumber(e.text[i]) === null) return e;
  const text = { ...e.text };
  delete text[i];
  return { ...e, text };
}

/** Learned full capacity of the edited pack (0x42), read when the edit starts. */
export type Full = { address: number; ah: number } | { address: number; error: string } | null;

/** After a write: what happened. */
export type Note = { text: string; error: boolean };

/** A 0xA1 refusal (RTN 06) that left switches live but not stored: the stored block (read right before) and the
 *  switches sent. Until the stored block is back, the pack takes no other write: a fresh read would show the live
 *  switches, and the rebase would adopt them as if the pack had changed. */
export interface Refused { stored: Parameters; live: number[] }

/** An input in the edited column; P65 only when unlocked. */
export function EditCell({ e, i, level, disabled, onChange }: { e: Edit; i: number; level?: ParamLevel; disabled: boolean; onChange: (f: (e: Edit) => Edit) => void }) {
  const { t, i18n } = useTranslation();
  const changed = e.raws[i] !== e.base[i];
  const bad = e.text[i] !== undefined && parseNumber(e.text[i]) === null;
  const shown = e.text[i] ?? new Intl.NumberFormat(i18n.language, { minimumFractionDigits: decimals(i), maximumFractionDigits: decimals(i), useGrouping: false }).format(fromRaw(i, e.raws[i]));
  return (
    <td className={cn("border-b border-l border-line px-1.5 py-1 text-right", changed && "bg-charge/12")}>
      <input value={shown} disabled={disabled} inputMode="decimal" aria-label={`P${i}`}
        title={i === SERIES_INDEX && !e.seriesUnlocked ? t("pw.seriesLocked") : undefined}
        onChange={(ev) => onChange((x) => typed(x, i, ev.target.value))} onBlur={() => onChange((x) => left(x, i))}
        className={cn("h-7 w-24 rounded border bg-surface px-1.5 text-right text-sm tabular-nums outline-none focus:border-charge disabled:opacity-50",
          bad ? "border-alarm text-alarm" : level === "over" ? "border-alarm/60 text-alarm" : changed ? "border-charge font-semibold" : "border-line")} />
    </td>
  );
}

/** A switch bit in the edited column. */
export function EditSwitch({ e, g, b, disabled, onChange }: { e: Edit; g: number; b: number; disabled: boolean; onChange: (f: (e: Edit) => Edit) => void }) {
  const { t } = useTranslation();
  const on = ((e.switches[g] >> b) & 1) === 1;
  const changed = (((e.switches[g] ^ e.baseSwitches[g]) >> b) & 1) === 1;
  return (
    <td className={cn("border-b border-l border-line px-3 py-1 text-right", changed && "bg-charge/12 font-semibold")}>
      <label className="inline-flex cursor-pointer items-center gap-1.5">
        <input type="checkbox" className="h-4 w-4 accent-[var(--charge)]" checked={on} disabled={disabled}
          onChange={() => onChange((x) => ({ ...x, switches: x.switches.map((s, k) => (k === g ? s ^ (1 << b) : s)) }))} />
        {on ? t("par.on") : t("par.off")}
      </label>
    </td>
  );
}

const MAX_LINES = 10;

/** The sticky bar of an edit: change list, datasheet findings, blockers, the capacity line, write and discard. */
export function EditBar({ e, pack, changes, blockers, sheet, full, note, refused, busy, locked, sources, onChange, onTakeOver, onWrite, onRestore, onDiscard }: {
  e: Edit; pack: string; changes: Change[]; blockers: Blocker[];
  /** a refusal of this pack whose switches are still live: writing waits for the restore */
  refused: Refused | null;
  /** datasheet findings of the changed values: index, level, limit text */
  sheet: { index: number; level: ParamLevel; limit: string }[];
  full: Full; note: Note | null; busy: boolean; locked: boolean;
  /** other columns with values, for „Werte übernehmen aus …“ */
  sources: { id: string; label: string }[];
  onChange: (f: (e: Edit) => Edit) => void; onTakeOver: (id: string) => void;
  onWrite: () => void; onRestore: (stored: Parameters) => void; onDiscard: () => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const fmt = (i: number, raw: number) => {
    const v = new Intl.NumberFormat(lang, { minimumFractionDigits: decimals(i), maximumFractionDigits: decimals(i) }).format(fromRaw(i, raw));
    return UNITS[i] ? `${v} ${UNITS[i]}` : v;
  };
  const pname = (i: number) => `P${i} ${helpTitle(`param.${KEYS[i]}`, lang)}`;
  const swName = (g: number, b: number) => switchName(g, b, lang);
  const line = (c: Change) => c.kind === "param"
    ? `${pname(c.index)}: ${fmt(c.index, c.from)} → ${fmt(c.index, c.to)}`
    : t("pw.switchLine", { name: swName(c.group, c.bit), g: c.group, b: c.bit, to: c.on ? t("par.on") : t("par.off") });
  const blockText = (b: Blocker) =>
    b.kind === "input" ? t("pw.blocked.input", { p: pname(b.index) })
    : b.kind === "range" ? t("pw.blocked.range", { p: pname(b.index) })
    : b.kind === "series" ? t("pw.blocked.series")
    : t(b.v.op === "le" ? "pw.rule.le" : "pw.rule.ge", { p: pname(b.v.index), other: "other" in b.v ? `P${b.v.other}` : fmt(b.v.index, b.v.bound) });
  const fullAh = full && full.address === e.address && "ah" in full ? full.ah : null;
  const nf2 = new Intl.NumberFormat(lang, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const resets = capacityResets(fullAh, e.raws[RATED_INDEX]);
  const canWrite = !busy && !locked && !refused && changes.length > 0 && blockers.length === 0;
  const capacityLine = (rated: number) => fullAh !== null && capacityResets(fullAh, rated) &&
    t("pw.full.resets", { full: nf2.format(fullAh), rated: nf2.format(fromRaw(RATED_INDEX, rated)) });

  return (
    <div className="sticky top-0 z-30 space-y-2 rounded-md border border-charge/50 bg-surface p-4 text-sm shadow-sm">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="flex items-center gap-1.5 font-medium"><PenLine className="h-4 w-4" aria-hidden />
          {changes.length ? t("pw.count", { count: changes.length, pack }) : t("pw.none", { pack })}<InfoIcon id="topic.paramWrite" />
        </span>
        <select value="" disabled={busy || !sources.length} onChange={(ev) => ev.target.value && onTakeOver(ev.target.value)} aria-label={t("pw.takeOver")}
          className="h-8 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge">
          <option value="">{t("pw.takeOver")}</option>
          {sources.map((s) => <option key={s.id} value={s.id}>{s.label}</option>)}
        </select>
        <label className="flex cursor-pointer items-center gap-2" title={t("pw.seriesHint")}>
          <input type="checkbox" className="h-4 w-4 accent-[var(--charge)]" checked={e.seriesUnlocked} disabled={busy}
            onChange={(ev) => onChange((x) => ({ ...x, seriesUnlocked: ev.target.checked }))} />
          {t("pw.unlockSeries")}
        </label>
        <span className="ml-auto flex gap-2">
          <Button variant="primary" size="sm" disabled={!canWrite} onClick={onWrite}>{busy ? t("pw.writing") : t("pw.write", { pack })}</Button>
          <Button variant="ghost" size="sm" disabled={busy} onClick={onDiscard}>{t("pw.discard")}</Button>
        </span>
      </div>

      {changes.length > 0 && (
        <ul className="list-disc pl-5 tabular-nums">
          {changes.slice(0, MAX_LINES).map((c) => <li key={c.kind === "param" ? `p${c.index}` : `s${c.group}.${c.bit}`}>{line(c)}</li>)}
          {changes.length > MAX_LINES && <li className="list-none text-muted">{t("pw.more", { n: changes.length - MAX_LINES })}</li>}
        </ul>
      )}
      {sheet.length > 0 && (
        <ul className="space-y-0.5">
          {sheet.map((s) => (
            <li key={s.index} className={s.level === "over" ? "text-alarm" : "text-charge"}>{t("pw.sheet", { p: pname(s.index), limit: s.limit })}</li>
          ))}
        </ul>
      )}
      {blockers.length > 0 && (
        <ul className="space-y-0.5 text-alarm">
          {blockers.slice(0, MAX_LINES).map((b, k) => <li key={k}>{blockText(b)}</li>)}
          {blockers.length > MAX_LINES && <li className="text-muted">{t("pw.more", { n: blockers.length - MAX_LINES })}</li>}
        </ul>
      )}

      {full?.address === e.address && "error" in full ? <p className="text-muted">{t("pw.full.unknown", { err: full.error })}</p>
        : full?.address !== e.address ? <p className="text-muted">{t("pw.full.reading")}</p>
        : resets && (
          <p className="flex flex-wrap items-center gap-2 text-discharge">
            <TriangleAlert className="h-4 w-4 shrink-0" aria-hidden />{capacityLine(e.raws[RATED_INDEX])}
            <Button size="sm" disabled={busy} onClick={() => onChange((x) => {
              const text = { ...x.text };
              delete text[RATED_INDEX];
              return { ...x, text, raws: x.raws.map((r, k) => (k === RATED_INDEX ? ratedFor(fullAh!) : r)) };
            })}>{t("pw.full.fix", { full: nf2.format(fullAh!) })}</Button>
          </p>
        )}

      {refused && <RefusedPanel refused={refused} fullAh={fullAh} busy={busy} locked={locked} onRestore={onRestore} />}
      {note && <p className={note.error ? "text-alarm" : "text-ok"}>{note.text}</p>}
    </div>
  );
}

const switchName = (g: number, b: number, lang: string) => helpTitle(`switch.${g}.${b}`, lang);

/** The pending refusal: which switches are live but not stored, and „Gespeicherte Schalter wiederherstellen“. */
export function RefusedPanel({ refused, fullAh, busy, locked, pack, onRestore }: {
  refused: Refused; fullAh: number | null; busy: boolean; locked: boolean; pack?: string; onRestore: (stored: Parameters) => void;
}) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const nf2 = new Intl.NumberFormat(lang, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const live = switchDiff(refused.live, refused.stored.function_switches);
  const rated = refused.stored.parameters[RATED_INDEX].raw;
  return (
    <div className="space-y-2 rounded-md border border-alarm/40 bg-alarm/10 p-3 text-sm">
      <p className="text-alarm">{pack ? `${t("par.pack", { n: pack })}: ` : ""}{t("pw.refused")}</p>
      {live.length ? (
        <div>
          <p>{t("pw.liveSwitches")}</p>
          <ul className="list-disc pl-5">{live.map((c) => <li key={`${c.group}.${c.bit}`}>{t("pw.switchLine", { name: switchName(c.group, c.bit, lang), g: c.group, b: c.bit, to: c.on ? t("par.on") : t("par.off") })}</li>)}</ul>
        </div>
      ) : <p className="text-muted">{t("pw.noLiveSwitches")}</p>}
      <p>{t("pw.pendingBlock")}</p>
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={busy || locked} onClick={() => onRestore(refused.stored)}>{busy ? t("pw.writing") : t("pw.restore")}</Button>
        <span className="text-muted">{t("pw.restartHint")}</span>
      </div>
      {fullAh !== null && capacityResets(fullAh, rated) && (
        <p className="text-discharge">{t("pw.full.resets", { full: nf2.format(fullAh), rated: nf2.format(fromRaw(RATED_INDEX, rated)) })}</p>
      )}
    </div>
  );
}
