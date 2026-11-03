import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { axisTicks, endLabel, overlapping, tickLabel } from "../timeAxis";

/** The time axis under the heatmap and the balancing lanes: both ends (with the date when the span crosses a day) and a
 *  few ticks in between, placed by time so every chart of the same span lines up. A tick whose label would touch an
 *  end label or another tick (a narrow chart) is hidden, measured from the rendered labels. */
export function TimeAxis({ from, to }: { from: number; to: number }) {
  const { i18n } = useTranslation();
  const lang = i18n.language;
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [hidden, setHidden] = useState<Set<number>>(new Set());
  const ticks = axisTicks(from, to);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const box = (n: Element): [number, number] => { const r = n.getBoundingClientRect(); return [r.left, r.right]; };
    const kids = [...el.children];
    const next = overlapping(box(kids[0]), box(kids[kids.length - 1]), kids.slice(1, -1).map((k) => box(k.lastElementChild ?? k)));
    setHidden((h) => (h.size === next.size && [...next].every((i) => h.has(i)) ? h : next));
  }, [width, from, to, lang]);
  return (
    <div ref={ref} className="relative h-6 text-xs leading-none text-muted tabular-nums">
      <span className="absolute left-0 top-2">{endLabel(from, from, to, lang)}</span>
      {ticks.map((t, i) => (
        <span key={t} className="absolute top-0 flex -translate-x-1/2 flex-col items-center" style={{ left: `${((t - from) / (to - from)) * 100}%`, visibility: hidden.has(i) ? "hidden" : undefined }}>
          <span className="mb-1 h-1 w-px bg-muted" aria-hidden />
          <span>{tickLabel(t, from, to, lang)}</span>
        </span>
      ))}
      <span className="absolute right-0 top-2">{endLabel(to, from, to, lang)}</span>
    </div>
  );
}
