import { useTranslation } from "react-i18next";
import { TriangleAlert } from "lucide-react";
import { errorText } from "../control";
import type { Outcome, SeriesEnd } from "../multiWrite";
import type { Change } from "../paramWrite";
import { cn } from "../lib/utils";
import { useWriteTexts } from "./ParamEdit";
import { Button } from "./ui/button";

const no = (a: number) => String(a).padStart(2, "0");

/** A pack of the run and what its write came to (none yet: waiting or being written). */
export interface Row { address: number; out?: Outcome }

/** „Auch auf weitere Packs …“: the other packs of the bus; only those with a read column of this connection can be
 *  picked (what is written there is the change on top of that read). */
export function TargetPicker({ candidates, chosen, disabled, onChange }: {
  candidates: { address: number; readable: boolean }[]; chosen: number[]; disabled: boolean; onChange: (chosen: number[]) => void;
}) {
  const { t } = useTranslation();
  const readable = candidates.filter((c) => c.readable).map((c) => c.address);
  const toggle = (a: number) => onChange(chosen.includes(a) ? chosen.filter((x) => x !== a) : [...chosen, a].sort((x, y) => x - y));
  return (
    <div className="space-y-2 border-t border-line pt-2">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
        {candidates.map((c) => (
          <label key={c.address} className={cn("flex items-center gap-1.5", c.readable ? "cursor-pointer" : "text-muted")} title={c.readable ? undefined : t("pw.multi.readFirst")}>
            <input type="checkbox" className="h-4 w-4 accent-[var(--charge)]" checked={chosen.includes(c.address)} disabled={disabled || !c.readable} onChange={() => toggle(c.address)} />
            {t("par.pack", { n: no(c.address) })}{!c.readable && <span className="text-xs">({t("pw.multi.readFirst")})</span>}
          </label>
        ))}
        <Button size="sm" variant="ghost" disabled={disabled || !readable.length} onClick={() => onChange(readable)}>{t("pw.multi.allRead")}</Button>
        <Button size="sm" variant="ghost" disabled={disabled || !chosen.length} onClick={() => onChange([])}>{t("pw.multi.none")}</Button>
      </div>
      <p className="text-muted">{t("pw.multi.hint")}</p>
    </div>
  );
}

/** The one confirmation of a run: the change once, the packs it goes to, the packs left out and why, and whose
 *  learned full capacity the write resets. `capacity` null: still being read (`reading`: the pack being read). */
export function MultiConfirm({ changes, packs, skipped, capacity, reading, onConfirm, onCancel }: {
  changes: Change[]; packs: number[]; skipped: Row[];
  capacity: { resets: number[]; unknown: number[] } | null; reading: number | null;
  onConfirm: () => void; onCancel: () => void;
}) {
  const { t } = useTranslation();
  const { fmt, pname, swLine } = useWriteTexts();
  const list = (xs: number[]) => xs.map(no).join(", ");
  return (
    <div className="space-y-2 rounded-md border border-charge/40 bg-charge/5 p-3">
      <p className="font-medium">{t("pw.multi.confirm", { count: changes.length, list: list(packs) })}</p>
      <ul className="list-disc pl-5 tabular-nums">
        {changes.map((c) => <li key={c.kind === "param" ? `p${c.index}` : `s${c.group}.${c.bit}`}>
          {c.kind === "param" ? `${pname(c.index)} → ${fmt(c.index, c.to)}` : swLine(c.group, c.bit, c.on)}
        </li>)}
      </ul>
      {skipped.length > 0 && (
        <ul className="space-y-0.5 text-discharge">
          {skipped.map((r) => <li key={r.address}>{t("par.pack", { n: no(r.address) })}: <OutcomeText out={r.out} /></li>)}
        </ul>
      )}
      {capacity === null ? <p className="text-muted">{t("pw.multi.readingFull", { pack: reading === null ? "" : no(reading) })}</p> : (
        <>
          {capacity.resets.length > 0 && (
            <p className="flex items-start gap-2 text-discharge"><TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
              {t("pw.multi.resets", { count: capacity.resets.length, list: list(capacity.resets) })}</p>
          )}
          {capacity.unknown.length > 0 && <p className="text-discharge">{t("pw.multi.fullUnknown", { count: capacity.unknown.length, list: list(capacity.unknown) })}</p>}
        </>
      )}
      <div className="flex flex-wrap gap-2">
        <Button variant="primary" size="sm" disabled={capacity === null || !packs.length} onClick={onConfirm}>{t("pw.multi.go", { count: packs.length })}</Button>
        <Button variant="ghost" size="sm" onClick={onCancel}>{t("pw.multi.cancel")}</Button>
      </div>
    </div>
  );
}

