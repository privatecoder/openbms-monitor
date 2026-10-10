import { useEffect, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

export interface Line { label: string; color: string; values: (number | null)[]; dash?: number[]; width?: number }

/** True while the dark theme is on (the html element's class, toggled in the sidebar). */
export function useDark() {
  const [dark, setDark] = useState(() => document.documentElement.classList.contains("dark"));
  useEffect(() => {
    const o = new MutationObserver(() => setDark(document.documentElement.classList.contains("dark")));
    o.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => o.disconnect();
  }, []);
  return dark;
}

const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** Axis labels by the visible span: seconds below 10 min, dates from 2 days. */
function timeLabels(lang: string) {
  const hm = new Intl.DateTimeFormat(lang, { hour: "2-digit", minute: "2-digit" });
  const hms = new Intl.DateTimeFormat(lang, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const dhm = new Intl.DateTimeFormat(lang, { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
  return (u: uPlot, splits: number[]) => {
    const span = (u.scales.x.max ?? 0) - (u.scales.x.min ?? 0);
    const f = span < 600 ? hms : span > 2 * 86400 ? dhm : hm;
    return splits.map((s) => f.format(s * 1000));
  };
}

/**
 * A time chart (uPlot). x in ms. Dragging selects a span and calls onZoom with it; double-click calls
 * onReset. Charts with the same syncKey share the cursor.
 */
export function Chart({ x, lines, unit, decimals, lang, timeLabel, syncKey, onZoom, onReset, height = 210 }: {
  x: number[]; lines: Line[]; unit: string; decimals: number; lang: string; timeLabel: string; syncKey: string;
  onZoom: (from: number, to: number) => void; onReset: () => void; height?: number;
}) {
  const box = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const dark = useDark();
  // callbacks change on every render; the chart reads them through refs
  const cb = useRef({ onZoom, onReset });
  cb.current = { onZoom, onReset };
  const shape = `${lines.map((l) => `${l.label}|${l.color}`).join(",")}|${unit}|${decimals}|${lang}|${dark}`;

  // (re)build when the series, the language or the theme change
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const ink = css("--muted"), grid = css("--line");
    const nf = new Intl.NumberFormat(lang, { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
    const full = new Intl.DateTimeFormat(lang, { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });
    const opts: uPlot.Options = {
      width: el.clientWidth, height,
      legend: { live: true },
      cursor: { sync: { key: syncKey }, drag: { x: true, y: false, setScale: false }, points: { size: 6 } },
      scales: { x: { time: true } },
      series: [
        { label: timeLabel, value: (_u, v) => (v == null ? "–" : full.format(v * 1000)) },
        ...lines.map((l) => ({
          label: l.label, stroke: l.color, width: l.width ?? 1.5, dash: l.dash, spanGaps: false, points: { show: false },
          value: (_u: uPlot, v: number | null) => (v == null ? "–" : `${nf.format(v)}${unit ? ` ${unit}` : ""}`),
        })),
      ],
      axes: [
        { stroke: ink, grid: { stroke: grid, width: 1 }, ticks: { stroke: grid, width: 1 }, values: timeLabels(lang), font: "12px Barlow, sans-serif" },
        { stroke: ink, grid: { stroke: grid, width: 1 }, ticks: { stroke: grid, width: 1 }, size: 56, font: "12px Barlow, sans-serif",
          values: (_u, splits) => splits.map((s) => nf.format(s)) },
      ],
      hooks: {
        setSelect: [(u) => {
          if (u.select.width < 4) return;
          const from = u.posToVal(u.select.left, "x"), to = u.posToVal(u.select.left + u.select.width, "x");
          u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
          cb.current.onZoom(from * 1000, to * 1000);
        }],
      },
    };
    const u = new uPlot(opts, [x.map((t) => t / 1000), ...lines.map((l) => l.values)] as uPlot.AlignedData, el);
    plot.current = u;
    const dbl = () => cb.current.onReset();
    u.over.addEventListener("dblclick", dbl);
    const ro = new ResizeObserver(() => u.setSize({ width: el.clientWidth, height }));
    ro.observe(el);
    return () => { ro.disconnect(); u.over.removeEventListener("dblclick", dbl); u.destroy(); plot.current = null; };
  }, [shape, height, syncKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // new data with the same series: only swap the data
  useEffect(() => {
    plot.current?.setData([x.map((t) => t / 1000), ...lines.map((l) => l.values)] as uPlot.AlignedData);
  }, [x, lines]);

  return <div ref={box} className="history-chart w-full" />;
}
