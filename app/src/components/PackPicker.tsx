import { Check, Minus } from "lucide-react";
import type { ReactNode } from "react";
import type { Group } from "../groups";
import { cn } from "../lib/utils";

const no = (a: number) => String(a).padStart(2, "0");

/** Groups as shown on the dashboard (only present packs), plus a section for packs in no group. */
export function packSections(groups: Group[], packs: number[], ungrouped: string, allLabel: string): Group[] {
  const grouped = groups.flatMap((g) => g.packs.filter((a) => packs.includes(a)));
  return [
    ...groups.map((g) => ({ name: g.name, packs: g.packs.filter((a) => packs.includes(a)) })).filter((g) => g.packs.length),
    ...(packs.some((a) => !grouped.includes(a)) ? [{ name: groups.length ? ungrouped : allLabel, packs: packs.filter((a) => !grouped.includes(a)) }] : []),
  ];
}

/** Packs by group: a tri-state tick per group and a chip per pack. `note` adds a line under a pack's chip. */
export function PackPicker({ sections, chosen, onToggle, count, note }: {
  sections: Group[]; chosen: number[]; onToggle: (packs: number[], on: boolean) => void;
  count: (on: number, of: number) => string; note?: (a: number) => ReactNode;
}) {
  return (
    <div className="space-y-2">
      {sections.map((g) => {
        const on = g.packs.filter((a) => chosen.includes(a)).length;
        const state = on === 0 ? "none" : on === g.packs.length ? "all" : "some";
        return (
          <div key={g.name} className="rounded-md border border-line p-3">
            <button type="button" onClick={() => onToggle(g.packs, state !== "all")} className="flex w-full items-center gap-2.5 text-left text-sm">
              <Tick state={state} />
              <span className="font-medium">{g.name}</span>
              <span className="ml-auto text-muted tabular-nums">{count(on, g.packs.length)}</span>
            </button>
            <div className="mt-2.5 flex flex-wrap gap-1.5 pl-7">
              {g.packs.map((a) => {
                const sel = chosen.includes(a);
                return (
                  <div key={a} className="flex flex-col items-center">
                    <button type="button" onClick={() => onToggle([a], !sel)} aria-pressed={sel}
                      className={cn("h-8 min-w-11 rounded-md border px-2 font-display text-sm tabular-nums transition-colors",
                        sel ? "border-ink bg-ink text-surface" : "border-line text-muted hover:border-ink/40 hover:text-ink")}>
                      {no(a)}
                    </button>
                    {note && <span className="mt-0.5 text-[11px] leading-tight text-muted">{note(a)}</span>}
                  </div>
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
}

export function Segmented({ value, onChange, items }: { value: string; onChange: (v: string) => void; items: [string, string][] }) {
  return (
    <div role="radiogroup" className="inline-flex rounded-md bg-sunken p-0.5">
      {items.map(([v, label]) => (
        <button type="button" role="radio" aria-checked={value === v} key={v} onClick={() => onChange(v)}
          className={cn("rounded px-3 py-1 text-sm transition-colors", value === v ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>
          {label}
        </button>
      ))}
    </div>
  );
}

export function Tick({ state }: { state: "none" | "some" | "all" }) {
  return (
    <span className={cn("flex h-4.5 w-4.5 items-center justify-center rounded border",
      state === "none" ? "border-line" : "border-ink bg-ink text-surface")} aria-hidden>
      {state === "all" && <Check className="h-3 w-3" strokeWidth={3} />}
      {state === "some" && <Minus className="h-3 w-3" strokeWidth={3} />}
    </span>
  );
}

export function Switch({ checked, onChange, disabled }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button type="button" role="switch" aria-checked={checked} disabled={disabled} onClick={() => onChange(!checked)}
      className={cn("relative mt-0.5 h-5 w-9 shrink-0 rounded-full transition-colors", checked ? "bg-ink" : "bg-line")}>
      <span className={cn("absolute left-0 top-0.5 h-4 w-4 rounded-full bg-surface shadow transition-transform", checked ? "translate-x-4.5" : "translate-x-0.5")} />
    </button>
  );
}
