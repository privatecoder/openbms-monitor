import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, RefreshCw, type LucideIcon } from "lucide-react";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";

/** The header row of a section in the Parameters view: [chevron] icon · title · (i) · refresh · „Stand …“, and when
 *  closed its hint cut to one line. Messages never go here; they go into a `StatusLine` below. */
export function SectionHead({ icon: I, title, help, open, onToggle, closedHint, onReload, reloading, reloadDisabled, reloadLabel, readAt }: {
  icon: LucideIcon; title: string; help?: string;
  /** collapsible sections: open state and toggle */
  open?: boolean; onToggle?: () => void; closedHint?: string;
  /** a section with a read of its own: the refresh button (spins while reading) and the time of the last good read */
  onReload?: () => void; reloading?: boolean; reloadDisabled?: boolean; reloadLabel?: string; readAt?: Date | null;
}) {
  const { t, i18n } = useTranslation();
  const label = (
    <>
      {onToggle && (open ? <ChevronDown className="h-4 w-4 shrink-0" aria-hidden /> : <ChevronRight className="h-4 w-4 shrink-0" aria-hidden />)}
      <I className="h-4 w-4 shrink-0" aria-hidden />{title}
    </>
  );
  return (
    <div className="flex min-w-0 items-center gap-1.5">
      {onToggle
        ? <button onClick={onToggle} aria-expanded={open} className="flex shrink-0 items-center gap-1.5 font-medium hover:text-ink">{label}</button>
        : <span className="flex shrink-0 items-center gap-1.5 font-medium">{label}</span>}
      {help && <InfoIcon id={help} />}
      {onReload && (
        <Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" disabled={reloadDisabled || reloading} onClick={onReload}
          aria-label={reloadLabel ?? t("sec.reload")} title={reloadLabel ?? t("sec.reload")}>
          <RefreshCw className={cn("h-4 w-4", reloading && "animate-spin")} />
        </Button>
      )}
      {onReload && readAt && <span className="shrink-0 whitespace-nowrap text-muted tabular-nums">{t("sec.asOf", { time: readAt.toLocaleTimeString(i18n.language) })}</span>}
      {onToggle && !open && closedHint && <span className="min-w-0 truncate text-muted" title={closedHint}>{closedHint}</span>}
    </div>
  );
}

export type StatusTone = "ok" | "warn" | "error" | "muted";
const TONE: Record<StatusTone, string> = { ok: "text-ok", warn: "text-discharge", error: "text-alarm", muted: "text-muted" };

/** A read state or result on its own full-width line, wrapping within max-w-3xl. */
export function StatusLine({ tone = "muted", children, className }: { tone?: StatusTone; children: ReactNode; className?: string }) {
  return <p className={cn("max-w-3xl break-words", TONE[tone], className)}>{children}</p>;
}
