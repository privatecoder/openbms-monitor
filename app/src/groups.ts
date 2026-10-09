import { useEffect, useState } from "react";

export interface Group { name: string; packs: number[] }

const keyFor = (site: string) => `groups:${site}`;

function read(site: string): Group[] {
  // Browser preview: ?groups=4 pre-fills groups of four packs.
  const demo = Number(new URLSearchParams(location.search).get("groups"));
  try {
    const stored = JSON.parse(localStorage.getItem(keyFor(site)) ?? "[]") as Group[];
    if (stored.length || !demo) return stored;
  } catch { if (!demo) return []; }
  return splitEvenly(Array.from({ length: 16 }, (_, i) => i), demo, "Turm");
}

/** User-defined pack groups (e.g. towers), stored per installation (gateway or port). */
export function useGroups(site: string) {
  const [groups, setGroups] = useState<Group[]>(() => read(site));
  useEffect(() => setGroups(read(site)), [site]);
  const save = (g: Group[]) => {
    setGroups(g);
    try { localStorage.setItem(keyFor(site), JSON.stringify(g)); } catch { /* preview without storage */ }
  };
  return { groups, save };
}

/** Split packs into consecutive groups of `size`, named with `label n`. */
export const splitEvenly = (packs: number[], size: number, label: string): Group[] =>
  Array.from({ length: Math.ceil(packs.length / size) }, (_, i) => ({ name: `${label} ${i + 1}`, packs: packs.slice(i * size, i * size + size) }));

/** Groups with only present packs, plus a trailing group for unassigned packs. */
export function arrange(groups: Group[], present: number[], unassignedLabel: string): Group[] {
  const used = new Set<number>();
  const out = groups.map((g) => {
    const packs = g.packs.filter((a) => present.includes(a) && !used.has(a));
    packs.forEach((a) => used.add(a));
    return { ...g, packs };
  }).filter((g) => g.packs.length);
  const rest = present.filter((a) => !used.has(a));
  if (rest.length) out.push({ name: groups.length ? unassignedLabel : "", packs: rest });
  return out;
}