/** A run's progress and result: one line per pack, „Abbrechen“ while it runs, and at a refusal with live switches
 *  the choice to go on (the pack's restore panel shows below). */
export function SeriesPanel({ rows, current, paused, end, held, onCancel, onGoOn, onClose }: {
  rows: Row[]; current: number | null; paused: number | null; end: SeriesEnd | null;
  /** a restore of the paused pack is being written: going on or cancelling waits for it */
  held?: boolean;
  onCancel: () => void; onGoOn: () => void; onClose: () => void;
}) {
  const { t } = useTranslation();
  const n = rows.filter((r) => r.out).length;
  return (
    <div className="space-y-2 rounded-md border border-line bg-surface p-4 text-sm">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-medium">
          {end === null ? t("pw.multi.running", { n, of: rows.length }) : end === "link" ? t("pw.multi.endLink") : end === "unclear" ? t("pw.multi.endUnclear") : t("pw.multi.end")}
        </span>
        <span className="ml-auto flex gap-2">
          {end === null && paused === null && <Button size="sm" variant="ghost" onClick={onCancel}>{t("pw.multi.cancel")}</Button>}
          {end !== null && <Button size="sm" variant="ghost" onClick={onClose}>{t("pw.multi.close")}</Button>}
        </span>
      </div>
      {paused !== null && (
        <div className="flex flex-wrap items-center gap-2 rounded-md border border-alarm/40 bg-alarm/10 p-3">
          <span className="text-alarm">{t("pw.multi.paused", { pack: no(paused) })}</span>
          <span className="ml-auto flex gap-2">
            <Button size="sm" disabled={held} onClick={onGoOn}>{t("pw.multi.goOn")}</Button>
            <Button size="sm" variant="ghost" disabled={held} onClick={onCancel}>{t("pw.multi.cancel")}</Button>
          </span>
        </div>
      )}
      <ul className="space-y-0.5 tabular-nums">
        {rows.map((r) => (
          <li key={r.address} className={cn(tone(r.out))}>
            {t("par.pack", { n: no(r.address) })}: {r.out ? <OutcomeText out={r.out} /> : r.address === current ? t("pw.writing") : t("pw.multi.waiting")}
          </li>
        ))}
      </ul>
    </div>
  );
}

const tone = (o?: Outcome) =>
  !o ? "text-muted" : o.kind === "written" ? "text-ok" : o.kind === "skipped" || o.kind === "cancelled" ? "text-discharge" : "text-alarm";

function OutcomeText({ out }: { out?: Outcome }) {
  const { t } = useTranslation();
  const { blockText } = useWriteTexts();
  if (!out) return null;
  switch (out.kind) {
    case "written": return <>{t(out.unanswered ? "pw.multi.res.writtenUnanswered" : "pw.multi.res.written")}</>;
    case "unclear": return <>{t("pw.multi.res.unclear", { err: errorText(out.error, t) })}</>;
    case "notTaken": return <>{t("pw.multi.res.notTaken", { list: [...out.params.map((i) => `P${i}`), ...out.switches.map((g) => t("pw.multi.switchByte", { g }))].join(", ") })}</>;
    case "unread": return <>{t("pw.multi.res.unread", { err: out.error })}</>;
    case "refused": return <>{t(out.refusal === "refused" ? "pw.multi.res.refused" : "pw.multi.res.invalid")}</>;
    case "unreached": return <>{t("pw.multi.res.unreached", { err: errorText(out.error, t) })}</>;
    case "cancelled": return <>{t("pw.multi.res.cancelled")}</>;
    case "skipped": {
      const why = out.why === "blocked" ? out.blockers.map(blockText).join("; ")
        : out.why === "bank" || out.why === "busy" ? errorText(out.error, t)
        : t(`pw.multi.why.${out.why}`);
      return <>{t("pw.multi.res.skipped", { why })}</>;
    }
  }
}
