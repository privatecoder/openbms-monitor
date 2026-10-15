import { useSettings } from "./installData";

export interface Group { name: string; packs: number[] }

/** Stored groups as far as they are well-formed. */
export const parseGroups = (v: unknown): Group[] =>
  Array.isArray(v) ? v.filter((g): g is Group => !!g && typeof g.name === "string" && Array.isArray(g.packs) && g.packs.every((a: unknown) => typeof a === "number")) : [];

/** User-defined pack groups (e.g. towers), stored per installation (`scope`; null: „Nur ansehen“, this session only). */
export function useGroups(scope: string | null) {
  const { settings, save } = useSettings(scope);
  let groups = parseGroups(settings.groups);
  // Browser preview: ?groups=4 pre-fills groups of four packs.
  const demo = Number(new URLSearchParams(location.search).get("groups"));
  if (!groups.length && demo) groups = splitEvenly(Array.from({ length: 16 }, (_, i) => i), demo, "Turm");
  return { groups, save: (g: Group[]) => save("groups", g) };
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
