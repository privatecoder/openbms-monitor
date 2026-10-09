/** Three stacked packs, the top one carrying a cell strip. */
export function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 28 28" className={className} aria-hidden>
      <rect x="3" y="3" width="22" height="6" rx="1.5" className="fill-ink" />
      {[0, 1, 2, 3, 4, 5].map((i) => <rect key={i} x={6 + i * 3} y={i % 3 === 1 ? 4.6 : 5.4} width="1.6" height={i % 3 === 1 ? 2.8 : 1.2} className="fill-surface" />)}
      <rect x="3" y="11" width="22" height="6" rx="1.5" className="fill-ink/60" />
      <rect x="3" y="19" width="22" height="6" rx="1.5" className="fill-charge" />
    </svg>
  );
}
