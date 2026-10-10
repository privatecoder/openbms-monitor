import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Sheet, SheetContent } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { splitEvenly, type Group } from "../groups";
import { neighbourPairs, validPairs, type Pair } from "../pairs";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function GroupEditor({ open, onOpenChange, groups, packs, onSave, pairs, onSavePairs }: {
  open: boolean; onOpenChange: (o: boolean) => void; groups: Group[]; packs: number[]; onSave: (g: Group[]) => void;
  pairs: Pair[]; onSavePairs: (p: Pair[]) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<Group[]>(groups);
  const [count, setCount] = useState(3);
  const perGroup = Math.ceil(packs.length / Math.max(1, count));
  const [pairDraft, setPairDraft] = useState<Pair[]>(pairs);
  useEffect(() => { if (open) { setDraft(groups.length ? groups : []); setPairDraft(pairs); } }, [open, groups, pairs]);
  const setPair = (i: number, k: keyof Pair, v: number) => setPairDraft((d) => d.map((p, j) => (j === i ? { ...p, [k]: v } : p)));

  const owner = (a: number) => draft.findIndex((g) => g.packs.includes(a));
  const toggle = (gi: number, a: number) => setDraft((d) => d.map((g, i) => {
    if (i === gi) return { ...g, packs: g.packs.includes(a) ? g.packs.filter((x) => x !== a) : [...g.packs, a].sort((x, y) => x - y) };
    return { ...g, packs: g.packs.filter((x) => x !== a) };
  }));
  const rename = (gi: number, name: string) => setDraft((d) => d.map((g, i) => (i === gi ? { ...g, name } : g)));
  const remove = (gi: number) => setDraft((d) => d.filter((_, i) => i !== gi));
  const add = () => setDraft((d) => [...d, { name: `${t("groups.defaultName")} ${d.length + 1}`, packs: [] }]);

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent title={t("groups.title")}>
        <div className="space-y-6">
          <p className="text-sm text-muted">{t("groups.intro")}</p>
          <div className="flex flex-wrap items-center gap-2 rounded-md bg-sunken p-3">
            <span className="text-sm">{t("groups.splitPre")}</span>
            <input type="number" min={1} max={Math.max(1, packs.length)} value={count} onChange={(e) => setCount(Math.max(1, Math.min(packs.length || 1, Number(e.target.value) || 1)))}
              className={cn(input, "w-16 text-center")} aria-label={t("groups.splitPre")} />
            <span className="text-sm">{t("groups.splitPost", { count: perGroup })}</span>
            <Button size="sm" className="ml-auto" onClick={() => setDraft(splitEvenly(packs, perGroup, t("groups.defaultName")))}>{t("groups.split")}</Button>
          </div>

          {draft.map((g, gi) => (
            <div key={gi} className="space-y-2">
              <div className="flex items-center gap-2">
                <input className={cn(input, "flex-1 font-medium")} value={g.name} onChange={(e) => rename(gi, e.target.value)} aria-label={t("groups.name")} />
                <Button variant="ghost" size="icon" onClick={() => remove(gi)} aria-label={t("groups.remove")}><Trash2 className="h-4 w-4" /></Button>
              </div>
              <div className="flex flex-wrap gap-1.5">
                {packs.map((a) => {
                  const o = owner(a), mine = o === gi;
                  return (
                    <button key={a} onClick={() => toggle(gi, a)} aria-pressed={mine}
                      title={o >= 0 && !mine ? draft[o].name : undefined}
                      className={cn("h-8 w-10 rounded-md border font-display text-sm font-semibold",
                        mine ? "border-ink bg-ink text-surface" : o >= 0 ? "border-line text-muted/50 line-through" : "border-line hover:bg-sunken")}>
                      {String(a).padStart(2, "0")}
                    </button>
                  );
                })}
              </div>
            </div>
          ))}

          <Button variant="ghost" onClick={add}><Plus className="h-4 w-4" />{t("groups.add")}</Button>

          <section className="space-y-3 border-t border-line pt-5">
            <h3 className="flex items-center gap-1.5 font-display text-lg font-semibold">{t("pairs.title")}<InfoIcon id="topic.pairs" /></h3>
            <p className="text-sm text-muted">{t("pairs.intro")}</p>
            <Button size="sm" onClick={() => setPairDraft(neighbourPairs(packs))}>{t("pairs.neighbours")}</Button>
            {pairDraft.map((p, i) => (
              <div key={i} className="flex flex-wrap items-center gap-2 text-sm">
                <span className="text-muted">{t("pairs.plus")}</span>
                <select className={cn(input, "w-20")} value={p.plus} onChange={(e) => setPair(i, "plus", Number(e.target.value))}>
                  {packs.map((a) => <option key={a} value={a}>{String(a).padStart(2, "0")}</option>)}
                </select>
                <span className="text-muted">{t("pairs.minus")}</span>
                <select className={cn(input, "w-20")} value={p.minus} onChange={(e) => setPair(i, "minus", Number(e.target.value))}>
                  {packs.map((a) => <option key={a} value={a}>{String(a).padStart(2, "0")}</option>)}
                </select>
                <Button variant="ghost" size="icon" aria-label={t("pairs.remove")} onClick={() => setPairDraft((d) => d.filter((_, j) => j !== i))}><Trash2 className="h-4 w-4" /></Button>
              </div>
            ))}
            <div className="flex gap-2">
              <Button variant="ghost" size="sm" onClick={() => setPairDraft((d) => [...d, { plus: packs[0] ?? 0, minus: packs[1] ?? 1 }])}><Plus className="h-4 w-4" />{t("pairs.add")}</Button>
              {pairDraft.length > 0 && <Button variant="ghost" size="sm" onClick={() => setPairDraft([])}>{t("pairs.clear")}</Button>}
            </div>
            {validPairs(pairDraft, packs).length !== pairDraft.length && <p className="text-sm text-discharge">{t("pairs.invalid")}</p>}
          </section>

          <div className="flex items-center gap-2 border-t border-line pt-4">
            <Button variant="primary" onClick={() => { onSave(draft.filter((g) => g.name.trim() || g.packs.length)); onSavePairs(validPairs(pairDraft, packs)); onOpenChange(false); }}>{t("groups.save")}</Button>
            <Button variant="ghost" onClick={() => onOpenChange(false)}>{t("groups.cancel")}</Button>
            {draft.length > 0 && <Button variant="ghost" className="ml-auto" onClick={() => setDraft([])}>{t("groups.clear")}</Button>}
          </div>
        </div>
      </SheetContent>
    </Sheet>
  );
}
