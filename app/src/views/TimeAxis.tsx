import { useTranslation } from "react-i18next";
import { axisTicks, endLabel, tickLabel } from "../timeAxis";

/** The time axis under the heatmap and the balancing lanes: both ends (with the date when the span crosses a day) and a
 *  few ticks in between, placed by time so every chart of the same span lines up. */
export function TimeAxis({ from, to }: { from: number; to: number }) {
  const { i18n } = useTranslation();
  const lang = i18n.language;
  return (
    <div className="relative h-6 text-xs leading-none text-muted tabular-nums">
      <span className="absolute left-0 top-2">{endLabel(from, from, to, lang)}</span>
      {axisTicks(from, to).map((t) => (
        <span key={t} className="absolute top-0 flex -translate-x-1/2 flex-col items-center" style={{ left: `${((t - from) / (to - from)) * 100}%` }}>
          <span className="mb-1 h-1 w-px bg-muted" aria-hidden />
          {tickLabel(t, from, to, lang)}
        </span>
      ))}
      <span className="absolute right-0 top-2">{endLabel(to, from, to, lang)}</span>
    </div>
  );
}
